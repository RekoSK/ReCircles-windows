#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod app;
mod cli;
#[cfg_attr(not(windows), allow(dead_code))]
mod d2d;
#[cfg_attr(not(windows), allow(dead_code))]
mod icon_loader;
#[cfg_attr(not(windows), allow(dead_code))]
mod layout;
#[cfg_attr(not(windows), allow(dead_code))]
mod motion_ring;
#[cfg_attr(not(windows), allow(dead_code))]
mod renderer;
#[cfg_attr(not(windows), allow(dead_code))]
mod svg;
#[cfg_attr(not(windows), allow(dead_code))]
mod vsync;

use cli::{Command, help_text, parse_args};
use recircles_core::config::ConfigStore;
use recircles_core::history::History;
use recircles_core::modules::InstalledModules;
use std::process::ExitCode;

fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match parse_args(&raw_args) {
        Ok(c) => c,
        Err(e) => {
            attach_console_if_needed();
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let installed = InstalledModules::detect_default();
    let store = ConfigStore::default_store();

    match cmd {
        Command::Help => {
            attach_console_if_needed();
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Command::Version => {
            attach_console_if_needed();
            println!("recircles {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Config { force } => {
            attach_console_if_needed();
            match store.generate_default_config(force, &installed) {
                Ok(path) => {
                    println!("Generated default config at {}.", path.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Add(src) => {
            attach_console_if_needed();
            let path = std::path::PathBuf::from(&src);
            if let Ok(id) = store.import_theme_file(&path) {
                println!("Imported theme '{id}'.");
                return ExitCode::SUCCESS;
            }
            match store.import_menu_file(&path, &installed) {
                Ok(id) => {
                    println!("Imported menu '{id}'.");
                    #[cfg(windows)]
                    let _ = recircles_core::ipc::win::notify_daemon_reload();
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Remove(id) => {
            attach_console_if_needed();
            match store.remove_menu(&id, &installed) {
                Ok(()) => {
                    println!("Removed menu '{id}'.");
                    #[cfg(windows)]
                    let _ = recircles_core::ipc::win::notify_daemon_reload();
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::ResetHistory => {
            attach_console_if_needed();
            match History::reset() {
                Ok(()) => {
                    println!("Usage memory reset.");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Update { check_only } => {
            attach_console_if_needed();
            let cfg = store.load_or_create(&installed).unwrap_or_default();
            match recircles_core::updater::check_for_update(
                store.dir(),
                cfg.updates.include_prereleases,
            ) {
                Ok(recircles_core::updater::CheckOutcome::UpToDate { current, .. }) => {
                    println!("ReCircles is up to date (v{current}).");
                    ExitCode::SUCCESS
                }
                Ok(recircles_core::updater::CheckOutcome::UpdateAvailable(rel)) => {
                    println!(
                        "Update available: v{} (current: v{})",
                        rel.version,
                        recircles_core::updater::CURRENT_VERSION
                    );
                    if check_only {
                        return ExitCode::SUCCESS;
                    }
                    println!("Downloading {}…", rel.asset_name);
                    match recircles_core::updater::download_update(store.dir(), &rel) {
                        Ok(path) => {
                            #[cfg(windows)]
                            {
                                let install_dir =
                                    recircles_core::updater::installed_dir_from_manifest(
                                        store.dir(),
                                    )
                                    .unwrap_or_else(recircles_core::paths::exe_dir);
                                match recircles_core::updater::launch_installer_upgrade(
                                    &path,
                                    &install_dir,
                                    store.dir(),
                                    Some(std::process::id()),
                                    recircles_core::updater::CURRENT_VERSION,
                                    &rel.version,
                                    false,
                                ) {
                                    Ok(()) => {
                                        println!("Launched updater for v{}.", rel.version);
                                        ExitCode::SUCCESS
                                    }
                                    Err(e) => {
                                        eprintln!("error: {e}");
                                        ExitCode::FAILURE
                                    }
                                }
                            }
                            #[cfg(not(windows))]
                            {
                                let _ = path;
                                ExitCode::SUCCESS
                            }
                        }
                        Err(e) => {
                            eprintln!("error: {e}");
                            ExitCode::FAILURE
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::ShowMain => run_gui(store, installed, None),
        Command::RunMenu(target) => run_gui(store, installed, Some(target)),
    }
}

#[cfg(windows)]
fn attach_console_if_needed() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    let h = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if h.is_null() || h == INVALID_HANDLE_VALUE {
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_console_if_needed() {}

#[cfg(not(windows))]
fn run_gui(_store: ConfigStore, _installed: InstalledModules, _start: Option<String>) -> ExitCode {
    eprintln!("ReCircles Windows GUI runs only on Windows.");
    ExitCode::FAILURE
}

#[cfg(windows)]
fn run_gui(store: ConfigStore, installed: InstalledModules, start: Option<String>) -> ExitCode {
    use app::{App, BackendBridge, UiEffect};
    use icon_loader::WM_APP_ICON_READY;
    use recircles_core::ipc::{
        UI_WINDOW_CLASS, WM_APP_CAPTURE_KEY, WM_APP_CLOSE_UI,
        win::{
            close_existing_ui_instances, force_foreground_window, notify_daemon_reload, to_wide_nul,
        },
    };
    use recircles_core::modules::{BuiltinSubmenu, host::SubmenuHost};
    use recircles_core::submenu_abi::{SubmenuRequest, SubmenuResponse};
    use recircles_core::win_hook::win::CaptureHook;
    use recircles_core::win_keys::Modifiers;
    use renderer::win_render::Renderer;
    use std::cell::RefCell;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::sync::Arc;
    use std::time::Instant;
    use vsync::{VsyncPacer, WM_APP_TICK};
    use windows_sys::Win32::Foundation::{FreeLibrary, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleW, GetProcAddress, LoadLibraryW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
        SetProcessDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, GetKeyboardState, ReleaseCapture, SetCapture, ToUnicode, VK_CONTROL,
        VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
        GetMessageW, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage,
        RegisterClassExW, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetForegroundWindow, SetWindowPos, ShowWindow, TranslateMessage, WM_ACTIVATEAPP,
        WM_CAPTURECHANGED, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSCHAR,
        WM_SYSKEYDOWN, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    const WS_EX_NOREDIRECTIONBITMAP: u32 = 0x0020_0000;

    struct WinDllBridge {
        host: SubmenuHost,
    }
    impl BackendBridge for WinDllBridge {
        fn invoke(&self, sub: BuiltinSubmenu, req: &SubmenuRequest) -> Result<SubmenuResponse, String> {
            self.host.invoke(sub, req)
        }
        fn is_installed(&self, sub: BuiltinSubmenu) -> bool {
            self.host.installed().is_installed(sub)
        }
    }

    // Check early before spawning UI if a deleted/uninstalled submenu was requested via CLI/shortcut
    let bridge = Arc::new(WinDllBridge {
        host: SubmenuHost::new(installed.clone()),
    });
    let mut app = match App::new(store, installed, bridge, start.as_deref()) {
        Ok(a) => a,
        Err(e) => {
            attach_console_if_needed();
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let _ = close_existing_ui_instances();
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    struct UiRuntime {
        hwnd: HWND,
        app: App,
        renderer: Renderer,
        pacer: VsyncPacer,
        mon_w: i32,
        mon_h: i32,
        anchor_px: (f64, f64),
        total_scale: f64,
        last_tick: Instant,
        opened_at: Instant,
        activated_once: bool,
        picking_file: bool,
        was_animating: bool,
        needs_paint: bool,
        pointer_capture: bool,
        last_mouse_phys: (i32, i32),
        capture_hook: Option<CaptureHook>,
    }

    thread_local! {
        static RT: RefCell<Option<UiRuntime>> = const { RefCell::new(None) };
    }

    let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
    let cls = to_wide_nul(UI_WINDOW_CLASS);
    let title = to_wide_nul("ReCircles");
    let (app_icon, app_icon_sm) = recircles_core::app_icon::win::load_or_create_app_icons();
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: app_icon,
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: cls.as_ptr(),
        hIconSm: app_icon_sm,
    };
    unsafe { RegisterClassExW(&wc) };

    // Determine active cursor monitor bounds (matching Linux gtk-layer-shell fullscreen overlay)
    let mut pt = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut pt) };
    let hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { GetMonitorInfoW(hmon, &mut mi) };
    let mon_x = mi.rcMonitor.left;
    let mon_y = mi.rcMonitor.top;
    let mon_w = (mi.rcMonitor.right - mi.rcMonitor.left).max(320);
    let mon_h = (mi.rcMonitor.bottom - mi.rcMonitor.top).max(240);

    // DirectComposition transparent overlay window (ZERO redirection bitmap, ZERO rectangular box)
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            cls.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            mon_x,
            mon_y,
            mon_w,
            mon_h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return ExitCode::FAILURE;
    }

    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f64;
    let dpi_scale = dpi / 96.0;
    let total_scale = (dpi_scale * app.cfg.appearance.scale).clamp(0.5, 4.0);

    let Some(mut renderer) = Renderer::new_with_store(
        hwnd,
        mon_w,
        mon_h,
        total_scale,
        ConfigStore::in_dir(app.store.dir().to_path_buf()),
    ) else {
        unsafe { DestroyWindow(hwnd) };
        attach_console_if_needed();
        eprintln!("error: failed to initialize Direct2D / DirectComposition renderer");
        return ExitCode::FAILURE;
    };

    // Capture the clean desktop BEFORE showing the overlay so GPU frosted blur is strictly
    // clipped inside the circular ring & capsules with zero rectangular DWM window behind it.
    renderer.capture_backdrop(mon_x, mon_y, dpi_scale, app.cfg.appearance.blur);

    let anchor_px = ((pt.x - mon_x) as f64, (pt.y - mon_y) as f64);
    app.set_viewport(
        mon_w as f64 / total_scale,
        mon_h as f64 / total_scale,
        anchor_px.0 / total_scale,
        anchor_px.1 / total_scale,
    );

    let pacer = VsyncPacer::start(hwnd);

    let mut rt = UiRuntime {
        hwnd,
        app,
        renderer,
        pacer,
        mon_w,
        mon_h,
        anchor_px,
        total_scale,
        last_tick: Instant::now(),
        opened_at: Instant::now(),
        activated_once: false,
        picking_file: false,
        was_animating: false,
        needs_paint: false,
        pointer_capture: false,
        last_mouse_phys: (pt.x - mon_x, pt.y - mon_y),
        capture_hook: None,
    };
    paint_frame(&mut rt);

    unsafe {
        ShowWindow(hwnd, SW_SHOWNORMAL);
    }
    let _ = force_foreground_window(hwnd);

    RT.with(|slot| *slot.borrow_mut() = Some(rt));

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    RT.with(|slot| {
        drop(slot.borrow_mut().take());
    });
    return ExitCode::SUCCESS;

    fn request_paint(rt: &mut UiRuntime) {
        if !rt.was_animating {
            paint_frame(rt);
        } else {
            rt.needs_paint = true;
        }
    }

    fn sync_capture_hook(rt: &mut UiRuntime, force_drop: bool) {
        if force_drop || rt.app.closing {
            rt.capture_hook = None;
            return;
        }
        match &rt.app.capture {
            Some(target) => {
                let allow_lone_win = matches!(target, app::CaptureTarget::MenuHotkey { .. });
                if let Some(hook) = &rt.capture_hook {
                    hook.set_active(true, allow_lone_win);
                } else {
                    rt.capture_hook =
                        CaptureHook::install(rt.hwnd, WM_APP_CAPTURE_KEY, allow_lone_win);
                }
            }
            None => {
                if let Some(hook) = &rt.capture_hook {
                    if hook.has_suppressed() {
                        hook.set_active(false, false);
                    } else {
                        rt.capture_hook = None;
                    }
                }
            }
        }
    }

    fn paint_frame(rt: &mut UiRuntime) {
        rt.needs_paint = false;
        let dpi = unsafe { GetDpiForWindow(rt.hwnd) }.max(96) as f64;
        let new_scale = ((dpi / 96.0) * rt.app.cfg.appearance.scale).clamp(0.5, 4.0);
        if (new_scale - rt.total_scale).abs() > 1e-4 {
            let old_scale = rt.total_scale;
            rt.total_scale = new_scale;
            rt.app.rescale_hover(old_scale / new_scale);
            rt.app.set_viewport(
                rt.mon_w as f64 / new_scale,
                rt.mon_h as f64 / new_scale,
                rt.anchor_px.0 / new_scale,
                rt.anchor_px.1 / new_scale,
            );
        }
        rt.renderer.render_and_present(rt.total_scale, &rt.app);
        let is_anim = rt.app.is_animating()
            || rt.app.is_dragging()
            || rt.needs_paint
            || rt.renderer.has_more_ready_icons();
        if is_anim && !rt.was_animating {
            rt.last_tick = Instant::now();
        }
        rt.was_animating = is_anim;
        rt.pacer.set_animating(is_anim);
    }

    fn current_mods() -> Modifiers {
        let down = |vk: u16| unsafe { (GetKeyState(vk as i32) as u16 & 0x8000) != 0 };
        Modifiers {
            ctrl: down(VK_CONTROL),
            alt: down(VK_MENU),
            shift: down(VK_SHIFT),
            win: down(VK_LWIN) || down(VK_RWIN),
        }
    }

    fn key_to_char(vk: u32, lparam: isize) -> Option<char> {
        let mut state = [0u8; 256];
        if unsafe { GetKeyboardState(state.as_mut_ptr()) } == 0 {
            return None;
        }
        let scan = ((lparam >> 16) & 0xFF) as u32;
        let mut buf = [0u16; 4];
        // On European layouts, AltGr sets both Ctrl and Alt to emit symbols like '#' or '@'.
        let ctrl_down = (state[0x11] & 0x80) != 0;
        let alt_down = (state[0x12] & 0x80) != 0;
        if ctrl_down && alt_down {
            let rc = unsafe { ToUnicode(vk, scan, state.as_ptr(), buf.as_mut_ptr(), 4, 4) };
            if rc == 1
                && let Some(c) = char::from_u32(buf[0] as u32)
                && !c.is_control()
            {
                return Some(c);
            }
        }
        // Clear Ctrl/Alt bits so ToUnicode resolves readable character instead of ASCII control code
        for idx in [0x11usize, 0x12, 0xA2, 0xA3, 0xA4, 0xA5] {
            state[idx] = 0;
        }
        let rc = unsafe { ToUnicode(vk, scan, state.as_ptr(), buf.as_mut_ptr(), 4, 0) };
        if rc == 1 {
            char::from_u32(buf[0] as u32)
        } else {
            None
        }
    }

    fn read_clipboard_unicode_text(hwnd: HWND) -> Option<String> {
        const CF_UNICODETEXT: u32 = 13;
        if unsafe { OpenClipboard(hwnd) } == 0 {
            return None;
        }
        let mut out = None;
        let h = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if !h.is_null() {
            let ptr = unsafe { GlobalLock(h) } as *const u16;
            if !ptr.is_null() {
                let mut len = 0usize;
                unsafe {
                    while *ptr.add(len) != 0 && len < 8192 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    out = Some(String::from_utf16_lossy(slice));
                    GlobalUnlock(h);
                }
            }
        }
        unsafe { CloseClipboard() };
        out
    }

    fn pick_import_file_dialog(hwnd: HWND, dialog_title: &str) -> Option<std::path::PathBuf> {
        #[repr(C)]
        #[allow(non_snake_case)]
        struct OpenFileNameW {
            lStructSize: u32,
            hwndOwner: HWND,
            hInstance: *mut std::ffi::c_void,
            lpstrFilter: *const u16,
            lpstrCustomFilter: *mut u16,
            nMaxCustFilter: u32,
            nFilterIndex: u32,
            lpstrFile: *mut u16,
            nMaxFile: u32,
            lpstrFileTitle: *mut u16,
            nMaxFileTitle: u32,
            lpstrInitialDir: *const u16,
            lpstrTitle: *const u16,
            Flags: u32,
            nFileOffset: u16,
            nFileExtension: u16,
            lpstrDefExt: *const u16,
            lCustData: isize,
            lpfnHook: *const std::ffi::c_void,
            lpTemplateName: *const u16,
            pvReserved: *mut std::ffi::c_void,
            dwReserved: u32,
            FlagsEx: u32,
        }
        type FnGetOpenFileNameW = unsafe extern "system" fn(*mut OpenFileNameW) -> i32;

        let dll = to_wide_nul("comdlg32.dll");
        let module = unsafe { LoadLibraryW(dll.as_ptr()) };
        if module.is_null() {
            return None;
        }
        let sym = unsafe { GetProcAddress(module, c"GetOpenFileNameW".as_ptr().cast()) };
        let Some(func_ptr) = sym else {
            unsafe { FreeLibrary(module) };
            return None;
        };
        let get_open_file_name: FnGetOpenFileNameW = unsafe { std::mem::transmute(func_ptr) };

        let mut filter: Vec<u16> =
            "Theme / Menu Files (*.json;*.zip)\0*.json;*.zip\0All Files (*.*)\0*.*\0\0"
                .encode_utf16()
                .collect();
        let title = to_wide_nul(dialog_title);
        let mut buf = vec![0u16; 4096];
        let mut ofn = OpenFileNameW {
            lStructSize: std::mem::size_of::<OpenFileNameW>() as u32,
            hwndOwner: hwnd,
            hInstance: std::ptr::null_mut(),
            lpstrFilter: filter.as_mut_ptr(),
            lpstrCustomFilter: std::ptr::null_mut(),
            nMaxCustFilter: 0,
            nFilterIndex: 1,
            lpstrFile: buf.as_mut_ptr(),
            nMaxFile: buf.len() as u32,
            lpstrFileTitle: std::ptr::null_mut(),
            nMaxFileTitle: 0,
            lpstrInitialDir: std::ptr::null(),
            lpstrTitle: title.as_ptr(),
            Flags: 0x0008_1800, // OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST
            nFileOffset: 0,
            nFileExtension: 0,
            lpstrDefExt: std::ptr::null(),
            lCustData: 0,
            lpfnHook: std::ptr::null(),
            lpTemplateName: std::ptr::null(),
            pvReserved: std::ptr::null_mut(),
            dwReserved: 0,
            FlagsEx: 0,
        };
        let ok = unsafe { get_open_file_name(&mut ofn) };
        unsafe { FreeLibrary(module) };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        if end == 0 {
            return None;
        }
        Some(std::path::PathBuf::from(OsString::from_wide(&buf[..end])))
    }

    fn run_file_picker(hwnd: HWND, is_menu: bool) {
        RT.with(|slot| {
            if let Some(rt) = slot.borrow_mut().as_mut() {
                rt.picking_file = true;
                if rt.pointer_capture {
                    rt.pointer_capture = false;
                    unsafe { ReleaseCapture() };
                }
            }
        });
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        let picked = pick_import_file_dialog(
            hwnd,
            if is_menu {
                "Import ReCircles Menu (.json or .zip)"
            } else {
                "Import ReCircles Theme (.json or .zip)"
            },
        );
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            SetForegroundWindow(hwnd);
        }
        let mut follow_eff = UiEffect::None;
        RT.with(|slot| {
            if let Some(rt) = slot.borrow_mut().as_mut() {
                rt.picking_file = false;
                if let Some(path) = picked {
                    if is_menu {
                        follow_eff = rt.app.import_menu_from_path(&path);
                    } else {
                        rt.app.import_theme_from_path(&path);
                    }
                }
                paint_frame(rt);
            }
        });
        handle_effect(hwnd, follow_eff);
    }

    fn closes_immediately(eff: &UiEffect) -> bool {
        matches!(
            eff,
            UiEffect::CloseApp | UiEffect::FocusWindow { close_now: true, .. }
        )
    }

    fn handle_effect(hwnd: HWND, eff: UiEffect) {
        match eff {
            UiEffect::None => {}
            UiEffect::PickThemeFile => run_file_picker(hwnd, false),
            UiEffect::PickMenuFile => run_file_picker(hwnd, true),
            UiEffect::NotifyDaemonReload => {
                let _ = notify_daemon_reload();
            }
            UiEffect::FocusWindow {
                hwnd: target_hwnd,
                close_now,
            } => {
                unsafe { ReleaseCapture() };
                let backend = RT.with(|slot| {
                    slot.try_borrow()
                        .ok()
                        .and_then(|g| g.as_ref().map(|rt| rt.app.backend.clone()))
                });
                if let Some(backend) = backend {
                    let _ = backend.invoke(
                        BuiltinSubmenu::Windows,
                        &SubmenuRequest::WindowsFocus { hwnd: target_hwnd },
                    );
                }
                if close_now {
                    unsafe { DestroyWindow(hwnd) };
                }
            }
            UiEffect::CloseApp => unsafe {
                ReleaseCapture();
                DestroyWindow(hwnd);
            },
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_ERASEBKGND => 1,
            WM_APP_TICK => {
                let mut effects_out = Vec::new();
                RT.with(|slot| {
                    if let Some(rt) = slot.borrow_mut().as_mut() {
                        rt.pacer.ack_tick();
                        let now = Instant::now();
                        let dt = (now - rt.last_tick).as_secs_f64().clamp(0.0, 0.05);
                        rt.last_tick = now;
                        effects_out = rt.app.tick(dt);
                        if !effects_out.iter().any(|e| matches!(e, UiEffect::CloseApp)) {
                            paint_frame(rt);
                        }
                    }
                });
                for eff in effects_out {
                    handle_effect(hwnd, eff);
                }
                0
            }
            WM_APP_ICON_READY => {
                RT.with(|slot| {
                    if let Some(rt) = slot.borrow_mut().as_mut() {
                        rt.renderer.ack_icon_notify();
                        if rt.renderer.drain_ready_icons(16) {
                            request_paint(rt);
                        }
                    }
                });
                0
            }
            WM_APP_CAPTURE_KEY => {
                let vk = wparam as u32;
                let mut pending_eff = UiEffect::None;
                RT.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut()
                        && let Some(rt) = guard.as_mut()
                    {
                        if vk == 0 {
                            // Drain-complete sentinel from CaptureHook after captured key release
                            sync_capture_hook(rt, false);
                        } else if rt.app.capture.is_some() {
                            let mods = Modifiers::from_bits(lparam as u8);
                            pending_eff = rt.app.handle_key_down(mods, vk, None);
                            sync_capture_hook(rt, closes_immediately(&pending_eff));
                            if !closes_immediately(&pending_eff) {
                                request_paint(rt);
                            }
                        } else {
                            sync_capture_hook(rt, false);
                        }
                    }
                });
                handle_effect(hwnd, pending_eff);
                0
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = wparam as u32;
                let mods = current_mods();
                let handled_by_hook = RT.with(|slot| {
                    slot.try_borrow()
                        .ok()
                        .and_then(|g| {
                            g.as_ref()
                                .map(|rt| rt.app.capture.is_some() && rt.capture_hook.is_some())
                        })
                        .unwrap_or(false)
                });
                if handled_by_hook {
                    return 0;
                }
                if mods.ctrl && !mods.alt && !mods.win && vk == 0x56 {
                    let clip = read_clipboard_unicode_text(hwnd);
                    let mut handled_paste = false;
                    RT.with(|slot| {
                        if let Some(rt) = slot.borrow_mut().as_mut()
                            && rt.app.capture.is_none()
                        {
                            if let Some(txt) = clip {
                                rt.app.paste_text(&txt);
                                request_paint(rt);
                            }
                            handled_paste = true;
                        }
                    });
                    if handled_paste {
                        return 0;
                    }
                }
                let ch = key_to_char(vk, lparam);
                let mut pending_eff = UiEffect::None;
                RT.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut()
                        && let Some(rt) = guard.as_mut()
                    {
                        pending_eff = rt.app.handle_key_down(mods, vk, ch);
                        sync_capture_hook(rt, closes_immediately(&pending_eff));
                        if !closes_immediately(&pending_eff) {
                            request_paint(rt);
                        }
                    }
                });
                handle_effect(hwnd, pending_eff);
                0
            }
            WM_SYSCHAR => 0,
            WM_MOUSEMOVE => {
                let px_i = (lparam & 0xFFFF) as i16 as i32;
                let py_i = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                RT.with(|slot| {
                    if let Some(rt) = slot.borrow_mut().as_mut() {
                        // Ignore Windows's synthetic WM_MOUSEMOVE on ShowWindow when the cursor
                        // has not moved so initial keyboard/list selection is never hijacked.
                        if (px_i, py_i) == rt.last_mouse_phys && !rt.app.is_dragging() {
                            return;
                        }
                        rt.last_mouse_phys = (px_i, py_i);
                        let lx = px_i as f64 / rt.total_scale;
                        let ly = py_i as f64 / rt.total_scale;
                        let dirty = rt.app.handle_pointer_move(lx, ly);
                        // Paint immediately when idle (0-latency start); when already animating or
                        // dragging on VsyncPacer, coalesce high-rate mouse moves to VSync.
                        if dirty {
                            request_paint(rt);
                        }
                    }
                });
                0
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
                let right = msg == WM_RBUTTONDOWN;
                let px_i = (lparam & 0xFFFF) as i16 as i32;
                let py_i = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                let mut pending_eff = UiEffect::None;
                let mut want_capture = false;
                RT.with(|slot| {
                    if let Some(rt) = slot.borrow_mut().as_mut() {
                        rt.last_mouse_phys = (px_i, py_i);
                        let lx = px_i as f64 / rt.total_scale;
                        let ly = py_i as f64 / rt.total_scale;
                        pending_eff = rt.app.handle_pointer_down(lx, ly, right);
                        sync_capture_hook(rt, closes_immediately(&pending_eff));
                        let has_press = rt.app.pressed.is_some()
                            || rt.app.is_dragging()
                            || rt.app.slider.as_ref().is_some_and(|s| s.pressed_button.is_some())
                            || rt.app.color_picker.as_ref().is_some_and(|c| c.pressed_button.is_some())
                            || rt.app.input_box.as_ref().is_some_and(|i| i.pressed_button.is_some());
                        if has_press {
                            rt.pointer_capture = true;
                            want_capture = true;
                        }
                        if !matches!(pending_eff, UiEffect::CloseApp) {
                            paint_frame(rt);
                        }
                    }
                });
                if want_capture {
                    unsafe { SetCapture(hwnd) };
                }
                handle_effect(hwnd, pending_eff);
                0
            }
            WM_LBUTTONUP | WM_RBUTTONUP => {
                let right = msg == WM_RBUTTONUP;
                let px_i = (lparam & 0xFFFF) as i16 as i32;
                let py_i = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                let mut pending_eff = UiEffect::None;
                let mut end_capture = false;
                RT.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut()
                        && let Some(rt) = guard.as_mut()
                    {
                        rt.last_mouse_phys = (px_i, py_i);
                        if rt.pointer_capture {
                            rt.pointer_capture = false;
                            end_capture = true;
                        }
                        let lx = px_i as f64 / rt.total_scale;
                        let ly = py_i as f64 / rt.total_scale;
                        let (dirty, eff) = rt.app.handle_pointer_up(lx, ly, right);
                        pending_eff = eff;
                        sync_capture_hook(rt, closes_immediately(&pending_eff));
                        if dirty && !closes_immediately(&pending_eff) {
                            paint_frame(rt);
                        }
                    }
                });
                if end_capture {
                    unsafe { ReleaseCapture() };
                }
                handle_effect(hwnd, pending_eff);
                0
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam >> 16) as i16) as f64;
                RT.with(|slot| {
                    if let Some(rt) = slot.borrow_mut().as_mut()
                        && rt.app.handle_wheel(delta)
                    {
                        request_paint(rt);
                    }
                });
                0
            }
            WM_APP_CLOSE_UI => {
                // wparam == 1 requests immediate close (replacement instance about to capture backdrop)
                if wparam == 1 {
                    RT.with(|slot| {
                        if let Ok(mut guard) = slot.try_borrow_mut()
                            && let Some(rt) = guard.as_mut()
                        {
                            sync_capture_hook(rt, true);
                        }
                    });
                    unsafe {
                        ReleaseCapture();
                        DestroyWindow(hwnd);
                    }
                    return 0;
                }
                let mut pending_eff = UiEffect::None;
                RT.with(|slot| {
                    if let Some(rt) = slot.borrow_mut().as_mut() {
                        pending_eff = rt.app.begin_close();
                        sync_capture_hook(rt, true);
                        if !matches!(pending_eff, UiEffect::CloseApp) {
                            paint_frame(rt);
                        }
                    }
                });
                handle_effect(hwnd, pending_eff);
                0
            }
            WM_ACTIVATEAPP => {
                if wparam != 0 {
                    RT.with(|slot| {
                        if let Ok(mut guard) = slot.try_borrow_mut()
                            && let Some(rt) = guard.as_mut()
                        {
                            rt.activated_once = true;
                        }
                    });
                } else {
                    let mut pending_eff = UiEffect::None;
                    RT.with(|slot| {
                        if let Ok(mut guard) = slot.try_borrow_mut()
                            && let Some(rt) = guard.as_mut()
                            && !rt.picking_file
                            && (rt.activated_once
                                || rt.opened_at.elapsed() >= std::time::Duration::from_millis(250))
                        {
                            pending_eff = rt.app.begin_close();
                            sync_capture_hook(rt, true);
                            if !closes_immediately(&pending_eff) {
                                paint_frame(rt);
                            }
                        }
                    });
                    handle_effect(hwnd, pending_eff);
                }
                0
            }
            WM_DPICHANGED => {
                let new_dpi = ((wparam & 0xFFFF) as u32).max(96) as f64;
                let suggested = lparam as *const RECT;
                RT.with(|slot| {
                    if let Some(rt) = slot.borrow_mut().as_mut() {
                        let new_scale = ((new_dpi / 96.0) * rt.app.cfg.appearance.scale).clamp(0.5, 4.0);
                        if !suggested.is_null() {
                            let rc = unsafe { *suggested };
                            let nw = (rc.right - rc.left).max(320);
                            let nh = (rc.bottom - rc.top).max(240);
                            unsafe {
                                SetWindowPos(
                                    hwnd,
                                    std::ptr::null_mut(),
                                    rc.left,
                                    rc.top,
                                    nw,
                                    nh,
                                    SWP_NOZORDER | SWP_NOACTIVATE,
                                );
                            }
                            rt.mon_w = nw;
                            rt.mon_h = nh;
                            rt.renderer.resize(nw, nh, new_scale);
                        }
                        let old_scale = rt.total_scale;
                        rt.total_scale = new_scale;
                        rt.app.rescale_hover(old_scale / new_scale);
                        rt.app.set_viewport(
                            rt.mon_w as f64 / new_scale,
                            rt.mon_h as f64 / new_scale,
                            rt.anchor_px.0 / new_scale,
                            rt.anchor_px.1 / new_scale,
                        );
                        paint_frame(rt);
                    }
                });
                0
            }
            WM_CAPTURECHANGED => {
                RT.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut()
                        && let Some(rt) = guard.as_mut()
                    {
                        let was_capturing = rt.pointer_capture;
                        rt.pointer_capture = false;
                        if was_capturing {
                            rt.app.cancel_active_pointer_press();
                            paint_frame(rt);
                        }
                    }
                });
                0
            }
            WM_DESTROY => {
                RT.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut()
                        && let Some(rt) = guard.as_mut()
                    {
                        sync_capture_hook(rt, true);
                    }
                });
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }
}
