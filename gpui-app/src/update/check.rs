//! 更新检查：访问 Gitea Release API，比较版本并挑选本平台资产。

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
        .map_err(|e| format!("请求失败: {e}"))?
        .into_body();
    body.read_json::<serde_json::Value>()
        .map_err(|e| format!("解析响应失败: {e}"))
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
                .find(|b| {
                    b.get("name").and_then(|n| n.as_str()) == Some(&format!("{name}.sha256"))
                })
                .and_then(|b| b.get("browser_download_url"))
                .and_then(|u| u.as_str())
                .map(|s| s.to_string());
            return Some((name.to_string(), url, size, sha));
        }
    }
    None
}

/// 检查更新。返回 Ok(None) 表示已是最新。
pub fn check_latest() -> Result<Option<UpdateInfo>, String> {
    let base = RELEASES_API;
    // 优先 latest 端点（GitHub 恒可用）；回退到列表取第一个非草稿/预发布，作容错
    let rel = match http_get_json(&format!("{base}/latest")) {
        Ok(v) => v,
        Err(_) => {
            let list = http_get_json(&format!("{base}?per_page=5"))?;
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
    let Some((name, url, size, sha)) = pick_assets(&rel) else {
        return Err(format!("Release {tag} 没有当前平台（{}）的产物", platform_triple()));
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
    }))
}

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
