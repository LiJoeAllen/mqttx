//! MQTT 引擎：在独立 tokio runtime 上管理多个连接的生命周期，
//! 通过 smol 无界通道把状态/消息/日志事件送回 GPUI 线程。
//!
//! - [`tls`] TLS/rustls 配置与跳过校验
//! - [`v5`] MQTT 5.0 客户端构建与事件循环
//! - [`v4`] MQTT 3.1.1 客户端构建与事件循环
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rumqttc::mqttbytes::v5::PublishProperties as V5PublishProperties;
use rumqttc::{
    AsyncClient as V5AsyncClient, EventLoop as V5EventLoop,
    PublishOptions as V5PublishOptions, RetainForwardRule, SubscribeFilterInput,
    SubscribeProperties as V5SubscribeProperties,
};
use rumqttc_v4 as rv4;
use tokio::sync::watch;

use crate::model::{
    ConnectionConfig, ConnectionStatus, LogEntry, LogLevel, MqttRecord, PublishParams, SubscribeOptions, Subscription,
};// ─── 引擎事件 ────────────────────────────────────────────────────────────────


mod tls;
mod v4;
mod v5;

use tls::*;
use v4::*;
use v5::*;

pub enum EngineEvent {
    /// 连接状态变化
    Status {
        connection_id: String,
        status: ConnectionStatus,
        error: Option<String>,
        reason_code: Option<u8>,
        session_present: Option<bool>,
    },
    /// 收到消息
    Message(MqttRecord),
    /// 一条本地发出的消息（已成功交给客户端）。payload 为实际发送的字节。
    /// 属性与接收记录对齐，本地发送记录可完整回显。
    Published {
        connection_id: String,
        topic: String,
        payload: Vec<u8>,
        qos: u8,
        retain: bool,
        content_type: Option<String>,
        user_properties: Vec<(String, String)>,
        response_topic: Option<String>,
        correlation_data: Option<String>,
        message_expiry_interval: Option<u32>,
    },
    /// 订阅请求结果（tracked，等待 SUBACK）
    SubscribeResult {
        connection_id: String,
        topic: String,
        qos: u8,
        ok: bool,
        error: Option<String>,
    },
    /// 日志
    Log(LogEntry),
}

pub type EventSender = smol::channel::Sender<EngineEvent>;

// ─── 句柄 ────────────────────────────────────────────────────────────────────

enum ClientKind {
    V5(V5AsyncClient),
    V4(rv4::AsyncClient),
}

struct ConnHandle {
    /// `None` 表示仍在建立中（占位）。建连完成后由 connect 的任务升级为 Some。
    client: Option<ClientKind>,
    cancel_tx: watch::Sender<bool>,
    connected: Arc<AtomicBool>,
    /// 连接代数：同一 id 的第几次 connect。事件循环退出清理时只在
    /// map 中仍是自己这一代时才移除，避免误删重连后的新句柄。
    generation: u64,
}

pub struct MqttEngine {
    runtime: tokio::runtime::Runtime,
    conns: Mutex<HashMap<String, ConnHandle>>,
    /// 下一次 connect 使用的代数
    next_generation: AtomicU64,
    /// 主题/连接 ID 驻留缓存：同一连接内主题高度重复，驻留让消息记录共享字符串
    topics: Mutex<HashMap<String, Arc<str>>>,
    tx: EventSender,
    seq: Arc<AtomicU64>,
}

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

impl MqttEngine {
    pub fn new(tx: EventSender) -> Arc<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("mqtt-engine")
            .build()
            .expect("创建 tokio runtime 失败");
        Arc::new(Self {
            runtime,
            conns: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(1),
            topics: Mutex::new(HashMap::new()),
            tx,
            seq: Arc::new(AtomicU64::new(1)),
        })
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.tx.try_send(event);
    }

    fn log(&self, conn_id: &str, level: LogLevel, event: &str, message: String, details: Option<String>) {
        self.emit(EngineEvent::Log(LogEntry {
            timestamp: now_ms(),
            connection_id: conn_id.to_string(),
            level,
            event: event.to_string(),
            message,
            details,
        }));
    }

    pub fn is_connected(&self, id: &str) -> bool {
        let Ok(map) = self.conns.lock() else {
            return false;
        };
        map.get(id)
            .map(|h| h.connected.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    // ── 连接 ──────────────────────────────────────────────────────────────

    pub fn connect(self: &Arc<Self>, cfg: ConnectionConfig) {
        // 关闭同名旧连接
        self.close(&cfg.id, false);

        let id = cfg.id.clone();
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let connected = Arc::new(AtomicBool::new(false));
        // 同步占位（client=None）：建连期间 close() 也能取消本次连接，
        // 且 is_connected() 在建连完成前保持 false。
        self.conns.lock().unwrap().insert(
            id.clone(),
            ConnHandle {
                client: None,
                cancel_tx,
                connected: connected.clone(),
                generation,
            },
        );

        self.emit(EngineEvent::Status {
            connection_id: id.clone(),
            status: ConnectionStatus::Connecting,
            error: None,
            reason_code: None,
            session_present: None,
        });
        self.log(
            &id,
            LogLevel::Info,
            "connect",
            format!(
                "正在连接 {}:{} ({}, {})",
                cfg.host,
                cfg.port,
                cfg.protocol.label(),
                cfg.transport.label()
            ),
            None,
        );

        let engine = Arc::clone(self);
        self.runtime.spawn(async move {
            let auto_reconnect = cfg.auto_reconnect;
            let max_reconnect_times = cfg.max_reconnect_times;
            // build 含证书/CA 文件读取等阻塞 IO，放 spawn_blocking，
            // 避免慢盘上卡住 2 线程 runtime 里其他连接的调度
            let build = tokio::task::spawn_blocking(move || {
                if cfg.protocol.is_v5() {
                    build_v5(&cfg).map(|(client, el)| {
                        (ClientKind::V5(client), EventLoopKind::V5(Box::new(el)))
                    })
                } else {
                    build_v4(&cfg).map(|(client, el)| {
                        (ClientKind::V4(client), EventLoopKind::V4(Box::new(el)))
                    })
                }
            })
            .await;

            let build_error = |engine: &Arc<MqttEngine>, id: &str, generation: u64, e: String| {
                engine.remove_handle_if(id, generation);
                engine.emit(EngineEvent::Status {
                    connection_id: id.to_string(),
                    status: ConnectionStatus::Error,
                    error: Some(e.clone()),
                    reason_code: None,
                    session_present: None,
                });
                engine.log(id, LogLevel::Error, "connect_error", format!("连接失败: {e}"), None);
            };
            let (client, eventloop) = match build {
                Ok(Ok(pair)) => pair,
                Ok(Err(e)) => {
                    build_error(&engine, &id, generation, e);
                    return;
                }
                Err(j) => {
                    build_error(&engine, &id, generation, format!("内部任务失败: {j}"));
                    return;
                }
            };

            // 建连期间该连接可能已被 close 或更新的 connect 顶掉：
            // 仅当占位句柄仍是本代时才升级为可用客户端，否则丢弃本次建连。
            {
                let mut map = match engine.conns.lock() {
                    Ok(m) => m,
                    Err(_) => return,
                };
                match map.get_mut(&id) {
                    Some(h) if h.generation == generation => h.client = Some(client),
                    _ => return,
                }
            }

            match eventloop {
                EventLoopKind::V5(el) => {
                    spawn_v5_loop(
                        engine.clone(),
                        id,
                        *el,
                        cancel_rx,
                        connected,
                        generation,
                        auto_reconnect,
                        max_reconnect_times,
                    );
                }
                EventLoopKind::V4(el) => {
                    spawn_v4_loop(
                        engine.clone(),
                        id,
                        *el,
                        cancel_rx,
                        connected,
                        generation,
                        auto_reconnect,
                        max_reconnect_times,
                    );
                }
            }
        });
    }

    /// 主动断开。`notify` 为 true 时发出 Disconnected 状态（用户操作）。
    pub fn close(&self, id: &str, user_initiated: bool) {
        let handle = self.conns.lock().unwrap().remove(id);
        if let Some(h) = handle {
            let _ = h.cancel_tx.send(true);
            let connected = h.connected.clone();
            // 发送 MQTT DISCONNECT 报文后丢弃客户端（建连未完成时无需发送）
            self.runtime.spawn(async move {
                match h.client {
                    Some(ClientKind::V5(c)) => {
                        let _ = c.disconnect().await;
                    }
                    Some(ClientKind::V4(c)) => {
                        let _ = c.disconnect().await;
                    }
                    None => {}
                }
                connected.store(false, Ordering::SeqCst);
            });
            if user_initiated {
                self.log(id, LogLevel::Info, "disconnect", "用户请求断开连接".into(), None);
                self.emit(EngineEvent::Status {
                    connection_id: id.to_string(),
                    status: ConnectionStatus::Disconnected,
                    error: None,
                    reason_code: None,
                    session_present: None,
                });
            }
        }
    }

    // ── 发布 ──────────────────────────────────────────────────────────────

    pub fn publish(self: &Arc<Self>, connection_id: String, params: PublishParams) {
        let engine = Arc::clone(self);
        self.runtime.spawn(async move {
            let Some(client) = engine.client_of(&connection_id) else {
                engine.log(
                    &connection_id,
                    LogLevel::Error,
                    "publish_error",
                    "发布失败: 连接不存在".into(),
                    None,
                );
                return;
            };
            let payload = params
                .raw_bytes
                .clone()
                .unwrap_or_else(|| params.payload.as_bytes().to_vec());
            // 实际发送的字节留给 Published 事件（记录需要真实内容）
            let sent_payload = payload.clone();
            // v5 与 v4 两个 crate 各自定义 ClientError，统一归一化为 String。
            let result: Result<(), String> = match &client {
                ClientKind::V5(client) => {
                    let props = V5PublishProperties {
                        user_properties: params.user_properties.clone(),
                        content_type: params.content_type.clone(),
                        message_expiry_interval: params.message_expiry_interval,
                        response_topic: params.response_topic.clone(),
                        correlation_data: params
                            .correlation_data
                            .clone()
                            .map(|s| s.into_bytes().into()),
                        ..Default::default()
                    };
                    let options = V5PublishOptions::new(v5_qos(params.qos))
                        .retain(params.retain)
                        .properties(props);
                    client
                        .publish(params.topic.clone(), payload, options)
                        .await
                        .map_err(|e| e.to_string())
                }
                ClientKind::V4(client) => {
                    let options =
                        rv4::PublishOptions::new(v4_qos(params.qos)).retain(params.retain);
                    client
                        .publish(params.topic.clone(), payload, options)
                        .await
                        .map_err(|e| e.to_string())
                }
            };
            match result {
                Ok(_) => {
                    engine.log(
                        &connection_id,
                        LogLevel::Info,
                        "publish_sent",
                        format!(
                            "发布消息到 {} (QoS {}, retain={})",
                            params.topic, params.qos, params.retain
                        ),
                        None,
                    );
                    engine.emit(EngineEvent::Published {
                        connection_id,
                        topic: params.topic,
                        payload: sent_payload,
                        qos: params.qos,
                        retain: params.retain,
                        content_type: params.content_type,
                        user_properties: params.user_properties,
                        response_topic: params.response_topic,
                        correlation_data: params.correlation_data,
                        message_expiry_interval: params.message_expiry_interval,
                    });
                }
                Err(e) => {
                    engine.log(
                        &connection_id,
                        LogLevel::Error,
                        "publish_error",
                        format!("发布失败: {e}"),
                        None,
                    );
                }
            }
        });
    }

    // ── 订阅 / 取消订阅 ─────────────────────────────────────────────────────

    /// 旧签名薄封装：不携带 v5 订阅选项（连接视图等现有调用点继续可用）。
    pub fn subscribe(self: &Arc<Self>, connection_id: String, sub: Subscription) {
        self.subscribe_with_options(connection_id, sub, SubscribeOptions::default());
    }

    /// 带 MQTT 5 订阅选项的订阅（sub_identifier / NL / RAP / Retain Handling）；
    /// v4 连接会忽略这些选项。
    pub fn subscribe_with_options(
        self: &Arc<Self>,
        connection_id: String,
        sub: Subscription,
        opts: SubscribeOptions,
    ) {
        let engine = Arc::clone(self);
        self.runtime.spawn(async move {
            let Some(client) = engine.client_of(&connection_id) else {
                engine.emit(EngineEvent::SubscribeResult {
                    connection_id,
                    topic: sub.topic,
                    qos: sub.qos,
                    ok: false,
                    error: Some("连接不存在".into()),
                });
                return;
            };
            engine.log(
                &connection_id,
                LogLevel::Info,
                "subscribe",
                format!("订阅主题: {} (QoS {})", sub.topic, sub.qos),
                None,
            );
            let result = match &client {
                ClientKind::V5(client) => {
                    // NL/RAP/Retain Handling 是「按主题过滤器」的订阅选项，
                    // 订阅标识符则在订阅报文属性里，分别写入两处。
                    let filter = SubscribeFilterInput::new(sub.topic.clone(), v5_qos(sub.qos))
                        .no_local(opts.no_local)
                        .preserve_retain(opts.retain_as_published)
                        .retain_forward_rule(match opts.retain_handling {
                            1 => RetainForwardRule::OnNewSubscribe,
                            2 => RetainForwardRule::Never,
                            _ => RetainForwardRule::OnEverySubscribe,
                        });
                    let properties = V5SubscribeProperties {
                        // 0 不是合法的订阅标识符，按未设置处理
                        id: opts.sub_identifier.filter(|v| *v > 0).map(|v| v as usize),
                        user_properties: Vec::new(),
                    };
                    match client
                        .subscribe_many_with_properties_tracked(std::iter::once(filter), properties)
                        .await
                    {
                        Ok(notice) => notice.wait_completion_async().await.map_err(|e| {
                            format!("订阅被拒绝: {e}")
                        }),
                        Err(e) => Err(format!("订阅请求失败: {e}")),
                    }
                }
                ClientKind::V4(client) => {
                    match client
                        .subscribe_tracked(sub.topic.clone(), v4_qos(sub.qos))
                        .await
                    {
                        Ok(notice) => notice.wait_completion_async().await.map_err(|e| {
                            format!("订阅被拒绝: {e}")
                        }),
                        Err(e) => Err(format!("订阅请求失败: {e}")),
                    }
                }
            };
            let (ok, error) = match result {
                Ok(()) => (true, None),
                Err(e) => {
                    engine.log(
                        &connection_id,
                        LogLevel::Error,
                        "subscribe_error",
                        e.clone(),
                        None,
                    );
                    (false, Some(e))
                }
            };
            engine.emit(EngineEvent::SubscribeResult {
                connection_id,
                topic: sub.topic,
                qos: sub.qos,
                ok,
                error,
            });
        });
    }

    pub fn unsubscribe(self: &Arc<Self>, connection_id: String, topic: String) {
        let engine = Arc::clone(self);
        self.runtime.spawn(async move {
            let Some(client) = engine.client_of(&connection_id) else {
                return;
            };
            let result: Result<(), String> = match &client {
                ClientKind::V5(client) => client
                    .unsubscribe(topic.clone())
                    .await
                    .map_err(|e| e.to_string()),
                ClientKind::V4(client) => client
                    .unsubscribe(topic.clone())
                    .await
                    .map_err(|e| e.to_string()),
            };
            match result {
                Ok(()) => {
                    engine.log(
                        &connection_id,
                        LogLevel::Info,
                        "unsubscribe",
                        format!("取消订阅: {topic}"),
                        None,
                    );
                }
                Err(e) => {
                    engine.log(
                        &connection_id,
                        LogLevel::Error,
                        "unsubscribe_error",
                        format!("取消订阅失败: {e}"),
                        None,
                    );
                }
            }
        });
    }

    /// 测试连接：使用配置的连接超时（上限 30 秒，防止误填过大值长时间挂起），
    /// 结果通过 channel 返回。
    /// 在引擎的 tokio runtime 上调度阻塞任务（OTA 下载等），
    /// 结果经 smol 通道送回 GPUI 执行器；任务 panic 时接收端以空错误结束。
    pub fn run_blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> smol::channel::Receiver<T> {
        let (tx, rx) = smol::channel::bounded(1);
        self.runtime.spawn(async move {
            if let Ok(value) = tokio::task::spawn_blocking(f).await {
                let _ = tx.send(value).await;
            }
        });
        rx
    }

    /// 驻留字符串（主题/连接 ID）：同一内容全进程只保留一份 `Arc<str>`。
    /// 缓存有上限，异常流量下整体清空重建，避免无限增长。
    pub(crate) fn intern(&self, s: &str) -> Arc<str> {
        const INTERN_CAP: usize = 10_000;
        let mut cache = self.topics.lock().unwrap();
        if let Some(arc) = cache.get(s) {
            return arc.clone();
        }
        if cache.len() >= INTERN_CAP {
            cache.clear();
        }
        let arc: Arc<str> = Arc::from(s);
        cache.insert(s.to_string(), arc.clone());
        arc
    }

    pub fn test_connection(
        self: &Arc<Self>,
        cfg: ConnectionConfig,
    ) -> smol::channel::Receiver<Result<(), String>> {
        let (tx, rx) = smol::channel::bounded(1);
        let secs = u64::from(cfg.connection_timeout_secs.clamp(1, 30));
        self.runtime.spawn(async move {
            let result = tokio::time::timeout(Duration::from_secs(secs), async {
                test_handshake(&cfg).await
            })
            .await;
            let reply = match result {
                Ok(inner) => inner,
                Err(_) => Err(format!(
                    "连接测试超时（{secs} 秒），请检查主机地址与端口"
                )),
            };
            let _ = tx.try_send(reply);
        });
        rx
    }

    // ── 内部工具 ───────────────────────────────────────────────────────────

    /// 克隆一个连接的异步客户端用于发命令。
    /// 不持有 std Mutex 守卫跨 await，避免与 tokio runtime 的 Send 要求冲突。
    /// 占位句柄（建连未完成）视为不存在。
    fn client_of(&self, id: &str) -> Option<ClientKind> {
        let map = self.conns.lock().ok()?;
        let client = map.get(id)?.client.as_ref()?;
        Some(match client {
            ClientKind::V5(c) => ClientKind::V5(c.clone()),
            ClientKind::V4(c) => ClientKind::V4(c.clone()),
        })
    }

    /// 仅当 map 中该 id 的句柄仍是 `generation` 这一代时才移除。
    /// 事件循环退出清理必须走这里：无条件按 id 删除会把重连后的
    /// 新句柄误删，导致连接无法再从 UI 断开。
    fn remove_handle_if(&self, id: &str, generation: u64) {
        if let Ok(mut map) = self.conns.lock()
            && map.get(id).map(|h| h.generation) == Some(generation)
        {
            map.remove(id);
        }
    }
}

enum EventLoopKind {
    // 两个事件循环都在 KB 级，装箱让枚举本身只有指针大小
    V5(Box<V5EventLoop>),
    V4(Box<rv4::EventLoop>),
}

// ─── 选项构建 ────────────────────────────────────────────────────────────────

/// 连接超时秒数：至少 1 秒，避免误填 0 导致立刻超时。
fn connection_timeout_secs(cfg: &ConnectionConfig) -> u64 {
    u64::from(cfg.connection_timeout_secs.max(1))
}

