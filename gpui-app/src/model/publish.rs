//! 发布：发布参数与可保存的发布预设。


use serde::{Deserialize, Serialize};


use super::*;

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
