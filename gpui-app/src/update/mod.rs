//! OTA 自更新：以 Gitea Release 为更新源。
//!
//! - [`check`] 检查最新版本与资产挑选
//! - [`install`] 下载暂存、校验安装、同意标记与清理
//!
//! 本模块只暴露版本比较/资产命名等纯工具与二级模块的公开 API。

use std::path::PathBuf;

use sha2::Digest as _;

mod check;
mod install;

pub use check::{check_and_download, check_latest, pick_assets};
pub use install::{
    cleanup_old, clear_install_consent, discard_staged, download_and_stage,
    install_consent_matches, install_staged, load_staged, mark_install_consent,
};

/// 更新源（自建 Gitea，仓库公开、附件可直接下载，无需认证）。
pub const GITEA_URL: &str = "https://gitea.heavenlybook.cn";
pub const PKG_OWNER: &str = "JoeAllen";
pub const PKG_REPO: &str = "mqttx";

const HTTP_TIMEOUT_SECS: u64 = 30;

/// 发现的可用更新。
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// 新版本号（不含 v 前缀），如 "1.0.1"
    pub version: String,
    /// Release 说明（Markdown 原文）
    pub notes: String,
    /// 附件名（判断 7z 压缩包 / 裸二进制）
    pub asset_name: String,
    /// 平台对应产物下载地址
    pub asset_url: String,
    /// sha256 校验文件地址（若 Release 提供了 `.sha256` 侧车）
    pub sha256_url: Option<String>,
    /// 产物字节数（API 提供，用于下载进度）
    pub size: u64,
}

/// 当前版本。测试可用环境变量 `MQTTX_VERSION_OVERRIDE` 伪装旧版本，
/// 验证"发现新版本"路径。
pub fn current_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        std::env::var("MQTTX_VERSION_OVERRIDE")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string())
    })
}

/// 构建目标的平台三元组，需与 release.yml / publish 脚本的产物命名一致。
pub fn platform_triple() -> &'static str {
    if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "x86_64-apple-darwin"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

/// 某版本解压后二进制的暂存名（与 CI 的命名约定 `mqttx-v<ver>-<triple>-mqttx[.exe]` 一致）。
pub fn binary_name_for(version: &str) -> String {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    format!("mqttx-v{}-{}-mqttx{}", version, platform_triple(), ext)
}

/// 某版本的 7z 压缩附件名（打包端与此约定一致；内部为 [`binary_name_for`]）。
pub fn asset_name_for(version: &str) -> String {
    format!("{}.7z", binary_name_for(version))
}

/// 去掉 v 前缀与预发布后缀（`-` 及之后），返回数字主体。
fn release_part(v: &str) -> &str {
    let v = v.trim().trim_start_matches(['v', 'V']);
    match v.find('-') {
        Some(i) => &v[..i],
        None => v,
    }
}

/// 从版本字符串提取数字段（容忍 v 前缀；预发布后缀（`-` 之后）不参与比较）。
pub fn parse_version(v: &str) -> Vec<u32> {
    release_part(v)
        .split('.')
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

/// a 是否比 b 新（按数字段逐位比较，缺失段视为 0）。
/// 预发布版本（含 `-` 后缀）不会比同数字段的正式版新（semver 语义）。
pub fn is_newer(a: &str, b: &str) -> bool {
    let (a, b) = (parse_version(a), parse_version(b));
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// 从 Release JSON 里挑选当前平台的产物与 sha256 侧车。
/// 优先 7z 压缩包（体积小、下载快）；Release 未提供 7z 时回退裸二进制
/// （兼容旧发布方式）。

#[derive(Debug, Clone)]
pub struct StagedUpdate {
    pub version: String,
    pub path: PathBuf,
}


/// 预发布后缀原样保留。
pub fn parse_staged_name(name: &str) -> Option<String> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let suffix = format!("-{}-mqttx{ext}", platform_triple());
    if name.contains(".tmp") {
        return None;
    }
    name.strip_prefix("mqttx-v")?
        .strip_suffix(&suffix)
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("v1.1.0", "1.0.9"));
        assert!(is_newer("2.0", "1.9.9"));
        // 预发布版本不比同数字段的正式版新（semver 语义）
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
        assert!(!is_newer("1.0.0-beta.2", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9", "1.0.0"));
        // 更高版本号的预发布仍是更新（1.0.1-beta > 1.0.0）
        assert!(is_newer("1.0.1-beta", "1.0.0"));
        assert_eq!(parse_version("v1.2.3-beta"), vec![1, 2, 3]);
        assert_eq!(parse_version("v1.2.3"), vec![1, 2, 3]);
    }

    #[cfg(windows)]
    #[test]
    fn staged_name_parsing_keeps_prerelease_suffix() {
        let triple = platform_triple();
        assert_eq!(
            parse_staged_name(&format!("mqttx-v1.0.3-{triple}-mqttx.exe")),
            Some("1.0.3".to_string())
        );
        assert_eq!(
            parse_staged_name(&format!("mqttx-v1.0.3-beta.1-{triple}-mqttx.exe")),
            Some("1.0.3-beta.1".to_string())
        );
        assert_eq!(
            parse_staged_name(&format!("mqttx-v1.0.3-{triple}-mqttx.exe.1234.56.tmp")),
            None
        );
        assert_eq!(parse_staged_name("其他文件.exe"), None);
    }

    #[test]
    fn asset_naming_matches_ci_convention() {
        let ext = if cfg!(windows) { ".exe" } else { "" };
        // 附件为 7z 压缩包；解压出的二进制保留原命名
        assert_eq!(
            asset_name_for("1.0.1"),
            format!("mqttx-v1.0.1-{triple}-mqttx{ext}.7z", triple = platform_triple())
        );
        assert_eq!(
            binary_name_for("1.0.1"),
            format!("mqttx-v1.0.1-{triple}-mqttx{ext}", triple = platform_triple())
        );
    }

    #[test]
    fn pick_assets_prefers_7z_over_raw_binary() {
        let rel: serde_json::Value = serde_json::json!({
            "tag_name": "v1.0.1",
            "assets": [
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe", "size": 21000000,
                 "browser_download_url": "https://x/win.exe"},
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe.sha256", "size": 65,
                 "browser_download_url": "https://x/win.exe.sha256"},
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe.7z", "size": 9000000,
                 "browser_download_url": "https://x/win.7z"},
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe.7z.sha256", "size": 65,
                 "browser_download_url": "https://x/win.7z.sha256"}
            ]
        });
        let (name, url, size, sha) = pick_assets(&rel).expect("应选中 7z");
        assert!(name.ends_with(".7z"), "应优先 7z 压缩包，实际 {name}");
        assert_eq!(url, "https://x/win.7z");
        assert_eq!(size, 9000000);
        assert_eq!(sha.as_deref(), Some("https://x/win.7z.sha256"));
    }

    #[test]
    fn pick_assets_falls_back_to_raw_binary() {
        // 旧发布方式只有裸二进制：仍可被选中
        let rel: serde_json::Value = serde_json::json!({
            "tag_name": "v1.0.1",
            "assets": [
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe", "size": 19132928,
                 "browser_download_url": "https://x/win"},
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe.sha256", "size": 65,
                 "browser_download_url": "https://x/win.sha256"}
            ]
        });
        let (name, url, _size, sha) = pick_assets(&rel).expect("应回退裸二进制");
        assert!(name.ends_with(".exe"));
        assert_eq!(url, "https://x/win");
        assert_eq!(sha.as_deref(), Some("https://x/win.sha256"));
    }

    #[test]
    fn pick_assets_skips_sidecars_and_other_platforms() {
        let rel: serde_json::Value = serde_json::json!({
            "tag_name": "v1.0.1",
            "assets": [
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe.sha256", "size": 65,
                 "browser_download_url": "https://x/side"},
            ]
        });
        if cfg!(windows) {
            // 只有侧车、没有主体 → 不应选中侧车
            assert!(pick_assets(&rel).is_none());
        }
    }
}
