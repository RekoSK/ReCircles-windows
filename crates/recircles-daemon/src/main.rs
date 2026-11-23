#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("recircles-daemon runs on Windows only.");
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--quit" || a == "-q") {
        let _ = recircles_core::ipc::win::stop_daemon_gracefully();
        return;
    }
    if args.iter().any(|a| a == "--reload" || a == "-r") {
        let _ = recircles_core::ipc::win::notify_daemon_reload();
        return;
    }
    win_daemon::run();
}

#[cfg(windows)]
mod win_daemon {
    use recircles_core::config::{AppConfig, ConfigStore, HotkeyBinding};
    use recircles_core::ipc::{
        DAEMON_WINDOW_CLASS, WM_APP_CLOSE_UI, WM_APP_HOOK_HOTKEY, WM_APP_QUIT_DAEMON,
        WM_APP_RELOAD_CONFIG, WM_APP_TRAY,
        win::{
            acquire_daemon_mutex, close_existing_ui_instances, find_ui_hwnd,
            launch_ui_detached, to_wide_nul,
        },
    };
    use recircles_core::modules::{BuiltinSubmenu, InstalledModules, host::SubmenuHost};
    use recircles_core::paths;
    use recircles_core::submenu_abi::SubmenuRequest;
    use recircles_core::win_hook::{HookBindingEntry, win::GlobalHotkeyHook};
    use recircles_core::win_keys::{is_known_windows_shell_hotkey, os_locked_hotkey_reason};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::DataExchange::{
        AddClipboardFormatListener, RemoveClipboardFormatListener,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::SetProcessWorkingSetSizeEx;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
        Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
        DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, HICON, MF_SEPARATOR,
        MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
        TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
        TranslateMessage, WM_CLIPBOARDUPDATE, WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP,
        WNDCLASSEXW, WS_OVERLAPPED,
    };

    const TRAY_UID: u32 = 1;
    const CMD_OPEN_MAIN: u32 = 1001;
    const CMD_RELOAD: u32 = 1002;
    const CMD_EXIT: u32 = 1003;
    const CMD_MENU_BASE: u32 = 2000;

    struct DaemonState {
        hwnd: HWND,
        store: ConfigStore,
        cfg: AppConfig,
        host: SubmenuHost,
        hotkeys: HashMap<i32, Option<String>>,
        registered_os_ids: Vec<i32>,
        hook_guard: Option<GlobalHotkeyHook>,
        failed_hotkeys: Vec<String>,
        clipboard_listening: bool,
        tray_icon: HICON,
    }

    thread_local! {
        static STATE: RefCell<Option<DaemonState>> = const { RefCell::new(None) };
    }

    pub fn run() {
        let Some(_mutex) = acquire_daemon_mutex() else {
            // Already running: trigger reload so updated configs/hotkeys take effect
            let _ = recircles_core::ipc::win::notify_daemon_reload();
            return;
        };

        let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
        let cls = to_wide_nul(DAEMON_WINDOW_CLASS);
        let wnd = to_wide_nul("ReCircles Daemon");
        let (app_icon, app_icon_sm) = recircles_core::app_icon::win::load_or_create_app_icons();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: app_icon,
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: app_icon_sm,
        };
        unsafe { RegisterClassExW(&wc) };

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                cls.as_ptr(),
                wnd.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            eprintln!("ReCircles Daemon: failed creating hidden message window");
            std::process::exit(1);
        }

        let store = ConfigStore::default_store();
        let installed = InstalledModules::detect_default();
        let cfg = store
            .load_or_create(&installed)
            .unwrap_or_else(|_| AppConfig::default_for_installed(&installed));
        let tray_icon = create_ring_tray_icon();

        let mut st = DaemonState {
            hwnd,
            store,
            cfg,
            host: SubmenuHost::new(installed),
            hotkeys: HashMap::new(),
            registered_os_ids: Vec::new(),
            hook_guard: None,
            failed_hotkeys: Vec::new(),
            clipboard_listening: false,
            tray_icon,
        };
        add_tray_icon(&st);
        apply_runtime_config(&mut st);
        trim_working_set();

        STATE.with(|slot| *slot.borrow_mut() = Some(st));

        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        STATE.with(|slot| {
            if let Some(mut st) = slot.borrow_mut().take() {
                unregister_all_hotkeys(&mut st);
                if st.clipboard_listening {
                    unsafe {
                        RemoveClipboardFormatListener(st.hwnd);
                    }
                    st.clipboard_listening = false;
                }
                remove_tray_icon(&st);
                if !st.tray_icon.is_null() {
                    unsafe {
                        DestroyIcon(st.tray_icon);
                    }
                }
            }
        });
    }

    fn apply_runtime_config(st: &mut DaemonState) {
        unregister_all_hotkeys(st);
        st.failed_hotkeys.clear();
        st.host.refresh_installed();
        let installed = st.host.installed().clone();
        st.cfg = st
            .store
            .load_or_create(&installed)
            .unwrap_or_else(|_| AppConfig::default_for_installed(&installed));

        let bindings: Vec<HotkeyBinding> = st
            .cfg
            .keybinds
            .collect_active_global_hotkeys(&installed, &st.cfg.menus);
        let mut hook_pending: Vec<(HotkeyBinding, HookBindingEntry)> = Vec::new();
        for binding in bindings {
            if let Some(reason) = os_locked_hotkey_reason(binding.parsed) {
                eprintln!(
                    "ReCircles Daemon: skipping OS-locked hotkey {} ({}): {reason}",
                    binding.label, binding.spec
                );
                st.failed_hotkeys.push(binding.label);
                continue;
            }

            if is_known_windows_shell_hotkey(binding.parsed) {
                let entry = HookBindingEntry {
                    id: binding.id,
                    mods: binding.parsed.mods,
                    vk: binding.parsed.vk,
                    lone_win: binding.parsed.is_lone_win(),
                };
                hook_pending.push((binding, entry));
                continue;
            }

            let flags = binding.parsed.mods.to_register_hotkey_flags();
            if unsafe { RegisterHotKey(st.hwnd, binding.id, flags, binding.parsed.vk) } != 0 {
                st.registered_os_ids.push(binding.id);
                st.hotkeys.insert(binding.id, binding.target_menu);
            } else if binding.parsed.uses_win_key() {
                // Fallback strictly for Win+... shortcuts whose RegisterHotKey was claimed by the OS/shell
                let entry = HookBindingEntry {
                    id: binding.id,
                    mods: binding.parsed.mods,
                    vk: binding.parsed.vk,
                    lone_win: binding.parsed.is_lone_win(),
                };
                hook_pending.push((binding, entry));
            } else {
                let name = binding.label;
                eprintln!("ReCircles Daemon: RegisterHotKey failed for {name}");
                st.failed_hotkeys.push(name);
            }
        }

        if !hook_pending.is_empty() {
            let entries: Vec<HookBindingEntry> = hook_pending.iter().map(|(_, e)| *e).collect();
            match GlobalHotkeyHook::install(st.hwnd, WM_APP_HOOK_HOTKEY, entries) {
                Ok(guard) => {
                    st.hook_guard = Some(guard);
                    for (binding, _) in hook_pending {
                        st.hotkeys.insert(binding.id, binding.target_menu);
                    }
                }
                Err(e) => {
                    eprintln!("ReCircles Daemon: low-level keyboard hook failed: {e}");
                    for (binding, _) in hook_pending {
                        st.failed_hotkeys.push(binding.label);
                    }
                }
            }
        }
        update_tray_tooltip(st);

        let want_clipboard = installed.is_installed(BuiltinSubmenu::Clipboard);
        if want_clipboard && !st.clipboard_listening {
            if unsafe { AddClipboardFormatListener(st.hwnd) } != 0 {
                st.clipboard_listening = true;
                // Seed initial clipboard item cleanly on attach if none
                let _ = st.host.invoke(
                    BuiltinSubmenu::Clipboard,
                    &SubmenuRequest::ClipboardCaptureNow {
                        data_dir: st.store.dir().to_string_lossy().into_owned(),
                        max_items: 120,
                        ignore_internal_marker: true,
                    },
                );
            }
        } else if !want_clipboard && st.clipboard_listening {
            unsafe {
                RemoveClipboardFormatListener(st.hwnd);
            }
            st.clipboard_listening = false;
        }
    }

    fn unregister_all_hotkeys(st: &mut DaemonState) {
        st.hook_guard = None;
        for id in st.registered_os_ids.drain(..) {
            unsafe {
                UnregisterHotKey(st.hwnd, id);
            }
        }
        st.hotkeys.clear();
    }

    fn trim_working_set() {
        unsafe {
            SetProcessWorkingSetSizeEx(GetCurrentProcess(), usize::MAX, usize::MAX, 0);
        }
    }

    fn handle_hotkey(st: &DaemonState, id: i32) {
        let Some(target) = st.hotkeys.get(&id) else {
            return;
        };
        let ui = find_ui_hwnd();
        if !ui.is_null() && target.is_none() {
            // Toggle Main Ring closed when already open
            unsafe {
                PostMessageW(ui, WM_APP_CLOSE_UI, 0, 0);
            }
            return;
        }
        if !ui.is_null() {
            let _ = close_existing_ui_instances();
        }
        let exe_dir = paths::exe_dir();
        let _ = launch_ui_detached(&exe_dir, target.as_deref());
    }

    fn handle_clipboard_update(st: &DaemonState) {
        if !st.clipboard_listening {
            return;
        }
        let _ = st.host.invoke(
            BuiltinSubmenu::Clipboard,
            &SubmenuRequest::ClipboardCaptureNow {
                data_dir: st.store.dir().to_string_lossy().into_owned(),
                max_items: 120,
                ignore_internal_marker: true,
            },
        );
    }

    struct TrayMenuSnapshot {
        main_kb: String,
        builtin_entries: Vec<(String, String)>,
        custom_entries: Vec<(String, String)>,
    }

    fn snapshot_tray_menu(st: &DaemonState) -> TrayMenuSnapshot {
        let main_kb = st.cfg.keybinds.main_menu.display();
        let installed = st.host.installed();
        let mut builtin_entries = Vec::new();
        for sub in installed.installed_list() {
            let kb = st.cfg.keybinds.menu_hotkey(sub.id());
            let title = if kb.is_empty() {
                sub.title().to_string()
            } else {
                format!("{}\t{}", sub.title(), kb.display())
            };
            builtin_entries.push((title, sub.id().to_string()));
        }

        let mut custom_entries = Vec::new();
        for (id, menu_cfg) in &st.cfg.menus {
            let display_name = if menu_cfg.title.trim().is_empty() {
                id.as_str()
            } else {
                menu_cfg.title.as_str()
            };
            let kb = st.cfg.keybinds.menu_hotkey(id);
            let title = if kb.is_empty() {
                display_name.to_string()
            } else {
                format!("{display_name}\t{}", kb.display())
            };
            custom_entries.push((title, id.clone()));
        }

        TrayMenuSnapshot {
            main_kb,
            builtin_entries,
            custom_entries,
        }
    }

    fn show_tray_menu(hwnd: HWND, snap: &TrayMenuSnapshot) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }

        let append = |id: u32, text: &str| {
            let w = to_wide_nul(text);
            unsafe {
                AppendMenuW(menu, MF_STRING, id as usize, w.as_ptr());
            }
        };

        append(CMD_OPEN_MAIN, &format!("Open ReCircles ({})", snap.main_kb));
        unsafe {
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        }

        let mut idx_map: Vec<(u32, String)> = Vec::new();
        let mut next_cmd = CMD_MENU_BASE;
        for (title, id) in &snap.builtin_entries {
            append(next_cmd, title);
            idx_map.push((next_cmd, id.clone()));
            next_cmd += 1;
        }

        if !snap.custom_entries.is_empty() {
            unsafe {
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            }
            for (title, id) in &snap.custom_entries {
                append(next_cmd, title);
                idx_map.push((next_cmd, id.clone()));
                next_cmd += 1;
            }
        }

        unsafe {
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        }
        append(CMD_RELOAD, "Reload Hotkeys & Config");
        append(CMD_EXIT, "Exit Daemon");

        let mut pt = POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut pt);
            SetForegroundWindow(hwnd);
        }
        let chosen = unsafe {
            TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
                pt.x,
                pt.y,
                0,
                hwnd,
                std::ptr::null(),
            )
        } as u32;
        unsafe {
            DestroyMenu(menu);
        }

        match chosen {
            0 => {}
            CMD_OPEN_MAIN => {
                let _ = launch_ui_detached(&paths::exe_dir(), None);
            }
            CMD_RELOAD => {
                unsafe {
                    PostMessageW(hwnd, WM_APP_RELOAD_CONFIG, 0, 0);
                }
            }
            CMD_EXIT => {
                unsafe {
                    PostMessageW(hwnd, WM_APP_QUIT_DAEMON, 0, 0);
                }
            }
            other => {
                if let Some((_, id)) = idx_map.into_iter().find(|(c, _)| *c == other) {
                    let _ = launch_ui_detached(&paths::exe_dir(), Some(&id));
                }
            }
        }
    }

    fn add_tray_icon(st: &DaemonState) {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = st.hwnd;
        nid.uID = TRAY_UID;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = st.tray_icon;
        let tip = to_wide_nul("ReCircles Daemon");
        let n = tip.len().min(nid.szTip.len());
        nid.szTip[..n].copy_from_slice(&tip[..n]);
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    fn update_tray_tooltip(st: &DaemonState) {
        let text = if st.failed_hotkeys.is_empty() {
            format!("ReCircles Daemon ({} hotkeys)", st.hotkeys.len())
        } else {
            format!("ReCircles Daemon (failed: {})", st.failed_hotkeys.join(", "))
        };
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = st.hwnd;
        nid.uID = TRAY_UID;
        nid.uFlags = NIF_TIP;
        let tip = to_wide_nul(&text);
        let n = tip.len().min(nid.szTip.len().saturating_sub(1));
        nid.szTip[..n].copy_from_slice(&tip[..n]);
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    fn remove_tray_icon(st: &DaemonState) {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = st.hwnd;
        nid.uID = TRAY_UID;
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    /// Generates an anti-aliased 32x32 ReCircles ring HICON in memory with zero external assets.
    fn create_ring_tray_icon() -> HICON {
        recircles_core::app_icon::win::create_ring_hicon()
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_HOTKEY | WM_APP_HOOK_HOTKEY => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow().as_ref() {
                        handle_hotkey(st, wparam as i32);
                    }
                });
                0
            }
            WM_CLIPBOARDUPDATE => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow().as_ref() {
                        handle_clipboard_update(st);
                        trim_working_set();
                    }
                });
                0
            }
            WM_APP_RELOAD_CONFIG => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        apply_runtime_config(st);
                        trim_working_set();
                    }
                });
                0
            }
            WM_APP_QUIT_DAEMON => {
                unsafe { DestroyWindow(hwnd) };
                0
            }
            WM_APP_TRAY => {
                let evt = (lparam & 0xFFFF) as u32;
                if evt == WM_RBUTTONUP {
                    let snap = STATE.with(|slot| slot.borrow().as_ref().map(snapshot_tray_menu));
                    if let Some(snap) = snap {
                        show_tray_menu(hwnd, &snap);
                    }
                } else if evt == WM_LBUTTONUP {
                    unsafe {
                        SetForegroundWindow(hwnd);
                    }
                    let _ = launch_ui_detached(&paths::exe_dir(), None);
                }
                0
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }
}
