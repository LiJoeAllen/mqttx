//! OTA 自更新：以 GitHub Release 为更新源。
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

/// 更新源（GitHub Releases；仓库公开，API 与附件下载均无需认证）。
///
/// - api.github.com 未认证配额为 60 次/小时/IP：共享出口 IP 打满时检查会失败，
///   按「错误仅记录、不打扰用户」静默降级，下次成功检查自动追上；
///   附件下载走 github.com CDN，不受 API 配额限制。
/// - Gitea（gitea.heavenlybook.cn）是 v1.0.0 之前的历史发布渠道，已弃用：
///   GitHub 侧发布历史自 v1.0.0 起独立开始，不再向 Gitea 镜像。
pub const RELEASES_API: &str = "https://api.github.com/repos/LiJoeAllen/mqttx/releases";
/// Releases 页面（github.com，非 API、无配额限制）：api.github.com 被限流时
/// 兜底发现最新 tag —— 跟踪 `{RELEASES_PAGE}/latest` 的 302，从最终 URL 取 tag，
/// 下载与校验按命名约定构造 URL，完整性（sha256）不受影响。
pub const RELEASES_PAGE: &str = "https://github.com/LiJoeAllen/mqttx/releases";
/// 更新源展示名（设置对话框「关于」）。
pub const UPDATE_SOURCE: &str = "GitHub";

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
///
/// 架构取自 `std::env::consts::ARCH` 而非写死：写死 x86_64 会让 Apple Silicon /
/// ARM Linux 客户端永远挑不到自己的产物（发布脚本能产出 aarch64 资产，但无人下载），
/// 甚至在只有 x86_64 资产时把错误架构的二进制装上去。
pub fn platform_triple() -> String {
    let arch = std::env::consts::ARCH;
    if cfg!(windows) {
        format!("{arch}-pc-windows-msvc")
    } else if cfg!(target_os = "macos") {
        format!("{arch}-apple-darwin")
    } else {
        format!("{arch}-unknown-linux-gnu")
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
    /// 暂存二进制的 sha256（小写 hex）。
    ///
    /// 注意它与 Release 附件的 `.sha256` 侧车不是同一个东西：侧车校验的是**下载到的
    /// 压缩包**，这里是**解压/落盘后的最终二进制**。安装同意标记绑定该哈希，安装前
    /// 也会用它复核，防止"下载校验通过"到"下次启动安装"之间文件被替换。
    pub sha256: String,
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
        // 附件名按**当前平台**三元组构造：写死 x86_64-pc-windows-msvc 会让本测试
        // 只在 Windows 上通过，Linux CI 必红
        let triple = platform_triple();
        let bin = if cfg!(windows) { "mqttx.exe" } else { "mqttx" };
        let base = format!("mqttx-v1.0.1-{triple}-{bin}");
        let (n_bin, n_bin_sha, n_7z, n_7z_sha) = (
            base.clone(),
            format!("{base}.sha256"),
            format!("{base}.7z"),
            format!("{base}.7z.sha256"),
        );
        let rel: serde_json::Value = serde_json::json!({
            "tag_name": "v1.0.1",
            "assets": [
                {"name": n_bin, "size": 21000000, "browser_download_url": "https://x/bin"},
                {"name": n_bin_sha, "size": 65, "browser_download_url": "https://x/bin.sha256"},
                {"name": n_7z, "size": 9000000, "browser_download_url": "https://x/bin.7z"},
                {"name": n_7z_sha, "size": 65, "browser_download_url": "https://x/bin.7z.sha256"}
            ]
        });
        let (name, url, size, sha) = pick_assets(&rel).expect("应选中 7z");
        assert!(name.ends_with(".7z"), "应优先 7z 压缩包，实际 {name}");
        assert_eq!(url, "https://x/bin.7z");
        assert_eq!(size, 9000000);
        assert_eq!(sha.as_deref(), Some("https://x/bin.7z.sha256"));
    }

    #[test]
    fn pick_assets_falls_back_to_raw_binary() {
        // 旧发布方式只有裸二进制：仍可被选中
        let triple = platform_triple();
        let bin = if cfg!(windows) { "mqttx.exe" } else { "mqttx" };
        let base = format!("mqttx-v1.0.1-{triple}-{bin}");
        let (n_bin, n_sha) = (base.clone(), format!("{base}.sha256"));
        let rel: serde_json::Value = serde_json::json!({
            "tag_name": "v1.0.1",
            "assets": [
                {"name": n_bin, "size": 19132928, "browser_download_url": "https://x/bin"},
                {"name": n_sha, "size": 65, "browser_download_url": "https://x/bin.sha256"}
            ]
        });
        let (name, url, _size, sha) = pick_assets(&rel).expect("应回退裸二进制");
        assert!(name.ends_with(bin), "实际 {name}");
        assert_eq!(url, "https://x/bin");
        assert_eq!(sha.as_deref(), Some("https://x/bin.sha256"));
    }

    // 非约定命名的附件不得被选中：非 Windows 下旧实现的 ends_with("") 恒真，
    // 会把 .tar.gz / .deb 这类附件当成可执行文件下载并替换自身
    #[test]
    fn pick_assets_rejects_non_convention_assets() {
        let triple = platform_triple();
        let rel: serde_json::Value = serde_json::json!({
            "tag_name": "v1.0.1",
            "assets": [
                {"name": format!("mqttx-v1.0.1-{triple}.tar.gz"), "size": 1024,
                 "browser_download_url": "https://x/pkg.tar.gz"},
                {"name": format!("mqttx-v1.0.1-{triple}-debug"), "size": 1024,
                 "browser_download_url": "https://x/dbg"}
            ]
        });
        assert!(pick_assets(&rel).is_none(), "非约定命名的附件不应被选中");
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
