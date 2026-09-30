//! 平台专属窗口操作。当前仅 Windows：置顶 / 取消置顶。

/// 窗口置顶 / 取消置顶。返回是否成功（非 Windows 或句柄不可得为 false）。
pub fn set_topmost(window: &gpui_kit::Window, topmost: bool) -> bool {
    // 属性 cfg 分派而非 cfg!() 运行时判断：非 Windows 下 set_topmost_windows
    // 被 cfg 移除，cfg! 分支仍参与编译会报方法不存在（CI Test job 实测暴露）
    #[cfg(target_os = "windows")]
    {
        set_topmost_windows(window, topmost)
    }
    #[cfg(not(target_os = "windows"))]
    {
        set_topmost_windows(window, topmost)
    }
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
    // windows 0.62 起 SetWindowPos 的插入位置参数改为 Option<HWND>
    unsafe { SetWindowPos(hwnd, Some(insert), 0, 0, 0, 0, flags).is_ok() }
}

#[cfg(not(target_os = "windows"))]
fn set_topmost_windows(_window: &gpui_kit::Window, _topmost: bool) -> bool {
    false
}

// 仅 Windows 采样路径使用；非 Windows 下该路径被 cfg 移除，常量会成为
// dead_code（CI 的 -D warnings 下即错误），按平台放行。
#[cfg(target_os = "windows")]
const SWP_NOSIZE: u32 = 0x0001;
#[cfg(target_os = "windows")]
const SWP_NOMOVE: u32 = 0x0002;
#[cfg(target_os = "windows")]
const SWP_NOACTIVATE: u32 = 0x0010;
