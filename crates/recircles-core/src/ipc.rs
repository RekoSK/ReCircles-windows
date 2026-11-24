pub const UI_WINDOW_CLASS: &str = "ReCirclesRingWindow";
pub const DAEMON_WINDOW_CLASS: &str = "ReCirclesDaemonMessageWindow";
pub const DAEMON_MUTEX_NAME: &str = "Local\\ReCirclesDaemonSingletonV1";

pub const WM_APP_RELOAD_CONFIG: u32 = 0x8000 + 101;
pub const WM_APP_CLOSE_UI: u32 = 0x8000 + 102;
pub const WM_APP_QUIT_DAEMON: u32 = 0x8000 + 103;
pub const WM_APP_TRAY: u32 = 0x8000 + 104;
pub const WM_APP_HOOK_HOTKEY: u32 = 0x8000 + 105;
pub const WM_APP_CAPTURE_KEY: u32 = 0x8000 + 106;
pub const WM_APP_UPDATE_RESULT: u32 = 0x8000 + 107;
pub const WM_APP_CHECK_UPDATES: u32 = 0x8000 + 108;
pub const WM_APP_BACKDROP_DIRTY: u32 = 0x8000 + 109;

pub const COPYDATA_RUN_MENU: usize = 0x5243_0001;

#[cfg(windows)]
pub mod win {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND};
    use windows_sys::Win32::Graphics::Dwm::DwmFlush;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows_sys::Win32::System::Threading::{
        AttachThreadInput, CreateMutexW, GetCurrentThreadId,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetActiveWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, FindWindowW, GA_ROOT, GWL_STYLE, GetAncestor,
        GetForegroundWindow, GetWindowLongW, GetWindowThreadProcessId, IsWindow, PostMessageW,
        SMTO_ABORTIFHUNG, SendMessageTimeoutW, SetForegroundWindow, WM_CLOSE, WS_CHILD,
    };

    pub fn to_wide_nul(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    pub fn is_wine() -> bool {
        unsafe {
            let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast());
            if ntdll.is_null() {
                return false;
            }
            GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_some()
        }
    }

    pub struct SingleInstanceMutex(HANDLE);

    impl Drop for SingleInstanceMutex {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    pub fn acquire_daemon_mutex() -> Option<SingleInstanceMutex> {
        let name = to_wide_nul(DAEMON_MUTEX_NAME);
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            return None;
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return None;
        }
        Some(SingleInstanceMutex(handle))
    }

    pub fn find_daemon_hwnd() -> HWND {
        let cls = to_wide_nul(DAEMON_WINDOW_CLASS);
        unsafe { FindWindowW(cls.as_ptr(), std::ptr::null()) }
    }

    pub fn find_ui_hwnd() -> HWND {
        let cls = to_wide_nul(UI_WINDOW_CLASS);
        unsafe { FindWindowW(cls.as_ptr(), std::ptr::null()) }
    }

    pub fn notify_daemon_reload() -> bool {
        let hwnd = find_daemon_hwnd();
        if hwnd.is_null() {
            return false;
        }
        unsafe { PostMessageW(hwnd, WM_APP_RELOAD_CONFIG, 0, 0) != 0 }
    }

    pub fn stop_daemon_gracefully() -> bool {
        let hwnd = find_daemon_hwnd();
        if hwnd.is_null() {
            return false;
        }
        let mut _res = 0usize;
        unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_APP_QUIT_DAEMON,
                0,
                0,
                SMTO_ABORTIFHUNG,
                1500,
                &mut _res,
            );
        }
        true
    }

    pub fn close_existing_ui_instances() -> bool {
        let hwnd = find_ui_hwnd();
        if hwnd.is_null() {
            return false;
        }
        let mut _res = 0usize;
        unsafe {
            // wparam = 1 tells the previous UI instance to destroy immediately without fading
            // so the new instance never captures the old ring into its frosted backdrop blur.
            SendMessageTimeoutW(
                hwnd,
                WM_APP_CLOSE_UI,
                1,
                0,
                SMTO_ABORTIFHUNG,
                800,
                &mut _res,
            );
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(150) {
            if unsafe { IsWindow(hwnd) } == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Let DWM finish composing one clean frame before the replacement captures the desktop.
        unsafe {
            let _ = DwmFlush();
        }
        true
    }

    pub fn launch_daemon_detached(install_dir: &Path) -> Result<(), String> {
        let exe = install_dir.join("recircles-daemon.exe");
        if !exe.is_file() {
            return Err(format!("Daemon binary missing at {}", exe.display()));
        }
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new(&exe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed starting {}: {e}", exe.display()))
    }

    pub fn launch_ui_detached(exe_dir: &Path, target_menu: Option<&str>) -> Result<(), String> {
        let exe = exe_dir.join("recircles.exe");
        if !exe.is_file() {
            return Err(format!("UI binary missing at {}", exe.display()));
        }
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = Command::new(&exe);
        if let Some(menu) = target_menu {
            cmd.arg("--run").arg(menu);
        }
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed starting {}: {e}", exe.display()))?;
        unsafe {
            let _ = AllowSetForegroundWindow(child.id());
        }
        Ok(())
    }

    /// Reliably brings `hwnd` to the foreground even when launched from a low-level keyboard hook
    /// (`WH_KEYBOARD_LL`), where Windows' foreground lock may reject a bare `SetForegroundWindow`.
    pub fn force_foreground_window(hwnd: HWND) -> bool {
        if hwnd.is_null() {
            return false;
        }
        if unsafe { SetForegroundWindow(hwnd) } != 0 {
            return true;
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

        let ok = unsafe {
            let _ = BringWindowToTop(hwnd);
            let res = SetForegroundWindow(hwnd) != 0;
            let _ = SetActiveWindow(hwnd);
            res
        };

        if attached {
            unsafe {
                AttachThreadInput(cur_thread, fg_thread, 0);
            }
        }
        ok
    }

    /// Out-of-context WinEvent watcher that notifies a window (`WM_APP_BACKDROP_DIRTY`) whenever
    /// external top-level windows change focus, move/resize, minimize/restore, open, close, hide,
    /// or cloak/uncloak — keeping frosted glass backdrops live without polling or DLL injection.
    pub struct BackdropWatch {
        hooks: [isize; 3],
    }

    mod backdrop_watch_impl {
        use super::*;
        use std::cell::Cell;

        type WinEventProc = unsafe extern "system" fn(
            h_win_event_hook: isize,
            event: u32,
            hwnd: HWND,
            id_object: i32,
            id_child: i32,
            id_event_thread: u32,
            dwms_event_time: u32,
        );

        #[link(name = "user32")]
        unsafe extern "system" {
            fn SetWinEventHook(
                event_min: u32,
                event_max: u32,
                hmod_win_event_proc: *mut core::ffi::c_void,
                pfn_win_event_proc: WinEventProc,
                id_process: u32,
                id_thread: u32,
                dw_flags: u32,
            ) -> isize;
            fn UnhookWinEvent(h_win_event_hook: isize) -> i32;
        }

        const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
        const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

        const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
        const EVENT_SYSTEM_MOVESIZEEND: u32 = 0x000B;
        const EVENT_SYSTEM_SWITCHEND: u32 = 0x0015;
        const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
        const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;

        const EVENT_OBJECT_DESTROY: u32 = 0x8001;
        const EVENT_OBJECT_SHOW: u32 = 0x8002;
        const EVENT_OBJECT_HIDE: u32 = 0x8003;
        const EVENT_OBJECT_CLOAKED: u32 = 0x8017;
        const EVENT_OBJECT_UNCLOAKED: u32 = 0x8018;

        const OBJID_WINDOW: i32 = 0;
        const CHILDID_SELF: i32 = 0;

        thread_local! {
            static TARGET_HWND: Cell<usize> = const { Cell::new(0) };
            static NOTIFIED: Cell<bool> = const { Cell::new(false) };
        }

        fn is_top_level_event_window(hwnd: HWND) -> bool {
            if hwnd.is_null() {
                return false;
            }
            let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
            if (style & WS_CHILD) != 0 {
                return false;
            }
            let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
            root.is_null() || root == hwnd
        }

        unsafe extern "system" fn win_event_proc(
            _hook: isize,
            event: u32,
            hwnd: HWND,
            id_object: i32,
            id_child: i32,
            _thread: u32,
            _time: u32,
        ) {
            let relevant = match event {
                EVENT_SYSTEM_FOREGROUND
                | EVENT_SYSTEM_MOVESIZEEND
                | EVENT_SYSTEM_SWITCHEND
                | EVENT_SYSTEM_MINIMIZESTART
                | EVENT_SYSTEM_MINIMIZEEND => true,
                EVENT_OBJECT_DESTROY
                | EVENT_OBJECT_SHOW
                | EVENT_OBJECT_HIDE
                | EVENT_OBJECT_CLOAKED
                | EVENT_OBJECT_UNCLOAKED => {
                    id_object == OBJID_WINDOW
                        && id_child == CHILDID_SELF
                        && is_top_level_event_window(hwnd)
                }
                _ => false,
            };
            if !relevant {
                return;
            }
            let target = TARGET_HWND.with(Cell::get) as HWND;
            if target.is_null() || target == hwnd {
                return;
            }
            let already = NOTIFIED.with(|n| n.replace(true));
            if !already {
                unsafe {
                    PostMessageW(target, WM_APP_BACKDROP_DIRTY, event as usize, 0);
                }
            }
        }

        impl BackdropWatch {
            pub fn install(hwnd: HWND) -> Self {
                TARGET_HWND.with(|t| t.set(hwnd as usize));
                NOTIFIED.with(|n| n.set(false));
                let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
                let h1 = unsafe {
                    SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_MINIMIZEEND,
                        std::ptr::null_mut(),
                        win_event_proc,
                        0,
                        0,
                        flags,
                    )
                };
                let h2 = unsafe {
                    SetWinEventHook(
                        EVENT_OBJECT_DESTROY,
                        EVENT_OBJECT_HIDE,
                        std::ptr::null_mut(),
                        win_event_proc,
                        0,
                        0,
                        flags,
                    )
                };
                let h3 = unsafe {
                    SetWinEventHook(
                        EVENT_OBJECT_CLOAKED,
                        EVENT_OBJECT_UNCLOAKED,
                        std::ptr::null_mut(),
                        win_event_proc,
                        0,
                        0,
                        flags,
                    )
                };
                Self {
                    hooks: [h1, h2, h3],
                }
            }

            pub fn ack(&self) {
                NOTIFIED.with(|n| n.set(false));
            }
        }

        impl Drop for BackdropWatch {
            fn drop(&mut self) {
                for h in self.hooks {
                    if h != 0 {
                        unsafe {
                            UnhookWinEvent(h);
                        }
                    }
                }
                TARGET_HWND.with(|t| t.set(0));
                NOTIFIED.with(|n| n.set(false));
            }
        }
    }
}
