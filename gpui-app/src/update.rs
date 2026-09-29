//! OTA 自更新：以 Gitea Release 为更新源。
//!
//! 流程：`check_latest`（比较最新 Release 与当前版本）→ `download_and_verify`
//! （下载到 exe 同目录临时文件并校验 sha256）→ `install_and_restart`
//! （改名自替换后拉起新进程）。全程 HTTPS + sha256 校验。

use std::io::Read;
use std::path::PathBuf;

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
        // 整体上限：防止慢速滴流的下载无限占用阻塞线程
        .timeout(std::time::Duration::from_secs(10 * 60))
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
pub fn pick_assets(rel: &serde_json::Value) -> Option<(String, String, u64, Option<String>)> {
    let assets = rel.get("assets")?.as_array()?;
    let want_ext = if cfg!(windows) { ".exe" } else { "" };
    let triple = platform_triple();
    for a in assets {
        let Some(name) = a.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        if !name.contains(triple) || !name.ends_with(want_ext) || name.ends_with(".sha256") {
            continue;
        }
        let Some(url) = a.get("browser_download_url").and_then(|u| u.as_str()) else {
            continue;
        };
        let url = url.to_string();
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

/// 已下载并校验、等待安装的更新包（存放在数据目录的暂存区）。
#[derive(Debug, Clone)]
pub struct StagedUpdate {
    pub version: String,
    pub path: PathBuf,
}

/// 暂存目录：数据目录下的 `updates/`，重启不丢失。
fn stage_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("MQTTX-GPUI")
        .join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建暂存目录失败: {e}"))?;
    Ok(dir)
}

/// 检查更新并静默下载到暂存区（供启动自动检查使用）。
/// 返回 Ok(None) 表示已是最新；错误仅记录、不打扰用户。
pub fn check_and_download() -> Result<Option<StagedUpdate>, String> {
    let Some(info) = check_latest()? else {
        return Ok(None);
    };
    // 已暂存同版本则不重复下载
    if let Some(staged) = load_staged()
        && staged.version == info.version
    {
        return Ok(Some(staged));
    }
    download_and_stage(&info, &|_, _| {}).map(Some)
}

/// 下载并校验到暂存区，返回待安装的更新。
///
/// `on_progress(downloaded, total)` 在阻塞线程上回调，total 为 0 表示未知。
pub fn download_and_stage(
    info: &UpdateInfo,
    on_progress: &dyn Fn(u64, u64),
) -> Result<StagedUpdate, String> {
    let dir = stage_dir()?;
    let final_name = asset_name_for(&info.version);
    // tmp 名加入毫秒时间戳：同 pid 并发下载不共用文件
    let tmp = dir.join(format!(
        "{}.{}.{}.tmp",
        final_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let dst = dir.join(&final_name);

    if let Err(e) = download_to(info, &tmp, on_progress) {
        // 失败路径统一清理临时文件
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    // 清掉其他版本的残留暂存文件，避免堆积与误装旧版
    remove_other_staged(&dir, &dst);
    std::fs::rename(&tmp, &dst).map_err(|e| format!("暂存失败: {e}"))?;
    Ok(StagedUpdate {
        version: info.version.clone(),
        path: dst,
    })
}

fn download_to(info: &UpdateInfo, tmp: &std::path::Path, on_progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let resp = agent()
        .get(&info.asset_url)
        .call()
        .map_err(|e| format!("下载失败: {e}"))?;
    let mut reader = resp.into_reader();
    let total = info.size;
    let mut file = std::fs::File::create(tmp).map_err(|e| format!("创建临时文件失败: {e}"))?;
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

    // 传输完整性：API 提供了大小却没下满/超量，视为损坏（read 返回 0
    // 无法区分干净结束与连接提前断开，必须与预期大小对账）
    if total > 0 && downloaded != total {
        return Err(format!(
            "下载不完整：期望 {total} 字节，实际 {downloaded} 字节"
        ));
    }

    // sha256 校验
    if let Some(expect) = fetch_expected_sha256(info)? {
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(&expect) {
            return Err(format!(
                "校验失败：期望 sha256 {expect}，实际 {actual}"
            ));
        }
    }
    Ok(())
}

/// 删除暂存目录中除 `keep` 外的其他版本暂存文件。
fn remove_other_staged(dir: &std::path::Path, keep: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p == *keep {
                continue;
            }
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("mqttx-v")
                && name.contains(platform_triple())
                && !name.ends_with(".tmp")
                && !name.ends_with(".sha256")
            {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
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

/// 从暂存文件名解析版本号（需匹配当前平台的命名约定；`.tmp` 不算）。
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

/// 扫描暂存区，返回待安装的更新（文件存在且比当前版本新才算）。
/// 多个版本并存时取最新版本（readdir 顺序不保证有序）。
pub fn load_staged() -> Option<StagedUpdate> {
    let dir = stage_dir().ok()?;
    let mut best: Option<StagedUpdate> = None;
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        if !e.path().is_file() {
            continue;
        }
        let name = e.file_name();
        let name = name.to_string_lossy().to_string();
        let Some(version) = parse_staged_name(&name) else {
            continue;
        };
        if !is_newer(&version, current_version()) {
            continue;
        }
        if best.as_ref().is_none_or(|b| is_newer(&version, &b.version)) {
            best = Some(StagedUpdate {
                version,
                path: e.path(),
            });
        }
    }
    best
}

/// 安装暂存的更新：替换自身并拉起新进程（旧文件改名 `.old`，
/// 新进程启动时由 [`cleanup_old`] 清理）。成功后调用方应退出当前进程。
pub fn install_staged(staged: &StagedUpdate) -> Result<(), String> {
    let cur = std::env::current_exe().map_err(|e| format!("无法定位自身: {e}"))?;
    let mut old_name = cur.clone().into_os_string();
    old_name.push(".old");
    let old = PathBuf::from(old_name);
    // 旧残留先清掉，避免改名失败
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&cur, &old).map_err(|e| format!("替换失败（可能无写入权限）: {e}"))?;
    if let Err(e) = std::fs::rename(&staged.path, &cur) {
        // 回滚，尽量保住当前可执行文件
        let rollback = std::fs::rename(&old, &cur);
        return Err(format!(
            "替换失败: {e}；{}",
            rollback_result(rollback, &old)
        ));
    }
    if let Err(e) = std::process::Command::new(&cur).spawn() {
        // 新文件无法启动（常见：杀毒软件锁定刚落地的文件）：恢复旧版本
        let _ = std::fs::remove_file(&cur);
        let rollback = std::fs::rename(&old, &cur);
        return Err(format!(
            "启动新版本失败: {e}；{}",
            rollback_result(rollback, &old)
        ));
    }
    clear_install_consent();
    Ok(())
}

/// 描述回滚结果；回滚失败时提示 `.old` 备份仍保留在原处。
fn rollback_result(rollback: std::io::Result<()>, old: &std::path::Path) -> String {
    match rollback {
        Ok(()) => "已恢复原版本".to_string(),
        Err(re) => format!(
            "回滚失败（{re}），原版本备份保留在 {}",
            old.display()
        ),
    }
}

/// 安装同意标记：仅当用户在更新对话框明确选择「下次启动安装」后才写入，
/// 启动时据此自动安装——用户未选择的更新不会被强制替换。
fn consent_path() -> Result<PathBuf, String> {
    stage_dir().map(|d| d.join("install-consent.txt"))
}

/// 记录对指定版本的安装同意。
pub fn mark_install_consent(version: &str) {
    if let Ok(p) = consent_path() {
        let _ = std::fs::write(p, version);
    }
}

/// 清除安装同意标记。
pub fn clear_install_consent() {
    if let Ok(p) = consent_path() {
        let _ = std::fs::remove_file(p);
    }
}

/// 同意标记是否与暂存的版本匹配（版本不匹配视为未同意）。
pub fn install_consent_matches(staged_version: &str) -> bool {
    consent_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|v| v.trim() == staged_version)
        .unwrap_or(false)
}

/// 用户拒绝本次更新：删除暂存包与同意标记。
pub fn discard_staged(staged: &StagedUpdate) {
    let _ = std::fs::remove_file(&staged.path);
    clear_install_consent();
}

/// 启动时清理上次更新遗留的旧文件（改名失败的 `.old`、中断的 `.tmp`）。
pub fn cleanup_old() {
    if let Ok(cur) = std::env::current_exe()
        && let Some(dir) = cur.parent()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for e in entries.flatten() {
            // Windows 文件系统不区分大小写，统一按小写匹配
            let name = e.file_name();
            let lower = name.to_string_lossy().to_lowercase();
            if (lower.ends_with(".old") && lower.starts_with("mqttx"))
                || lower.ends_with(".update.tmp")
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    // 暂存目录里中断的 .tmp。跳过最近修改的文件，避免误删
    // 并行实例正在写入的临时文件。
    if let Ok(dir) = stage_dir()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        for e in entries.flatten() {
            let name = e.file_name();
            if !name.to_string_lossy().ends_with(".tmp") {
                continue;
            }
            let recent = std::fs::metadata(e.path())
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age < std::time::Duration::from_secs(10 * 60));
            if recent {
                continue;
            }
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
