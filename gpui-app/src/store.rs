//! JSON 文件持久化。数据存放在系统应用数据目录的 `MQTTX-GPUI/` 下：
//! Windows: %APPDATA%\MQTTX-GPUI，macOS: ~/Library/Application Support/MQTTX-GPUI，
//! Linux: ~/.local/share/MQTTX-GPUI。
//!
//! 刻意与官方 Electron MQTTX 的 `MQTTX/` 目录区分，避免互相覆盖数据。

/// 存储层诊断输出到 stderr（避免仅为文件持久化引入日志框架），
/// 并同步上报 Sentry（未初始化时为空操作）。
macro_rules! elog {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        eprintln!("[store][{}] {}",
            chrono::Local::now().format("%H:%M:%S"),
            msg);
        sentry::capture_message(&msg, sentry::Level::Error);
    }};
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};

use crate::{
    aliyun::AliyunPreset,
    model::{AppSettings, ConnectionConfig, GlobalVariable, PublishPreset, Subscription},
};

#[derive(Debug, Clone)]
pub struct Storage {
    dir: PathBuf,
    /// 本次会话中加载失败的文件名。下次保存前先把原文件备份为
    /// `<name>.corrupt-<时间戳>`，避免损坏文件被空数据直接覆盖。
    corrupt: std::sync::Arc<Mutex<HashSet<String>>>,
}

impl Storage {
    pub fn new() -> Self {
        let dir = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("MQTTX-GPUI");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            elog!("创建数据目录 {} 失败: {e}", dir.display());
        }
        Self {
            dir,
            corrupt: std::sync::Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn read_json<T: for<'de> serde::Deserialize<'de>>(&self, name: &str) -> Result<Option<T>> {
        let p = self.path(name);
        if !p.exists() {
            return Ok(None);
        }
        let result = self.read_json_inner(&p);
        if let Err(e) = &result {
            // 读取/解析失败不能静默：否则「连接全部消失」无任何线索，
            // 且下一次保存会用空数据覆盖本可抢救的原文件。
            elog!("加载 {} 失败，本次会话将以空数据继续: {e}", p.display());
            if let Ok(mut set) = self.corrupt.lock() {
                set.insert(name.to_string());
            }
        }
        result
    }

    fn read_json_inner<T: for<'de> serde::Deserialize<'de>>(&self, p: &Path) -> Result<Option<T>> {
        let mut text = std::fs::read_to_string(p)
            .with_context(|| crate::ui::i18n::tf("io.err_read", &[("p", &p.display().to_string())]))?;
        // 容忍 Windows 编辑器/PowerShell 写入的 UTF-8 BOM（BOM 只有 1 个字符，逐字符剥离）
        if let Some(rest) = text.strip_prefix('\u{feff}') {
            text = rest.to_string();
        }
        if text.trim().is_empty() {
            return Ok(None);
        }
        let value =
            serde_json::from_str(&text).with_context(|| crate::ui::i18n::tf("io.err_parse_json_path", &[("p", &p.display().to_string())]))?;
        Ok(Some(value))
    }

    fn write_json<T: serde::Serialize + ?Sized>(&self, name: &str, value: &T) -> Result<()> {
        // 曾加载失败过的文件：覆盖前先备份原文件，把数据丢失降为可恢复
        let was_corrupt = self.corrupt.lock().map(|mut s| s.remove(name)).unwrap_or(false);
        let target = self.path(name);
        if was_corrupt && target.exists() {
            let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let backup = self.path(&format!("{name}.corrupt-{ts}"));
            match std::fs::rename(&target, &backup) {
                Ok(()) => elog!(
                    "{} 加载失败过，原文件已备份为 {} 后重新写入",
                    name,
                    backup.display()
                ),
                Err(e) => elog!("备份损坏文件 {} 失败: {e}", target.display()),
            }
        }

        // tmp 名拼入 pid，避免并发调用同名互踩；写完 fsync 再 rename，
        // 保证掉电后 rename 提交的目标内容完整。
        let tmp = self.path(&format!("{name}.{}.tmp", std::process::id()));
        let result = (|| -> Result<()> {
            let text = serde_json::to_string_pretty(value)?;
            let mut f = std::fs::File::create(&tmp)
                .with_context(|| format!("写入 {} 失败", tmp.display()))?;
            std::io::Write::write_all(&mut f, text.as_bytes())?;
            f.sync_all().with_context(|| format!("刷盘 {} 失败", tmp.display()))?;
            drop(f);
            std::fs::rename(&tmp, &target)
                .with_context(|| format!("替换 {} 失败", target.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
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

    // ── ui state（标签页会话恢复） ──
    pub fn load_ui_state(&self) -> UiState {
        self.read_json("ui-state.json")
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    pub fn save_ui_state(&self, state: &UiState) {
        if let Err(e) = self.write_json("ui-state.json", state) {
            elog!("保存界面状态失败: {e}");
        }
    }
}

/// 会话 UI 状态：打开的标签页与活动标签，用于重启后恢复工作区。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    #[serde(default)]
    pub open_tabs: Vec<String>,
    #[serde(default)]
    pub active_tab: Option<String>,
    /// 连接侧栏抽屉是否收起（抽屉式折叠，Ctrl+B 切换）
    #[serde(default)]
    pub sidebar_collapsed: bool,
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
        .map_err(|e| crate::ui::i18n::tf("io.err_serialize", &[("e", &e.to_string())]))?;
    std::fs::write(path, text).map_err(|e| crate::ui::i18n::tf("io.err_write", &[("p", &path.display().to_string()), ("e", &e.to_string())]))
}

/// 从 JSON 文件导入连接。
///
/// 容错规则：容忍 UTF-8 BOM；顶层允许是数组，也可能是 `{ "connections": [...] }` 对象；
/// 任一项不是合法连接配置、或 `id` 为空白时，返回带序号的错误信息。
pub fn import_connections_from(path: &Path) -> Result<Vec<ConnectionConfig>, String> {
    let bytes = std::fs::read(path).map_err(|e| crate::ui::i18n::tf("io.err_read", &[("p", &path.display().to_string()), ("e", &e.to_string())]))?;
    let mut text =
        String::from_utf8(bytes).map_err(|_| crate::ui::i18n::tf("io.err_utf8", &[("p", &path.display().to_string())]))?;
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        text = rest.to_string();
    }
    let value: serde_json::Value =
        serde_json::from_str(text.trim()).map_err(|e| crate::ui::i18n::tf("io.err_parse_json", &[("e", &e.to_string())]))?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(mut map) => match map.remove("connections") {
            Some(serde_json::Value::Array(items)) => items,
            _ => {
                return Err(
                    crate::ui::i18n::t("io.err_missing_field").to_string(),
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
                    return Err(crate::ui::i18n::tf("io.err_item_id", &[("n", &(i + 1))]));
                }
                conns.push(c);
            }
            Err(e) => return Err(crate::ui::i18n::tf("io.err_item_invalid", &[("n", &(i + 1)), ("e", &e.to_string())])),
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

    // 损坏文件：加载失败要被感知，且重新保存前原文件必须被备份，
    // 防止「连接全部消失后被空数据覆盖」的不可恢复丢失
    #[test]
    fn corrupted_file_is_logged_and_backed_up_before_overwrite() {
        let dir = std::env::temp_dir().join(format!(
            "mqttx-store-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("创建测试目录");
        let storage = Storage {
            dir: dir.clone(),
            corrupt: std::sync::Arc::new(Mutex::new(HashSet::new())),
        };
        std::fs::write(dir.join("connections.json"), "{ 这不是 JSON").expect("写入损坏文件");

        assert!(storage.load_connections().is_empty(), "损坏文件按空数据处理");

        storage.save_connections(&[ConnectionConfig::new()]);

        let names: Vec<String> = std::fs::read_dir(&dir)
            .expect("列出测试目录")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with("connections.json.corrupt-")),
            "覆盖前应备份损坏文件，实际文件: {names:?}"
        );
        assert!(names.contains(&"connections.json".to_string()), "新文件应已写入");
        // 备份内容仍是损坏原文，可人工抢救
        let backup = names
            .iter()
            .find(|n| n.starts_with("connections.json.corrupt-"))
            .expect("备份存在");
        assert_eq!(
            std::fs::read_to_string(dir.join(backup)).expect("读备份"),
            "{ 这不是 JSON"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
