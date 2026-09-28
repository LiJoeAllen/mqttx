//! OTA 自更新：以 Gitea Release 为更新源。
//!
//! 流程：`check_latest`（比较最新 Release 与当前版本）→ `download_and_verify`
//! （下载到 exe 同目录临时文件并校验 sha256）→ `install_and_restart`
//! （改名自替换后拉起新进程）。全程 HTTPS + sha256 校验。

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::Digest as _;

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

/// 某版本的产物文件名（与 CI 的命名约定 `mqttx-<ver>-<triple>-mqttx[.exe]` 一致）。
pub fn asset_name_for(version: &str) -> String {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    format!("mqttx-v{}-{}-mqttx{}", version, platform_triple(), ext)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .timeout_read(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
}

fn http_get_json(url: &str) -> Result<serde_json::Value, String> {
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| format!("请求失败: {e}"))?;
    resp.into_json::<serde_json::Value>()
        .map_err(|e| format!("解析响应失败: {e}"))
}

/// 从版本字符串提取数字段（容忍 v 前缀与 -beta 之类后缀）。
pub fn parse_version(v: &str) -> Vec<u32> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-'])
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

/// a 是否比 b 新（按数字段逐位比较，缺失段视为 0）。
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
pub fn pick_assets(rel: &serde_json::Value) -> Option<(String, String, u64, Option<String>)> {
    let assets = rel.get("assets")?.as_array()?;
    let want_ext = if cfg!(windows) { ".exe" } else { "" };
    let triple = platform_triple();
    for a in assets {
        let name = a.get("name")?.as_str()?;
        if !name.contains(triple) || !name.ends_with(want_ext) || name.ends_with(".sha256") {
            continue;
        }
        let url = a.get("browser_download_url")?.as_str()?.to_string();
        let size = a.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
        // 侧车命名：<asset>.sha256
        let sha = assets
            .iter()
            .find(|b| {
                b.get("name").and_then(|n| n.as_str()) == Some(&format!("{name}.sha256"))
            })
            .and_then(|b| b.get("browser_download_url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string());
        return Some((name.to_string(), url, size, sha));
    }
    None
}

/// 检查更新。返回 Ok(None) 表示已是最新。
pub fn check_latest() -> Result<Option<UpdateInfo>, String> {
    let base = format!("{GITEA_URL}/api/v1/repos/{PKG_OWNER}/{PKG_REPO}/releases");
    // 优先 latest 端点；旧版 Gitea 没有时回退到列表取第一个非草稿/预发布
    let rel = match http_get_json(&format!("{base}/latest")) {
        Ok(v) => v,
        Err(_) => {
            let list = http_get_json(&format!("{base}?limit=5"))?;
            let first = list
                .as_array()
                .and_then(|l| l.iter().find(|r| {
                    r.get("draft").and_then(|d| d.as_bool()) == Some(false)
                        && r.get("prerelease").and_then(|d| d.as_bool()) == Some(false)
                }))
                .ok_or_else(|| "没有可用 Release".to_string())?;
            first.clone()
        }
    };

    let tag = rel
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "Release 缺少 tag_name".to_string())?;
    if !is_newer(tag, current_version()) {
        return Ok(None);
    }
    let Some((_, url, size, sha)) = pick_assets(&rel) else {
        return Err(format!("Release {tag} 没有当前平台（{}）的产物", platform_triple()));
    };
    Ok(Some(UpdateInfo {
        version: tag.trim_start_matches('v').to_string(),
        notes: rel
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or_default()
            .to_string(),
        asset_url: url,
        sha256_url: sha,
        size,
    }))
}

/// 下载并校验到 exe 同目录的临时文件，返回临时文件路径。
///
/// `on_progress(downloaded, total)` 在阻塞线程上回调，total 为 0 时表示未知。
pub fn download_and_verify(
    info: &UpdateInfo,
    on_progress: &dyn Fn(u64, u64),
) -> Result<PathBuf, String> {
    let exe_dir = std::env::current_exe()
        .map_err(|e| format!("无法定位自身: {e}"))?
        .parent()
        .ok_or("无法定位安装目录")?
        .to_path_buf();
    let tmp = exe_dir.join(format!("mqttx.update.{}.tmp", std::process::id()));

    let resp = agent()
        .get(&info.asset_url)
        .call()
        .map_err(|e| format!("下载失败: {e}"))?;
    let mut reader = resp.into_reader();
    let total = if info.size > 0 { info.size } else { 0 };
    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut hasher = sha2::Sha256::new();
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("下载中断: {e}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        on_progress(downloaded, total);
    }
    std::io::Write::flush(&mut file).ok();
    drop(file);

    // sha256 校验
    let expect = fetch_expected_sha256(info)?;
    if let Some(expect) = expect {
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(&expect) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!(
                "校验失败：期望 sha256 {expect}，实际 {actual}"
            ));
        }
    }

    Ok(tmp)
}

/// 读取 `.sha256` 侧车内容，返回其中的哈希值。没有侧车时返回 None。
fn fetch_expected_sha256(info: &UpdateInfo) -> Result<Option<String>, String> {
    let Some(url) = &info.sha256_url else {
        return Ok(None);
    };
    let text = agent()
        .get(url)
        .call()
        .map_err(|e| format!("获取校验文件失败: {e}"))?
        .into_string()
        .map_err(|e| format!("读取校验文件失败: {e}"))?;
    // 常见格式：<hex>  <filename> 或纯 hex
    let hex = text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(Some(hex))
    } else {
        Err("校验文件格式异常".into())
    }
}

/// 安装：把临时文件替换为自身，并拉起新进程（旧文件改名为 `.old`，
/// 下次启动由 [`cleanup_old`] 清理）。成功后调用方应立即退出进程。
pub fn install_and_restart(new_exe: &Path) -> Result<(), String> {
    let cur = std::env::current_exe().map_err(|e| format!("无法定位自身: {e}"))?;
    let mut old_name = cur.clone().into_os_string();
    old_name.push(".old");
    let old = PathBuf::from(old_name);
    // 旧残留先清掉，避免改名失败
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&cur, &old).map_err(|e| format!("替换失败（可能无写入权限）: {e}"))?;
    if let Err(e) = std::fs::rename(new_exe, &cur) {
        // 回滚，尽量保住当前可执行文件
        let _ = std::fs::rename(&old, &cur);
        return Err(format!("替换失败: {e}"));
    }
    std::process::Command::new(&cur)
        .spawn()
        .map_err(|e| format!("启动新版本失败: {e}"))?;
    Ok(())
}

/// 启动时清理上次更新遗留的旧文件（改名失败的 `.old`、异常退出的 `.tmp`）。
pub fn cleanup_old() {
    let Ok(cur) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = cur.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if (name.ends_with(".old") && name.starts_with("mqttx"))
            || name.ends_with(".update.tmp")
        {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("v1.1.0", "1.0.9"));
        assert!(is_newer("2.0", "1.9.9"));
        // 预发布后缀不做特殊处理（只比较数字段，1.0.0-beta == 1.0.0）
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9", "1.0.0"));
        assert_eq!(parse_version("v1.2.3-beta"), vec![1, 2, 3]);
    }

    #[test]
    fn asset_naming_matches_ci_convention() {
        assert_eq!(
            asset_name_for("1.0.1"),
            format!("mqttx-v1.0.1-{}-mqttx{}", platform_triple(), if cfg!(windows) { ".exe" } else { "" })
        );
    }

    #[test]
    fn pick_assets_selects_platform_and_sidecar() {
        let rel: serde_json::Value = serde_json::json!({
            "tag_name": "v1.0.1",
            "assets": [
                {"name": "mqttx-v1.0.1-x86_64-unknown-linux-gnu-mqttx", "size": 1,
                 "browser_download_url": "https://x/linux"},
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe", "size": 19132928,
                 "browser_download_url": "https://x/win"},
                {"name": "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe.sha256", "size": 65,
                 "browser_download_url": "https://x/win.sha256"}
            ]
        });
        let (name, url, size, sha) = pick_assets(&rel).expect("应选中平台产物");
        if cfg!(windows) {
            assert_eq!(name, "mqttx-v1.0.1-x86_64-pc-windows-msvc-mqttx.exe");
            assert_eq!(url, "https://x/win");
            assert_eq!(size, 19132928);
            assert_eq!(sha.as_deref(), Some("https://x/win.sha256"));
        } else {
            assert_eq!(name, "mqttx-v1.0.1-x86_64-unknown-linux-gnu-mqttx");
            assert_eq!(url, "https://x/linux");
            assert_eq!(sha, None);
        }
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
