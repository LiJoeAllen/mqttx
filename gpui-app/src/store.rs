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
        // 容忍 Windows 编辑器/PowerShell 写入的 UTF-8 BOM（BOM 只有 1 个字符，逐字符剥离）
        if let Some(rest) = text.strip_prefix('\u{feff}') {
            text = rest.to_string();
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

// ── 连接导入 / 导出（Phase 2 UI 调用） ──────────────────────────────────────

/// 导出连接为 JSON 数组（与 connections.json 同构：缩进格式化、UTF-8 无 BOM）。
pub fn export_connections_to(path: &Path, conns: &[ConnectionConfig]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(conns)
        .map_err(|e| format!("序列化连接失败: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("写入 {} 失败: {e}", path.display()))
}

/// 从 JSON 文件导入连接。
///
/// 容错规则：容忍 UTF-8 BOM；顶层允许是数组，也可能是 `{ "connections": [...] }` 对象；
/// 任一项不是合法连接配置、或 `id` 为空白时，返回带序号的错误信息。
pub fn import_connections_from(path: &Path) -> Result<Vec<ConnectionConfig>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let mut text =
        String::from_utf8(bytes).map_err(|_| format!("{} 不是有效的 UTF-8 文件", path.display()))?;
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        text = rest.to_string();
    }
    let value: serde_json::Value =
        serde_json::from_str(text.trim()).map_err(|e| format!("解析 JSON 失败: {e}"))?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(mut map) => match map.remove("connections") {
            Some(serde_json::Value::Array(items)) => items,
            _ => {
                return Err(
                    "JSON 对象中缺少 \"connections\" 数组字段".to_string(),
                );
            }
        },
        _ => return Err("JSON 顶层应为连接数组".to_string()),
    };
    let mut conns = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        match serde_json::from_value::<ConnectionConfig>(item) {
            Ok(c) => {
                // 空 id 会在去重/订阅关联时产生悬空引用，导入层直接拒绝
                if c.id.trim().is_empty() {
                    return Err(format!("第 {} 项的 id 不能为空", i + 1));
                }
                conns.push(c);
            }
            Err(e) => return Err(format!("第 {} 项不是合法的连接配置: {e}", i + 1)),
        }
    }
    Ok(conns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mqttx-store-test-{}-{name}",
            uuid::Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn export_import_roundtrip() {
        let path = temp_path("roundtrip.json");
        let mut c = ConnectionConfig::new();
        c.group = Some("测试组".into());
        c.connection_timeout_secs = 25;
        c.max_reconnect_times = 5;
        export_connections_to(&path, std::slice::from_ref(&c)).expect("导出应成功");
        let back = import_connections_from(&path).expect("导入应成功");
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, c.id);
        assert_eq!(back[0].group.as_deref(), Some("测试组"));
        assert_eq!(back[0].connection_timeout_secs, 25);
        assert_eq!(back[0].max_reconnect_times, 5);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_tolerates_bom_and_object_form() {
        let path = temp_path("bom-object.json");
        let c = ConnectionConfig::new();
        let body = serde_json::to_string(&serde_json::json!({
            "connections": [serde_json::to_value(&c).unwrap()]
        }))
        .unwrap();
        // 写入 BOM 前缀，验证容错
        std::fs::write(&path, format!("\u{feff}{body}")).expect("写入测试文件");
        let back = import_connections_from(&path).expect("BOM + 对象形式应可导入");
        assert_eq!(back.len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_reports_invalid_item_index() {
        let path = temp_path("invalid.json");
        let good = serde_json::to_value(ConnectionConfig::new()).unwrap();
        let body = serde_json::json!([good, {"id": "bad"}]).to_string();
        std::fs::write(&path, body).expect("写入测试文件");
        let err = import_connections_from(&path).expect_err("非法项应报错");
        assert!(err.contains("第 2 项"), "错误应定位到序号: {err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_rejects_empty_id() {
        let path = temp_path("empty-id.json");
        let mut c = ConnectionConfig::new();
        c.id = "   ".into();
        let good = serde_json::to_value(ConnectionConfig::new()).unwrap();
        let body = serde_json::json!([good, serde_json::to_value(&c).unwrap()]).to_string();
        std::fs::write(&path, body).expect("写入测试文件");
        let err = import_connections_from(&path).expect_err("空 id 应报错");
        assert!(err.contains("第 2 项"), "错误应定位到序号: {err}");
        assert!(err.contains("id"), "错误应说明 id 为空: {err}");
        let _ = std::fs::remove_file(&path);
    }
}
