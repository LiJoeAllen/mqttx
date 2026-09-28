//! 应用数据模型：连接配置、订阅、消息、日志、预设、全局变量、设置。
//!
//! 本层不依赖 GPUI，可被持久化层与 MQTT 引擎共用。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

// ─── 连接配置 ────────────────────────────────────────────────────────────────

/// MQTT 协议版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum ProtocolVersion {
    /// MQTT 3.1.1
    V311,
    /// MQTT 5.0
    #[default]
    V5,
}

impl ProtocolVersion {
    pub const ALL: [Self; 2] = [Self::V5, Self::V311];

    pub fn label(self) -> &'static str {
        match self {
            Self::V311 => "MQTT 3.1.1",
            Self::V5 => "MQTT 5.0",
        }
    }

    pub fn is_v5(self) -> bool {
        matches!(self, Self::V5)
    }
}


/// 传输方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TransportKind {
    /// 明文 TCP，默认 1883
    #[default]
    Tcp,
    /// TLS over TCP，默认 8883
    Tls,
    /// WebSocket (ws://)，默认 8083
    Ws,
    /// WebSocket over TLS (wss://)，默认 8084
    Wss,
}

impl TransportKind {
    pub const ALL: [Self; 4] = [Self::Tcp, Self::Tls, Self::Ws, Self::Wss];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Tls => "TLS",
            Self::Ws => "WebSocket",
            Self::Wss => "WSS",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Tcp => 1883,
            Self::Tls => 8883,
            Self::Ws => 8083,
            Self::Wss => 8084,
        }
    }

    pub fn is_tls(self) -> bool {
        matches!(self, Self::Tls | Self::Wss)
    }

    pub fn is_websocket(self) -> bool {
        matches!(self, Self::Ws | Self::Wss)
    }
}


/// MQTT 5 遗嘱消息（3.1.1 下忽略 v5 专属属性）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LastWill {
    pub topic: String,
    pub payload: String,
    pub qos: u8,
    pub retain: bool,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub response_topic: Option<String>,
}

/// SSL/TLS 自定义配置（CA、双向认证、跳过校验）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SslConfig {
    /// 自定义 CA 文件路径（PEM），作为系统根证书的补充
    #[serde(default)]
    pub ca_file: String,
    /// 客户端证书（PEM），双向认证用
    #[serde(default)]
    pub client_cert_file: String,
    /// 客户端私钥（PEM），与客户端证书配套
    #[serde(default)]
    pub client_key_file: String,
    /// 跳过服务器证书校验（insecure），仅调试环境使用
    #[serde(default)]
    pub ignore_ca: bool,
}

impl SslConfig {
    /// 是否完全未配置（走 rumqttc 默认 TLS 即可）。
    pub fn is_default(&self) -> bool {
        !self.ignore_ca
            && self.ca_file.is_empty()
            && self.client_cert_file.is_empty()
            && self.client_key_file.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    /// 主机（不含端口、协议前缀）
    pub host: String,
    pub port: u16,
    /// WebSocket 路径，如 /mqtt
    #[serde(default)]
    pub path: String,

    #[serde(default)]
    pub protocol: ProtocolVersion,
    #[serde(default)]
    pub transport: TransportKind,

    pub client_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,

    #[serde(default = "default_true")]
    pub clean_start: bool,
    #[serde(default = "default_keep_alive")]
    pub keep_alive: u16,

    // ── MQTT 5 专属 ──
    #[serde(default)]
    pub session_expiry_interval: u32,
    #[serde(default)]
    pub receive_maximum: Option<u16>,
    #[serde(default)]
    pub maximum_packet_size: Option<u32>,
    #[serde(default)]
    pub topic_alias_maximum: Option<u16>,

    /// 连接成功后自动恢复已保存的订阅
    #[serde(default = "default_true")]
    pub auto_resubscribe: bool,
    /// 断线自动重连
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,

    #[serde(default)]
    pub last_will: Option<LastWill>,

    /// 创建时间（unix 秒），用于排序
    #[serde(default)]
    pub created_at: i64,

    /// 连接分组（对标 MQTTX folder）
    #[serde(default)]
    pub group: Option<String>,
    /// 连接超时（秒），覆盖建连与 CONNACK 等待
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u16,
    /// 最大重连次数，0 = 无限重连
    #[serde(default)]
    pub max_reconnect_times: u32,
    /// SSL/TLS 自定义配置
    #[serde(default)]
    pub ssl: SslConfig,
}

fn default_true() -> bool {
    true
}
fn default_keep_alive() -> u16 {
    60
}
fn default_connection_timeout() -> u16 {
    10
}

/// 生成随机 Client ID（新建连接与表单「重新生成」共用同一逻辑）。
pub fn generate_client_id() -> String {
    format!("mqttx_{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

impl ConnectionConfig {
    pub fn new() -> Self {
        let transport = TransportKind::default();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "新建连接".into(),
            host: "broker.emqx.io".into(),
            port: transport.default_port(),
            path: "/mqtt".into(),
            protocol: ProtocolVersion::V5,
            transport,
            client_id: generate_client_id(),
            username: String::new(),
            password: String::new(),
            clean_start: true,
            keep_alive: 60,
            session_expiry_interval: 0,
            receive_maximum: None,
            maximum_packet_size: None,
            topic_alias_maximum: None,
            auto_resubscribe: true,
            auto_reconnect: true,
            last_will: None,
            created_at: chrono::Local::now().timestamp(),
            group: None,
            connection_timeout_secs: default_connection_timeout(),
            max_reconnect_times: 0,
            ssl: SslConfig::default(),
        }
    }

    /// broker.emqx.io 等公共服务器的快速预设。
    pub fn with_public(name: &str, host: &str, port: u16, transport: TransportKind) -> Self {
        let mut c = Self::new();
        c.name = name.into();
        c.host = host.into();
        c.port = port;
        c.transport = transport;
        c
    }

    pub fn display_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// 与另一份配置相比，影响 broker 会话建立的关键参数是否变化。
    /// 这些字段改动只有在重新建立连接后才会生效，用于提示用户重连。
    pub fn session_params_changed(&self, other: &ConnectionConfig) -> bool {
        self.host != other.host
            || self.port != other.port
            || self.transport != other.transport
            || self.protocol != other.protocol
            || self.client_id != other.client_id
            || self.username != other.username
            || self.password != other.password
            || self.clean_start != other.clean_start
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 订阅 ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub connection_id: String,
    pub topic: String,
    pub qos: u8,
    /// 本地备注颜色（与 MQTTX 一致，按订阅给消息着色），0..=359 色相
    #[serde(default)]
    pub color: Option<f32>,
    /// 订阅别名（对标 MQTTX topic alias），仅用于界面展示
    #[serde(default)]
    pub alias: Option<String>,
    /// 是否启用该订阅；禁用后界面置灰、恢复订阅时跳过
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// MQTT 5 订阅标识符
    #[serde(default)]
    pub sub_identifier: Option<u32>,
    /// MQTT 5 No Local (NL)
    #[serde(default)]
    pub no_local: bool,
    /// MQTT 5 Retain As Published (RAP)
    #[serde(default)]
    pub retain_as_published: bool,
    /// MQTT 5 Retain Handling (0/1/2)
    #[serde(default)]
    pub retain_handling: u8,
}

impl Subscription {
    pub fn new(connection_id: impl Into<String>, topic: impl Into<String>, qos: u8) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            connection_id: connection_id.into(),
            topic: topic.into(),
            qos,
            color: None,
            alias: None,
            enabled: true,
            sub_identifier: None,
            no_local: false,
            retain_as_published: false,
            retain_handling: 0,
        }
    }
}

/// MQTT 5 订阅选项（NL/RAP/Retain Handling/订阅标识符）；v4 连接会忽略这些选项。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribeOptions {
    /// 订阅标识符，0 视为未设置
    #[serde(default)]
    pub sub_identifier: Option<u32>,
    #[serde(default)]
    pub no_local: bool,
    #[serde(default)]
    pub retain_as_published: bool,
    /// 0=每次订阅都发送保留消息，1=仅新订阅发送，2=不发送
    #[serde(default)]
    pub retain_handling: u8,
}

impl From<&Subscription> for SubscribeOptions {
    fn from(sub: &Subscription) -> Self {
        Self {
            sub_identifier: sub.sub_identifier,
            no_local: sub.no_local,
            retain_as_published: sub.retain_as_published,
            retain_handling: sub.retain_handling,
        }
    }
}

// ─── 消息 ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Received,
    Published,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Received => "接收",
            Self::Published => "发布",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PayloadFormat {
    #[default]
    Plaintext,
    Json,
    Base64,
    Hex,
}

impl PayloadFormat {
    pub const ALL: [Self; 4] = [Self::Plaintext, Self::Json, Self::Base64, Self::Hex];

    pub fn label(self) -> &'static str {
        match self {
            Self::Plaintext => "Plaintext",
            Self::Json => "JSON",
            Self::Base64 => "Base64",
            Self::Hex => "Hex",
        }
    }

    pub fn encode(self, text: &str) -> Result<Vec<u8>, String> {
        match self {
            Self::Plaintext | Self::Json => Ok(text.as_bytes().to_vec()),
            Self::Base64 => {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD
                    .decode(text.trim())
                    .map_err(|e| format!("Base64 解码失败: {e}"))
            }
            Self::Hex => {
                let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                if !cleaned.len().is_multiple_of(2) {
                    return Err("Hex 长度必须为偶数（每两个字符一个字节）".into());
                }
                (0..cleaned.len())
                    .step_by(2)
                    .map(|i| {
                        u8::from_str_radix(&cleaned[i..i + 2], 16)
                            .map_err(|e| format!("Hex 解码失败: {e}"))
                    })
                    .collect()
            }
        }
    }
}


/// 一条收到/发出的消息记录。payload 以 UTF-8 文本保存（非文本按 lossy 显示）。
///
/// 内存优化：`connection_id`/`topic` 用 `Arc<str>`——同一连接内主题高度重复，
/// 驻留后数千条记录只保留一份字符串。
#[derive(Debug, Clone)]
pub struct MqttRecord {
    pub seq: u64,
    pub connection_id: Arc<str>,
    pub direction: Direction,
    pub topic: Arc<str>,
    /// Arc 共享：消息列表渲染每帧克隆最多数百条记录，零拷贝
    pub payload: Arc<str>,
    pub qos: u8,
    pub retain: bool,
    /// unix 毫秒
    pub timestamp: i64,
    pub user_properties: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub response_topic: Option<String>,
    pub correlation_data: Option<String>,
    pub message_expiry_interval: Option<u32>,
    pub subscription_identifier: Option<u32>,
    /// payload 超过 [`MAX_PAYLOAD_RETAIN`] 被截断
    pub payload_truncated: bool,
}

/// 消息历史中单条 payload 的保留上限。超限截断，避免个别大报文
/// 长期占据历史缓冲（MQTTX 亦有类似限制）。
pub const MAX_PAYLOAD_RETAIN: usize = 256 * 1024;

/// 把原始 payload 字节转成用于历史记录的共享文本，超限截断。
/// 返回 (Arc<str>, 是否截断)。截断时回退到 UTF-8 字符边界。
pub fn retained_payload(raw: &[u8]) -> (Arc<str>, bool) {
    if raw.len() <= MAX_PAYLOAD_RETAIN {
        (Arc::from(String::from_utf8_lossy(raw).as_ref()), false)
    } else {
        let mut end = MAX_PAYLOAD_RETAIN;
        while end > 0 && (raw[end] & 0xC0) == 0x80 {
            end -= 1;
        }
        (
            Arc::from(String::from_utf8_lossy(&raw[..end]).as_ref()),
            true,
        )
    }
}

// ─── 发布参数 / 发布预设 ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublishParams {
    pub topic: String,
    /// 用于界面展示与记录的文本负载。
    pub payload: String,
    #[serde(default)]
    pub payload_format: PayloadFormat,
    pub qos: u8,
    pub retain: bool,
    /// MQTT 5 用户属性
    #[serde(default)]
    pub user_properties: Vec<(String, String)>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub message_expiry_interval: Option<u32>,
    #[serde(default)]
    pub response_topic: Option<String>,
    #[serde(default)]
    pub correlation_data: Option<String>,
    /// 实际发送的字节。Base64/Hex 时为解码结果；不持久化、不参与预设。
    /// None 时引擎回退为 `payload.as_bytes()`。
    #[serde(skip)]
    pub raw_bytes: Option<Vec<u8>>,
}

/// 发布预设：保存常用的发布参数，一键复用（对标 MQTTX Publish Scripts/Preset）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishPreset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(flatten)]
    pub params: PublishParams,
}

impl PublishPreset {
    pub fn new(name: impl Into<String>, params: PublishParams) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            connection_id: None,
            params,
        }
    }
}

// ─── 全局变量 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalVariable {
    pub key: String,
    pub value: String,
}

/// 解析 `{{key}}` 模板：
/// - 用户定义的全局变量优先；
/// - 内置变量：`{{$ts}}`（unix 秒）、`{{$ts_ms}}`（毫秒）、`{{$uuid}}`；
/// - 找不到的占位符保持原样。
pub fn render_template(input: &str, vars: &[GlobalVariable]) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    // 同一次渲染内时间基准保持一致，避免一条消息里多个时间变量取值不同。
    let now_ms = chrono::Local::now().timestamp_millis();
    let now_s = now_ms / 1000;
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{'
            && let Some(rel_end) = input[i + 2..].find("}}") {
                let key = &input[i + 2..i + 2 + rel_end];
                let resolved = match key.trim() {
                    "$ts" => Some(now_s.to_string()),
                    "$ts_ms" => Some(now_ms.to_string()),
                    "$uuid" => Some(uuid::Uuid::new_v4().to_string()),
                    k => vars
                        .iter()
                        .find(|v| v.key == k)
                        .map(|v| v.value.clone()),
                };
                match resolved {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&input[i..i + 2 + rel_end + 2]),
                }
                i = i + 2 + rel_end + 2;
                continue;
            }
        // 按字符推进，避免把多字节 UTF-8 拆开
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 连接前渲染遗嘱消息中的 `{{变量}}`。
/// 只作用于本次连接的配置副本，落盘的连接配置保持模板原文，
/// 这样改全局变量后下次重连即生效，`$ts` 等也按连接时刻求值。
pub fn render_will_templates(cfg: &mut ConnectionConfig, vars: &[GlobalVariable]) {
    let Some(will) = cfg.last_will.as_mut() else {
        return;
    };
    will.topic = render_template(&will.topic, vars);
    will.payload = render_template(&will.payload, vars);
    if let Some(v) = will.content_type.take() {
        will.content_type = Some(render_template(&v, vars));
    }
    if let Some(v) = will.response_topic.take() {
        will.response_topic = Some(render_template(&v, vars));
    }
}

// ─── 日志 ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "debug" => Self::Debug,
            "warn" => Self::Warn,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: i64,
    pub connection_id: String,
    pub level: LogLevel,
    pub event: String,
    pub message: String,
    pub details: Option<String>,
}

// ─── 连接状态 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Error,
}

impl ConnectionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "未连接",
            Self::Connecting => "连接中",
            Self::Connected => "已连接",
            Self::Error => "错误",
        }
    }
}

// ─── 应用设置 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeModePref {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemeModePref {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "跟随系统",
            Self::Light => "浅色",
            Self::Dark => "深色",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub theme: ThemeModePref,
    /// 每条连接在内存中保留的最大消息条数
    #[serde(default = "default_max_messages")]
    pub max_messages: usize,
    /// 时间戳是否显示毫秒
    #[serde(default = "default_true")]
    pub show_millis: bool,
    /// 是否自动检查更新（占位，暂不实现更新）
    #[serde(default = "default_true")]
    pub auto_check_update: bool,
}

fn default_max_messages() -> usize {
    2000
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeModePref::default(),
            max_messages: default_max_messages(),
            show_millis: true,
            auto_check_update: true,
        }
    }
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        let (p, trunc) = retained_payload(small);
        assert_eq!(&*p, "hello");
        assert!(!trunc);

        // 中文每字符 3 字节，让上限落在字符中间（256K 不是 3 的倍数）
        let big: Vec<u8> = "中".repeat(MAX_PAYLOAD_RETAIN).into_bytes();
        let (p, trunc) = retained_payload(&big);
        assert!(trunc, "超过上限必须截断");
        assert!(p.len() <= MAX_PAYLOAD_RETAIN);
        assert_eq!(p.chars().last(), Some('中'), "不能切坏 UTF-8 字符");
        assert!(
            big.starts_with(p.as_bytes()),
            "截断内容必须是原数据前缀"
        );

        // 恰好上限不截断
        let exact = vec![b'a'; MAX_PAYLOAD_RETAIN];
        let (_, trunc) = retained_payload(&exact);
        assert!(!trunc);
    }
}
