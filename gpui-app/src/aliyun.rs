//! 阿里云 IoT 平台一键接入预设（对标官方 MQTTX 的阿里云模板）。
//!
//! Token 模式鉴权参数：
//! - clientId: `{GroupId}@@@{DeviceId}`
//! - username: `Signature|{AccessKeyId}|{InstanceId}`
//! - password: base64(HMAC-SHA1(AccessKeySecret, clientId))

use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;

use crate::model::{ConnectionConfig, ProtocolVersion, TransportKind};

type HmacSha1 = Hmac<Sha1>;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    // 共用
    pub device_id: String,

    // 一机一密模式
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
            device_secret: String::new(),
        }
    }

    /// HMAC-SHA1(secret, client_id) 的 Base64 编码。
    pub fn signature(secret: &str, client_id: &str) -> Result<String, String> {
        let mut mac =
            HmacSha1::new_from_slice(secret.as_bytes()).map_err(|e| format!("HMAC 初始化失败: {e}"))?;
        mac.update(client_id.as_bytes());
        Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
    }

    /// 接入点 host：`{instanceId}.mqtt.iothub.aliyuncs.com`
    /// （实例级接入点在各区域通用；region 为空时同样可用）
    pub fn host(&self) -> String {
        if self.region.is_empty() {
            format!("{}.mqtt.iothub.aliyuncs.com", self.instance_id)
        } else {
            format!(
                "{}.mqtt.{}.aliyuncs.com",
                self.instance_id, self.region
            )
        }
    }

    /// 由预设生成一份连接配置（不写入 id/name 之外的覆盖项）。
    pub fn to_connection(&self) -> Result<ConnectionConfig, String> {
        let mut conn = ConnectionConfig::new();
        conn.name = self.name.clone();
        conn.host = self.host();
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
                let client_id = format!("{}@@@{}", self.group_id, self.device_id);
                let password = Self::signature(&self.access_key_secret, &client_id)?;
                conn.client_id = client_id;
                conn.username =
                    format!("Signature|{}|{}", self.access_key_id, self.instance_id);
                conn.password = password;
            }
            AliyunAuthMode::DeviceCredential => {
                if self.device_secret.is_empty() {
                    return Err("设备密钥 (DeviceSecret) 不能为空".into());
                }
                // 一机一密三元组：clientId=设备名，username=设备名&实例，password=HMAC-SHA1(DeviceSecret, clientId)
                let client_id = self.device_id.clone();
                let password = Self::signature(&self.device_secret, &client_id)?;
                conn.client_id = client_id;
                conn.username = format!("{}&{}", self.device_id, self.instance_id);
                conn.password = password;
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
