//! 更新执行：下载暂存（7z 优先）→ sha256 校验 → 重启自替换安装，以及同意标记与旧档清理。

use std::io::Read;
use std::path::{Path, PathBuf};


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
        // 安全前置校验：sevenz-rust 0.6.1 的 decompress_file 直接把 entry.name()
        // 拼到目标目录（de_funcs.rs: dest.join(entry.name())），不校验 ".."、
        // 绝对路径与盘符，还会为条目创建父目录，恶意归档足以越界写文件。
        // 打包端约定归档内只有 binary_name 一个条目，因此这里要求完全一致。
        if let Err(e) = verify_archive_entries(&tmp, &binary_name) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
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
    // 非 Windows 平台补可执行位：File::create 落盘默认 0644，rename 保留权限，
    // 直接 spawn 会 EACCES —— 这正是 Linux/macOS 自更新必定失败的原因。
    if let Err(e) = make_executable(&staged_path) {
        let _ = std::fs::remove_file(&staged_path);
        return Err(e);
    }
    // 记录最终二进制的哈希：安装同意标记与安装前复核都绑定它
    let sha256 = file_sha256(&staged_path)?;
    Ok(StagedUpdate {
        version: info.version.clone(),
        path: staged_path,
        sha256,
    })
}

/// 计算文件的 sha256（小写 hex）。
fn file_sha256(path: &Path) -> Result<String, String> {
    use sha2::Digest as _;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("打开 {} 失败: {e}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    // digest 0.11 的 Output 不再实现 LowerHex，手动转 hex
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// 校验 7z 归档的条目清单（只读归档头，不解压数据）。
///
/// 依赖 sevenz-rust 0.6.1 的 `decompress_file` 会把 `entry.name()` 直接拼到目标目录
/// （`de_funcs.rs: dest.join(entry.name())`），既不校验 `..`、绝对路径与盘符，还会为
/// 条目自动创建父目录——带恶意条目的压缩包足以写出暂存目录之外（如启动目录），
/// 随后被 `install_staged` 执行。这里在解压**之前**按归档头校验条目：必须恰好一个，
/// 且名字与约定文件名完全一致，任何偏差都拒绝（fail-closed）。
fn verify_archive_entries(archive: &Path, expected: &str) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("打开更新包失败: {e}"))?;
    let len = file
        .metadata()
        .map_err(|e| format!("读取更新包大小失败: {e}"))?
        .len();
    let reader = sevenz_rust::SevenZReader::new(file, len, sevenz_rust::Password::empty())
        .map_err(|e| format!("解析更新包失败: {e}"))?;
    let files = &reader.archive().files;
    if files.len() != 1 {
        return Err(format!(
            "更新包应恰好包含 1 个条目 '{expected}'，实际 {} 个",
            files.len()
        ));
    }
    let entry = &files[0];
    let name = entry.name();
    if entry.is_directory() || name != expected || name.contains('/') || name.contains('\\') {
        return Err(format!(
            "更新包内容异常：条目 '{name}' 不符合约定（期望仅 '{expected}'）"
        ));
    }
    Ok(())
}

/// Unix 下补可执行位（Windows 无此概念，空实现）。
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)
        .map_err(|e| format!("读取文件权限失败: {e}"))?
        .permissions()
        .mode();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o755))
        .map_err(|e| format!("设置可执行权限失败: {e}"))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// 把新版本文件放到目标位置。
///
/// 暂存在数据目录（`%APPDATA%` / `~/.local/share`），可执行文件在安装目录，二者可能
/// 不同卷：Windows 的 `MoveFile` 与 Unix 的 `rename` 跨卷都会失败，因此失败时回退为
/// 「复制 + 刷盘 + 删除源文件」，而不是直接报"可能无写入权限"。
fn place_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            let mut input = std::fs::File::open(src)?;
            let mut output = std::fs::File::create(dst)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            drop(output);
            let _ = std::fs::remove_file(src);
            Ok(())
        }
    }
}

/// 安装互斥锁：暂存目录下的 `install.lock`。
///
/// 用 `create_new` 原子抢占；超过 10 分钟的锁视为上次异常退出（`exit` 不跑 Drop）
/// 留下的陈旧锁，可被接管。两个实例同时安装会互删暂存包、争抢同一个 `.old`。
struct InstallLock {
    path: PathBuf,
}

impl InstallLock {
    fn acquire() -> Result<Self, String> {
        let path = stage_dir()?.join("install.lock");
        let stale_after = std::time::Duration::from_secs(10 * 60);
        for attempt in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    use std::io::Write as _;
                    let _ = write!(f, "{}", std::process::id());
                    return Ok(Self { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let is_stale = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > stale_after);
                    if is_stale && attempt == 0 {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    return Err("另一个实例正在安装更新，已跳过本次安装".into());
                }
                Err(e) => return Err(format!("创建安装锁失败: {e}")),
            }
        }
        Err("创建安装锁失败".into())
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn download_to(info: &UpdateInfo, tmp: &std::path::Path, on_progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let resp = agent()
        .get(&info.asset_url)
        .call()
        .map_err(|e| format!("下载失败: {e}"))?;
    let mut reader = resp.into_body().into_reader();
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

    // sha256 校验：缺侧车不再静默跳过——一次漏传就会让所有客户端在"零完整性校验"
    // 状态下安装，而 UI 仍宣称"已通过 sha256 校验"。发布脚本两侧都会生成侧车，
    // 因此这里 fail-closed 不会误伤正常发布。
    let expect = fetch_expected_sha256(info)?.ok_or_else(|| {
        "Release 未提供 .sha256 校验文件，无法验证完整性，已拒绝安装".to_string()
    })?;
    let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !actual.eq_ignore_ascii_case(&expect) {
        return Err(format!("校验失败：期望 sha256 {expect}，实际 {actual}"));
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
                && name.contains(platform_triple().as_str())
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
    let mut body = agent()
        .get(url)
        .call()
        .map_err(|e| format!("获取校验文件失败: {e}"))?
        .into_body();
    let text = body
        .read_to_string()
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
            // 启动扫描时重算哈希：安装同意标记绑定了它，缺了就无法判定"是否已同意"
            let Ok(sha256) = file_sha256(&e.path()) else {
                continue;
            };
            best = Some(StagedUpdate {
                version,
                path: e.path(),
                sha256,
            });
        }
    }
    best
}

/// 安装暂存的更新：替换自身并拉起新进程（旧文件改名 `.old`，
/// 新进程启动时由 [`cleanup_old`] 清理）。成功后调用方应退出当前进程。
pub fn install_staged(staged: &StagedUpdate) -> Result<(), String> {
    // 安装前复核：从"下载校验通过"到"下次启动安装"之间，暂存文件可能被替换或损坏
    let actual = file_sha256(&staged.path)?;
    if !actual.eq_ignore_ascii_case(&staged.sha256) {
        return Err(format!(
            "暂存文件校验失败（可能被替换或损坏）：期望 {}，实际 {actual}",
            staged.sha256
        ));
    }

    let cur = std::env::current_exe().map_err(|e| format!("无法定位自身: {e}"))?;
    let mut old_name = cur.clone().into_os_string();
    old_name.push(".old");
    let old = PathBuf::from(old_name);

    // 安装互斥：两个实例同时安装会互删暂存包、争抢同一个 .old
    let lock = InstallLock::acquire()?;

    // 旧残留先清掉，避免改名失败
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&cur, &old).map_err(|e| format!("替换失败（可能无写入权限）: {e}"))?;
    if let Err(e) = place_file(&staged.path, &cur) {
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
    // 显式释放：调用方随后就 exit/quit，Drop 不保证执行，残留锁会让下次安装被误判为并发
    drop(lock);
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

/// 记录安装同意：写入「版本 + 暂存文件 sha256」两行。
/// 绑定哈希后，暂存文件被替换就不再被视为"用户已同意安装的那一份"。
pub fn mark_install_consent(staged: &StagedUpdate) {
    if let Ok(p) = consent_path() {
        let _ = std::fs::write(p, format!("{}\n{}\n", staged.version, staged.sha256));
    }
}

/// 清除安装同意标记。
pub fn clear_install_consent() {
    if let Ok(p) = consent_path() {
        let _ = std::fs::remove_file(p);
    }
}

/// 同意标记是否与暂存的版本**与哈希**都匹配（任一不匹配视为未同意）。
pub fn install_consent_matches(staged: &StagedUpdate) -> bool {
    let Some(p) = consent_path().ok() else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(p) else {
        return false;
    };
    let mut lines = text.lines();
    let version_ok = lines.next().map(str::trim) == Some(staged.version.as_str());
    let sha_ok = lines.next().map(str::trim) == Some(staged.sha256.as_str());
    version_ok && sha_ok
}

/// 用户拒绝本次更新：删除暂存包与同意标记。
pub fn discard_staged(staged: &StagedUpdate) {
    let _ = std::fs::remove_file(&staged.path);
    clear_install_consent();
}

/// `.old` 回滚备份的保留时长：新版本首启不立即删除备份，
/// 否则新版本一旦启动即崩溃，用户就再也没有自助回退的路径。
const OLD_BACKUP_KEEP: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

/// 启动时清理上次更新遗留的旧文件（改名失败的 `.old`、中断的 `.tmp`）。
///
/// `.old` 是本版本唯一的回滚备份：**不能一启动就删**。策略是保留最近一份直到超过
/// `OLD_BACKUP_KEEP`，更早的备份立即清理。
pub fn cleanup_old() {
    if let Ok(cur) = std::env::current_exe()
        && let Some(dir) = cur.parent()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        let mut backups: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        for e in entries.flatten() {
            // Windows 文件系统不区分大小写，统一按小写匹配
            let name = e.file_name();
            let lower = name.to_string_lossy().to_lowercase();
            if lower.ends_with(".old") && lower.starts_with("mqttx") {
                let modified = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                backups.push((modified, e.path()));
            }
        }
        // 新的在前
        backups.sort_by_key(|b| std::cmp::Reverse(b.0));
        for (i, (modified, path)) in backups.iter().enumerate() {
            let age = modified.elapsed().unwrap_or_default();
            // 保留最近一份作为回滚备份，直到超过保留期；其余立即清理
            if i == 0 && age < OLD_BACKUP_KEEP {
                continue;
            }
            let _ = std::fs::remove_file(path);
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

    /// sha256→hex 转换（digest 0.11 起 Output 不再实现 LowerHex）是 OTA
    /// 完整性校验的关键路径，用固定向量锁定。
    #[test]
    fn file_sha256_matches_known_vector() {
        let path = std::env::temp_dir().join(format!(
            "mqttx-sha-test-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, b"abc").expect("写入测试文件");
        let hex = file_sha256(&path).expect("计算 sha256");
        let _ = std::fs::remove_file(&path);
        // echo -n abc | sha256sum
        assert_eq!(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
