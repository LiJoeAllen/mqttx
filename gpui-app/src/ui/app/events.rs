//! 引擎事件泵：把 MqttEngine 的状态/消息/日志事件写入应用状态。

use std::sync::Arc;

use gpui_kit::component::{ notification::Notification,  };
use gpui_kit::{ Context, Window,  };

use crate::model::{ ConnectionStatus, Direction, LogEntry, LogLevel, MqttRecord, MessageRing, SubscribeOptions, Subscription, retained_payload,  };
use crate::mqtt::EngineEvent;

use super::*;
use crate::ui::i18n;

impl MqttXApp {
pub(super) fn on_engine_event(&mut self, event: EngineEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            EngineEvent::Status {
                connection_id,
                status,
                error,
                ..
            } => {
                // 引擎 close 与事件循环退出之间有窗口，删除连接后其排队
                // 事件仍会送达：为已删除的连接重建状态/消息属于泄漏，
                // 一律丢弃（删除时已主动 close）。
                if !self.connections.iter().any(|c| c.id == connection_id) {
                    return;
                }
                self.statuses.insert(connection_id.clone(), status);
                match error {
                    Some(e) => {
                        self.errors.insert(connection_id.clone(), e);
                    }
                    None => {
                        self.errors.remove(&connection_id);
                    }
                }
                if status == ConnectionStatus::Connected {
                    self.on_connected(&connection_id);
                }
                if let Some(view) = self.views.get(&connection_id) {
                    view.update(cx, |_, cx| cx.notify());
                }
                cx.notify();
            }
            EngineEvent::Message(mut record) => {
                let id = record.connection_id.clone();
                if !self.connections.iter().any(|c| c.id == id.as_ref()) {
                    return;
                }
                record.seq = self.next_seq();
                self.push_message(id.clone(), record);
                self.notify_message_view(&id, cx);
            }
            EngineEvent::Published {
                connection_id,
                topic,
                payload,
                qos,
                retain,
                content_type,
                user_properties,
                response_topic,
                correlation_data,
                message_expiry_interval,
            } => {
                if !self.connections.iter().any(|c| c.id == connection_id) {
                    return;
                }
                let (payload, raw_bytes, payload_truncated) = retained_payload(&payload);
                let record = MqttRecord {
                    seq: self.next_seq(),
                    connection_id: Arc::from(connection_id.as_str()),
                    direction: Direction::Published,
                    topic: Arc::from(topic.as_str()),
                    payload,
                    qos,
                    retain,
                    timestamp: chrono::Local::now().timestamp_millis(),
                    user_properties: Arc::from(user_properties),
                    content_type,
                    response_topic,
                    correlation_data,
                    message_expiry_interval,
                    subscription_identifier: None,
                    payload_truncated,
                    raw_bytes,
                    preview: Arc::from(""),
                };
                self.push_message(Arc::from(connection_id.as_str()), record);
                self.notify_message_view(&connection_id, cx);
            }
            EngineEvent::SubscribeResult {
                connection_id,
                topic,
                qos,
                ok,
                error,
            } => {
                if !self.connections.iter().any(|c| c.id == connection_id) {
                    return;
                }
                if ok {
                    // 乐观更新已在发送前入库；这里只在订阅仍存在时同步 QoS，
                    // 避免 SUBACK 前被删除的订阅被此事件用默认字段复活。
                    if let Some(existing) = self.subscriptions.iter_mut().find(|s| {
                        s.connection_id == connection_id && s.topic == topic
                    }) {
                        existing.qos = qos;
                    }
                } else {
                    if let Some(e) = error {
                        window.push_notification(
                            Notification::error(i18n::tf(
                                "sub.failed",
                                &[("e", &e)],
                            )),
                            cx,
                        );
                    }
                    // broker 拒绝订阅：移除乐观插入的订阅行，与引擎状态保持一致
                    // （不发 UNSUBSCRIBE——本就未订阅成功）
                    self.subscriptions
                        .retain(|s| !(s.connection_id == connection_id && s.topic == topic));
                    self.storage.save_subscriptions(&self.subscriptions);
                }
                if let Some(view) = self.views.get(&connection_id) {
                    view.update(cx, |_, cx| cx.notify());
                }
            }
            EngineEvent::Log(entry) => {
                self.push_log(entry);
                if let Some(id) = self.active_tab.clone()
                    && let Some(view) = self.views.get(&id) {
                        view.update(cx, |_, cx| cx.notify());
                    }
            }
        }
    }

    fn on_connected(&mut self, connection_id: &str) {
        // 连接成功后自动恢复已保存订阅（auto resubscribe），禁用的订阅跳过。
        let Some(config) = self.connections.iter().find(|c| c.id == connection_id).cloned() else {
            return;
        };
        if !config.auto_resubscribe {
            return;
        }
        let subs: Vec<Subscription> = self
            .subscriptions
            .iter()
            .filter(|s| s.connection_id == connection_id)
            .filter(|s| s.enabled)
            .cloned()
            .collect();
        for sub in subs {
            let opts = SubscribeOptions::from(&sub);
            self.engine
                .subscribe_with_options(connection_id.to_string(), sub, opts);
        }
    }

    // ── 数据工具 ──────────────────────────────────────────────────────────

    /// 消息到达后刷新对应工作区；用户按下「暂停」时跳过，
    /// 避免高频消息流不断把正在阅读的内容顶走（消息仍照常入环形缓冲）。
    fn notify_message_view(&self, connection_id: &str, cx: &mut Context<Self>) {
        if let Some(view) = self.views.get(connection_id)
            && !view.read(cx).is_paused()
        {
            view.update(cx, |_, cx| cx.notify());
        }
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn push_message(&mut self, connection_id: Arc<str>, mut record: MqttRecord) {
        // 折叠态预览在此唯一入口计算一次；渲染热路径逐帧直接取用
        record.compute_preview();
        let cap = self.settings.max_messages;
        self.messages
            .entry(connection_id)
            .or_insert_with(|| MessageRing::new(cap))
            .push(record, cap);
    }

    /// 清空指定连接的消息（内存缓冲随 drop 释放）。
    pub fn clear_messages(&mut self, id: &str) {
        self.messages.remove(id);
    }

    fn push_log(&mut self, entry: LogEntry) {
        // 落盘只在事件路径执行（不在 render 热路径），与内存环形缓冲互不影响
        self.write_log_file(&entry);
        if self.logs.len() >= MAX_LOGS {
            self.logs.pop_front();
        }
        self.logs.push_back(Arc::new(entry));
    }

    /// 追加写入按日切分的日志文件 `mqttx-YYYY-MM-DD.log`。
    /// 文件句柄按日期缓存复用，只有跨日或写失败后才重新打开；
    /// 失败只在首次 eprintln 一次后保持静默（成功一次即复位，便于恢复后再次提示），
    /// 且绝不回调 log/push_log（防止递归触发日志）。
    fn write_log_file(&mut self, entry: &LogEntry) {
use std::sync::atomic::{ AtomicBool, Ordering };

        static WRITE_FAILED: AtomicBool = AtomicBool::new(false);

        match self.append_log_line(entry) {
            Ok(()) => WRITE_FAILED.store(false, Ordering::Relaxed),
            Err(e) => {
                // 关句柄置空，下次写入自动重开重试
                self.log_file = None;
                if !WRITE_FAILED.swap(true, Ordering::Relaxed) {
                    eprintln!("[app] 写入日志文件失败（后续静默）: {e}");
                }
            }
        }
    }

    /// 打开（或复用按日缓存的）日志文件并追加一行。
    /// 文件套 BufWriter：高频消息流下每条日志不再触发多次落盘 syscall；
    /// 跨日重开/出错关句柄时随 drop 自动 flush。
    fn append_log_line(&mut self, entry: &LogEntry) -> std::io::Result<()> {
        use std::io::Write as _;
        use chrono::TimeZone as _;

        let ts = chrono::Local
            .timestamp_millis_opt(entry.timestamp)
            .single()
            .unwrap_or_else(chrono::Local::now);
        let day = ts.format("%Y-%m-%d").to_string();
        let need_reopen = match &self.log_file {
            Some((cached_day, _)) => *cached_day != day,
            None => true,
        };
        if need_reopen {
            let dir = self.storage.log_dir();
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("mqttx-{day}.log"));
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            self.log_file = Some((day, std::io::BufWriter::new(file)));
        }
        // 连接名查不到时回退为 id，保证行格式恒定
        let conn = self
            .connections
            .iter()
            .find(|c| c.id == entry.connection_id)
            .map(|c| c.name.as_str())
            .unwrap_or(entry.connection_id.as_str());
        let file = &mut self
            .log_file
            .as_mut()
            .expect("上方已确保日志句柄存在")
            .1;
        writeln!(
            file,
            "{} [{}] {}/{}: {}",
            ts.format("%Y-%m-%d %H:%M:%S%.3f"),
            entry.level.label(),
            conn,
            entry.event,
            entry.message
        )
    }

    pub fn log(&mut self, conn: &str, level: LogLevel, event: &str, message: String) {
        self.push_log(LogEntry {
            timestamp: chrono::Local::now().timestamp_millis(),
            connection_id: conn.to_string(),
            level,
            event: event.to_string(),
            message,
            details: None,
        });
    }

    // ── 连接 CRUD / 生命周期 ──────────────────────────────────────────────
}
