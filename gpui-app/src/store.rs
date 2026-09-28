//! JSON 文件持久化。数据存放在系统应用数据目录的 `MQTTX-GPUI/` 下：
//! Windows: %APPDATA%\MQTTX-GPUI，macOS: ~/Library/Application Support/MQTTX-GPUI，
//! Linux: ~/.local/share/MQTTX-GPUI。
//!
//! 刻意与官方 Electron MQTTX 的 `MQTTX/` 目录区分，避免互相覆盖数据。

/// 存储层诊断输出到 stderr（避免仅为文件持久化引入日志框架）。
macro_rules! elog {
    ($($arg:tt)*) => {
        eprintln!("[store][{}] {}",
            chrono::Local::now().format("%H:%M:%S"),
            format!($($arg)*))
    };
}

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{
    aliyun::AliyunPreset,
    model::{AppSettings, ConnectionConfig, GlobalVariable, PublishPreset, Subscription},
};

#[derive(Debug, Clone)]
pub struct Storage {
    dir: PathBuf,
}

impl Storage {
    pub fn new() -> Self {
        let dir = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("MQTTX-GPUI");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            elog!("创建数据目录 {} 失败: {e}", dir.display());
        }
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn read_json<T: for<'de> serde::Deserialize<'de>>(&self, name: &str) -> Result<Option<T>> {
        let p = self.path(name);
        if !p.exists() {
            return Ok(None);
        }
        let mut text = std::fs::read_to_string(&p)
            .with_context(|| format!("读取 {} 失败", p.display()))?;
        // 容忍 Windows 编辑器/PowerShell 写入的 UTF-8 BOM
        if text.starts_with('\u{feff}') {
            text.drain(..3);
        }
        if text.trim().is_empty() {
            return Ok(None);
        }
        let value = serde_json::from_str(&text)
            .with_context(|| format!("解析 {} 失败", p.display()))?;
        Ok(Some(value))
    }

    fn write_json<T: serde::Serialize + ?Sized>(&self, name: &str, value: &T) -> Result<()> {
        let target = self.path(name);
        let tmp = target.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(value)?;
        std::fs::write(&tmp, text)
            .with_context(|| format!("写入 {} 失败", tmp.display()))?;
        std::fs::rename(&tmp, &target)
            .with_context(|| format!("替换 {} 失败", target.display()))?;
        Ok(())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn log_dir(&self) -> PathBuf {
        self.dir.join("logs")
    }

    // ── connections ──
    pub fn load_connections(&self) -> Vec<ConnectionConfig> {
        self.read_json("connections.json")
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    pub fn save_connections(&self, conns: &[ConnectionConfig]) {
        if let Err(e) = self.write_json("connections.json", conns) {
            elog!("保存连接失败: {e}");
        }
    }

    // ── subscriptions ──
    pub fn load_subscriptions(&self) -> Vec<Subscription> {
        self.read_json("subscriptions.json")
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    pub fn save_subscriptions(&self, subs: &[Subscription]) {
        if let Err(e) = self.write_json("subscriptions.json", subs) {
            elog!("保存订阅失败: {e}");
        }
    }

    // ── presets ──
    pub fn load_presets(&self) -> Vec<PublishPreset> {
        self.read_json("presets.json")
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    pub fn save_presets(&self, presets: &[PublishPreset]) {
        if let Err(e) = self.write_json("presets.json", presets) {
            elog!("保存预设失败: {e}");
        }
    }

    // ── variables ──
    pub fn load_variables(&self) -> Vec<GlobalVariable> {
        self.read_json("variables.json")
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    pub fn save_variables(&self, vars: &[GlobalVariable]) {
        if let Err(e) = self.write_json("variables.json", vars) {
            elog!("保存全局变量失败: {e}");
        }
    }

    // ── aliyun presets ──
    pub fn load_aliyun(&self) -> Vec<AliyunPreset> {
        self.read_json("aliyun.json")
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    pub fn save_aliyun(&self, presets: &[AliyunPreset]) {
        if let Err(e) = self.write_json("aliyun.json", presets) {
            elog!("保存阿里云预设失败: {e}");
        }
    }

    // ── settings ──
    pub fn load_settings(&self) -> AppSettings {
        self.read_json("settings.json")
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    pub fn save_settings(&self, settings: &AppSettings) {
        if let Err(e) = self.write_json("settings.json", settings) {
            elog!("保存设置失败: {e}");
        }
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}
