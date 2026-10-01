//! MQTT 5.0：MqttOptions/last-will/属性组装与事件循环泵。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rumqttc::mqttbytes::v5::{
    ConnectReturnCode as V5ReturnCode, Packet as V5Packet, Publish as V5Publish,
};
use rumqttc::{
    AsyncClient as V5AsyncClient, Broker as V5Broker, ConnectionError as V5Error,
    Event as V5Event, EventLoop as V5EventLoop, MqttOptions as V5Options, Outgoing as V5Outgoing, QoS as V5QoS, Transport as V5Transport,
};
use tokio::sync::watch;

use crate::model::{
    ConnectionConfig, ConnectionStatus, Direction, LogLevel, MqttRecord, TransportKind,
};// ─── 引擎事件 ────────────────────────────────────────────────────────────────


use super::*;

pub(super) fn build_v5(
    cfg: &ConnectionConfig,
) -> Result<(V5AsyncClient, V5EventLoop), String> {
    let custom_tls = resolve_tls_config(cfg)?;
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
            match &custom_tls {
                Some(tls) => V5Options::websocket_with_tls_config(
                    cfg.client_id.clone(),
                    url,
                    tls.clone(),
                )
                .map_err(|e| format!("{e:?}"))?,
                None => V5Options::try_websocket_with_default_tls(cfg.client_id.clone(), url)
                    .map_err(|e| format!("{e:?}"))?,
            }
        }
    };

    if cfg.transport == TransportKind::Tls {
        match &custom_tls {
            Some(tls) => {
                opts.set_transport(V5Transport::tls_with_config(tls.clone()));
            }
            None => {
                opts.set_transport(
                    V5Transport::try_tls_with_default_config()
                        .map_err(|e| format!("TLS 初始化失败: {e:?}"))?,
                );
            }
        };
    }

    // 连接超时同时覆盖底层建连与 CONNACK 等待
    let timeout = Duration::from_secs(connection_timeout_secs(cfg));
    opts.set_connect_timeout(timeout);
    let mut network_options = rumqttc::NetworkOptions::new();
    network_options.set_connection_timeout(connection_timeout_secs(cfg));
    opts.set_network_options(network_options);

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
            // v5 遗嘱属性：content_type / response_topic（3.1.1 下无此扩展）
            let properties = if will.content_type.is_some() || will.response_topic.is_some() {
                Some(rumqttc::mqttbytes::v5::LastWillProperties {
                    delay_interval: None,
                    payload_format_indicator: None,
                    message_expiry_interval: None,
                    content_type: non_blank(will.content_type.clone()),
                    response_topic: non_blank(will.response_topic.clone()),
                    correlation_data: None,
                    user_properties: Vec::new(),
                })
            } else {
                None
            };
            let lw = rumqttc::mqttbytes::v5::LastWill::new(
                will.topic.clone(),
                will.payload.as_bytes().to_vec(),
                v5_qos(will.qos),
                will.retain,
                properties,
            );
            opts.set_last_will(lw);
        }

    Ok(V5AsyncClient::builder(opts).build())
}

pub(super) fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        "/mqtt".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

pub(super) fn v5_qos(qos: u8) -> V5QoS {
    match qos {
        1 => V5QoS::AtLeastOnce,
        2 => V5QoS::ExactlyOnce,
        _ => V5QoS::AtMostOnce,
    }
}

fn v5_record(engine: &MqttEngine, seq: u64, conn_id: &str, p: &V5Publish) -> MqttRecord {
    let props = p.properties.as_ref();
    let (payload, raw_bytes, payload_truncated) = crate::model::retained_payload(&p.payload);
    MqttRecord {
        seq,
        connection_id: engine.intern(conn_id),
        direction: Direction::Received,
        topic: engine.intern(&String::from_utf8_lossy(&p.topic)),
        payload,
        qos: v5_qos_u8(p.qos),
        retain: p.retain,
        timestamp: now_ms(),
        user_properties: props
            .map(|p| Arc::from(p.user_properties.clone()))
            .unwrap_or_default(),
        content_type: props.and_then(|p| p.content_type.clone()),
        response_topic: props.and_then(|p| p.response_topic.clone()),
        correlation_data: props.and_then(|p| {
            p.correlation_data
                .as_ref()
                .map(|d| correlation_display(d))
        }),
        message_expiry_interval: props.and_then(|p| p.message_expiry_interval),
        subscription_identifier: props
            .and_then(|p| p.subscription_identifiers.first().map(|v| *v as u32)),
        payload_truncated,
        raw_bytes,
        preview: Arc::from(""),
    }
}

/// correlation data 的可读表示：优先按 UTF-8 文本展示，非文本回退 hex。
/// 发送端把字符串按字节原样发出，两侧展示保持对称（此前收端一律 hex、
/// 发端保留原文，同一条数据两侧对不上）。
fn correlation_display(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => data.iter().map(|b| format!("{b:02x}")).collect(),
    }
}

fn v5_qos_u8(qos: V5QoS) -> u8 {
    match qos {
        V5QoS::AtMostOnce => 0,
        V5QoS::AtLeastOnce => 1,
        V5QoS::ExactlyOnce => 2,
    }
}

pub(super) fn spawn_v5_loop(
    engine: Arc<MqttEngine>,
    id: String,
    mut eventloop: V5EventLoop,
    mut cancel_rx: watch::Receiver<bool>,
    connected: Arc<AtomicBool>,
    generation: u64,
    auto_reconnect: bool,
    max_reconnect_times: u32,
) {
    #![allow(clippy::too_many_arguments)]
    let runtime = engine.runtime.handle().clone();
    runtime.spawn(async move {
        let mut first_connect = true;
        // 已执行的重连次数，用于 max_reconnect_times（0=无限）限流
        let mut reconnect_attempts: u32 = 0;
        loop {
            tokio::select! {
                event = eventloop.poll() => {
                    match event {
                        Ok(V5Event::Incoming(V5Packet::Publish(p))) => {
                            let topic = String::from_utf8_lossy(&p.topic).to_string();
                            let record = v5_record(&engine, engine.next_seq(), &id, &p);
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
                                // 重连成功后清零计数：max_reconnect_times 限流的是
                                // 「连续失败次数」，不能把历史错误累计进来
                                reconnect_attempts = 0;
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
                            // max_reconnect_times>0 时按次数停止重试；0 表示无限重连
                            if max_reconnect_times > 0 && reconnect_attempts >= max_reconnect_times {
                                engine.log(&id, LogLevel::Error, "reconnect_give_up",
                                    format!("已重连 {reconnect_attempts} 次仍失败，达到最大重连次数 {max_reconnect_times}，停止重连: {e}"), None);
                                break;
                            }
                            reconnect_attempts += 1;
                            let delay = reconnect_delay(reconnect_attempts);
                            engine.log(&id, LogLevel::Warn, "connection_error",
                                format!("连接错误，{:.1} 秒后重连（第 {reconnect_attempts} 次）: {e}", delay.as_secs_f32()), None);
                            // 退避等待也必须可取消：否则 close() 的取消信号要等一整个
                            // 退避周期才可见，期间会对用户已关闭的连接继续重连。
                            tokio::select! {
                                _ = cancel_rx.changed() => break,
                                _ = tokio::time::sleep(delay) => {}
                            }
                        }
                    }
                }
                _ = cancel_rx.changed() => {
                    engine.log(&id, LogLevel::Info, "disconnect_cancelled", "连接已取消".into(), None);
                    break;
                }
            }
        }
        // 仅当 map 中仍是本代句柄时才移除，避免误删重连后的新句柄
        engine.remove_handle_if(&id, generation);
    });
}
