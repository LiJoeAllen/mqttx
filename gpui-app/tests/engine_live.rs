//! 真实 MQTT 引擎连通性测试：连接公共 broker，订阅并验证消息回环。
//! 默认忽略；设置 MQTTX_LIVE=1 并加 --ignored 运行（依赖外网）。

use std::time::{Duration, Instant};

use mqttx_desktop::model::{
    ConnectionConfig, ConnectionStatus, ProtocolVersion, PublishParams, Subscription, TransportKind,
};
use mqttx_desktop::mqtt::{EngineEvent, MqttEngine};

#[test]
#[ignore = "需外网：MQTTX_LIVE=1 cargo test --test engine_live -- --ignored"]
fn v5_tcp_pubsub_roundtrip() {
    let (tx, rx) = smol::channel::unbounded();
    let engine = MqttEngine::new(tx);

    let mut cfg = ConnectionConfig::new();
    cfg.name = "live-v5".into();
    cfg.host = "broker.emqx.io".into();
    cfg.port = 1883;
    cfg.transport = TransportKind::Tcp;
    cfg.protocol = ProtocolVersion::V5;
    cfg.auto_reconnect = false;
    cfg.auto_resubscribe = false;
    let cid = cfg.id.clone();
    let topic = format!("mqttx/gpui-test/{}/{}", cid, uuid::Uuid::new_v4());

    engine.connect(cfg);

    assert!(wait_connected(&rx), "未能在 15 秒内连接 broker.emqx.io");

    engine.subscribe(cid.clone(), Subscription::new(cid.clone(), topic.clone(), 0));
    assert!(wait_subscribe(&rx, &topic), "订阅未确认");

    // 等 broker 传播订阅状态
    std::thread::sleep(Duration::from_millis(500));

    engine.publish(
        cid.clone(),
        PublishParams {
            topic: topic.clone(),
            payload: "hello-gpui".into(),
            qos: 0,
            ..Default::default()
        },
    );

    assert!(wait_message(&rx, &topic), "未收到自己发布的消息（回环失败）");
}

#[test]
#[ignore = "需外网：MQTTX_LIVE=1 cargo test --test engine_live -- --ignored"]
fn v311_tcp_connect() {
    let (tx, rx) = smol::channel::unbounded();
    let engine = MqttEngine::new(tx);

    let mut cfg = ConnectionConfig::new();
    cfg.host = "broker.emqx.io".into();
    cfg.port = 1883;
    cfg.protocol = ProtocolVersion::V311;
    cfg.auto_reconnect = false;
    cfg.auto_resubscribe = false;

    engine.connect(cfg);
    assert!(wait_connected(&rx), "MQTT 3.1.1 连接失败");
}

fn pump<F: FnMut(&EngineEvent) -> bool>(
    rx: &smol::channel::Receiver<EngineEvent>,
    timeout: Duration,
    mut pred: F,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.try_recv() {
            Ok(ev) => {
                if pred(&ev) {
                    return true;
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(30)),
        }
    }
    false
}

fn wait_connected(rx: &smol::channel::Receiver<EngineEvent>) -> bool {
    pump(rx, Duration::from_secs(15), |ev| match ev {
        EngineEvent::Status {
            status: ConnectionStatus::Connected,
            ..
        } => true,
        EngineEvent::Status {
            status: ConnectionStatus::Error,
            error,
            ..
        } => panic!("连接错误: {error:?}"),
        _ => false,
    })
}

fn wait_subscribe(rx: &smol::channel::Receiver<EngineEvent>, topic: &str) -> bool {
    pump(rx, Duration::from_secs(8), |ev| match ev {
        EngineEvent::SubscribeResult { topic: t, ok, .. } => t == topic && *ok,
        _ => false,
    })
}

fn wait_message(rx: &smol::channel::Receiver<EngineEvent>, topic: &str) -> bool {
    pump(rx, Duration::from_secs(10), |ev| match ev {
        EngineEvent::Message(m) => m.topic == topic && m.payload == "hello-gpui",
        _ => false,
    })
}
