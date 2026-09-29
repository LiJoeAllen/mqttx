//! 更新执行：下载暂存（7z 优先）→ sha256 校验 → 重启自替换安装，以及同意标记与旧档清理。

use std::io::Read;
use std::path::PathBuf;


use super::check::agent;
use super::*;

/// 暂存目录：数据目录下的 `updates/`，重启不丢失。
fn stage_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("MQTTX-GPUI")
        .join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建暂存目录失败: {e}"))?;
    Ok(dir)
}


/// 下载并校验到暂存区，返回待安装的更新。
///
/// 附件为 7z 压缩包：下载 → 校验 → 解压出二进制暂存（压缩包随即删除）。
/// 回退路径（裸二进制附件）直接校验暂存。
/// `on_progress(downloaded, total)` 在阻塞线程上回调，total 为 0 表示未知。
pub fn download_and_stage(
    info: &UpdateInfo,
    on_progress: &dyn Fn(u64, u64),
) -> Result<StagedUpdate, String> {
    let dir = stage_dir()?;
    let binary_name = binary_name_for(&info.version);
    let is_7z = info.asset_name.ends_with(".7z");
    let final_name = if is_7z {
        asset_name_for(&info.version)
    } else {
        binary_name.clone()
    };
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
    let staged_path = if is_7z {
        // 解压到暂存目录，取出二进制后删掉压缩包
        if let Err(e) = sevenz_rust::decompress_file(&tmp, &dir) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("解压更新包失败: {e}"));
        }
        let _ = std::fs::remove_file(&tmp);
        let extracted = dir.join(&binary_name);
        if !extracted.exists() {
            return Err(format!(
                "更新包内缺少 {}，压缩包内容异常",
                binary_name
            ));
        }
        // 确认解压产物后再清理其他版本，最后返回二进制路径
        remove_other_staged(&dir, &extracted);
        extracted
    } else {
        remove_other_staged(&dir, &dst);
        std::fs::rename(&tmp, &dst).map_err(|e| format!("暂存失败: {e}"))?;
        dst
    };
    Ok(StagedUpdate {
        version: info.version.clone(),
        path: staged_path,
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
                && !name.ends_with(".7z")
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
