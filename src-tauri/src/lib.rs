use rumqttc::mqttbytes::v5::{
    ConnectReturnCode, Packet, Publish, PublishProperties,
};
use rumqttc::{
    AsyncClient, Broker, ConnectionError, Event, EventLoop, MqttOptions, Outgoing,
    PublishOptions,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{RwLock, watch};

// ─── DTO structs ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConnectionDto {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub client_id: String,
    pub clean_start: bool,
    pub session_expiry_interval: u32,
    pub keep_alive: u64,
    pub ssl: bool,
    pub receive_maximum: Option<u16>,
    pub maximum_packet_size: Option<u32>,
    pub topic_alias_maximum: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishDto {
    pub connection_id: String,
    pub topic: String,
    pub payload: String,
    pub qos: u8,
    pub retain: bool,
    pub user_properties: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub message_expiry_interval: Option<u32>,
    pub response_topic: Option<String>,
    pub correlation_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeDto {
    pub connection_id: String,
    pub topic: String,
    pub qos: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct MqttMessageEvent {
    pub connection_id: String,
    pub topic: String,
    pub payload: String,
    pub qos: u8,
    pub retain: bool,
    pub timestamp: i64,
    pub reason_code: Option<u8>,
    pub user_properties: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub content_encoding: Option<String>,
    pub response_topic: Option<String>,
    pub correlation_data: Option<String>,
    pub message_expiry_interval: Option<u32>,
    pub subscription_identifier: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MqttStatusEvent {
    pub connection_id: String,
    pub status: String,
    pub error: Option<String>,
    pub session_present: Option<bool>,
    pub reason_code: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MqttLogEvent {
    pub timestamp: i64,
    pub connection_id: String,
    pub level: String,
    pub event: String,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatus {
    pub id: String,
    pub connected: bool,
    pub error: Option<String>,
}

// ─── Log file writer ────────────────────────────────────────────────────────────

struct LogWriter {
    dir: PathBuf,
}

impl LogWriter {
    fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }

    fn date_from_ts(ts: i64) -> (i32, u32, u32, u32, u32, u32, u32) {
        // Unix time to UTC date
        let mut s = ts / 1000;
        let ms = (ts % 1000).unsigned_abs() as u32;

        // Days since epoch
        let mut d = (s / 86400) as i32;
        s %= 86400;

        let h = (s / 3600) as u32;
        s %= 3600;
        let m = (s / 60) as u32;
        let sec = (s % 60) as u32;

        // Date from days since 1970-01-01
        let mut y = 1970i32;
        loop {
            let days_in_year = if Self::is_leap(y) { 366 } else { 365 };
            if d < days_in_year { break; }
            d -= days_in_year;
            y += 1;
        }
        let (mo, day) = Self::month_day(y, d as i32);
        (y, mo, day, h, m, sec, ms as u32)
    }

    fn is_leap(y: i32) -> bool {
        (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
    }

    fn month_day(y: i32, d: i32) -> (u32, u32) {
        let days = [31, if Self::is_leap(y) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut remaining = d;
        for (i, &dim) in days.iter().enumerate() {
            if remaining < dim {
                return ((i + 1) as u32, (remaining + 1) as u32);
            }
            remaining -= dim;
        }
        (12, 31)
    }

    fn write(&self, timestamp: i64, level: &str, event: &str, connection_id: &str, message: &str, details: Option<&str>) {
        let (y, mo, d, h, mi, s, ms) = Self::date_from_ts(timestamp);
        let time_str = format!("{:02}:{:02}:{:02}.{:03}", h, mi, s, ms);
        let file_date = format!("{:04}-{:02}-{:02}", y, mo, d);
        let log_file = self.dir.join(format!("mqttx-{}.log", file_date));

        let line = format!(
            "[{}] [{}] [{}] [{}] {}{}\n",
            time_str,
            level,
            event,
            connection_id,
            message,
            details.map(|d| format!(" | {}", d)).unwrap_or_default()
        );

        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .open(&log_file)
            .and_then(|f| {
                use std::io::Write;
                let mut f = f;
                f.write_all(line.as_bytes())
            });
    }
}

// ─── State management ──────────────────────────────────────────────────────────

struct ClientHandle {
    client: AsyncClient,
    cancel_tx: watch::Sender<bool>,
    connected: Arc<AtomicBool>,
}

struct AppState {
    clients: RwLock<HashMap<String, ClientHandle>>,
    log_writer: Option<LogWriter>,
}

fn qos_to_u8(qos: rumqttc::QoS) -> u8 {
    match qos {
        rumqttc::QoS::AtMostOnce => 0,
        rumqttc::QoS::AtLeastOnce => 1,
        rumqttc::QoS::ExactlyOnce => 2,
    }
}

fn qos_from_u8(qos: u8) -> rumqttc::QoS {
    match qos {
        0 => rumqttc::QoS::AtMostOnce,
        1 => rumqttc::QoS::AtLeastOnce,
        2 => rumqttc::QoS::ExactlyOnce,
        _ => rumqttc::QoS::AtMostOnce,
    }
}

fn now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn emit_log(app_handle: &AppHandle, connection_id: &str, level: &str, event: &str, message: String, details: Option<String>) {
    let ts = now_timestamp();
    let log = MqttLogEvent {
        timestamp: ts,
        connection_id: connection_id.to_string(),
        level: level.to_string(),
        event: event.to_string(),
        message: message.clone(),
        details: details.clone(),
    };
    let _ = app_handle.emit("mqtt:log", &log);

    // Also write to log file
    if let Some(state) = app_handle.try_state::<AppState>() {
        if let Some(writer) = &state.log_writer {
            writer.write(ts, level, event, connection_id, &message, details.as_deref());
        }
    }
}

fn extract_publish_info(publish: &Publish) -> MqttMessageEvent {
    let payload_str = String::from_utf8_lossy(&publish.payload).to_string();
    let props = publish.properties.as_ref();

    MqttMessageEvent {
        connection_id: String::new(), // will be filled by caller
        topic: String::from_utf8_lossy(&publish.topic).to_string(),
        payload: payload_str,
        qos: qos_to_u8(publish.qos),
        retain: publish.retain,
        timestamp: now_timestamp(),
        reason_code: None,
        user_properties: props
            .map(|p| {
                p.user_properties
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        content_type: props.and_then(|p| p.content_type.clone()),
        content_encoding: None,
        response_topic: props.and_then(|p| p.response_topic.clone()),
        correlation_data: props
            .and_then(|p| p.correlation_data.as_ref().map(|d| {
                d.iter().map(|b| format!("{:02x}", b)).collect::<String>()
            })),
        message_expiry_interval: props.and_then(|p| p.message_expiry_interval),
        subscription_identifier: props
            .and_then(|p| p.subscription_identifiers.first().copied().map(|v| v as u32)),
    }
}

fn describe_publish_properties(props: &Option<PublishProperties>) -> String {
    props.as_ref().map(|p| {
        let mut parts = Vec::new();
        if let Some(ct) = &p.content_type {
            parts.push(format!("content_type: {ct}"));
        }
        if let Some(rt) = &p.response_topic {
            parts.push(format!("response_topic: {rt}"));
        }
        if let Some(cd) = &p.correlation_data {
            parts.push(format!("correlation_data: {} bytes", cd.len()));
        }
        if let Some(mei) = p.message_expiry_interval {
            parts.push(format!("expiry: {mei}s"));
        }
        if let Some(ta) = p.topic_alias {
            parts.push(format!("topic_alias: {ta}"));
        }
        if !p.subscription_identifiers.is_empty() {
            parts.push(format!("sub_ids: {:?}", p.subscription_identifiers));
        }
        if let Some(ps) = p.payload_format_indicator {
            parts.push(format!("payload_format: {ps}"));
        }
        if !p.user_properties.is_empty() {
            let ups: Vec<String> = p.user_properties.iter()
                .take(5)
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            parts.push(format!("user_properties: [{}]", ups.join(", ")));
        }
        parts.join(", ")
    }).unwrap_or_default()
}

// ─── Background event loop task ────────────────────────────────────────────────

fn spawn_event_loop(
    connection_id: String,
    mut eventloop: EventLoop,
    mut cancel_rx: watch::Receiver<bool>,
    connected: Arc<AtomicBool>,
    app_handle: AppHandle,
) {
    let conn_id = connection_id.clone();
    let ah = app_handle.clone();
    emit_log(&ah, &conn_id, "info", "event_loop_started", "MQTT 事件循环已启动".to_string(), None);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                event = eventloop.poll() => {
                    match event {
                        Ok(Event::Incoming(Packet::Publish(publish))) => {
                            let topic = String::from_utf8_lossy(&publish.topic).to_string();
                            let payload_len = publish.payload.len();
                            let payload_preview = String::from_utf8_lossy(&publish.payload).to_string().chars().take(200).collect::<String>();
                            let props_desc = describe_publish_properties(&publish.properties);
                            emit_log(&app_handle, &connection_id, "info", "publish_received",
                                format!("收到消息: {topic} ({} 字节, QoS {})", payload_len, qos_to_u8(publish.qos)),
                                Some(format!("payload: {}{}\n{}",
                                    payload_preview,
                                    if payload_len > 200 { format!("... (共 {} 字节)", payload_len) } else { String::new() },
                                    props_desc)),
                            );
                            let mut msg = extract_publish_info(&publish);
                            msg.connection_id = connection_id.clone();
                            let _ = app_handle.emit("mqtt:message", &msg);
                        }

                        Ok(Event::Incoming(Packet::ConnAck(connack))) => {
                            connected.store(true, Ordering::SeqCst);
                            let reason_code = connack.code as u8;
                            let is_success = connack.code == ConnectReturnCode::Success;
                            if is_success {
                                let props_desc = connack.properties.as_ref().map(|p| {
                                    format!("assigned_client_id: {:?}, topic_alias_max: {:?}, server_keepalive: {:?}, receive_max: {:?}",
                                        p.assigned_client_identifier,
                                        p.topic_alias_max,
                                        p.server_keep_alive,
                                        p.receive_max)
                                }).unwrap_or_default();
                                emit_log(&app_handle, &connection_id, "info", "connack",
                                    format!("连接成功 (session_present: {})", connack.session_present),
                                    Some(props_desc).filter(|d| !d.is_empty()),
                                );
                            } else {
                                emit_log(&app_handle, &connection_id, "error", "connack",
                                    format!("连接被拒绝: {:?} (code: {})", connack.code, reason_code), None);
                            }
                            let status_event = MqttStatusEvent {
                                connection_id: connection_id.clone(),
                                status: "connected".to_string(),
                                error: None,
                                session_present: Some(connack.session_present),
                                reason_code: Some(reason_code),
                            };
                            let _ = app_handle.emit("mqtt:status", &status_event);
                        }

                        Ok(Event::Incoming(Packet::Disconnect(disconnect))) => {
                            connected.store(false, Ordering::SeqCst);
                            let reason_code = disconnect.reason_code as u8;
                            let reason_desc = format!("{:?}", disconnect.reason_code);
                            let props_info = disconnect.properties.as_ref().map(|p| {
                                let mut parts = Vec::new();
                                if let Some(ref s) = p.session_expiry_interval { parts.push(format!("session_expiry: {s}")); }
                                if let Some(ref s) = p.reason_string { parts.push(format!("reason: {s}")); }
                                if let Some(ref s) = p.server_reference { parts.push(format!("server_ref: {s}")); }
                                for (k, v) in p.user_properties.iter().take(3) {
                                    parts.push(format!("{k}: {v}"));
                                }
                                parts.join(", ")
                            }).unwrap_or_default();
                            emit_log(&app_handle, &connection_id, "warn", "disconnect_received",
                                format!("收到断开连接: {reason_desc} (code: {reason_code})"),
                                Some(props_info).filter(|d| !d.is_empty()),
                            );
                            let status_event = MqttStatusEvent {
                                connection_id: connection_id.clone(),
                                status: "disconnected".to_string(),
                                error: Some(format!("Disconnect reason: {reason_desc}")),
                                session_present: None,
                                reason_code: Some(reason_code),
                            };
                            let _ = app_handle.emit("mqtt:status", &status_event);
                            break;
                        }

                        Ok(Event::Incoming(Packet::SubAck(suback))) => {
                            let codes: Vec<String> = suback.return_codes.iter().map(|rc| {
                                match rc {
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::Success(qos) => format!("成功(QoS {})", qos_to_u8(*qos)),
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::Unspecified => "未指定错误(0x80)".to_string(),
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::ImplementationSpecific => "实现特定错误(0x81)".to_string(),
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::NotAuthorized => "未授权(0x83)".to_string(),
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::TopicFilterInvalid => "主题过滤无效(0x82)".to_string(),
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::QuotaExceeded => "配额超限(0x97)".to_string(),
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::SharedSubscriptionsNotSupported => "不支持共享订阅(0x9E)".to_string(),
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::SubscriptionIdNotSupported => "不支持订阅标识符(0xA1)".to_string(),
                                    rumqttc::mqttbytes::v5::SubscribeReasonCode::WildcardSubscriptionsNotSupported => "不支持通配符订阅(0xA2)".to_string(),
                                    _ => format!("未知({:?})", rc),
                                }
                            }).collect();
                            let any_failure = suback.return_codes.iter().any(|rc| !matches!(rc, rumqttc::mqttbytes::v5::SubscribeReasonCode::Success(_)));
                            let level = if any_failure { "warn" } else { "info" };
                            emit_log(&app_handle, &connection_id, level, "suback",
                                format!("订阅确认 (packet_id: {}): {}", suback.pkid, codes.join(", ")),
                                None,
                            );
                        }

                        Ok(Event::Incoming(Packet::UnsubAck(unsuback))) => {
                            let reasons: Vec<String> = unsuback.reasons.iter().map(|r| {
                                match r {
                                    rumqttc::mqttbytes::v5::UnsubAckReason::Success => "成功".to_string(),
                                    rumqttc::mqttbytes::v5::UnsubAckReason::NoSubscriptionExisted => "无此订阅".to_string(),
                                    rumqttc::mqttbytes::v5::UnsubAckReason::UnspecifiedError => "未指定错误".to_string(),
                                    rumqttc::mqttbytes::v5::UnsubAckReason::ImplementationSpecificError => "实现特定错误".to_string(),
                                    rumqttc::mqttbytes::v5::UnsubAckReason::NotAuthorized => "未授权".to_string(),
                                    rumqttc::mqttbytes::v5::UnsubAckReason::TopicFilterInvalid => "主题过滤无效".to_string(),
                                    rumqttc::mqttbytes::v5::UnsubAckReason::PacketIdentifierInUse => "包标识符使用中".to_string(),
                                }
                            }).collect();
                            emit_log(&app_handle, &connection_id, "info", "unsuback",
                                format!("取消订阅确认: {}", reasons.join(", ")), None);
                        }

                        Ok(Event::Incoming(Packet::PubAck(puback))) => {
                            let reason = format!("{:?}", puback.reason);
                            let details = puback.properties.as_ref().map(|p| {
                                let mut parts = Vec::new();
                                if let Some(ref s) = p.reason_string { parts.push(format!("reason: {s}")); }
                                for (k, v) in p.user_properties.iter().take(3) {
                                    parts.push(format!("{k}: {v}"));
                                }
                                parts.join(", ")
                            }).unwrap_or_default();
                            emit_log(&app_handle, &connection_id, "info", "puback",
                                format!("发布确认 (packet_id: {}, reason: {})", puback.pkid, reason),
                                Some(details).filter(|d| !d.is_empty()),
                            );
                        }

                        Ok(Event::Incoming(Packet::PubRec(pubrec))) => {
                            let reason = format!("{:?}", pubrec.reason);
                            emit_log(&app_handle, &connection_id, "info", "pubrec",
                                format!("QoS2 发布收到 (packet_id: {}, reason: {})", pubrec.pkid, reason), None);
                        }

                        Ok(Event::Incoming(Packet::PubRel(pubrel))) => {
                            emit_log(&app_handle, &connection_id, "info", "pubrel",
                                format!("QoS2 发布释放 (packet_id: {})", pubrel.pkid), None);
                        }

                        Ok(Event::Incoming(Packet::PubComp(pubcomp))) => {
                            let reason = format!("{:?}", pubcomp.reason);
                            emit_log(&app_handle, &connection_id, "info", "pubcomp",
                                format!("QoS2 发布完成 (packet_id: {}, reason: {})", pubcomp.pkid, reason), None);
                        }

                        Ok(Event::Incoming(Packet::PingResp)) => {
                            // Ping responses are too noisy to log every time
                        }

                        Ok(Event::Incoming(Packet::Auth(auth))) => {
                            let method = auth.properties.as_ref()
                                .and_then(|p| p.method.as_ref())
                                .cloned()
                                .unwrap_or_default();
                            emit_log(&app_handle, &connection_id, "info", "auth",
                                format!("认证交换 (reason: {:?}, method: {method})", auth.code), None);
                        }

                        // ─── Outgoing events (packet IDs only) ──────────────────────

                        Ok(Event::Outgoing(Outgoing::PingReq)) => {
                            // Ping requests are too noisy
                        }

                        Ok(Event::Outgoing(Outgoing::Publish(pkid))) => {
                            emit_log(&app_handle, &connection_id, "info", "publish_sent",
                                format!("发送消息 (packet_id: {pkid})"), None);
                        }

                        Ok(Event::Outgoing(Outgoing::Subscribe(pkid))) => {
                            emit_log(&app_handle, &connection_id, "info", "subscribe_sent",
                                format!("发送订阅请求 (packet_id: {pkid})"), None);
                        }

                        Ok(Event::Outgoing(Outgoing::Unsubscribe(pkid))) => {
                            emit_log(&app_handle, &connection_id, "info", "unsubscribe_sent",
                                format!("发送取消订阅请求 (packet_id: {pkid})"), None);
                        }

                        Ok(Event::Outgoing(Outgoing::PubAck(pkid))) => {
                            emit_log(&app_handle, &connection_id, "info", "puback_sent",
                                format!("发送发布确认 (packet_id: {pkid})"), None);
                        }

                        Ok(Event::Outgoing(Outgoing::PubRec(pkid))) => {
                            emit_log(&app_handle, &connection_id, "info", "pubrec_sent",
                                format!("发送QoS2发布收到 (packet_id: {pkid})"), None);
                        }

                        Ok(Event::Outgoing(Outgoing::PubRel(pkid))) => {
                            emit_log(&app_handle, &connection_id, "info", "pubrel_sent",
                                format!("发送QoS2发布释放 (packet_id: {pkid})"), None);
                        }

                        Ok(Event::Outgoing(Outgoing::PubComp(pkid))) => {
                            emit_log(&app_handle, &connection_id, "info", "pubcomp_sent",
                                format!("发送QoS2发布完成 (packet_id: {pkid})"), None);
                        }

                        Ok(Event::Outgoing(Outgoing::PingResp)) => {
                            // Ping responses are too noisy
                        }

                        Ok(Event::Outgoing(Outgoing::AwaitAck(pkid))) => {
                            // AwaitAck is internal, log it
                            emit_log(&app_handle, &connection_id, "info", "await_ack",
                                format!("等待应答 (packet_id: {pkid})"), None);
                        }

                        Ok(Event::Outgoing(Outgoing::Auth)) => {
                            emit_log(&app_handle, &connection_id, "info", "auth_sent",
                                "发送认证请求".to_string(), None);
                        }

                        Ok(Event::Outgoing(Outgoing::Disconnect)) => {
                            connected.store(false, Ordering::SeqCst);
                            emit_log(&app_handle, &connection_id, "info", "disconnect_sent",
                                "已发送断开连接请求".to_string(), None);
                            let status_event = MqttStatusEvent {
                                connection_id: connection_id.clone(),
                                status: "disconnected".to_string(),
                                error: None,
                                session_present: None,
                                reason_code: None,
                            };
                            let _ = app_handle.emit("mqtt:status", &status_event);
                            break;
                        }

                        Err(ConnectionError::RequestsDone) => {
                            connected.store(false, Ordering::SeqCst);
                            emit_log(&app_handle, &connection_id, "info", "requests_done",
                                "所有请求已完成，连接关闭".to_string(), None);
                            let status_event = MqttStatusEvent {
                                connection_id: connection_id.clone(),
                                status: "disconnected".to_string(),
                                error: None,
                                session_present: None,
                                reason_code: None,
                            };
                            let _ = app_handle.emit("mqtt:status", &status_event);
                            break;
                        }

                        Err(e) => {
                            connected.store(false, Ordering::SeqCst);
                            emit_log(&app_handle, &connection_id, "error", "connection_error",
                                format!("连接错误: {e}"), None);
                            let status_event = MqttStatusEvent {
                                connection_id: connection_id.clone(),
                                status: "error".to_string(),
                                error: Some(e.to_string()),
                                session_present: None,
                                reason_code: None,
                            };
                            let _ = app_handle.emit("mqtt:status", &status_event);
                            break;
                        }

                        Ok(Event::Incoming(other)) => {
                            emit_log(&app_handle, &connection_id, "info", "incoming_other",
                                format!("未处理入站事件: {:?}", std::mem::discriminant(&other)), None);
                        }

                        Ok(Event::Auth(auth_event)) => {
                            match auth_event {
                                rumqttc::AuthEvent::Started { method, .. } => {
                                    emit_log(&app_handle, &connection_id, "info", "auth_started",
                                        format!("认证开始 (method: {method})"), None);
                                }
                                rumqttc::AuthEvent::Continue { method, .. } => {
                                    emit_log(&app_handle, &connection_id, "info", "auth_continue",
                                        format!("认证继续 (method: {method})"), None);
                                }
                                rumqttc::AuthEvent::Succeeded { method, .. } => {
                                    emit_log(&app_handle, &connection_id, "info", "auth_succeeded",
                                        format!("认证成功 (method: {method})"), None);
                                }
                                rumqttc::AuthEvent::Failed { method, reason, .. } => {
                                    emit_log(&app_handle, &connection_id, "warn", "auth_failed",
                                        format!("认证失败 (method: {method}, reason: {reason:?})"), None);
                                }
                            }
                        }
                    }
                }

                _ = cancel_rx.changed() => {
                    connected.store(false, Ordering::SeqCst);
                    emit_log(&app_handle, &connection_id, "info", "disconnect_cancelled",
                        "用户取消连接".to_string(), None);
                    let status_event = MqttStatusEvent {
                        connection_id: connection_id.clone(),
                        status: "disconnected".to_string(),
                        error: None,
                        session_present: None,
                        reason_code: None,
                    };
                    let _ = app_handle.emit("mqtt:status", &status_event);
                    break;
                }
            }
        }
    });
}

// ─── Tauri Commands ────────────────────────────────────────────────────────────

#[tauri::command]
async fn mqtt_connect(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
    conn: MqttConnectionDto,
) -> Result<(), String> {
    let broker = Broker::tcp(&conn.host, conn.port);
    let mut mqttopts = MqttOptions::new(&conn.client_id, broker);
    mqttopts.set_keep_alive(conn.keep_alive as u16);
    mqttopts.set_clean_start(conn.clean_start);

    if conn.session_expiry_interval > 0 {
        mqttopts.set_session_expiry_interval(Some(conn.session_expiry_interval));
    }

    if !conn.username.is_empty() {
        mqttopts.set_credentials(conn.username.clone(), conn.password.clone());
    }

    if let Some(rm) = conn.receive_maximum {
        mqttopts.set_receive_maximum(Some(rm));
    }
    if let Some(mps) = conn.maximum_packet_size {
        mqttopts.set_max_packet_size(Some(mps));
    }
    if let Some(tam) = conn.topic_alias_maximum {
        mqttopts.set_topic_alias_max(Some(tam));
    }

    // Cancel existing connection if any
    {
        let clients = state.clients.read().await;
        if let Some(handle) = clients.get(&conn.id) {
            let _ = handle.cancel_tx.send(true);
        }
    }

    let (client, eventloop) = AsyncClient::builder(mqttopts).build();
    let connected = Arc::new(AtomicBool::new(false));
    let (cancel_tx, cancel_rx) = watch::channel(false);

    let handle = ClientHandle {
        client: client.clone(),
        cancel_tx,
        connected: connected.clone(),
    };

    {
        let mut clients = state.clients.write().await;
        clients.insert(conn.id.clone(), handle);
    }

    // Emit connecting status
    let status_event = MqttStatusEvent {
        connection_id: conn.id.clone(),
        status: "connecting".to_string(),
        error: None,
        session_present: None,
        reason_code: None,
    };
    let _ = app_handle.emit("mqtt:status", &status_event);

    emit_log(&app_handle, &conn.id, "info", "connect",
        format!("正在连接 {0}:{1}", conn.host, conn.port), None);

    // Spawn background task
    spawn_event_loop(conn.id, eventloop, cancel_rx, connected, app_handle);

    Ok(())
}

#[tauri::command]
async fn mqtt_test_connection(conn: MqttConnectionDto) -> Result<String, String> {
    use tokio::time::timeout;

    let test_id = format!("{}_test_{}", conn.client_id, std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis());

    let broker = Broker::tcp(&conn.host, conn.port);
    let mut mqttopts = MqttOptions::new(&test_id, broker);
    mqttopts.set_keep_alive(10);
    mqttopts.set_clean_start(true);

    if !conn.username.is_empty() {
        mqttopts.set_credentials(conn.username.clone(), conn.password.clone());
    }

    let (client, mut eventloop) = AsyncClient::builder(mqttopts).build();

    let result = timeout(std::time::Duration::from_secs(5), async {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(connack))) => {
                    if connack.code == ConnectReturnCode::Success {
                        return Ok("连接测试成功！".to_string());
                    } else {
                        return Err(format!("连接被拒绝: {:?}", connack.code));
                    }
                }
                Ok(Event::Incoming(Packet::Disconnect(disconnect))) => {
                    return Err(format!("连接被拒绝: {:?}", disconnect.reason_code));
                }
                Err(e) => {
                    return Err(format!("连接失败: {}", e));
                }
                _ => {}
            }
        }
    }).await;

    // Drop client to close the connection
    drop(client);

    match result {
        Ok(r) => r,
        Err(_) => Err("连接测试超时（5秒），请检查主机地址和端口是否正确".to_string()),
    }
}

#[tauri::command]
async fn mqtt_disconnect(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<(), String> {
    let mut clients = state.clients.write().await;
    if let Some(handle) = clients.remove(&connection_id) {
        emit_log(&app_handle, &connection_id, "info", "disconnect",
            "用户请求断开连接".to_string(), None);
        let _ = handle.cancel_tx.send(true);
        Ok(())
    } else {
        emit_log(&app_handle, &connection_id, "warn", "disconnect",
            "断开连接失败: 连接不存在".to_string(), None);
        Err("Connection not found".to_string())
    }
}

#[tauri::command]
async fn mqtt_publish(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
    publish: PublishDto,
) -> Result<(), String> {
    let clients = state.clients.read().await;
    let handle = clients
        .get(&publish.connection_id)
        .ok_or_else(|| "Connection not found".to_string())?;

    let qos = qos_from_u8(publish.qos);
    let payload = publish.payload.into_bytes();

    let mut props = PublishProperties::default();
    props.user_properties = publish.user_properties;
    props.content_type = publish.content_type.clone();
    props.message_expiry_interval = publish.message_expiry_interval;
    props.response_topic = publish.response_topic.clone();
    props.correlation_data = publish
        .correlation_data
        .map(|s| s.into_bytes().into());

    emit_log(&app_handle, &publish.connection_id, "info", "publish_sent",
        format!("发布消息到 {topic}: {size} 字节 (QoS {qos}, retain={retain})",
            topic = publish.topic,
            size = payload.len(),
            qos = publish.qos,
            retain = publish.retain,
        ),
        Some(format!("payload: {}", String::from_utf8_lossy(&payload).to_string().chars().take(200).collect::<String>())),
    );

    let options = PublishOptions::new(qos)
        .retain(publish.retain)
        .properties(props);

    handle
        .client
        .publish(publish.topic, payload, options)
        .await
        .map_err(|e| {
            emit_log(&app_handle, &publish.connection_id, "error", "publish_error",
                format!("发布失败: {e}"), None);
            e.to_string()
        })?;

    Ok(())
}

#[tauri::command]
async fn mqtt_subscribe(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
    subscribe: SubscribeDto,
) -> Result<(), String> {
    let clients = state.clients.read().await;
    let handle = clients
        .get(&subscribe.connection_id)
        .ok_or_else(|| "Connection not found".to_string())?;

    let qos = qos_from_u8(subscribe.qos);

    emit_log(&app_handle, &subscribe.connection_id, "info", "subscribe",
        format!("订阅主题: {topic} (QoS {qos})", topic = subscribe.topic, qos = subscribe.qos), None);

    // Use tracked subscribe to wait for SubAck result
    let notice = handle
        .client
        .subscribe_tracked(subscribe.topic, qos)
        .await
        .map_err(|e| {
            emit_log(&app_handle, &subscribe.connection_id, "error", "subscribe_error",
                format!("订阅请求失败: {e}"), None);
            e.to_string()
        })?;

    // Wait for SubAck and check return codes
    notice.wait_completion_async().await.map_err(|e| {
        let err_msg = format!("订阅被拒绝: {e}");
        emit_log(&app_handle, &subscribe.connection_id, "error", "subscribe_rejected",
            err_msg.clone(), None);
        err_msg
    })?;

    Ok(())
}

#[tauri::command]
async fn mqtt_unsubscribe(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
    connection_id: String,
    topic: String,
) -> Result<(), String> {
    let clients = state.clients.read().await;
    let handle = clients
        .get(&connection_id)
        .ok_or_else(|| "Connection not found".to_string())?;

    emit_log(&app_handle, &connection_id, "info", "unsubscribe",
        format!("取消订阅主题: {topic}"), None);

    handle
        .client
        .unsubscribe(topic)
        .await
        .map_err(|e| {
            emit_log(&app_handle, &connection_id, "error", "unsubscribe_error",
                format!("取消订阅失败: {e}"), None);
            e.to_string()
        })?;

    Ok(())
}

#[tauri::command]
async fn mqtt_get_connections(state: tauri::State<'_, AppState>) -> Result<Vec<ConnectionStatus>, String> {
    let clients = state.clients.read().await;
    Ok(clients
        .iter()
        .map(|(id, handle)| ConnectionStatus {
            id: id.clone(),
            connected: handle.connected.load(Ordering::SeqCst),
            error: None,
        })
        .collect())
}

#[tauri::command]
async fn mqtt_get_connection_status(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<ConnectionStatus, String> {
    let clients = state.clients.read().await;
    clients
        .get(&connection_id)
        .map(|handle| ConnectionStatus {
            id: connection_id,
            connected: handle.connected.load(Ordering::SeqCst),
            error: None,
        })
        .ok_or_else(|| "Connection not found".to_string())
}

// ─── App entry ─────────────────────────────────────────────────────────────────

#[tauri::command]
fn mqtt_get_log_dir(app_handle: AppHandle) -> Result<String, String> {
    let dir = app_handle.path().app_data_dir()
        .map_err(|e| format!("获取日志目录失败: {e}"))?
        .join("logs");
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_writer = std::env::current_dir()
        .ok()
        .map(|p| p.join("logs"))
        .or_else(|| {
            // Fallback: use a temp directory
            std::env::temp_dir().join("mqttx-logs").into()
        })
        .map(|dir| LogWriter::new(dir));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            clients: RwLock::new(HashMap::new()),
            log_writer,
        })
        .invoke_handler(tauri::generate_handler![
            mqtt_connect,
            mqtt_disconnect,
            mqtt_publish,
            mqtt_subscribe,
            mqtt_unsubscribe,
            mqtt_get_connections,
            mqtt_get_connection_status,
            mqtt_test_connection,
            mqtt_get_log_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}