//! Thin Win32 helpers: foreground window, process name, idle time.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Console::{FreeConsole, GetConsoleWindow};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, GetWindowTextW, IsWindow, ShowWindow, SW_HIDE,
};

/// Information about the current foreground window.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Raw handle as isize so it can cross threads cheaply.
    pub hwnd: isize,
    pub title: String,
    pub pid: u32,
}

/// Return the foreground window, if any.
pub fn foreground_window() -> Option<WindowInfo> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        let title = if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        };
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        Some(WindowInfo {
            hwnd: hwnd.0 as isize,
            title,
            pid,
        })
    }
}

/// Milliseconds since the last keyboard/mouse input anywhere on the system.
pub fn idle_millis() -> u64 {
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            GetTickCount().wrapping_sub(info.dwTime) as u64
        } else {
            0
        }
    }
}

/// Resolve the full executable path of a process. Returns None when access
/// is denied (system processes) or the process already exited.
pub fn process_exe(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok || size == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        Some(path)
    }
}

/// Lower-case file name of the executable for a pid, e.g. "chrome.exe".
pub fn exe_name_of(pid: u32) -> Option<String> {
    let path = process_exe(pid)?;
    let name = std::path::Path::new(&path)
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Detach the current process from its console (used with `watch --hidden`).
///
/// `ShowWindow` is not enough. A classic console window allocated for us at
/// logon can be hidden, but a pseudo console (Windows Terminal, VS Code, any
/// ConPTY host) cannot: `ShowWindow` reports success and leaves the terminal
/// window open, so the tracker stays welded to it and closing that terminal
/// kills tracking too. Detaching unconditionally handles both cases — the
/// console window goes away, or the terminal simply closes, and the tracker
/// keeps running independently either way.
///
/// Note: after detaching, `Ctrl+C` no longer reaches the process; use
/// `tracker stop` (which signals through the database) instead.
pub fn hide_console_window() {
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.is_invalid() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        let _ = FreeConsole();
    }
}
