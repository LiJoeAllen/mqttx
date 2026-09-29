//! 应用设置：连接状态枚举、主题偏好与全局设置项。


use serde::{Deserialize, Serialize};


use super::*;

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
