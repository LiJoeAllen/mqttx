//! 订阅：订阅条目与下发引擎的订阅选项。


use serde::{Deserialize, Serialize};


use super::*;

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
