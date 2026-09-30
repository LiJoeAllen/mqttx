//! 连接配置：协议版本、传输方式、遗嘱、TLS 与连接参数。


use serde::{Deserialize, Serialize};



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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

// Debug 手写而非 derive：`{:?}` 一次格式化就会把 `password` 送进 stderr 与 Sentry
//（panic 钩子会连同载荷一起上报），见文件底部的实现。
#[derive(Clone, Serialize, Deserialize)]
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
    /// 应用启动时自动连接该连接
    #[serde(default)]
    pub auto_connect: bool,

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

pub(super) fn default_true() -> bool {
    true
}
pub(super) fn default_keep_alive() -> u16 {
    60
}
pub(super) fn default_connection_timeout() -> u16 {
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
            auto_connect: false,
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
    /// WS 路径、遗嘱、TLS 配置、keep_alive 随 CONNECT 报文一次性生效；
    /// MQTT 5 的四个 CONNECT 属性（会话过期/接收上限/最大报文/主题别名）同样如此，
    /// 全部纳入比较——漏掉的字段会让"重连后生效"提示静默失效。
    pub fn session_params_changed(&self, other: &ConnectionConfig) -> bool {
        self.host != other.host
            || self.port != other.port
            || self.path != other.path
            || self.transport != other.transport
            || self.protocol != other.protocol
            || self.client_id != other.client_id
            || self.username != other.username
            || self.password != other.password
            || self.clean_start != other.clean_start
            || self.keep_alive != other.keep_alive
            || self.last_will != other.last_will
            || self.ssl != other.ssl
            || self.session_expiry_interval != other.session_expiry_interval
            || self.receive_maximum != other.receive_maximum
            || self.maximum_packet_size != other.maximum_packet_size
            || self.topic_alias_maximum != other.topic_alias_maximum
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// 脱敏 Debug：`password` 以 `<redacted>` 输出。
/// 该结构在日志/panic 场景下可能被整体格式化（`{:?}`），derive 出的 Debug 会把
/// 明文密码带进 stderr 与 Sentry（main.rs 的 panic 钩子会上报），因此必须手写。
impl std::fmt::Debug for ConnectionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionConfig")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("path", &self.path)
            .field("protocol", &self.protocol)
            .field("transport", &self.transport)
            .field("client_id", &self.client_id)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("clean_start", &self.clean_start)
            .field("keep_alive", &self.keep_alive)
            .field("auto_resubscribe", &self.auto_resubscribe)
            .field("auto_reconnect", &self.auto_reconnect)
            .field("auto_connect", &self.auto_connect)
            .field("has_last_will", &self.last_will.is_some())
            .field("group", &self.group)
            .field("ssl", &"<redacted>")
            .finish_non_exhaustive()
    }
}

// ─── 订阅 ────────────────────────────────────────────────────────────────────
