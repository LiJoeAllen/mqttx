//! i18n 门面：语义键（`ns.key`）+ rust-i18n（gpui-kit 官方方案）。
//!
//! # 架构
//! - 语言包在 `locales/ui.yml`（en / zh-CN 双语并排，`_version: 2`），
//!   由 crate 根的 `rust_i18n::i18n!("locales", fallback = "en")` 编译期内嵌；
//!   gpui-kit 组件内置文案经 main.rs 的 `rust_i18n::extend!` 随语言切换。
//! - 调用点写 `t("common.save")` / `tf("msg.count", &[("n", &count)])`，
//!   key 是**稳定标识符**，与任何自然语言解耦——改文案措辞不动代码，
//!   也不存在「一词多义撞 key」。
//! - [`t`]/[`tf`] 直接查 [`crate::_rust_i18n_translate`]（rust-i18n 生成、
//!   接受运行时 key 的公开函数），不经 `t!` 宏——`label()` 这类运行时
//!   值同样可查。
//!
//! # 规则（单元测试强制，违反即测试失败）
//! 1. 新增文案：调用点 `t("ns.key")`，`locales/ui.yml` 加 key 块（en/zh-CN 必填）。
//! 2. 源码每个 `t("…")` / `tf("…")` 字面量必须在语言包中存在。
//! 3. 模型 `label()` 返回的 key 必须有**非自映射**的 en 词条。
//! 4. 同一 key 的 zh-CN 与 en 占位符 `{name}` 集合必须一致
//!    （`tf` 按名替换，两侧不一致即静默漏参）。
//!
//! # 已知限制
//! 语言切换对**已打开页签里的下拉框**（QoS / Retain Handling 等）不即时生效——
//! 这些 SelectState 在构造时固化了标签列表；重开页签即刷新。其余界面即时生效
//! （save_settings 内 `window.refresh()` 兜底整窗重绘）。

use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

const LOCALE_ZH: &str = "zh-CN";
const LOCALE_EN: &str = "en";

/// 0 = 中文，1 = English。默认中文。
static LANG: AtomicU8 = AtomicU8::new(0);

/// 切换语言：同时更新 rust-i18n 全局 locale（gpui-kit 组件内置文案随之切换）。
/// 已渲染界面的刷新由调用方负责（settings 保存路径带整窗 refresh）。
pub fn set_lang(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
    gpui_kit::component::set_locale(match lang {
        Lang::Zh => LOCALE_ZH,
        Lang::En => LOCALE_EN,
    });
}

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 {
        Lang::En
    } else {
        Lang::Zh
    }
}

/// 按系统语言解析初始语言（AppSettings::System 时使用）。
pub fn from_system() -> Lang {
    match sys_locale::get_locale().unwrap_or_default().to_lowercase() {
        l if l.starts_with("zh") => Lang::Zh,
        _ => Lang::En,
    }
}

/// 查指定 locale 的词条。
fn translate<'k>(locale: &str, key: &'k str) -> Cow<'k, str> {
    crate::_rust_i18n_translate(locale, key)
}

/// 取当前语言的界面文案。miss 时 rust-i18n 原样返回 key（测试会拦截，
/// 运行期裸 key 只是最后的安全网）。
pub fn t(key: &'static str) -> &'static str {
    let locale = match lang() {
        Lang::Zh => LOCALE_ZH,
        Lang::En => LOCALE_EN,
    };
    match translate(locale, key) {
        // 无参查表只返回 Borrowed（静态表值或原 key）；Owned 视为异常回退 key
        Cow::Borrowed(s) => s,
        Cow::Owned(_) => key,
    }
}

/// 带参数的界面文案：语言包内占位符写作 `{name}`（非 rust-i18n 的 `%{name}`，
/// 由本函数按名替换），参数值只需 `Display`。
pub fn tf(key: &'static str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = t(key).to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

// ─── 单元测试：语言包完整性由测试强制，而不是靠人工纪律 ──────────────────────

/// 提取 `{name}` 形式的占位符（ASCII 标识符；`{{x}}` 双括号、`{$builtin}`
/// 与中文花括号说明文不算），排序后比较集合。
#[cfg(test)]
fn placeholders(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' {
            let before_ok = i == 0 || b[i - 1] != b'{';
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let after_ok = j < b.len() && b[j] == b'}' && (j + 1 == b.len() || b[j + 1] != b'}');
            let name = &s[i + 1..j];
            let name_ok = name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            if before_ok && after_ok && name_ok {
                out.push(name.to_string());
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out.sort();
    out
}

/// 解析生成器产出的规整 YAML（key 行 + 缩进的 en/zh-CN 值行）。
/// 格式意外时 panic——语言包是构建产物，格式受控。
#[cfg(test)]
fn load_ui_yml() -> std::collections::BTreeMap<String, (String, String)> {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales/ui.yml"),
    )
    .expect("locales/ui.yml 应存在");
    let unquote = |v: &str| -> String {
        let v = v.trim();
        let inner = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(v);
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    };
    let mut map = std::collections::BTreeMap::new();
    let mut key: Option<String> = None;
    let mut en: Option<String> = None;
    let mut zh: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.starts_with("_version") || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            if let (Some(k), Some(e), Some(z)) = (key.take(), en.take(), zh.take()) {
                map.insert(k, (e, z));
            }
            key = Some(unquote(line.trim_end_matches(':')));
        } else if let Some(v) = line.trim().strip_prefix("en:") {
            en = Some(unquote(v));
        } else if let Some(v) = line.trim().strip_prefix("zh-CN:") {
            zh = Some(unquote(v));
        }
    }
    if let (Some(k), Some(e), Some(z)) = (key, en, zh) {
        map.insert(k, (e, z));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个 key 都必须有非空 en 与 zh-CN；占位符两侧集合一致。
    #[test]
    fn yml_complete_and_placeholder_parity() {
        let map = load_ui_yml();
        assert!(!map.is_empty(), "语言包不能为空");
        for (k, (en, zh)) in &map {
            assert!(!en.trim().is_empty(), "en 为空: {k:?}");
            assert!(!zh.trim().is_empty(), "zh-CN 为空: {k:?}");
            assert_eq!(
                placeholders(zh),
                placeholders(en),
                "占位符两侧不一致: {k:?}"
            );
        }
    }

    /// 扫描 src/ 全部源码里的 `t("…")` / `tf("…")` 字面量，
    /// 逐一断言语言包有词条——漏翻是测试失败，不再是静默回退。
    #[test]
    fn call_sites_covered_by_yml() {
        let map = load_ui_yml();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect_rs(&root, &mut files);
        assert!(!files.is_empty(), "应扫描到源码文件");
        let mut missing = Vec::new();
        for f in files {
            if f.ends_with("i18n.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&f).unwrap();
            for key in extract_t_literals(&src) {
                if !map.contains_key(&key) {
                    missing.push(format!("{}: {key:?}", f.display()));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "以下 t()/tf() 字面量没有语言包词条:\n{}",
            missing.join("\n")
        );
    }

    /// 模型 `label()` 返回的 key 必须有真实（非自映射）的 en 词条。
    #[test]
    fn model_labels_have_real_translations() {
        let labels = [
            crate::model::ConnectionStatus::Disconnected.label(),
            crate::model::ConnectionStatus::Connecting.label(),
            crate::model::ConnectionStatus::Connected.label(),
            crate::model::ConnectionStatus::Error.label(),
            crate::model::Direction::Received.label(),
            crate::model::Direction::Published.label(),
            crate::model::ThemeModePref::System.label(),
            crate::model::ThemeModePref::Light.label(),
            crate::model::ThemeModePref::Dark.label(),
        ];
        let missing: Vec<&str> = labels
            .iter()
            .filter(|l| translate(LOCALE_EN, l) == Cow::Borrowed(**l))
            .copied()
            .collect();
        assert!(missing.is_empty(), "模型 label 缺 en 词条: {missing:?}");
    }

    fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_rs(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// 手写扫描（避免引入 regex 依赖）：找 `::t(` / `::tf(` 后的字面量。
    /// `(` 与 `"` 之间允许空白/换行——覆盖多行调用的写法
    /// `i18n::tf(\n    "key",\n    &[...]`；`::t(` 针不会命中 `::tf(`
    /// （f 不是空白，也到不了引号分支）。
    fn extract_t_literals(src: &str) -> Vec<String> {
        let b = src.as_bytes();
        let mut out = Vec::new();
        for needle in [b"::tf(".as_slice(), b"::t(".as_slice()] {
            let mut i = 0;
            while let Some(rel) = find(b, i, needle) {
                let mut start = rel + needle.len();
                while start < b.len() && b[start].is_ascii_whitespace() {
                    start += 1;
                }
                if start < b.len() && b[start] == b'"'
                    && let Some(lit) = read_string(b, start).0
                {
                    out.push(lit);
                }
                i = rel + needle.len();
            }
        }
        out
    }

    fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
        hay[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|p| p + from)
    }

    /// 从 `start`（首字节应为 `"`）读一个 Rust 字符串字面量，
    /// 处理转义，返回 (解码内容, 结束位置)。
    fn read_string(b: &[u8], start: usize) -> (Option<String>, usize) {
        if start >= b.len() || b[start] != b'"' {
            return (None, start + 1);
        }
        let mut out = String::new();
        let mut i = start + 1;
        while i < b.len() {
            match b[i] {
                b'\\' => {
                    i += 1;
                    if i < b.len() {
                        match b[i] {
                            b'"' => out.push('"'),
                            b'\\' => out.push('\\'),
                            other => {
                                out.push('\\');
                                out.push(other as char);
                            }
                        }
                        i += 1;
                    }
                }
                b'"' => return (Some(out), i + 1),
                _ => {
                    let mut end = i + 1;
                    while end < b.len() && (b[end] & 0xC0) == 0x80 {
                        end += 1;
                    }
                    out.push_str(&String::from_utf8_lossy(&b[i..end]));
                    i = end;
                }
            }
        }
        (None, i)
    }
}
