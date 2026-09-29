//! 进程自身资源采样与窗口置顶辅助。
//!
//! 仅 Windows 实现数据采集（`windows` crate）；其他平台采样返回
//! 全零数据、置顶返回 false，调用方按缺失能力展示。

use std::time::{Duration, Instant};

/// 进程资源的一次采样。
#[derive(Debug, Clone, Default)]
pub struct Sample {
    /// 工作集（物理内存占用）
    pub working_set: u64,
    /// 峰值工作集（本次会话以来最高）
    pub peak_working_set: u64,
    /// 提交内存（私有字节）
    pub private_bytes: u64,
    /// 自上次采样以来的 CPU 占用百分比（按可用核数归一，0~100×核数内）
    pub cpu_percent: f32,
    /// 线程数
    pub threads: u32,
    /// 句柄数（采样失败为 0）
    pub handles: u32,
}

/// 周期采样器：内部记录上次 CPU 时间，用于差分计算占用率。
pub struct Sampler {
    pid: u32,
    last_cpu: Option<(u64, u64, Instant)>,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            pid: std::process::id(),
            last_cpu: None,
        }
    }

    /// 采样一次；CPU 占用首次采样为 0（无差分基准）。
    pub fn sample(&mut self) -> Option<Sample> {
        if !cfg!(target_os = "windows") {
            return Some(Sample::default());
        }
        self.sample_windows()
    }

    #[cfg(target_os = "windows")]
    fn sample_windows(&mut self) -> Option<Sample> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
            THREADENTRY32,
        };
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        };
        use windows::Win32::System::Threading::{
            GetProcessHandleCount, GetProcessTimes, OpenProcess,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::Foundation::FILETIME;

        let process = unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, self.pid)
                .ok()?
        };

        // ── 内存 ──
        // windows 0.58 的 EX 结构是扁平的（cb 在首位），按 EX 大小传 cb，
        // GetProcessMemoryInfo 即填充 PrivateUsage 扩展字段
        let mut mem: PROCESS_MEMORY_COUNTERS_EX = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            PageFaultCount: 0,
            PeakWorkingSetSize: 0,
            WorkingSetSize: 0,
            QuotaPeakPagedPoolUsage: 0,
            QuotaPagedPoolUsage: 0,
            QuotaPeakNonPagedPoolUsage: 0,
            QuotaNonPagedPoolUsage: 0,
            PagefileUsage: 0,
            PeakPagefileUsage: 0,
            PrivateUsage: 0,
        };
        let ok = unsafe {
            GetProcessMemoryInfo(
                process,
                &mut mem as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
                mem.cb,
            )
            .is_ok()
        };
        if !ok {
            let _ = unsafe { CloseHandle(process) };
            return None;
        }

        // ── CPU 时间差分 ──
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let mut cpu_percent = 0f32;
        let now = Instant::now();
        let have_times = unsafe {
            GetProcessTimes(
                process,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
            .is_ok()
        };
        if have_times {
            let k = ((kernel.dwHighDateTime as u64) << 32) | kernel.dwLowDateTime as u64;
            let u = ((user.dwHighDateTime as u64) << 32) | user.dwLowDateTime as u64;
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
            if let Some((pk, pu, pt)) = self.last_cpu {
                let dt = now.duration_since(pt).as_secs_f32();
                if dt > 0.05 {
                    let cpu_secs = ((k - pk) + (u - pu)) as f32 / 1e7;
                    cpu_percent = cpu_secs / dt / cores * 100.;
                }
            }
            self.last_cpu = Some((k, u, now));
        }

        // ── 线程数 / 句柄数 ──
        let mut threads = 0u32;
        unsafe {
            if let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) {
                let mut entry = THREADENTRY32 {
                    dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                    ..std::mem::zeroed()
                };
                if Thread32First(snapshot, &mut entry).is_ok() {
                    loop {
                        if entry.th32OwnerProcessID == self.pid {
                            threads += 1;
                        }
                        if Thread32Next(snapshot, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snapshot);
            }
        }
        let mut handles = 0u32;
        unsafe {
            let _ = GetProcessHandleCount(process, &mut handles);
        }

        let _ = unsafe { CloseHandle(process) };
        Some(Sample {
            working_set: mem.WorkingSetSize as u64,
            peak_working_set: mem.PeakWorkingSetSize as u64,
            private_bytes: mem.PrivateUsage as u64,
            cpu_percent,
            threads,
            handles,
        })    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// 进程启动时调用一次（main / 库初始化），用于计算运行时长。
pub fn init() {
    let _ = START.set(Instant::now());
}

/// 进程运行时长；未调用 [`init`] 时返回 0。
pub fn uptime() -> Duration {
    START.get().map(|t| t.elapsed()).unwrap_or_default()
}

/// 窗口置顶 / 取消置顶。返回是否成功（非 Windows 或句柄不可得为 false）。
pub fn set_topmost(window: &gpui_kit::Window, topmost: bool) -> bool {
    if !cfg!(target_os = "windows") {
        return false;
    }
    set_topmost_windows(window, topmost)
}

#[cfg(target_os = "windows")]
fn set_topmost_windows(window: &gpui_kit::Window, topmost: bool) -> bool {
    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SET_WINDOW_POS_FLAGS,
    };

    // gpui 的 Window 自带同名方法，HasWindowHandle 必须全路径调用
    let handle = match raw_window_handle::HasWindowHandle::window_handle(window) {
        Ok(h) => h,
        Err(_) => return false,
    };
    // WindowHandle::raw 是私有字段；HasWindowHandle 的实现返回自身拷贝，
    // trait 方法在此类型上拿不到 RawWindowHandle，用 deref 后的 as_raw 途径
    let raw = handle.as_raw();
    #[allow(clippy::infallible_destructuring_match)]
    let RawWindowHandle::Win32(w) = raw else {
        return false;
    };
    let hwnd = HWND(w.hwnd.get() as *mut core::ffi::c_void);
    let insert = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    let flags = SET_WINDOW_POS_FLAGS(SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    unsafe { SetWindowPos(hwnd, insert, 0, 0, 0, 0, flags).is_ok() }
}

#[cfg(not(target_os = "windows"))]
fn set_topmost_windows(_window: &gpui_kit::Window, _topmost: bool) -> bool {
    false
}

const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOACTIVATE: u32 = 0x0010;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_returns_a_sample() {
        init();
        let mut s = Sampler::new();
        let first = s.sample().expect("采样应成功");
        std::thread::sleep(Duration::from_millis(30));
        let second = s.sample().expect("第二次采样应成功");
        assert!(second.working_set > 0, "工作集应大于 0");
        let _ = (first, second);
    }

    #[test]
    fn uptime_increases() {
        init();
        std::thread::sleep(Duration::from_millis(10));
        assert!(uptime().as_millis() >= 10);
    }
}
