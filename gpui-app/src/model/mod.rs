//! 应用数据模型：按领域拆分的纯数据层（不依赖 GPUI）。
//!
//! - [`connection`] 连接配置与协议/传输枚举
//! - [`subscription`] 订阅与订阅选项
//! - [`message`] 消息记录与环形缓冲
//! - [`publish`] 发布参数与发布预设
//! - [`variable`] 全局变量与模板渲染
//! - [`log`] 运行日志
//! - [`settings`] 应用设置与主题偏好

mod connection;
mod log;
mod message;
mod publish;
mod settings;
mod subscription;
mod variable;

pub use connection::*;
pub use log::*;
pub use message::*;
pub use publish::*;
pub use settings::*;
pub use subscription::*;
pub use variable::*;

#[cfg(test)]
mod tests {
    use super::*;
    
    use std::sync::Arc;

    // 修复回归：Hex 奇数长度必须报错，不能静默截断
    #[test]
    fn hex_odd_length_is_rejected() {
        let err = PayloadFormat::Hex.encode("abc").unwrap_err();
        assert!(err.contains("偶数"), "应提示偶数长度，实际: {err}");
        assert!(PayloadFormat::Hex.encode("").is_ok());
        assert_eq!(
            PayloadFormat::Hex.encode("68 69").unwrap(),
            vec![0x68, 0x69],
            "空白应被忽略"
        );
        assert!(PayloadFormat::Hex.encode("zz").is_err());
    }

    #[test]
    fn base64_encode_roundtrip() {
        use base64::Engine as _;
        let text = "hello 世界";
        let encoded = base64::engine::general_purpose::STANDARD.encode(text);
        assert_eq!(PayloadFormat::Base64.encode(&encoded).unwrap(), text.as_bytes());
        assert!(PayloadFormat::Base64.encode("not base64!!!").is_err());
    }

    #[test]
    fn plaintext_and_json_encode_as_utf8() {
        assert_eq!(
            PayloadFormat::Plaintext.encode("中文").unwrap(),
            "中文".as_bytes()
        );
        assert_eq!(
            PayloadFormat::Json.encode(r#"{"a":1}"#).unwrap(),
            r#"{"a":1}"#.as_bytes()
        );
    }

    // 修复回归：raw_bytes 是运行时字段，不得写入持久化 JSON
    #[test]
    fn raw_bytes_not_serialized() {
        let p = PublishParams {
            topic: "t".into(),
            payload: "hi".into(),
            raw_bytes: Some(vec![0x00, 0xff]),
            ..Default::default()
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("raw_bytes"), "raw_bytes 不应被序列化: {json}");
        let back: PublishParams = serde_json::from_str(&json).unwrap();
        assert!(back.raw_bytes.is_none());
        // 反序列化时输入中的 raw_bytes 字段应被忽略（skip 双向生效）
        let mut with_junk = serde_json::to_string(&PublishParams::default()).unwrap();
        with_junk.pop(); // 去掉结尾 '}'
        with_junk.push_str(r#", "raw_bytes":[1,2]}"#);
        let back: PublishParams = serde_json::from_str(&with_junk).unwrap();
        assert!(back.raw_bytes.is_none(), "#[serde(skip)] 应忽略输入字段");
    }

    // 修复回归：同一条消息内 $ts 与 $ts_ms 基于同一时刻
    #[test]
    fn template_timestamps_share_one_timebase() {
        let out = render_template("s={{$ts}} ms={{$ts_ms}}", &[]);
        let (s_part, ms_part) = out.split_once(" ms=").expect("格式 s=.. ms=..");
        let s: i64 = s_part
            .strip_prefix("s=")
            .expect("秒字段")
            .parse()
            .unwrap();
        let ms: i64 = ms_part.parse().expect("毫秒字段");
        assert_eq!(s, ms / 1000, "秒与毫秒应来自同一时间基准: {out}");
    }

    #[test]
    fn template_unknown_placeholder_kept_and_custom_var_resolved() {
        assert_eq!(render_template("{{$unknown}}", &[]), "{{$unknown}}");
        let vars = vec![GlobalVariable {
            key: "device".into(),
            value: "sensor-1".into(),
        }];
        assert!(render_template("d={{device}}/{{$uuid}}", &vars).starts_with("d=sensor-1/"));
        // 多字节字符不被拆坏
        assert_eq!(render_template("中文{{$no}}", &[]), "中文{{$no}}");
    }

    // 连接前遗嘱模板注入：只改副本、空遗嘱不 panic、v5 属性一并渲染
    #[test]
    fn render_will_templates_injects_and_keeps_no_will_safe() {
        let vars = vec![GlobalVariable {
            key: "device".into(),
            value: "sensor-9".into(),
        }];
        // 无遗嘱：直接返回
        let mut plain = ConnectionConfig::new();
        render_will_templates(&mut plain, &vars);
        assert!(plain.last_will.is_none());

        let mut cfg = ConnectionConfig::new();
        cfg.last_will = Some(LastWill {
            topic: "alarm/{{device}}".into(),
            payload: "d={{device}}".into(),
            qos: 1,
            retain: false,
            content_type: Some("text/{{device}}".into()),
            response_topic: Some("ack/{{device}}".into()),
        });
        render_will_templates(&mut cfg, &vars);
        let w = cfg.last_will.as_ref().unwrap();
        assert_eq!(w.topic, "alarm/sensor-9");
        assert_eq!(w.payload, "d=sensor-9");
        assert_eq!(w.content_type.as_deref(), Some("text/sensor-9"));
        assert_eq!(w.response_topic.as_deref(), Some("ack/sensor-9"));
        // 原始模板串不因注入而需要回写：连接配置持久化的是模板原文，
        // 这里只验证副本注入结果；未知占位符保留
        let mut raw = ConnectionConfig::new();
        raw.last_will = Some(LastWill {
            topic: "t/{{unknown}}".into(),
            ..Default::default()
        });
        render_will_templates(&mut raw, &vars);
        assert_eq!(raw.last_will.as_ref().unwrap().topic, "t/{{unknown}}");
    }

    // 预设持久化往返：raw_bytes 不落盘，参数完整保留
    #[test]
    fn publish_preset_roundtrip_skips_raw_bytes() {
        let preset = PublishPreset {
            id: "p1".into(),
            name: "报警".into(),
            connection_id: Some("c1".into()),
            params: PublishParams {
                topic: "alarm/{{device}}".into(),
                payload: "6869".into(),
                payload_format: PayloadFormat::Hex,
                qos: 1,
                retain: true,
                content_type: Some("application/json".into()),
                user_properties: vec![("k".into(), "v".into())],
                raw_bytes: Some(vec![0x68, 0x69]),
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&preset).unwrap();
        let back: PublishPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "报警");
        assert_eq!(back.params.topic, "alarm/{{device}}");
        assert_eq!(back.params.payload_format, PayloadFormat::Hex);
        assert_eq!(back.params.qos, 1);
        assert!(back.params.retain);
        assert_eq!(back.params.content_type.as_deref(), Some("application/json"));
        assert_eq!(back.params.user_properties.len(), 1);
        assert!(back.params.raw_bytes.is_none());
    }

    #[test]
    fn payload_format_equality_and_default() {
        assert_eq!(PayloadFormat::default(), PayloadFormat::Plaintext);
        assert_ne!(PayloadFormat::Hex, PayloadFormat::Base64);
        let json = serde_json::to_string(&PayloadFormat::Hex).unwrap();
        assert_eq!(
            serde_json::from_str::<PayloadFormat>(&json).unwrap(),
            PayloadFormat::Hex
        );
    }

    // 向后兼容：旧版 connections.json 不含 Phase 1 新字段，必须能反序列化且取默认值
    #[test]
    fn legacy_connection_json_fills_new_defaults() {
        let legacy = r#"{
            "id": "c1",
            "name": "旧连接",
            "host": "broker.emqx.io",
            "port": 1883,
            "client_id": "legacy_cid",
            "keep_alive": 30
        }"#;
        let c: ConnectionConfig = serde_json::from_str(legacy).expect("旧版 JSON 应能反序列化");
        assert_eq!(c.name, "旧连接");
        assert_eq!(c.keep_alive, 30);
        assert!(c.group.is_none(), "group 默认应为 None");
        assert_eq!(c.connection_timeout_secs, 10, "连接超时默认 10 秒");
        assert_eq!(c.max_reconnect_times, 0, "最大重连次数默认 0（无限）");
        assert!(c.ssl.ca_file.is_empty());
        assert!(c.ssl.client_cert_file.is_empty());
        assert!(c.ssl.client_key_file.is_empty());
        assert!(!c.ssl.ignore_ca, "ignore_ca 默认关闭");
    }

    // 向后兼容：旧版订阅 JSON 必须默认 enabled=true，其余 v5 选项为关闭/None
    #[test]
    fn legacy_subscription_json_defaults_enabled() {
        let legacy = r#"{"id":"s1","connection_id":"c1","topic":"a/b","qos":1}"#;
        let s: Subscription = serde_json::from_str(legacy).expect("旧版订阅应能反序列化");
        assert!(s.enabled, "旧订阅默认启用");
        assert!(s.alias.is_none());
        assert!(s.sub_identifier.is_none());
        assert!(!s.no_local);
        assert!(!s.retain_as_published);
        assert_eq!(s.retain_handling, 0);
        // Subscription::new 同样产出默认启用的订阅
        assert!(Subscription::new("c1", "t", 0).enabled);
    }

    // payload 截断：上限内不截断；超出时截断且不切坏 UTF-8 字符
    #[test]
    fn retained_payload_caps_and_preserves_utf8() {
        let small = b"hello";
        let (p, raw, trunc) = retained_payload(small);
        assert_eq!(&*p, "hello");
        assert!(raw.is_none(), "合法 UTF-8 不保留原始字节");
        assert!(!trunc);

        // 中文每字符 3 字节，让上限落在字符中间（256K 不是 3 的倍数）
        let big: Vec<u8> = "中".repeat(MAX_PAYLOAD_RETAIN).into_bytes();
        let (p, _, trunc) = retained_payload(&big);
        assert!(trunc, "超过上限必须截断");
        assert!(p.len() <= MAX_PAYLOAD_RETAIN);
        assert_eq!(p.chars().last(), Some('中'), "不能切坏 UTF-8 字符");
        assert!(
            big.starts_with(p.as_bytes()),
            "截断内容必须是原数据前缀"
        );

        // 恰好上限不截断
        let exact = vec![b'a'; MAX_PAYLOAD_RETAIN];
        let (_, raw, trunc) = retained_payload(&exact);
        assert!(!trunc);
        assert!(raw.is_none());
    }

    // 非文本负载：保留原始字节供 Hex/Base64 详情，文本侧 lossy 展示
    #[test]
    fn retained_payload_keeps_raw_bytes_for_binary() {
        let binary = vec![0x00, 0xff, 0xfe, 0x01];
        let (p, raw, trunc) = retained_payload(&binary);
        assert!(!trunc);
        assert_eq!(&*p, String::from_utf8_lossy(&binary));
        let raw = raw.expect("非法 UTF-8 必须保留原始字节");
        assert_eq!(&*raw, &binary);
    }

    // Hex 编码：多字节字符不得 panic（曾按字节切片落入字符边界内而崩溃）
    #[test]
    fn hex_encode_rejects_non_hex_without_panic() {
        assert!(PayloadFormat::Hex.encode("deadBEEF").is_ok());
        assert!(PayloadFormat::Hex.encode("de ad be ef").is_ok());
        assert!(PayloadFormat::Hex.encode("abc").is_err(), "奇数长度报错");
        assert!(PayloadFormat::Hex.encode("zz").is_err(), "非法字符报错");
        // 偶数字节长度的多字节字符：修复前此处在字符边界内切片 panic
        assert!(PayloadFormat::Hex.encode("中文").is_err());
        assert!(PayloadFormat::Hex.encode("😀").is_err());
    }

    // 会话参数比较必须覆盖所有随 CONNECT 报文一次性生效的字段
    #[test]
    fn session_params_changed_covers_connect_time_fields() {
        let a = ConnectionConfig::default();
        let mut b = a.clone();
        assert!(!a.session_params_changed(&b));

        b.keep_alive = a.keep_alive + 1;
        assert!(a.session_params_changed(&b), "keep_alive 改动应触发重连提示");
        b = a.clone();

        b.last_will = Some(LastWill {
            topic: "w/t".into(),
            ..Default::default()
        });
        assert!(a.session_params_changed(&b), "遗嘱改动应触发重连提示");
        b = a.clone();

        b.ssl.ca_file = "/tmp/ca.pem".into();
        assert!(a.session_params_changed(&b), "TLS 配置改动应触发重连提示");

        // MQTT 5 的 CONNECT 属性同样随报文一次性生效（此前遗漏，导致
        // "配置已保存，重连后生效"的提示静默失效）
        b = a.clone();
        b.session_expiry_interval = a.session_expiry_interval + 1;
        assert!(a.session_params_changed(&b), "会话过期间隔改动应触发重连提示");
        b = a.clone();
        b.receive_maximum = Some(10);
        assert!(a.session_params_changed(&b), "接收上限改动应触发重连提示");
        b = a.clone();
        b.maximum_packet_size = Some(1024);
        assert!(a.session_params_changed(&b), "最大报文长度改动应触发重连提示");
        b = a.clone();
        b.topic_alias_maximum = Some(5);
        assert!(a.session_params_changed(&b), "主题别名上限改动应触发重连提示");
    }

    fn ring_record(seq: u64, payload_len: usize, raw_len: usize) -> MqttRecord {
        MqttRecord {
            seq,
            connection_id: "c".into(),
            direction: Direction::Received,
            topic: "t".into(),
            payload: Arc::from("a".repeat(payload_len)),
            qos: 0,
            retain: false,
            timestamp: 0,
            user_properties: Default::default(),
            content_type: None,
            response_topic: None,
            correlation_data: None,
            message_expiry_interval: None,
            subscription_identifier: None,
            payload_truncated: false,
            raw_bytes: if raw_len > 0 {
                Some(Arc::from(vec![0u8; raw_len]))
            } else {
                None
            },
            preview: Arc::from(""),
        }
    }

    // 消息环形缓冲：条数上限正常驱逐
    #[test]
    fn message_ring_enforces_count_limit() {
        let mut ring = MessageRing::new(4);
        for i in 0..10 {
            ring.push(ring_record(i, 64, 0), 4);
        }
        assert_eq!(ring.len(), 4);
        assert_eq!(
            ring.iter().next().map(|r| r.seq),
            Some(6),
            "应从最旧开始驱逐"
        );
        assert_eq!(ring.retained_bytes(), 4 * 64);
    }

    // 消息环形缓冲：大报文流下字节预算优先于条数上限，内存有上界
    #[test]
    fn message_ring_enforces_byte_budget() {
        let mut ring = MessageRing::new(2000);
        // 每条 256KB（payload 128KB + raw 128KB）：2000 条本会到 512MB
        let per = MAX_PAYLOAD_RETAIN;
        for i in 0..2000 {
            ring.push(ring_record(i, per, per), 2000);
        }
        assert!(
            ring.retained_bytes() <= MAX_CONNECTION_MESSAGE_BYTES + 2 * per,
            "字节数 {} 超出预算 {}",
            ring.retained_bytes(),
            MAX_CONNECTION_MESSAGE_BYTES
        );
        assert!(
            ring.len() < 2000,
            "大报文流不应保留满额条数（实际 {} 条）",
            ring.len()
        );
        assert_eq!(ring.iter().next().map(|r| r.seq), Some((2000 - ring.len()) as u64));
    }

    // 单条超预算的记录也至少保留最新一条（列表不能被清空）
    #[test]
    fn message_ring_never_empties_on_oversized_record() {
        let mut ring = MessageRing::new(10);
        ring.push(ring_record(0, MAX_PAYLOAD_RETAIN, MAX_PAYLOAD_RETAIN), 10);
        assert_eq!(ring.len(), 1);
        ring.push(ring_record(1, 64, 0), 10);
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.iter().next().map(|r| r.seq), Some(0));
    }
}
