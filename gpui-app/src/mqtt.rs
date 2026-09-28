//! MQTT 引擎：在独立 tokio runtime 上管理多个连接的生命周期，
//! 通过 smol 无界通道把状态/消息/日志事件送回 GPUI 线程。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rumqttc::mqttbytes::v5::{
    ConnectReturnCode as V5ReturnCode, Packet as V5Packet, Publish as V5Publish,
    PublishProperties as V5PublishProperties,
};
use rumqttc::{
    AsyncClient as V5AsyncClient, Broker as V5Broker, ConnectionError as V5Error,
    Event as V5Event, EventLoop as V5EventLoop, MqttOptions as V5Options, Outgoing as V5Outgoing,
    PublishOptions as V5PublishOptions, QoS as V5QoS, Transport as V5Transport,
};
use rumqttc_v4 as rv4;
use tokio::sync::watch;

use crate::model::{
    ConnectionConfig, ConnectionStatus, Direction, LogEntry, LogLevel, MqttRecord, PublishParams,
    Subscription, TransportKind,
};// ─── 引擎事件 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
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
    /// 一条本地发出的消息（已成功交给客户端）
    Published {
        connection_id: String,
        topic: String,
        payload: String,
        qos: u8,
        retain: bool,
        content_type: Option<String>,
        user_properties: Vec<(String, String)>,
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
    client: ClientKind,
    cancel_tx: watch::Sender<bool>,
    connected: Arc<AtomicBool>,
}

pub struct MqttEngine {
    runtime: tokio::runtime::Runtime,
    conns: Mutex<HashMap<String, ConnHandle>>,
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
            let build = if cfg.protocol.is_v5() {
                build_v5(&cfg)
                    .map(|(client, el)| (ClientKind::V5(client), EventLoopKind::V5(Box::new(el))))
            } else {
                build_v4(&cfg)
                    .map(|(client, el)| (ClientKind::V4(client), EventLoopKind::V4(Box::new(el))))
            };

            let (client, eventloop) = match build {
                Ok(pair) => pair,
                Err(e) => {
                    engine.emit(EngineEvent::Status {
                        connection_id: id.clone(),
                        status: ConnectionStatus::Error,
                        error: Some(e.clone()),
                        reason_code: None,
                        session_present: None,
                    });
                    engine.log(&id, LogLevel::Error, "connect_error", format!("连接失败: {e}"), None);
                    return;
                }
            };

            let connected = Arc::new(AtomicBool::new(false));
            let (cancel_tx, cancel_rx) = watch::channel(false);
            let handle = ConnHandle {
                client,
                cancel_tx,
                connected: connected.clone(),
            };
            engine
                .conns
                .lock()
                .unwrap()
                .insert(id.clone(), handle);

            match eventloop {
                EventLoopKind::V5(el) => {
                    spawn_v5_loop(
                        engine.clone(),
                        id,
                        *el,
                        cancel_rx,
                        connected,
                        cfg.auto_reconnect,
                    );
                }
                EventLoopKind::V4(el) => {
                    spawn_v4_loop(
                        engine.clone(),
                        id,
                        *el,
                        cancel_rx,
                        connected,
                        cfg.auto_reconnect,
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
            // 发送 MQTT DISCONNECT 报文后丢弃客户端
            self.runtime.spawn(async move {
                match h.client {
                    ClientKind::V5(c) => {
                        let _ = c.disconnect().await;
                    }
                    ClientKind::V4(c) => {
                        let _ = c.disconnect().await;
                    }
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
                        payload: params.payload,
                        qos: params.qos,
                        retain: params.retain,
                        content_type: params.content_type,
                        user_properties: params.user_properties,
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

    pub fn subscribe(self: &Arc<Self>, connection_id: String, sub: Subscription) {
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
                    match client
                        .subscribe_tracked(sub.topic.clone(), v5_qos(sub.qos))
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

    /// 测试连接：5 秒内等待 CONNACK，结果通过 channel 返回。
    pub fn test_connection(
        self: &Arc<Self>,
        cfg: ConnectionConfig,
    ) -> smol::channel::Receiver<Result<(), String>> {
        let (tx, rx) = smol::channel::bounded(1);
        self.runtime.spawn(async move {
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                test_handshake(&cfg).await
            })
            .await;
            let reply = match result {
                Ok(inner) => inner,
                Err(_) => Err("连接测试超时（5 秒），请检查主机地址与端口".into()),
            };
            let _ = tx.try_send(reply);
        });
        rx
    }

    // ── 内部工具 ───────────────────────────────────────────────────────────

    /// 克隆一个连接的异步客户端用于发命令。
    /// 不持有 std Mutex 守卫跨 await，避免与 tokio runtime 的 Send 要求冲突。
    fn client_of(&self, id: &str) -> Option<ClientKind> {
        let map = self.conns.lock().ok()?;
        let h = map.get(id)?;
        Some(match &h.client {
            ClientKind::V5(c) => ClientKind::V5(c.clone()),
            ClientKind::V4(c) => ClientKind::V4(c.clone()),
        })
    }
}

enum EventLoopKind {
    // 两个事件循环都在 KB 级，装箱让枚举本身只有指针大小
    V5(Box<V5EventLoop>),
    V4(Box<rv4::EventLoop>),
}

// ─── 选项构建 ────────────────────────────────────────────────────────────────

fn build_v5(
    cfg: &ConnectionConfig,
) -> Result<(V5AsyncClient, V5EventLoop), String> {
    let mut opts = match cfg.transport {
        TransportKind::Tcp | TransportKind::Tls => {
            V5Options::new(cfg.client_id.clone(), V5Broker::tcp(&cfg.host, cfg.port))
        }
        TransportKind::Ws => {
            let url = format!("ws://{}:{}{}", cfg.host, cfg.port, normalize_path(&cfg.path));
            V5Options::new(cfg.client_id.clone(), V5Broker::websocket(url).map_err(|e| format!("{e:?}"))?)
        }
        TransportKind::Wss => {
            let url = format!("wss://{}:{}{}", cfg.host, cfg.port, normalize_path(&cfg.path));
            V5Options::try_websocket_with_default_tls(cfg.client_id.clone(), url)
                .map_err(|e| format!("{e:?}"))?
        }
    };

    if cfg.transport == TransportKind::Tls {
        opts.set_transport(
            V5Transport::try_tls_with_default_config().map_err(|e| format!("TLS 初始化失败: {e:?}"))?,
        );
    }

    opts.set_keep_alive(cfg.keep_alive);
    opts.set_clean_start(cfg.clean_start);
    if cfg.session_expiry_interval > 0 {
        opts.set_session_expiry_interval(Some(cfg.session_expiry_interval));
    }
    if !cfg.username.is_empty() {
        opts.set_credentials(cfg.username.clone(), cfg.password.clone());
    }
    if let Some(v) = cfg.receive_maximum {
        opts.set_receive_maximum(Some(v));
    }
    if let Some(v) = cfg.maximum_packet_size {
        opts.set_max_packet_size(Some(v));
    }
    if let Some(v) = cfg.topic_alias_maximum {
        opts.set_topic_alias_max(Some(v));
    }
    if let Some(will) = &cfg.last_will
        && !will.topic.is_empty() {
            let lw = rumqttc::mqttbytes::v5::LastWill::new(
                will.topic.clone(),
                will.payload.as_bytes().to_vec(),
                v5_qos(will.qos),
                will.retain,
                None,
            );
            opts.set_last_will(lw);
        }

    Ok(V5AsyncClient::builder(opts).build())
}

fn build_v4(
    cfg: &ConnectionConfig,
) -> Result<(rv4::AsyncClient, rv4::EventLoop), String> {
    use rv4::{Broker as V4Broker, MqttOptions as V4Options, Transport as V4Transport};

    let mut opts = match cfg.transport {
        TransportKind::Tcp | TransportKind::Tls => {
            V4Options::new(cfg.client_id.clone(), V4Broker::tcp(&cfg.host, cfg.port))
        }
        TransportKind::Ws => {
            let url = format!("ws://{}:{}{}", cfg.host, cfg.port, normalize_path(&cfg.path));
            V4Options::new(cfg.client_id.clone(), V4Broker::websocket(url).map_err(|e| format!("{e:?}"))?)
        }
        TransportKind::Wss => {
            let url = format!("wss://{}:{}{}", cfg.host, cfg.port, normalize_path(&cfg.path));
            V4Options::try_websocket_with_default_tls(cfg.client_id.clone(), url)
                .map_err(|e| format!("{e:?}"))?
        }
    };

    if cfg.transport == TransportKind::Tls {
        opts.set_transport(V4Transport::try_tls_with_default_config().map_err(
            |e| format!("TLS 初始化失败: {e:?}"),
        )?);
    }

    opts.set_keep_alive(cfg.keep_alive);
    opts.set_clean_session(cfg.clean_start);
    if !cfg.username.is_empty() {
        opts.set_credentials(cfg.username.clone(), cfg.password.clone());
    }
    if let Some(will) = &cfg.last_will
        && !will.topic.is_empty() {
            let lw = rv4::mqttbytes::v4::LastWill::new(
                will.topic.clone(),
                will.payload.as_bytes().to_vec(),
                v4_qos(will.qos),
                will.retain,
            );
            opts.set_last_will(lw);
        }

    Ok(rv4::AsyncClient::builder(opts).build())
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        "/mqtt".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn v5_qos(qos: u8) -> V5QoS {
    match qos {
        1 => V5QoS::AtLeastOnce,
        2 => V5QoS::ExactlyOnce,
        _ => V5QoS::AtMostOnce,
    }
}

fn v4_qos(qos: u8) -> rv4::QoS {
    match qos {
        1 => rv4::QoS::AtLeastOnce,
        2 => rv4::QoS::ExactlyOnce,
        _ => rv4::QoS::AtMostOnce,
    }
}

// ─── 测试连接握手 ────────────────────────────────────────────────────────────

async fn test_handshake(cfg_in: &ConnectionConfig) -> Result<(), String> {
    // 握手测试必须使用独立 client_id，否则会把使用相同 client_id 的现有连接
    // 从 broker 顶下线（MQTT 同 client_id 单会话规则）。
    let mut cfg = cfg_in.clone();
    cfg.client_id = format!("mqttx_test_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let cfg = &cfg;
    if cfg.protocol.is_v5() {
        let (client, mut eventloop) = build_v5(cfg)?;
        loop {
            match eventloop.poll().await {
                Ok(V5Event::Incoming(V5Packet::ConnAck(ack))) => {
                    if ack.code == V5ReturnCode::Success {
                        // 测试完成，主动断开避免在 broker 侧残留会话
                        let _ = client.disconnect().await;
                        return Ok(());
                    }
                    return Err(format!("连接被拒绝: {:?}", ack.code));
                }
                Ok(V5Event::Incoming(V5Packet::Disconnect(d))) => {
                    return Err(format!("服务端断开: {:?}", d.reason_code));
                }
                Err(e) => return Err(format!("连接失败: {e}")),
                _ => {}
            }
        }
    } else {
        let (client, mut eventloop) = build_v4(cfg)?;
        loop {
            match eventloop.poll().await {
                Ok(rv4::Event::Incoming(rv4::Packet::ConnAck(ack))) => {
                    if ack.code == rv4::ConnectReturnCode::Success {
                        let _ = client.disconnect().await;
                        return Ok(());
                    }
                    return Err(format!("连接被拒绝: {:?}", ack.code));
                }
                Ok(rv4::Event::Incoming(rv4::Packet::Disconnect)) => {
                    return Err("服务端主动断开".into());
                }
                Err(e) => return Err(format!("连接失败: {e}")),
                _ => {}
            }
        }
    }
}

// ─── 事件循环 ────────────────────────────────────────────────────────────────

fn v5_record(seq: u64, conn_id: &str, p: &V5Publish) -> MqttRecord {
    let props = p.properties.as_ref();
    MqttRecord {
        seq,
        connection_id: conn_id.to_string(),
        direction: Direction::Received,
        topic: String::from_utf8_lossy(&p.topic).to_string(),
        payload: String::from_utf8_lossy(&p.payload).to_string(),
        qos: v5_qos_u8(p.qos),
        retain: p.retain,
        timestamp: now_ms(),
        user_properties: props
            .map(|p| p.user_properties.clone())
            .unwrap_or_default(),
        content_type: props.and_then(|p| p.content_type.clone()),
        response_topic: props.and_then(|p| p.response_topic.clone()),
        correlation_data: props.and_then(|p| {
            p.correlation_data
                .as_ref()
                .map(|d| d.iter().map(|b| format!("{b:02x}")).collect())
        }),
        message_expiry_interval: props.and_then(|p| p.message_expiry_interval),
        subscription_identifier: props
            .and_then(|p| p.subscription_identifiers.first().map(|v| *v as u32)),
    }
}

fn v5_qos_u8(qos: V5QoS) -> u8 {
    match qos {
        V5QoS::AtMostOnce => 0,
        V5QoS::AtLeastOnce => 1,
        V5QoS::ExactlyOnce => 2,
    }
}

fn spawn_v5_loop(
    engine: Arc<MqttEngine>,
    id: String,
    mut eventloop: V5EventLoop,
    mut cancel_rx: watch::Receiver<bool>,
    connected: Arc<AtomicBool>,
    auto_reconnect: bool,
) {
    let runtime = engine.runtime.handle().clone();
    runtime.spawn(async move {
        let mut first_connect = true;
        loop {
            tokio::select! {
                event = eventloop.poll() => {
                    match event {
                        Ok(V5Event::Incoming(V5Packet::Publish(p))) => {
                            let topic = String::from_utf8_lossy(&p.topic).to_string();
                            let record = v5_record(engine.next_seq(), &id, &p);
                            engine.log(
                                &id,
                                LogLevel::Info,
                                "publish_received",
                                format!("收到消息: {topic} ({} 字节, QoS {})", p.payload.len(), v5_qos_u8(p.qos)),
                                None,
                            );
                            engine.emit(EngineEvent::Message(record));
                        }
                        Ok(V5Event::Incoming(V5Packet::ConnAck(ack))) => {
                            let code = ack.code as u8;
                            connected.store(ack.code == V5ReturnCode::Success, Ordering::SeqCst);
                            if ack.code == V5ReturnCode::Success {
                                first_connect = false;
                                engine.log(
                                    &id,
                                    LogLevel::Info,
                                    "connack",
                                    format!("连接成功 (session_present: {})", ack.session_present),
                                    None,
                                );
                                engine.emit(EngineEvent::Status {
                                    connection_id: id.clone(),
                                    status: ConnectionStatus::Connected,
                                    error: None,
                                    reason_code: Some(code),
                                    session_present: Some(ack.session_present),
                                });
                            } else {
                                engine.log(
                                    &id,
                                    LogLevel::Error,
                                    "connack",
                                    format!("连接被拒绝: {:?} (code {code})", ack.code),
                                    None,
                                );
                                engine.emit(EngineEvent::Status {
                                    connection_id: id.clone(),
                                    status: ConnectionStatus::Error,
                                    error: Some(format!("{:?}", ack.code)),
                                    reason_code: Some(code),
                                    session_present: Some(ack.session_present),
                                });
                                break;
                            }
                        }
                        Ok(V5Event::Incoming(V5Packet::Disconnect(d))) => {
                            connected.store(false, Ordering::SeqCst);
                            engine.log(
                                &id,
                                LogLevel::Warn,
                                "disconnect_received",
                                format!("收到断开连接: {:?} (code {})", d.reason_code, d.reason_code as u8),
                                None,
                            );
                            engine.emit(EngineEvent::Status {
                                connection_id: id.clone(),
                                status: ConnectionStatus::Disconnected,
                                error: Some(format!("{:?}", d.reason_code)),
                                reason_code: Some(d.reason_code as u8),
                                session_present: None,
                            });
                            break;
                        }
                        Ok(V5Event::Incoming(V5Packet::SubAck(s))) => {
                            let granted: Vec<String> = s
                                .return_codes
                                .iter()
                                .map(|c| format!("{c:?}"))
                                .collect();
                            engine.log(&id, LogLevel::Info, "suback",
                                format!("订阅确认 (packet {}): {}", s.pkid, granted.join(", ")), None);
                        }
                        Ok(V5Event::Incoming(V5Packet::UnsubAck(_))) => {
                            engine.log(&id, LogLevel::Info, "unsuback", "取消订阅确认".into(), None);
                        }
                        Ok(V5Event::Incoming(V5Packet::PubAck(a))) => {
                            engine.log(&id, LogLevel::Debug, "puback",
                                format!("发布确认 (packet {}, reason {:?})", a.pkid, a.reason), None);
                        }
                        Ok(V5Event::Incoming(V5Packet::PubRec(a))) => {
                            engine.log(&id, LogLevel::Debug, "pubrec",
                                format!("QoS2 发布收到 (packet {})", a.pkid), None);
                        }
                        Ok(V5Event::Incoming(V5Packet::PubComp(a))) => {
                            engine.log(&id, LogLevel::Debug, "pubcomp",
                                format!("QoS2 发布完成 (packet {})", a.pkid), None);
                        }
                        Ok(V5Event::Outgoing(V5Outgoing::Publish(pkid))) => {
                            engine.log(&id, LogLevel::Debug, "publish_outgoing",
                                format!("消息已发出 (packet {pkid})"), None);
                        }
                        Ok(_) => {}
                        Err(V5Error::RequestsDone) => {
                            connected.store(false, Ordering::SeqCst);
                            engine.emit(EngineEvent::Status {
                                connection_id: id.clone(),
                                status: ConnectionStatus::Disconnected,
                                error: None,
                                reason_code: None,
                                session_present: None,
                            });
                            break;
                        }
                        Err(e) => {
                            connected.store(false, Ordering::SeqCst);
                            engine.emit(EngineEvent::Status {
                                connection_id: id.clone(),
                                status: ConnectionStatus::Error,
                                error: Some(e.to_string()),
                                reason_code: None,
                                session_present: None,
                            });
                            if !auto_reconnect || first_connect {
                                engine.log(&id, LogLevel::Error, "connection_error",
                                    format!("连接错误: {e}"), None);
                                break;
                            }
                            engine.log(&id, LogLevel::Warn, "connection_error",
                                format!("连接错误，2 秒后重连: {e}"), None);
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                    }
                }
                _ = cancel_rx.changed() => {
                    engine.log(&id, LogLevel::Info, "disconnect_cancelled", "连接已取消".into(), None);
                    break;
                }
            }
        }
        engine.conns.lock().ok().and_then(|mut m| m.remove(&id));
    });
}

fn spawn_v4_loop(
    engine: Arc<MqttEngine>,
    id: String,
    mut eventloop: rv4::EventLoop,
    mut cancel_rx: watch::Receiver<bool>,
    connected: Arc<AtomicBool>,
    auto_reconnect: bool,
) {
    let runtime = engine.runtime.handle().clone();
    runtime.spawn(async move {
        let mut first_connect = true;
        loop {
            tokio::select! {
                event = eventloop.poll() => {
                    match event {
                        Ok(rv4::Event::Incoming(rv4::Packet::Publish(p))) => {
                            let record = MqttRecord {
                                seq: engine.next_seq(),
                                connection_id: id.clone(),
                                direction: Direction::Received,
                                topic: String::from_utf8_lossy(&p.topic).to_string(),
                                payload: String::from_utf8_lossy(&p.payload).to_string(),
                                qos: match p.qos {
                                    rv4::QoS::AtMostOnce => 0,
                                    rv4::QoS::AtLeastOnce => 1,
                                    rv4::QoS::ExactlyOnce => 2,
                                },
                                retain: p.retain,
                                timestamp: now_ms(),
                                user_properties: Vec::new(),
                                content_type: None,
                                response_topic: None,
                                correlation_data: None,
                                message_expiry_interval: None,
                                subscription_identifier: None,
                            };
                            engine.log(
                                &id, LogLevel::Info, "publish_received",
                                format!("收到消息: {} ({} 字节)", record.topic, p.payload.len()), None);
                            engine.emit(EngineEvent::Message(record));
                        }
                        Ok(rv4::Event::Incoming(rv4::Packet::ConnAck(ack))) => {
                            connected.store(ack.code == rv4::ConnectReturnCode::Success, Ordering::SeqCst);
                            if ack.code == rv4::ConnectReturnCode::Success {
                                first_connect = false;
                                engine.log(&id, LogLevel::Info, "connack",
                                    format!("连接成功 (session_present: {})", ack.session_present), None);
                                engine.emit(EngineEvent::Status {
                                    connection_id: id.clone(),
                                    status: ConnectionStatus::Connected,
                                    error: None,
                                    reason_code: Some(0),
                                    session_present: Some(ack.session_present),
                                });
                            } else {
                                engine.log(&id, LogLevel::Error, "connack",
                                    format!("连接被拒绝: {:?}", ack.code), None);
                                engine.emit(EngineEvent::Status {
                                    connection_id: id.clone(),
                                    status: ConnectionStatus::Error,
                                    error: Some(format!("{:?}", ack.code)),
                                    reason_code: Some(1),
                                    session_present: Some(ack.session_present),
                                });
                                break;
                            }
                        }
                        Ok(rv4::Event::Incoming(rv4::Packet::Disconnect)) => {
                            connected.store(false, Ordering::SeqCst);
                            engine.emit(EngineEvent::Status {
                                connection_id: id.clone(),
                                status: ConnectionStatus::Disconnected,
                                error: Some("服务端断开".into()),
                                reason_code: None,
                                session_present: None,
                            });
                            break;
                        }
                        Ok(rv4::Event::Incoming(rv4::Packet::SubAck(s))) => {
                            engine.log(&id, LogLevel::Info, "suback",
                                format!("订阅确认 (packet {})", s.pkid), None);
                        }
                        Ok(rv4::Event::Incoming(rv4::Packet::UnsubAck(a))) => {
                            engine.log(&id, LogLevel::Info, "unsuback",
                                format!("取消订阅确认 (packet {})", a.pkid), None);
                        }
                        Ok(rv4::Event::Incoming(rv4::Packet::PubAck(a))) => {
                            engine.log(&id, LogLevel::Debug, "puback",
                                format!("发布确认 (packet {})", a.pkid), None);
                        }
                        Ok(rv4::Event::Outgoing(rv4::Outgoing::Publish(pkid))) => {
                            engine.log(&id, LogLevel::Debug, "publish_outgoing",
                                format!("消息已发出 (packet {pkid})"), None);
                        }
                        Ok(_) => {}
                        Err(rv4::ConnectionError::RequestsDone) => {
                            connected.store(false, Ordering::SeqCst);
                            engine.emit(EngineEvent::Status {
                                connection_id: id.clone(),
                                status: ConnectionStatus::Disconnected,
                                error: None, reason_code: None, session_present: None,
                            });
                            break;
                        }
                        Err(e) => {
                            connected.store(false, Ordering::SeqCst);
                            engine.emit(EngineEvent::Status {
                                connection_id: id.clone(),
                                status: ConnectionStatus::Error,
                                error: Some(e.to_string()),
                                reason_code: None, session_present: None,
                            });
                            if !auto_reconnect || first_connect {
                                engine.log(&id, LogLevel::Error, "connection_error",
                                    format!("连接错误: {e}"), None);
                                break;
                            }
                            engine.log(&id, LogLevel::Warn, "connection_error",
                                format!("连接错误，2 秒后重连: {e}"), None);
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                    }
                }
                _ = cancel_rx.changed() => {
                    engine.log(&id, LogLevel::Info, "disconnect_cancelled", "连接已取消".into(), None);
                    break;
                }
            }
        }
        engine.conns.lock().ok().and_then(|mut m| m.remove(&id));
    });
}
