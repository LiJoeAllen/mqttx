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
    std::fs::create_dir_all(&dir)
        .map_err(|e| crate::ui::i18n::tf("update.err_stage_dir", &[("e", &e)]))?;
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

    if let Err(e) = download_with_retry(&download_candidates(info), &tmp, info.size, on_progress) {
        // 失败路径统一清理临时文件
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    // sha256 校验（fail-closed）：跨源候选逐个尝试侧车（API 显式 URL 优先，其余按
    // `<asset_url>.sha256` 构造）。一次漏传就会让所有客户端在"零完整性校验"状态下
    // 安装，而 UI 仍宣称"已通过 sha256 校验"，所以缺侧车与校验不过都拒绝安装。
    // 校验放在下载完成后整体读盘：断点续传会把一个文件拆进多个连接/多个镜像源，
    // 流式哈希的增量状态管理远比一次性重读复杂，20MB 级文件的重读成本可忽略。
    let expect = match fetch_expected_sha256(&sidecar_candidates(info)) {
        Ok(Some(h)) => h,
        Ok(None) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(crate::ui::i18n::t("update.err_no_sha_sidecar").to_string());
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    let actual = match file_sha256(&tmp) {
        Ok(h) => h,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    if !actual.eq_ignore_ascii_case(&expect) {
        let _ = std::fs::remove_file(&tmp);
        return Err(crate::ui::i18n::tf(
            "update.err_sha_mismatch",
            &[("expect", &expect), ("actual", &actual)],
        ));
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
            return Err(crate::ui::i18n::tf("update.err_decompress", &[("e", &e)]));
        }
        let _ = std::fs::remove_file(&tmp);
        let extracted = dir.join(&binary_name);
        if !extracted.exists() {
            return Err(crate::ui::i18n::tf(
                "update.err_missing_binary",
                &[("name", &binary_name)],
            ));
        }
        // 确认解压产物后再清理其他版本，最后返回二进制路径
        remove_other_staged(&dir, &extracted);
        extracted
    } else {
        remove_other_staged(&dir, &dst);
        std::fs::rename(&tmp, &dst)
            .map_err(|e| crate::ui::i18n::tf("update.err_stage_rename", &[("e", &e)]))?;
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
    let mut file = std::fs::File::open(path).map_err(|e| {
        crate::ui::i18n::tf("update.err_open_file", &[("p", &path.display()), ("e", &e)])
    })?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| {
            crate::ui::i18n::tf("update.err_read_file", &[("p", &path.display()), ("e", &e)])
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    // digest 0.11 的 Output 不再实现 LowerHex，手动转 hex
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// 校验 7z 归档的条目清单（只读归档头，不解压数据）。
///
/// 依赖 sevenz-rust 0.6.1 的 `decompress_file` 会把 `entry.name()` 直接拼到目标目录
/// （`de_funcs.rs: dest.join(entry.name())`），既不校验 `..`、绝对路径与盘符，还会为
/// 条目自动创建父目录——带恶意条目的压缩包足以写出暂存目录之外（如启动目录），
/// 随后被 `install_staged` 执行。这里在解压**之前**按归档头校验条目：必须恰好一个，
/// 且名字与约定文件名完全一致，任何偏差都拒绝（fail-closed）。
fn verify_archive_entries(archive: &Path, expected: &str) -> Result<(), String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| crate::ui::i18n::tf("update.err_open_archive", &[("e", &e)]))?;
    let len = file
        .metadata()
        .map_err(|e| crate::ui::i18n::tf("update.err_archive_size", &[("e", &e)]))?
        .len();
    let reader = sevenz_rust::SevenZReader::new(file, len, sevenz_rust::Password::empty())
        .map_err(|e| crate::ui::i18n::tf("update.err_parse_archive", &[("e", &e)]))?;
    let files = &reader.archive().files;
    if files.len() != 1 {
        return Err(crate::ui::i18n::tf(
            "update.err_archive_entries",
            &[("expected", &expected), ("n", &files.len())],
        ));
    }
    let entry = &files[0];
    let name = entry.name();
    if entry.is_directory() || name != expected || name.contains('/') || name.contains('\\') {
        return Err(crate::ui::i18n::tf(
            "update.err_archive_entry",
            &[("name", &name), ("expected", &expected)],
        ));
    }
    Ok(())
}

/// Unix 下补可执行位（Windows 无此概念，空实现）。
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)
        .map_err(|e| crate::ui::i18n::tf("update.err_read_permissions", &[("e", &e)]))?
        .permissions()
        .mode();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o755))
        .map_err(|e| crate::ui::i18n::tf("update.err_set_executable", &[("e", &e)]))
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
                    return Err(crate::ui::i18n::t("update.err_install_busy").to_string());
                }
                Err(e) => return Err(crate::ui::i18n::tf("update.err_lock_create", &[("e", &e)])),
            }
        }
        Err(crate::ui::i18n::t("update.err_lock_create_generic").to_string())
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 下载候选通道：发现源优先，其余镜像按命名约定构造（同一资产名在
/// GitHub/Gitee/GitCode 三处一致）。跨源 URL 没有发现阶段佐证，垃圾响应
/// 由 sha256 校验兜底，不会误装。
fn download_candidates(info: &UpdateInfo) -> Vec<String> {
    let mut urls = vec![info.asset_url.clone()];
    let tag = format!("v{}", info.version);
    for base in [GITEE_RELEASES_PAGE, GITCODE_RELEASES_PAGE] {
        let u = format!("{base}/download/{tag}/{}", info.asset_name);
        if !urls.contains(&u) {
            urls.push(u);
        }
    }
    urls
}

/// sha256 侧车候选：API 显式 URL 优先，其余按 `<asset_url>.sha256` 构造。
fn sidecar_candidates(info: &UpdateInfo) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(u) = &info.sha256_url {
        urls.push(u.clone());
    }
    for a in download_candidates(info) {
        let s = format!("{a}.sha256");
        if !urls.contains(&s) {
            urls.push(s);
        }
    }
    urls
}

/// 每个通道的重试次数：断点续传让重试成本只是增量传输，三次足够。
const DOWNLOAD_ATTEMPTS_PER_URL: usize = 3;
/// 重试退避（秒）：第 2、3 次尝试前等待。远端瞬断/代理抖动通常秒级恢复。
const RETRY_BACKOFF_SECS: [u64; 2] = [2, 8];

/// 依序尝试所有下载通道，通道内最多 [`DOWNLOAD_ATTEMPTS_PER_URL`] 次。
///
/// 临时文件跨尝试、跨通道保留已下载部分（断点续传）：字节即字节，来源混拼
/// 没有关系，最终由 sha256 一票裁决。全部通道重试穷尽才报错。
fn download_with_retry(
    urls: &[String],
    tmp: &Path,
    total: u64,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), String> {
    let mut last_err = String::new();
    for url in urls {
        for attempt in 0..DOWNLOAD_ATTEMPTS_PER_URL {
            if attempt > 0 {
                let secs = RETRY_BACKOFF_SECS[(attempt - 1).min(RETRY_BACKOFF_SECS.len() - 1)];
                std::thread::sleep(std::time::Duration::from_secs(secs));
            }
            match download_once(url, tmp, total, on_progress) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_err = e;
                    // 保留 tmp：下次尝试从已下载字节处续传
                }
            }
        }
    }
    Err(crate::ui::i18n::tf(
        "update.err_download_all_sources",
        &[("n", &urls.len()), ("last", &last_err)],
    ))
}

/// 单次完整下载尝试（支持从 tmp 现有字节续传）。返回 Ok 表示传输完整：
/// total 已知时长度对账通过，未知时以 EOF 为准（sha256 兜底）。
fn download_once(
    url: &str,
    tmp: &Path,
    total: u64,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), String> {
    let mut offset = std::fs::metadata(tmp).map(|m| m.len()).unwrap_or(0);
    // 残留超过预期大小：损坏的临时文件，从头来
    if total > 0 && offset > total {
        offset = 0;
    }
    let resp = if offset > 0 {
        agent()
            .get(url)
            .header("Range", &format!("bytes={offset}-"))
            .call()
    } else {
        agent().get(url).call()
    };
    let resp = resp.map_err(|e| crate::ui::i18n::tf("update.err_download", &[("e", &e)]))?;
    // 服务器无视 Range 回 200 全量（镜像附件节点行为不一）：截断从头重写
    let resumed = offset > 0 && resp.status().as_u16() == 206;
    let mut file = if resumed {
        std::fs::OpenOptions::new()
            .append(true)
            .open(tmp)
            .map_err(|e| crate::ui::i18n::tf("update.err_tmp_create", &[("e", &e)]))?
    } else {
        offset = 0;
        std::fs::File::create(tmp)
            .map_err(|e| crate::ui::i18n::tf("update.err_tmp_create", &[("e", &e)]))?
    };
    let mut reader = resp.into_body().into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded = offset;
    on_progress(downloaded, total);
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| crate::ui::i18n::tf("update.err_download_interrupted", &[("e", &e)]))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| crate::ui::i18n::tf("update.err_tmp_write", &[("e", &e)]))?;
        downloaded += n as u64;
        on_progress(downloaded, total);
    }
    std::io::Write::flush(&mut file).ok();
    drop(file);

    // 传输完整性：API 提供了大小却没下满/超量，视为损坏（read 返回 0
    // 无法区分干净结束与连接提前断开，必须与预期大小对账）
    if total > 0 && downloaded != total {
        return Err(crate::ui::i18n::tf(
            "update.err_size_mismatch",
            &[("total", &total), ("downloaded", &downloaded)],
        ));
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

/// 依次尝试各 `.sha256` 侧车候选，返回第一个内容合法的哈希。
/// 全部候选都不可用时返回 None，调用方 fail-closed 拒绝安装。
/// 单个候选的失败（网络/格式）静默跳过——最终由 None 或 sha256 对比兜底。
fn fetch_expected_sha256(candidates: &[String]) -> Result<Option<String>, String> {
    for url in candidates {
        let text = match agent().get(url).call() {
            Ok(resp) => match resp.into_body().read_to_string() {
                Ok(t) => t,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        // 常见格式：<hex>  <filename> 或纯 hex
        let hex = text
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase();
        if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(Some(hex));
        }
    }
    Ok(None)
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
        return Err(crate::ui::i18n::tf(
            "update.err_staged_verify",
            &[("expect", &staged.sha256), ("actual", &actual)],
        ));
    }

    let cur = std::env::current_exe()
        .map_err(|e| crate::ui::i18n::tf("update.err_locate_self", &[("e", &e)]))?;
    let mut old_name = cur.clone().into_os_string();
    old_name.push(".old");
    let old = PathBuf::from(old_name);

    // 安装互斥：两个实例同时安装会互删暂存包、争抢同一个 .old
    let lock = InstallLock::acquire()?;

    // 旧残留先清掉，避免改名失败
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&cur, &old)
        .map_err(|e| crate::ui::i18n::tf("update.err_replace_no_perm", &[("e", &e)]))?;
    if let Err(e) = place_file(&staged.path, &cur) {
        // 回滚，尽量保住当前可执行文件
        let rollback = std::fs::rename(&old, &cur);
        return Err(crate::ui::i18n::tf(
            "update.err_replace",
            &[("e", &e), ("rollback", &rollback_result(rollback, &old))],
        ));
    }
    if let Err(e) = std::process::Command::new(&cur).spawn() {
        // 新文件无法启动（常见：杀毒软件锁定刚落地的文件）：恢复旧版本
        let _ = std::fs::remove_file(&cur);
        let rollback = std::fs::rename(&old, &cur);
        return Err(crate::ui::i18n::tf(
            "update.err_spawn_new",
            &[("e", &e), ("rollback", &rollback_result(rollback, &old))],
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
        Ok(()) => crate::ui::i18n::t("update.rollback_done").to_string(),
        Err(re) => crate::ui::i18n::tf(
            "update.rollback_failed",
            &[("e", &re), ("p", &old.display())],
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
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    fn info_with_asset_url(asset_url: &str) -> UpdateInfo {
        UpdateInfo {
            version: "1.2.2".to_string(),
            notes: String::new(),
            asset_name: asset_name_for("1.2.2"),
            asset_url: asset_url.to_string(),
            sha256_url: None,
            size: 0,
            source: crate::update::SourceKind::GitHubApi,
        }
    }

    /// 下载通道：origin 恒在首位；Gitee/GitCode 按命名约定追加；
    /// origin 本身就是某镜像时不得重复（发现源与兜底源同一站点）。
    #[test]
    fn download_candidates_orders_origin_first_without_dupes() {
        let from_github = info_with_asset_url(&format!(
            "{}/download/v1.2.2/{}",
            RELEASES_PAGE,
            asset_name_for("1.2.2")
        ));
        let urls = download_candidates(&from_github);
        assert_eq!(urls.len(), 3, "GitHub 起源应追加 Gitee+GitCode: {urls:?}");
        assert_eq!(urls[0], from_github.asset_url);
        assert!(urls[1].starts_with(GITEE_RELEASES_PAGE));
        assert!(urls[2].starts_with(GITCODE_RELEASES_PAGE));
        assert!(urls[1].ends_with(&format!("/download/v1.2.2/{}", asset_name_for("1.2.2"))));

        let from_gitee = info_with_asset_url(&format!(
            "{}/download/v1.2.2/{}",
            GITEE_RELEASES_PAGE,
            asset_name_for("1.2.2")
        ));
        let urls = download_candidates(&from_gitee);
        assert_eq!(urls.len(), 2, "Gitee 起源不得重复出现: {urls:?}");
        assert_eq!(urls[0], from_gitee.asset_url);
        assert!(urls[1].starts_with(GITCODE_RELEASES_PAGE));
    }

    /// 侧车候选：API 显式 URL 优先；随后按候选下载 URL 逐一构造 .sha256，去重。
    #[test]
    fn sidecar_candidates_puts_explicit_url_first() {
        let mut info = info_with_asset_url(&format!(
            "{}/download/v1.2.2/{}",
            RELEASES_PAGE,
            asset_name_for("1.2.2")
        ));
        info.sha256_url = Some(format!("{}.sha256", info.asset_url));
        let urls = sidecar_candidates(&info);
        let expect = info.sha256_url.as_deref().expect("上一行刚设置");
        assert_eq!(urls.first().map(String::as_str), Some(expect));
        assert_eq!(urls.len(), 3, "显式 + 构造 2 条，无重复: {urls:?}");
        assert!(urls[1].starts_with(GITEE_RELEASES_PAGE));
        assert!(urls.iter().all(|u| u.ends_with(".sha256")));
    }
}
