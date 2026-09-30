//! 阿里云一键接入预设（两种鉴权模式对应两款产品）。
//!
//! Token 模式（云消息队列 MQTT 版）鉴权参数：
//! - 接入点: `{instanceId}.mqtt.aliyuncs.com`
//! - clientId: `{GroupId}@@@{DeviceId}`
//! - username: `Signature|{AccessKeyId}|{InstanceId}`
//! - password: base64(HMAC-SHA1(AccessKeySecret, clientId))
//!
//! 一机一密模式（物联网平台 MQTT 直连，官方规范）：
//! - 接入点: `{productKey}.iot-as-mqtt.{region}.aliyuncs.com`
//! - clientId: `{ProductKey}.{DeviceName}|securemode=2,signmethod=hmacsha256,timestamp={ts}|`
//! - username: `{DeviceName}&{ProductKey}`
//! - password: HMAC-SHA256(DeviceSecret, "clientId{ProductKey}.{DeviceName}deviceName{DeviceName}productKey{ProductKey}timestamp{ts}")

use base64::Engine as _;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::Sha256;

use crate::model::{ConnectionConfig, ProtocolVersion, TransportKind};

type HmacSha1 = Hmac<Sha1>;
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AliyunAuthMode {
    /// Group ID + 设备 ID，使用账号 AccessKey 签名
    #[default]
    Token,
    /// 一机一密：设备独立 DeviceName + DeviceSecret（直接以设备密钥鉴权）
    DeviceCredential,
}

impl AliyunAuthMode {
    pub const ALL: [Self; 2] = [Self::Token, Self::DeviceCredential];

    pub fn label(self) -> &'static str {
        match self {
            Self::Token => "Group 令牌 (Token)",
            Self::DeviceCredential => "一机一密",
        }
    }
}

// Debug 手写脱敏：AccessKeySecret 与 DeviceSecret 是长期凭据，一次 `{:?}` 格式化
// 就会把它们送进 stderr 与 Sentry（panic 钩子会一并上报），因此不能 derive。
#[derive(Clone, Serialize, Deserialize)]
pub struct AliyunPreset {
    pub id: String,
    pub name: String,
    /// 分组（仅 UI 分类用）
    #[serde(default)]
    pub group: String,
    /// 实例 ID，如 post-cn-4591dq2ra1i
    pub instance_id: String,
    /// 地域 host 后缀前的 region，如 cn-shanghai；留空则使用公共接入点
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub auth_mode: AliyunAuthMode,

    // Token 模式
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub access_key_secret: String,
    #[serde(default)]
    pub group_id: String,

    // 共用：Token 模式下是 DeviceId；一机一密模式下是 DeviceName
    pub device_id: String,

    // 一机一密模式
    #[serde(default)]
    pub product_key: String,
    #[serde(default)]
    pub device_secret: String,
}

impl AliyunPreset {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "阿里云设备".into(),
            group: String::new(),
            instance_id: String::new(),
            region: String::new(),
            auth_mode: AliyunAuthMode::Token,
            access_key_id: String::new(),
            access_key_secret: String::new(),
            group_id: String::new(),
            device_id: format!("device_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
            product_key: String::new(),
            device_secret: String::new(),
        }
    }

    /// HMAC-SHA1(secret, client_id) 的 Base64 编码（Token 模式签名）。
    pub fn signature(secret: &str, client_id: &str) -> Result<String, String> {
        let mut mac =
            HmacSha1::new_from_slice(secret.as_bytes()).map_err(|e| format!("HMAC 初始化失败: {e}"))?;
        mac.update(client_id.as_bytes());
        Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
    }

    /// HMAC-SHA256(secret, msg) 的小写 hex（一机一密签名）。
    fn hmac_sha256_hex(secret: &str, msg: &str) -> Result<String, String> {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| format!("HMAC 初始化失败: {e}"))?;
        mac.update(msg.as_bytes());
        Ok(mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }

    /// 接入点 host。
    /// Token 模式（云消息队列 MQTT 版）：`{instanceId}.mqtt.aliyuncs.com`。
    /// 一机一密（物联网平台）：`{productKey}.iot-as-mqtt.{region}.aliyuncs.com`，
    /// region 留空时用默认区域 cn-shanghai。
    pub fn host(&self) -> String {
        match self.auth_mode {
            AliyunAuthMode::DeviceCredential => {
                let region = if self.region.is_empty() {
                    "cn-shanghai"
                } else {
                    &self.region
                };
                format!("{}.iot-as-mqtt.{}.aliyuncs.com", self.product_key, region)
            }
            // 实例级接入点不带 region 段；region 字段仅作 UI 备注保留
            AliyunAuthMode::Token => format!("{}.mqtt.aliyuncs.com", self.instance_id),
        }
    }

    /// 由预设生成一份连接配置（不写入 id/name 之外的覆盖项）。
    pub fn to_connection(&self) -> Result<ConnectionConfig, String> {
        let mut conn = ConnectionConfig::new();
        conn.name = self.name.clone();
        conn.port = TransportKind::Tcp.default_port();
        conn.transport = TransportKind::Tcp;
        conn.protocol = ProtocolVersion::V311;
        conn.clean_start = true;

        match self.auth_mode {
            AliyunAuthMode::Token => {
                if self.group_id.is_empty()
                    || self.access_key_id.is_empty()
                    || self.access_key_secret.is_empty()
                {
                    return Err("Group ID、AccessKey ID 与 AccessKey Secret 不能为空".into());
                }
                // 空 DeviceName 会拼出 "group@@@" 这样的非法 clientId，服务端报错难懂
                if self.device_id.trim().is_empty() {
                    return Err("Device ID 不能为空".into());
                }
                conn.host = self.host();
                let client_id = format!("{}@@@{}", self.group_id, self.device_id);
                let password = Self::signature(&self.access_key_secret, &client_id)?;
                conn.client_id = client_id;
                conn.username =
                    format!("Signature|{}|{}", self.access_key_id, self.instance_id);
                conn.password = password;
            }
            AliyunAuthMode::DeviceCredential => {
                if self.product_key.is_empty() || self.device_secret.is_empty() {
                    return Err("一机一密需要 ProductKey 与 DeviceSecret (设备密钥)".into());
                }
                // 空 DeviceName 会拼出 "pk.|securemode=..." 与 "&pk"，服务端报错难懂
                if self.device_id.trim().is_empty() {
                    return Err("一机一密需要 DeviceName（设备名）".into());
                }
                conn.host = self.host();
                // 物联网平台「一机一密」直连三元组（见模块文档），DeviceName 复用 device_id
                let device_name = self.device_id.trim();
                let ts = chrono::Utc::now().timestamp_millis();
                conn.client_id = format!(
                    "{}.{}|securemode=2,signmethod=hmacsha256,timestamp={ts}|",
                    self.product_key, device_name
                );
                conn.username = format!("{}&{}", device_name, self.product_key);
                let content = format!(
                    "clientId{}.{}deviceName{}productKey{}timestamp{ts}",
                    self.product_key, device_name, device_name, self.product_key
                );
                conn.password = Self::hmac_sha256_hex(&self.device_secret, &content)?;
            }
        }
        Ok(conn)
    }
}

impl Default for AliyunPreset {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AliyunPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliyunPreset")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("group", &self.group)
            .field("instance_id", &self.instance_id)
            .field("region", &self.region)
            .field("auth_mode", &self.auth_mode)
            .field("access_key_id", &self.access_key_id)
            .field("access_key_secret", &"<redacted>")
            .field("group_id", &self.group_id)
            .field("device_id", &self.device_id)
            .field("product_key", &self.product_key)
            .field("device_secret", &"<redacted>")
            .finish()
    }
}
