//! 更新检查：访问 GitHub Releases，比较版本并挑选本平台资产。

use super::*;

pub(super) fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            // GitHub API 要求请求携带 User-Agent（缺省 ureq/x.y 也能过，
            // 但标识应用更符合其规范，也便于服务端区分流量）
            .user_agent("mqttx-ota")
            // 整体上限：防止慢速滴流的下载无限占用阻塞线程
            .timeout_global(Some(std::time::Duration::from_secs(10 * 60)))
            .timeout_connect(Some(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS)))
            .build(),
    )
}

pub(super) fn http_get_json(url: &str) -> Result<serde_json::Value, String> {
    let mut body = agent()
        .get(url)
        .call()
        .map_err(|e| crate::ui::i18n::tf("update.err_request", &[("e", &e)]))?
        .into_body();
    body.read_json::<serde_json::Value>()
        .map_err(|e| crate::ui::i18n::tf("update.err_parse_response", &[("e", &e)]))
}

pub fn pick_assets(rel: &serde_json::Value) -> Option<(String, String, u64, Option<String>)> {
    let assets = rel.get("assets")?.as_array()?;
    let triple = platform_triple();
    // 产物命名约定：`mqttx-v<ver>-<triple>-mqttx[.exe]`（压缩包再加 `.7z`）。
    // 必须按固定后缀精确匹配：非 Windows 下扩展名为空串时 `ends_with("")` 恒真，
    // 任何含三元组的附件（.tar.gz/.deb/.txt）都会被当成可执行文件下载并替换自身。
    let bin_name = if cfg!(windows) { "mqttx.exe" } else { "mqttx" };

    // 第一轮 7z 压缩包，第二轮裸二进制
    for suffix in [".7z", ""] {
        for a in assets {
            let Some(name) = a.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            if !name.contains(triple.as_str())
                || !name.ends_with(&format!("{bin_name}{suffix}"))
                || name.ends_with(".sha256")
            {
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
                .find(|b| b.get("name").and_then(|n| n.as_str()) == Some(&format!("{name}.sha256")))
                .and_then(|b| b.get("browser_download_url"))
                .and_then(|u| u.as_str())
                .map(|s| s.to_string());
            return Some((name.to_string(), url, size, sha));
        }
    }
    None
}

/// 检查更新。返回 Ok(None) 表示已是最新。
///
/// 发现链（逐级兜底，全部失败才报错）：
/// 1. GitHub API —— 信息最全（notes/资产清单/大小），但 api.github.com 未认证
///    配额仅 60 次/小时/IP，共享出口 IP（VPN/办公 NAT）经常打满；
/// 2. GitHub Releases 页面 302 重定向 —— 无 API 配额，但拿不到 notes/大小；
/// 3. Gitee 列表 API —— 国内直连可达（免认证）；Gitee v5 没有 /releases/latest
///    API 端点，只能列出来筛第一个非预发布；assets 无 size 字段（进度未知）；
/// 4. Gitee Releases 页面 302 重定向 —— URL 形态与 GitHub 完全同构。
///
/// 完整性不依赖来源：sha256 侧车在下载阶段强制校验（见 install.rs）。
pub fn check_latest() -> Result<Option<UpdateInfo>, String> {
    let mut errors: Vec<String> = Vec::new();
    for attempt in [
        check_latest_via_api as fn() -> Result<Option<UpdateInfo>, String>,
        check_latest_via_redirect,
        check_latest_via_gitee_api,
        check_latest_via_gitee_redirect,
    ] {
        match attempt() {
            Ok(Some(info)) => return Ok(Some(info)),
            // 该渠道确认"已是最新"：这不是失败，直接采纳（各渠道指向同一发布源）
            Ok(None) => return Ok(None),
            // 每条错误自身已带"请求失败/页面异常"等上下文，直接拼接即可
            Err(e) => errors.push(e),
        }
    }
    Err(errors.join("；"))
}

fn check_latest_via_api() -> Result<Option<UpdateInfo>, String> {
    let base = RELEASES_API;
    // 优先 latest 端点（GitHub 恒可用）；回退到列表取第一个非草稿/预发布，作容错
    let rel = match http_get_json(&format!("{base}/latest")) {
        Ok(v) => v,
        Err(_) => {
            let list = http_get_json(&format!("{base}?per_page=5"))?;
            let first = list
                .as_array()
                .and_then(|l| {
                    l.iter().find(|r| {
                        r.get("draft").and_then(|d| d.as_bool()) == Some(false)
                            && r.get("prerelease").and_then(|d| d.as_bool()) == Some(false)
                    })
                })
                .ok_or_else(|| crate::ui::i18n::t("update.err_no_release").to_string())?;
            first.clone()
        }
    };

    let tag = rel
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| crate::ui::i18n::t("update.err_no_tag").to_string())?;
    if !is_newer(tag, current_version()) {
        return Ok(None);
    }
    let Some((name, url, size, sha)) = pick_assets(&rel) else {
        return Err(crate::ui::i18n::tf(
            "update.err_no_asset",
            &[("tag", &tag), ("triple", &platform_triple())],
        ));
    };
    Ok(Some(UpdateInfo {
        version: tag.trim_start_matches('v').to_string(),
        notes: rel
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or_default()
            .to_string(),
        asset_name: name,
        asset_url: url,
        sha256_url: sha,
        size,
        source: SourceKind::GitHubApi,
    }))
}

/// GitHub 页面 302 重定向发现。拿不到 notes 与资产清单：notes 置空、大小置 0，
/// 下载 URL 与 `.sha256` 侧车按命名约定构造，sha256 校验仍然强制执行。
fn check_latest_via_redirect() -> Result<Option<UpdateInfo>, String> {
    let (tag, page) = latest_tag_via_redirect(RELEASES_PAGE)?;
    if !is_newer(&tag, current_version()) {
        return Ok(None);
    }
    let info = update_info_from_tag(&tag, SourceKind::GitHubRedirect, &page);
    Ok(Some(info))
}

/// Gitee 兜底：列表 API（免认证）。Gitee v5 没有 /releases/latest API 端点，
/// 列表接口返回的第一个即最新；无 draft 字段（Gitee 即建即发布）、assets 无 size。
fn check_latest_via_gitee_api() -> Result<Option<UpdateInfo>, String> {
    let list = http_get_json(&format!("{GITEE_RELEASES_API}?per_page=5"))?;
    let first = list
        .as_array()
        .and_then(|l| {
            l.iter()
                .find(|r| r.get("prerelease").and_then(|d| d.as_bool()) != Some(true))
        })
        .ok_or_else(|| crate::ui::i18n::t("update.err_no_release").to_string())?;
    let tag = first
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| crate::ui::i18n::t("update.err_no_tag").to_string())?;
    if !is_newer(tag, current_version()) {
        return Ok(None);
    }
    let Some((name, url, size, sha)) = pick_assets(first) else {
        return Err(crate::ui::i18n::tf(
            "update.err_no_asset",
            &[("tag", &tag), ("triple", &platform_triple())],
        ));
    };
    Ok(Some(UpdateInfo {
        version: tag.trim_start_matches('v').to_string(),
        notes: first
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or_default()
            .to_string(),
        asset_name: name,
        asset_url: url,
        sha256_url: sha,
        // Gitee 列表 JSON 的 assets 不带 size 字段，unwrap_or(0) → 进度未知、大小对账跳过
        size,
        source: SourceKind::GiteeApi,
    }))
}

/// Gitee 页面 302 重定向发现（`/releases/latest` → `/releases/tag/<tag>`，
/// 与 GitHub 同构，复用同一套 tag 提取与 URL 构造）。
fn check_latest_via_gitee_redirect() -> Result<Option<UpdateInfo>, String> {
    let (tag, page) = latest_tag_via_redirect(GITEE_RELEASES_PAGE)?;
    if !is_newer(&tag, current_version()) {
        return Ok(None);
    }
    let info = update_info_from_tag(&tag, SourceKind::GiteeRedirect, &page);
    Ok(Some(info))
}

/// 跟踪 `{page}/latest` 的 302，从最终 URL 提取最新 tag。GitHub 与 Gitee 同构。
fn latest_tag_via_redirect(page: &str) -> Result<(String, String), String> {
    use ureq::ResponseExt as _;
    let resp = agent()
        .get(&format!("{page}/latest"))
        .call()
        .map_err(|e| crate::ui::i18n::tf("update.err_page_request", &[("e", &e)]))?;
    let uri = resp.get_uri().to_string();
    let Some(tag) = tag_from_releases_url(&uri) else {
        return Err(crate::ui::i18n::tf("update.err_page_url", &[("uri", &uri)]));
    };
    Ok((tag.to_string(), page.to_string()))
}

/// 从 tag 按命名约定构造重定向路径的 UpdateInfo（GitHub/Gitee 共用）。
fn update_info_from_tag(tag: &str, source: SourceKind, page: &str) -> UpdateInfo {
    let version = tag.trim_start_matches('v');
    let triple = platform_triple();
    let bin = if cfg!(windows) { "mqttx.exe" } else { "mqttx" };
    // 与 publish 脚本/工作流的产物命名约定一致（pick_assets 同款匹配规则）
    let asset_base = format!("mqttx-v{version}-{triple}-{bin}");
    let asset = format!("{asset_base}.7z");
    UpdateInfo {
        version: version.to_string(),
        notes: String::new(),
        asset_name: asset.clone(),
        asset_url: format!("{page}/download/{tag}/{asset}"),
        sha256_url: Some(format!("{page}/download/{tag}/{asset}.sha256")),
        size: 0,
        source,
    }
}

/// 从 Releases 页面 URL 提取 tag（如 `.../releases/tag/v1.0.1` → `v1.0.1`）。
fn tag_from_releases_url(uri: &str) -> Option<&str> {
    let tag = uri.split("/releases/tag/").nth(1)?;
    let tag = tag.trim_end_matches('/');
    // 防御：tag 不含路径段，且必须以 v 开头（约定）
    (tag.starts_with('v') && !tag.contains('/')).then_some(tag)
}

/// 自动检查的节流窗口：24h 内启动不再重复请求 GitHub（官方 auto-update
/// 指南建议缓存上次检查时间）。手动「检查更新」不受此限制。
const AUTO_CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;

fn stamp_path() -> std::path::PathBuf {
    crate::store::Storage::new()
        .dir()
        .join(".last-update-check")
}

fn last_check_recent() -> bool {
    let Ok(meta) = std::fs::metadata(stamp_path()) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|d| d.as_secs() < AUTO_CHECK_INTERVAL_SECS as u64)
        .unwrap_or(false)
}

fn touch_check_stamp() {
    let _ = std::fs::write(stamp_path(), chrono::Local::now().to_rfc3339());
}

/// 返回 Ok(None) 表示已是最新；错误仅记录、不打扰用户。
/// 走 24h 节流：距上次自动检查不足一天时直接跳过（返回 None）。
pub fn check_and_download() -> Result<Option<StagedUpdate>, String> {
    if last_check_recent() {
        return Ok(None);
    }
    let Some(info) = check_latest()? else {
        // 检查成功且已是最新才盖章。此前盖章在检查之前，一次瞬时网络故障
        // 会让自动检查静默哑火一整天。
        touch_check_stamp();
        return Ok(None);
    };
    // 已暂存同版本则不重复下载
    if let Some(staged) = load_staged()
        && staged.version == info.version
    {
        touch_check_stamp();
        return Ok(Some(staged));
    }
    download_and_stage(&info, &|_, _| {}).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_extraction_from_releases_url() {
        // /releases/latest 302 后的最终 URL 形态（GitHub 与 Gitee 同构）
        assert_eq!(
            tag_from_releases_url("https://github.com/LiJoeAllen/mqttx/releases/tag/v1.0.1"),
            Some("v1.0.1")
        );
        assert_eq!(
            tag_from_releases_url("https://gitee.com/LiJoeAllen/mqttx/releases/tag/v1.2.1"),
            Some("v1.2.1")
        );
        assert_eq!(
            tag_from_releases_url("https://github.com/LiJoeAllen/mqttx/releases/tag/v1.2.3-rc.1/"),
            Some("v1.2.3-rc.1")
        );
        // 非 tag 页面 / 异常形态一律拒绝
        assert_eq!(
            tag_from_releases_url("https://github.com/LiJoeAllen/mqttx/releases"),
            None
        );
        assert_eq!(
            tag_from_releases_url("https://github.com/LiJoeAllen/mqttx/releases/tag/1.0.0"),
            None
        );
        assert_eq!(
            tag_from_releases_url("https://github.com/LiJoeAllen/mqttx/releases/tag/v1.0.0/extra"),
            None
        );
    }

    /// Gitee 列表 API 的真实形状（2026-11 实测）：assets 无 size 字段、
    /// release 无 draft 字段、prerelease 为布尔。pick_assets 必须能在缺
    /// size 的 JSON 上工作（size 落到 0 → 进度未知、大小对账跳过）。
    #[test]
    fn pick_assets_handles_gitee_shape_without_size() {
        let triple = platform_triple();
        let bin = if cfg!(windows) { "mqttx.exe" } else { "mqttx" };
        let base = format!("mqttx-v1.2.2-{triple}-{bin}");
        let rel: serde_json::Value = serde_json::json!({
            "tag_name": "v1.2.2",
            "prerelease": false,
            "body": "notes",
            "assets": [
                {"name": format!("{base}.7z"),
                 "browser_download_url": format!("https://gitee.com/x/dl/{base}.7z")},
                {"name": format!("{base}.7z.sha256"),
                 "browser_download_url": format!("https://gitee.com/x/dl/{base}.7z.sha256")}
            ]
        });
        let (name, _url, size, sha) = pick_assets(&rel).expect("应选中 Gitee 形状的 7z");
        assert!(name.ends_with(".7z"));
        assert_eq!(size, 0, "Gitee 无 size 字段应落 0");
        assert_eq!(
            sha.as_deref(),
            Some(format!("https://gitee.com/x/dl/{base}.7z.sha256").as_str())
        );
    }

    /// 重定向路径的 URL 构造：Gitee 页面与 GitHub 同构，按约定拼下载与侧车地址。
    #[test]
    fn update_info_from_tag_builds_convention_urls() {
        let info = update_info_from_tag("v1.2.2", SourceKind::GiteeRedirect, GITEE_RELEASES_PAGE);
        let asset = asset_name_for("1.2.2");
        assert_eq!(info.version, "1.2.2");
        assert_eq!(info.source, SourceKind::GiteeRedirect);
        assert_eq!(
            info.asset_url,
            format!("{GITEE_RELEASES_PAGE}/download/v1.2.2/{asset}")
        );
        assert_eq!(
            info.sha256_url.as_deref(),
            Some(format!("{GITEE_RELEASES_PAGE}/download/v1.2.2/{asset}.sha256").as_str())
        );
        assert_eq!(info.size, 0);
        assert!(info.notes.is_empty());
    }
}
