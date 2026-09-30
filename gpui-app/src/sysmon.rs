//! 进程自身资源采样（CPU/内存/线程/句柄）。
//!
//! 仅 Windows 实现数据采集（`windows` crate）；其他平台采样返回全零数据，
//! 调用方按缺失能力展示。窗口置顶等平台操作见 [`crate::platform`]。

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
    /// 自上次采样以来的 CPU 占用（占整机容量的百分比 0~100，已按核数归一，
    /// 与任务管理器的"总利用率"同口径）
    pub cpu_percent: f32,
    /// 线程数
    pub threads: u32,
    /// 句柄数（采样失败为 0）
    pub handles: u32,
}

/// 周期采样器：内部记录上次 CPU 时间，用于差分计算占用率。
// pid/last_cpu/last_percent 仅 Windows 采样路径读写；非 Windows 下该路径被 cfg
// 移除，这三个字段会成为 dead_code（CI 的 -D warnings 下即错误），按平台放行。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct Sampler {
    pid: u32,
    last_cpu: Option<(u64, u64, Instant)>,
    /// 上一次成功算出的占用率：采样间隔过近时沿用，避免读数跳回 0
    last_percent: f32,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            pid: std::process::id(),
            last_cpu: None,
            last_percent: 0.0,
        }
    }

    /// 采样一次；CPU 占用首次采样为 0（无差分基准）。
    pub fn sample(&mut self) -> Option<Sample> {
        // 属性 cfg 分派而非 cfg!() 运行时判断：非 Windows 下 sample_windows
        // 被 cfg 移除，cfg! 分支仍参与编译会报方法不存在（CI Test job 实测暴露）
        #[cfg(target_os = "windows")]
        {
            self.sample_windows()
        }
        #[cfg(not(target_os = "windows"))]
        {
            Some(Sample::default())
        }
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
                    self.last_percent = cpu_percent;
                    // 只有真正算出结果才前移差分基准：否则连续两次快速采样
                    // 会既报 0 又把基准推到"刚刚"，用户手点"立即刷新"永远读到 0%
                    self.last_cpu = Some((k, u, now));
                } else {
                    cpu_percent = self.last_percent;
                }
            } else {
                self.last_cpu = Some((k, u, now));
            }
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
        // 采样能力分平台：Windows 采集真实数据，其余平台按契约返回全零占位样本。
        // 这里用 if cfg! 而不是 #[cfg]：两个分支都参与编译，避免只在某一个平台上
        // 才发现语法/类型错误。此前无条件断言 > 0，导致 Linux 上 cargo test 必失败，
        // 而 CI 的 test job 正是跑在 ubuntu-latest 上，v* tag 会直接停在测试阶段。
        if cfg!(target_os = "windows") {
            assert!(second.working_set > 0, "工作集应大于 0");
        } else {
            assert_eq!(second.working_set, 0, "非 Windows 平台应返回全零占位样本");
        }
        let _ = (first, second);
    }

    #[test]
    fn uptime_increases() {
        init();
        std::thread::sleep(Duration::from_millis(10));
        assert!(uptime().as_millis() >= 10);
    }
}
