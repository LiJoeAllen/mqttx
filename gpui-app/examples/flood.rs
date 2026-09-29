//! 消息流量压测工具：连接 broker 后按指定速率向主题发布消息。
//!
//! 用法：cargo run --example flood -- <host> <port> <topic> <count> <per_second> [payload_bytes]
//! 例：向私有 broker 发 500 条 1KB 消息、每秒 50 条：
//!   cargo run --example flood -- mqtt.nex-i.cn 1883 mqttx-flood 500 50 1024

use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!("usage: flood <host> <port> <topic> <count> <per_second> [payload_bytes]");
        std::process::exit(1);
    }
    let host = args[1].clone();
    let port: u16 = args[2].parse().unwrap();
    let topic = args[3].clone();
    let count: u64 = args[4].parse().unwrap();
    let per_second: u64 = args[5].parse().unwrap();
    let payload_size: usize = args.get(6).map(|s| s.parse().unwrap()).unwrap_or(512);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        let opts = rumqttc::MqttOptions::new(
            format!("mqttx_flood_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
            rumqttc::Broker::tcp(&host, port),
        );
        let (client, mut eventloop) = rumqttc::AsyncClient::builder(opts).build();

        // eventloop 独立持续 poll：请求出队全靠它
        tokio::spawn(async move {
            loop {
                if eventloop.poll().await.is_err() {
                    break;
                }
            }
        });

        let payload = vec![b'x'; payload_size];
        let interval = Duration::from_secs_f64(1.0 / per_second.max(1) as f64);
        let start = Instant::now();
        for i in 0..count {
            let topic_i = format!("{topic}/{i}");
            let options = rumqttc::PublishOptions::new(rumqttc::QoS::AtMostOnce);
            if let Err(e) = client.publish(&topic_i, payload.clone(), options).await {
                eprintln!("publish error at {i}: {e}");
                break;
            }
            let next = start + interval * (i as u32 + 1);
            let now = Instant::now();
            if next > now {
                tokio::time::sleep(next - now).await;
            }
        }
        eprintln!(
            "flooded {count} msgs ({payload_size}B each) in {:.1}s",
            start.elapsed().as_secs_f64()
        );
        // 等网络冲刷
        tokio::time::sleep(Duration::from_secs(3)).await;
    });
}
