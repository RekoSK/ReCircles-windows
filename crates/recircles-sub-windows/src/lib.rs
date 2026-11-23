use recircles_submenu_abi::{SubmenuRequest, SubmenuResponse, WindowDto, export_submenu_dll};

export_submenu_dll!("windows", handle_request);

pub fn handle_request(req: SubmenuRequest) -> SubmenuResponse {
    match req {
        SubmenuRequest::WindowsList => SubmenuResponse::WindowsList {
            windows: list_windows(),
        },
        SubmenuRequest::WindowsFocus { hwnd } => match focus_window(hwnd) {
            Ok(()) => SubmenuResponse::ActionOutcome {
                ok: true,
                message: "Focused window".into(),
            },
            Err(e) => SubmenuResponse::ActionOutcome {
                ok: false,
                message: e,
            },
        },
        other => SubmenuResponse::Error {
            message: format!("Unsupported request for windows DLL: {other:?}"),
        },
    }
}

pub fn should_include_alt_tab_window(
    visible: bool,
    title: &str,
    ex_style: u32,
    has_owner: bool,
    cloaked: u32,
    same_process: bool,
) -> bool {
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_EX_APPWINDOW: u32 = 0x0004_0000;
    if !visible || same_process || cloaked != 0 || title.trim().is_empty() {
        return false;
    }
    if (ex_style & WS_EX_APPWINDOW) != 0 {
        return true;
    }
    if (ex_style & WS_EX_TOOLWINDOW) != 0 || has_owner {
        return false;
    }
    true
}

#[cfg(windows)]
pub fn list_windows() -> Vec<WindowDto> {
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GW_OWNER, GetWindow, GetWindowLongW, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    };

    struct CollectCtx {
        self_pid: u32,
        out: Vec<WindowDto>,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> i32 {
        let ctx = unsafe { &mut *(lparam as *mut CollectCtx) };
        let visible = unsafe { IsWindowVisible(hwnd) != 0 };
        if !visible {
            return 1;
        }

        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return 1;
        }
        let mut buf = vec![0u16; (len as usize) + 2];
        let copied = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        let title = if copied > 0 {
            String::from_utf16_lossy(&buf[..copied as usize])
        } else {
            return 1;
        };

        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        let has_owner = unsafe { !GetWindow(hwnd, GW_OWNER).is_null() };

        let mut cloaked = 0u32;
        unsafe {
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
        }

        if !should_include_alt_tab_window(
            visible,
            &title,
            ex_style,
            has_owner,
            cloaked,
            pid == ctx.self_pid,
        ) {
            return 1;
        }

        let mut exe_path = String::new();
        let mut app_id = "window".to_string();
        if pid != 0 {
            let proc = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if !proc.is_null() {
                let mut pbuf = [0u16; 1024];
                let mut plen = pbuf.len() as u32;
                if unsafe { QueryFullProcessImageNameW(proc, 0, pbuf.as_mut_ptr(), &mut plen) != 0 }
                {
                    exe_path = String::from_utf16_lossy(&pbuf[..plen as usize]);
                    if let Some(stem) = std::path::Path::new(&exe_path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                    {
                        app_id = stem.to_string();
                    }
                }
                unsafe { CloseHandle(proc) };
            }
        }

        let minimized = unsafe { IsIconic(hwnd) != 0 };
        ctx.out.push(WindowDto {
            hwnd: hwnd as usize as u64,
            title,
            app_id,
            exe_path,
            minimized,
        });
        1
    }

    let mut ctx = CollectCtx {
        self_pid: unsafe { GetCurrentProcessId() },
        out: Vec::new(),
    };
    unsafe {
        EnumWindows(Some(enum_cb), (&mut ctx as *mut CollectCtx) as isize);
    }
    ctx.out
}

#[cfg(not(windows))]
pub fn list_windows() -> Vec<WindowDto> {
    Vec::new()
}

#[cfg(windows)]
pub fn focus_window(hwnd_u64: u64) -> Result<(), String> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, HWND_TOP, IsIconic, IsWindow, SW_RESTORE,
        SWP_ASYNCWINDOWPOS, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowPos,
        ShowWindowAsync,
    };

    let hwnd = (hwnd_u64 as usize) as HWND;
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 {
        return Err("Target window no longer exists".into());
    }

    if unsafe { IsIconic(hwnd) } != 0 {
        // Use ShowWindowAsync so restoring another process's window never blocks our UI thread.
        unsafe { ShowWindowAsync(hwnd, SW_RESTORE) };
    }

    let fg = unsafe { GetForegroundWindow() };
    let fg_thread = if !fg.is_null() {
        unsafe { GetWindowThreadProcessId(fg, std::ptr::null_mut()) }
    } else {
        0
    };
    let cur_thread = unsafe { GetCurrentThreadId() };
    let attached = fg_thread != 0
        && fg_thread != cur_thread
        && unsafe { AttachThreadInput(cur_thread, fg_thread, 1) != 0 };

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_ASYNCWINDOWPOS,
        );
        let _ = SetForegroundWindow(hwnd);
    }

    if attached {
        unsafe {
            AttachThreadInput(cur_thread, fg_thread, 0);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn focus_window(_hwnd: u64) -> Result<(), String> {
    Err("Window focus only runs on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::should_include_alt_tab_window;

    #[test]
    fn alt_tab_filter_excludes_tool_cloaked_hidden_and_owned_windows() {
        const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
        const WS_EX_APPWINDOW: u32 = 0x0004_0000;

        assert!(should_include_alt_tab_window(true, "Notepad", 0, false, 0, false));
        assert!(!should_include_alt_tab_window(false, "Hidden", 0, false, 0, false));
        assert!(!should_include_alt_tab_window(true, "   ", 0, false, 0, false));
        assert!(!should_include_alt_tab_window(true, "Cloaked", 0, false, 1, false));
        assert!(!should_include_alt_tab_window(true, "Tool", WS_EX_TOOLWINDOW, false, 0, false));
        assert!(should_include_alt_tab_window(true, "AppTool", WS_EX_APPWINDOW, true, 0, false));
        assert!(!should_include_alt_tab_window(true, "Dialog", 0, true, 0, false));
        assert!(!should_include_alt_tab_window(true, "Self", 0, false, 0, true));
    }
}
