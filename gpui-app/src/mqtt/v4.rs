//! MQTT 3.1.1：MqttOptions/last-will 组装与事件循环泵。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rumqttc::mqttbytes::v5::{
    ConnectReturnCode as V5ReturnCode, Packet as V5Packet,
};
use rumqttc::Event as V5Event;
use rumqttc_v4 as rv4;
use tokio::sync::watch;

use crate::model::{
    ConnectionConfig, ConnectionStatus, Direction, LogLevel, MqttRecord, TransportKind,
};// ─── 引擎事件 ────────────────────────────────────────────────────────────────


use super::*;

pub(super) fn build_v4(
    cfg: &ConnectionConfig,
) -> Result<(rv4::AsyncClient, rv4::EventLoop), String> {
    use rv4::{Broker as V4Broker, MqttOptions as V4Options, Transport as V4Transport};

    let custom_tls = resolve_tls_config(cfg)?;
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
            match &custom_tls {
                Some(tls) => V4Options::websocket_with_tls_config(
                    cfg.client_id.clone(),
                    url,
                    tls.clone(),
                )
                .map_err(|e| format!("{e:?}"))?,
                None => V4Options::try_websocket_with_default_tls(cfg.client_id.clone(), url)
                    .map_err(|e| format!("{e:?}"))?,
            }
        }
    };

    if cfg.transport == TransportKind::Tls {
        match &custom_tls {
            Some(tls) => {
                opts.set_transport(V4Transport::tls_with_config(tls.clone()));
            }
            None => {
                opts.set_transport(
                    V4Transport::try_tls_with_default_config()
                        .map_err(|e| format!("TLS 初始化失败: {e:?}"))?,
                );
            }
        };
    }

    opts.set_keep_alive(cfg.keep_alive);
    opts.set_clean_session(cfg.clean_start);
    if !cfg.username.is_empty() {
        opts.set_credentials(cfg.username.clone(), cfg.password.clone());
    }
    if let Some(will) = &cfg.last_will
        && !will.topic.is_empty() {
            // 3.1.1 的遗嘱没有 content_type / response_topic 属性，忽略
            let lw = rv4::mqttbytes::v4::LastWill::new(
                will.topic.clone(),
                will.payload.as_bytes().to_vec(),
                v4_qos(will.qos),
                will.retain,
            );
            opts.set_last_will(lw);
        }

    let (client, mut eventloop) = rv4::AsyncClient::builder(opts).build();
    // v4 的连接超时（含 CONNACK 等待）由 EventLoop 的 NetworkOptions 控制
    let mut network_options = rv4::NetworkOptions::new();
    network_options.set_connection_timeout(connection_timeout_secs(cfg));
    eventloop.set_network_options(network_options);
    Ok((client, eventloop))
}

pub(super) fn v4_qos(qos: u8) -> rv4::QoS {
    match qos {
        1 => rv4::QoS::AtLeastOnce,
        2 => rv4::QoS::ExactlyOnce,
        _ => rv4::QoS::AtMostOnce,
    }
}

// ─── 测试连接握手 ────────────────────────────────────────────────────────────

pub(super) async fn test_handshake(cfg_in: &ConnectionConfig) -> Result<(), String> {
    // 握手测试必须使用独立 client_id，否则会把使用相同 client_id 的现有连接
    // 从 broker 顶下线（MQTT 同 client_id 单会话规则）。
    let mut cfg = cfg_in.clone();
    cfg.client_id = format!("mqttx_test_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let cfg = &cfg;
    if cfg.protocol.is_v5() {
        // 建 client 会读 CA/证书文件（阻塞 IO）：与 connect 保持一致放 spawn_blocking，
        // 避免占住 2 线程引擎 runtime 的 worker
        let owned = cfg.clone();
        let (client, mut eventloop) = tokio::task::spawn_blocking(move || build_v5(&owned))
            .await
            .map_err(|e| format!("内部任务失败: {e}"))??;
        loop {
            match eventloop.poll().await {
                Ok(V5Event::Incoming(V5Packet::ConnAck(ack))) => {
                    if ack.code == V5ReturnCode::Success {
                        // 测试完成，主动断开避免在 broker 侧残留会话。
                        // disconnect() 只是把请求入队，需再 poll 才会把
                        // DISCONNECT 真正写上网络。
                        let _ = client.disconnect().await;
                        for _ in 0..3 {
                            if eventloop.poll().await.is_err() {
                                break;
                            }
                        }
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
        // 同上：v4 分支同样把阻塞的建连放到阻塞线程池
        let owned = cfg.clone();
        let (client, mut eventloop) = tokio::task::spawn_blocking(move || build_v4(&owned))
            .await
            .map_err(|e| format!("内部任务失败: {e}"))??;
        loop {
            match eventloop.poll().await {
                Ok(rv4::Event::Incoming(rv4::Packet::ConnAck(ack))) => {
                    if ack.code == rv4::ConnectReturnCode::Success {
                        // 同 v5：disconnect() 只入队，需 poll 把报文冲出
                        let _ = client.disconnect().await;
                        for _ in 0..3 {
                            if eventloop.poll().await.is_err() {
                                break;
                            }
                        }
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

pub(super) fn spawn_v4_loop(
    engine: Arc<MqttEngine>,
    id: String,
    mut eventloop: rv4::EventLoop,
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
                        Ok(rv4::Event::Incoming(rv4::Packet::Publish(p))) => {
                            let (payload, raw_bytes, payload_truncated) =
                                crate::model::retained_payload(&p.payload);
                            let record = MqttRecord {
                                seq: engine.next_seq(),
                                connection_id: engine.intern(&id),
                                direction: Direction::Received,
                                topic: engine.intern(&String::from_utf8_lossy(&p.topic)),
                                payload,
                                qos: match p.qos {
                                    rv4::QoS::AtMostOnce => 0,
                                    rv4::QoS::AtLeastOnce => 1,
                                    rv4::QoS::ExactlyOnce => 2,
                                },
                                retain: p.retain,
                                timestamp: now_ms(),
                                user_properties: Default::default(),
                                content_type: None,
                                response_topic: None,
                                correlation_data: None,
                                message_expiry_interval: None,
                                subscription_identifier: None,
                                payload_truncated,
                                raw_bytes,
                                preview: Arc::from(""),
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
                                // 重连成功后清零计数：max_reconnect_times 限流的是
                                // 「连续失败次数」，不能把历史错误累计进来
                                reconnect_attempts = 0;
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
                                    // 上报真实的 CONNACK 拒绝码，不再硬编码为 1
                                    reason_code: Some(ack.code as u8),
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
