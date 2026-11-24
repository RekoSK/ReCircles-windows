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
    use recircles_core::config::{AppConfig, ConfigStore, HotkeyBinding, UpdateMode};
    use recircles_core::ipc::{
        DAEMON_WINDOW_CLASS, WM_APP_CHECK_UPDATES, WM_APP_CLOSE_UI, WM_APP_HOOK_HOTKEY,
        WM_APP_QUIT_DAEMON, WM_APP_RELOAD_CONFIG, WM_APP_TRAY, WM_APP_UPDATE_RESULT,
        win::{
            acquire_daemon_mutex, close_existing_ui_instances, find_ui_hwnd,
            launch_ui_detached, to_wide_nul,
        },
    };
    use recircles_core::modules::{BuiltinSubmenu, InstalledModules, host::SubmenuHost};
    use recircles_core::paths;
    use recircles_core::submenu_abi::SubmenuRequest;
    use recircles_core::updater::{
        self, CURRENT_VERSION, CheckOutcome, ReleaseInfo, UpdateState,
    };
    use recircles_core::win_hook::{HookBindingEntry, win::GlobalHotkeyHook};
    use recircles_core::win_keys::{is_known_windows_shell_hotkey, os_locked_hotkey_reason};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::DataExchange::{
        AddClipboardFormatListener, RemoveClipboardFormatListener,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleW, GetProcAddress, LoadLibraryW,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
        DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, HICON, KillTimer, MF_CHECKED,
        MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage,
        RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, SetTimer, TPM_BOTTOMALIGN,
        TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
        WM_CLIPBOARDUPDATE, WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER,
        WNDCLASSEXW, WS_OVERLAPPED,
    };

    const TRAY_UID: u32 = 1;
    const TIMER_UPDATE_CHECK: usize = 1;
    const TIMER_IDLE_APPLY: usize = 2;

    const WM_POWERBROADCAST: u32 = 0x0218;
    const PBT_APMSUSPEND: usize = 0x0004;
    const PBT_APMRESUMESUSPEND: usize = 0x0007;
    const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

    const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
    const NOTIFY_FOR_THIS_SESSION: u32 = 0;
    const WTS_CONSOLE_CONNECT: usize = 0x1;
    const WTS_CONSOLE_DISCONNECT: usize = 0x2;
    const WTS_REMOTE_CONNECT: usize = 0x3;
    const WTS_REMOTE_DISCONNECT: usize = 0x4;
    const WTS_SESSION_LOCK: usize = 0x7;
    const WTS_SESSION_UNLOCK: usize = 0x8;

    static TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

    const CMD_OPEN_MAIN: u32 = 1001;
    const CMD_RELOAD: u32 = 1002;
    const CMD_EXIT: u32 = 1003;
    const CMD_UPDATE_CHECK: u32 = 1010;
    const CMD_UPDATE_INSTALL: u32 = 1011;
    const CMD_UPDATE_MODE_AUTO: u32 = 1012;
    const CMD_UPDATE_MODE_NOTIFY: u32 = 1013;
    const CMD_UPDATE_MODE_MANUAL: u32 = 1014;
    const CMD_UPDATE_PRERELEASE: u32 = 1015;
    const CMD_MENU_BASE: u32 = 2000;

    enum UpdateJobResult {
        Checked {
            manual: bool,
            outcome: Result<CheckOutcome, String>,
        },
        Downloaded {
            manual: bool,
            release: ReleaseInfo,
            result: Result<PathBuf, String>,
        },
    }

    enum DaemonTask {
        LaunchUi {
            close_existing: bool,
            target: Option<String>,
        },
        CaptureClipboard {
            host: Arc<SubmenuHost>,
            data_dir: String,
        },
    }

    struct DaemonWorker {
        tx: Option<mpsc::Sender<DaemonTask>>,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl DaemonWorker {
        fn spawn() -> Self {
            let (tx, rx) = mpsc::channel::<DaemonTask>();
            let join = std::thread::Builder::new()
                .name("recircles-daemon-worker".into())
                .spawn(move || {
                    while let Ok(task) = rx.recv() {
                        match task {
                            DaemonTask::LaunchUi {
                                close_existing,
                                target,
                            } => {
                                if close_existing {
                                    let _ = close_existing_ui_instances();
                                }
                                let exe_dir = paths::exe_dir();
                                let _ = launch_ui_detached(&exe_dir, target.as_deref());
                            }
                            DaemonTask::CaptureClipboard { host, data_dir } => {
                                let _ = host.invoke(
                                    BuiltinSubmenu::Clipboard,
                                    &SubmenuRequest::ClipboardCaptureNow {
                                        data_dir,
                                        max_items: 120,
                                        ignore_internal_marker: true,
                                    },
                                );
                            }
                        }
                    }
                })
                .ok();
            Self { tx: Some(tx), join }
        }

        fn enqueue_launch(&self, close_existing: bool, target: Option<String>) {
            if let Some(tx) = &self.tx {
                let _ = tx.send(DaemonTask::LaunchUi {
                    close_existing,
                    target,
                });
            } else {
                if close_existing {
                    let _ = close_existing_ui_instances();
                }
                let _ = launch_ui_detached(&paths::exe_dir(), target.as_deref());
            }
        }

        fn enqueue_clipboard(&self, host: Arc<SubmenuHost>, data_dir: String) {
            if let Some(tx) = &self.tx {
                let _ = tx.send(DaemonTask::CaptureClipboard { host, data_dir });
            }
        }
    }

    impl Drop for DaemonWorker {
        fn drop(&mut self) {
            self.tx.take();
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    type FnWtsRegisterSessionNotification = unsafe extern "system" fn(HWND, u32) -> i32;
    type FnWtsUnRegisterSessionNotification = unsafe extern "system" fn(HWND) -> i32;

    struct WtsSessionGuard {
        module: HMODULE,
        hwnd: HWND,
        unregister: Option<FnWtsUnRegisterSessionNotification>,
        registered: bool,
    }

    impl WtsSessionGuard {
        fn register(hwnd: HWND) -> Self {
            let dll = to_wide_nul("wtsapi32.dll");
            let module = unsafe { LoadLibraryW(dll.as_ptr()) };
            if module.is_null() {
                return Self {
                    module,
                    hwnd,
                    unregister: None,
                    registered: false,
                };
            }
            let reg_sym = unsafe {
                GetProcAddress(module, c"WTSRegisterSessionNotification".as_ptr().cast())
            };
            let unreg_sym = unsafe {
                GetProcAddress(module, c"WTSUnRegisterSessionNotification".as_ptr().cast())
            };
            let (Some(reg_ptr), Some(unreg_ptr)) = (reg_sym, unreg_sym) else {
                unsafe { FreeLibrary(module) };
                return Self {
                    module: std::ptr::null_mut(),
                    hwnd,
                    unregister: None,
                    registered: false,
                };
            };
            let reg_fn: FnWtsRegisterSessionNotification = unsafe { std::mem::transmute(reg_ptr) };
            let unreg_fn: FnWtsUnRegisterSessionNotification =
                unsafe { std::mem::transmute(unreg_ptr) };
            let ok = unsafe { reg_fn(hwnd, NOTIFY_FOR_THIS_SESSION) } != 0;
            if !ok {
                unsafe { FreeLibrary(module) };
                return Self {
                    module: std::ptr::null_mut(),
                    hwnd,
                    unregister: None,
                    registered: false,
                };
            }
            Self {
                module,
                hwnd,
                unregister: Some(unreg_fn),
                registered: true,
            }
        }
    }

    impl Drop for WtsSessionGuard {
        fn drop(&mut self) {
            if self.registered
                && let Some(unreg) = self.unregister
            {
                unsafe {
                    let _ = unreg(self.hwnd);
                }
                self.registered = false;
            }
            if !self.module.is_null() {
                unsafe {
                    FreeLibrary(self.module);
                }
                self.module = std::ptr::null_mut();
            }
        }
    }

    struct DaemonState {
        hwnd: HWND,
        store: ConfigStore,
        cfg: AppConfig,
        host: Arc<SubmenuHost>,
        worker: DaemonWorker,
        _wts_guard: WtsSessionGuard,
        hotkeys: HashMap<i32, Option<String>>,
        registered_os_ids: Vec<i32>,
        hook_guard: Option<GlobalHotkeyHook>,
        failed_hotkeys: Vec<String>,
        clipboard_listening: bool,
        tray_added: bool,
        tray_icon: HICON,
        update_busy: bool,
        available_update: Option<ReleaseInfo>,
        notified_version: Option<String>,
        pending_auto_install: Option<(ReleaseInfo, PathBuf)>,
        update_inbox: Arc<Mutex<Vec<UpdateJobResult>>>,
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

        let tb_name = to_wide_nul("TaskbarCreated");
        let tb_msg = unsafe { RegisterWindowMessageW(tb_name.as_ptr()) };
        TASKBAR_CREATED_MSG.store(tb_msg, Ordering::Relaxed);

        let wts_guard = WtsSessionGuard::register(hwnd);
        let store = ConfigStore::default_store();
        updater::cleanup_old_installers(store.dir());
        let cached_avail = UpdateState::load(store.dir())
            .available_update_for(CURRENT_VERSION)
            .cloned();
        let installed = InstalledModules::detect_default();
        let cfg = store
            .load_or_create(&installed)
            .unwrap_or_else(|_| AppConfig::default_for_installed(&installed));
        let tray_icon = create_ring_tray_icon();

        let mut st = DaemonState {
            hwnd,
            store,
            cfg,
            host: Arc::new(SubmenuHost::new(installed)),
            worker: DaemonWorker::spawn(),
            _wts_guard: wts_guard,
            hotkeys: HashMap::new(),
            registered_os_ids: Vec::new(),
            hook_guard: None,
            failed_hotkeys: Vec::new(),
            clipboard_listening: false,
            tray_added: false,
            tray_icon,
            update_busy: false,
            available_update: cached_avail,
            notified_version: None,
            pending_auto_install: None,
            update_inbox: Arc::new(Mutex::new(Vec::new())),
        };
        // Install global hotkeys / WH_KEYBOARD_LL BEFORE Shell_NotifyIconW so Win+E works
        // immediately at logon even while explorer.exe is still bringing up the taskbar.
        apply_runtime_config(&mut st);
        add_tray_icon(&mut st);
        update_tray_tooltip(&st);
        // Check for updates immediately on tray/daemon startup when Auto-install or Notify is enabled
        if st.cfg.updates.mode != UpdateMode::Manual {
            start_update_check(&mut st, false);
        }

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
                unsafe {
                    KillTimer(st.hwnd, TIMER_UPDATE_CHECK);
                    KillTimer(st.hwnd, TIMER_IDLE_APPLY);
                }
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
        unregister_os_hotkeys(st);
        st.failed_hotkeys.clear();
        let installed = InstalledModules::detect_default();
        let modules_changed = BuiltinSubmenu::ALL
            .iter()
            .any(|&sub| installed.is_installed(sub) != st.host.installed().is_installed(sub));
        if modules_changed {
            st.host = Arc::new(SubmenuHost::new(installed.clone()));
        }
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
            if let Some(hook) = &st.hook_guard {
                // Update active bindings in-place on the dedicated hook thread (zero unhook gap).
                hook.update_bindings(entries);
                for (binding, _) in hook_pending {
                    st.hotkeys.insert(binding.id, binding.target_menu);
                }
            } else {
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
        } else {
            st.hook_guard = None;
        }
        if st.tray_added {
            update_tray_tooltip(st);
        }

        let want_clipboard = installed.is_installed(BuiltinSubmenu::Clipboard);
        if want_clipboard && !st.clipboard_listening {
            if unsafe { AddClipboardFormatListener(st.hwnd) } != 0 {
                st.clipboard_listening = true;
                // Seed initial clipboard item cleanly off the daemon UI thread
                st.worker.enqueue_clipboard(
                    Arc::clone(&st.host),
                    st.store.dir().to_string_lossy().into_owned(),
                );
            }
        } else if !want_clipboard && st.clipboard_listening {
            unsafe {
                RemoveClipboardFormatListener(st.hwnd);
            }
            st.clipboard_listening = false;
        }
        schedule_update_timer(st);
    }

    fn schedule_update_timer(st: &DaemonState) {
        unsafe {
            KillTimer(st.hwnd, TIMER_UPDATE_CHECK);
        }
        if st.cfg.updates.mode == UpdateMode::Manual {
            return;
        }
        let hours = st.cfg.updates.check_interval_hours.clamp(1, 168) as u64;
        let ms = (hours * 3_600_000).min(u32::MAX as u64) as u32;
        unsafe {
            SetTimer(st.hwnd, TIMER_UPDATE_CHECK, ms, None);
        }
    }

    fn start_update_check(st: &mut DaemonState, manual: bool) {
        if st.update_busy {
            return;
        }
        st.update_busy = true;
        update_tray_tooltip(st);
        let config_dir = st.store.dir().to_path_buf();
        let include_pre = st.cfg.updates.include_prereleases;
        let inbox = st.update_inbox.clone();
        let hwnd_raw = st.hwnd as usize;
        std::thread::spawn(move || {
            let outcome = updater::check_for_update(&config_dir, include_pre);
            if let Ok(mut g) = inbox.lock() {
                g.push(UpdateJobResult::Checked { manual, outcome });
            }
            unsafe {
                PostMessageW(hwnd_raw as HWND, WM_APP_UPDATE_RESULT, 0, 0);
            }
        });
    }

    fn start_update_download(st: &mut DaemonState, release: ReleaseInfo, manual: bool) {
        if st.update_busy {
            return;
        }
        st.update_busy = true;
        update_tray_tooltip(st);
        let config_dir = st.store.dir().to_path_buf();
        let inbox = st.update_inbox.clone();
        let hwnd_raw = st.hwnd as usize;
        std::thread::spawn(move || {
            let result = updater::download_update(&config_dir, &release);
            if let Ok(mut g) = inbox.lock() {
                g.push(UpdateJobResult::Downloaded {
                    manual,
                    release,
                    result,
                });
            }
            unsafe {
                PostMessageW(hwnd_raw as HWND, WM_APP_UPDATE_RESULT, 0, 0);
            }
        });
    }

    fn try_launch_upgrade(st: &mut DaemonState, release: &ReleaseInfo, installer_path: &PathBuf) -> Result<(), String> {
        let install_dir = updater::installed_dir_from_manifest(st.store.dir())
            .unwrap_or_else(paths::exe_dir);
        updater::launch_installer_upgrade(
            installer_path,
            &install_dir,
            st.store.dir(),
            Some(std::process::id()),
            CURRENT_VERSION,
            &release.version,
            false,
        )
    }

    fn handle_update_results(st: &mut DaemonState) {
        let items: Vec<UpdateJobResult> = st
            .update_inbox
            .lock()
            .map(|mut g| g.drain(..).collect())
            .unwrap_or_default();

        for item in items {
            st.update_busy = false;
            match item {
                UpdateJobResult::Checked { manual, outcome } => match outcome {
                    Ok(CheckOutcome::UpdateAvailable(rel)) => {
                        let ver = rel.version.clone();
                        st.available_update = Some(rel.clone());
                        let installed_copy = updater::is_installed_copy(st.store.dir());
                        if !manual && st.cfg.updates.mode == UpdateMode::AutoInstall && installed_copy {
                            start_update_download(st, rel, false);
                        } else if manual || st.notified_version.as_deref() != Some(&ver) {
                            st.notified_version = Some(ver.clone());
                            show_tray_balloon(
                                st,
                                "ReCircles Update Available",
                                &format!(
                                    "Version v{ver} is available. Right-click the tray icon -> Updates -> Install Update."
                                ),
                            );
                        }
                    }
                    Ok(CheckOutcome::UpToDate { current, .. }) => {
                        st.available_update = None;
                        if manual {
                            show_tray_balloon(
                                st,
                                "ReCircles is Up to Date",
                                &format!("You are running the latest version (v{current})."),
                            );
                        }
                    }
                    Err(err) => {
                        eprintln!("ReCircles Daemon: update check failed: {err}");
                        if manual {
                            show_tray_balloon(st, "Update Check Failed", &err);
                        }
                    }
                },
                UpdateJobResult::Downloaded {
                    manual,
                    release,
                    result,
                } => match result {
                    Ok(installer_path) => {
                        if !manual && !find_ui_hwnd().is_null() {
                            // Wait until the ring UI is closed/idle before upgrading automatically
                            st.pending_auto_install = Some((release, installer_path));
                            unsafe {
                                SetTimer(st.hwnd, TIMER_IDLE_APPLY, 10_000, None);
                            }
                        } else {
                            match try_launch_upgrade(st, &release, &installer_path) {
                                Ok(()) => {
                                    // Exit gracefully so the 3D updater overlay can replace our binary cleanly
                                    unsafe {
                                        PostMessageW(st.hwnd, WM_APP_QUIT_DAEMON, 0, 0);
                                    }
                                }
                                Err(err) => {
                                    eprintln!("ReCircles Daemon: failed launching updater: {err}");
                                    if manual {
                                        show_tray_balloon(st, "Update Failed", &err);
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("ReCircles Daemon: update download failed: {err}");
                        if manual {
                            show_tray_balloon(st, "Update Download Failed", &err);
                        }
                    }
                },
            }
        }
        update_tray_tooltip(st);
    }

    fn set_update_mode(st: &mut DaemonState, mode: UpdateMode) {
        if st.cfg.updates.mode == mode {
            return;
        }
        st.cfg.updates.mode = mode;
        let _ = st.store.save(&st.cfg);
        schedule_update_timer(st);
        update_tray_tooltip(st);
        if mode == UpdateMode::AutoInstall
            && let Some(rel) = st.available_update.clone()
            && updater::is_installed_copy(st.store.dir())
        {
            start_update_download(st, rel, false);
        }
    }

    fn toggle_prereleases(st: &mut DaemonState) {
        st.cfg.updates.include_prereleases = !st.cfg.updates.include_prereleases;
        let _ = st.store.save(&st.cfg);
        if st.cfg.updates.mode != UpdateMode::Manual {
            start_update_check(st, false);
        }
    }

    fn unregister_os_hotkeys(st: &mut DaemonState) {
        for id in st.registered_os_ids.drain(..) {
            unsafe {
                UnregisterHotKey(st.hwnd, id);
            }
        }
        st.hotkeys.clear();
    }

    fn unregister_all_hotkeys(st: &mut DaemonState) {
        st.hook_guard = None;
        unregister_os_hotkeys(st);
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
        st.worker.enqueue_launch(!ui.is_null(), target.clone());
    }

    fn handle_clipboard_update(st: &DaemonState) {
        if !st.clipboard_listening {
            return;
        }
        st.worker.enqueue_clipboard(
            Arc::clone(&st.host),
            st.store.dir().to_string_lossy().into_owned(),
        );
    }

    struct TrayMenuSnapshot {
        main_kb: String,
        builtin_entries: Vec<(String, String)>,
        custom_entries: Vec<(String, String)>,
        update_mode: UpdateMode,
        include_prereleases: bool,
        update_busy: bool,
        available_version: Option<String>,
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
            update_mode: st.cfg.updates.mode,
            include_prereleases: st.cfg.updates.include_prereleases,
            update_busy: st.update_busy,
            available_version: st.available_update.as_ref().map(|r| r.version.clone()),
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
        if let Some(ver) = &snap.available_version {
            let lbl = if snap.update_busy {
                format!("Downloading Update v{ver}…")
            } else {
                format!("Install Update (v{ver})")
            };
            let w = to_wide_nul(&lbl);
            let flags = if snap.update_busy {
                MF_STRING | MF_GRAYED
            } else {
                MF_STRING
            };
            unsafe {
                AppendMenuW(menu, flags, CMD_UPDATE_INSTALL as usize, w.as_ptr());
            }
        }
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

        // Updates submenu (Check now, Install if available, Mode selector, Beta toggle)
        let updates_menu = unsafe { CreatePopupMenu() };
        if !updates_menu.is_null() {
            let append_sub = |flags: u32, id: u32, text: &str| {
                let w = to_wide_nul(text);
                unsafe {
                    AppendMenuW(updates_menu, flags, id as usize, w.as_ptr());
                }
            };
            if let Some(ver) = &snap.available_version {
                append_sub(
                    if snap.update_busy {
                        MF_STRING | MF_GRAYED
                    } else {
                        MF_STRING
                    },
                    CMD_UPDATE_INSTALL,
                    &format!("Install Update (v{ver})"),
                );
            }
            append_sub(
                if snap.update_busy {
                    MF_STRING | MF_GRAYED
                } else {
                    MF_STRING
                },
                CMD_UPDATE_CHECK,
                if snap.update_busy {
                    "Checking for Updates…"
                } else {
                    "Check for Updates"
                },
            );
            unsafe {
                AppendMenuW(updates_menu, MF_SEPARATOR, 0, std::ptr::null());
            }
            let mode_flag = |m: UpdateMode| {
                if snap.update_mode == m {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                }
            };
            append_sub(
                mode_flag(UpdateMode::AutoInstall),
                CMD_UPDATE_MODE_AUTO,
                "Auto-install Updates",
            );
            append_sub(
                mode_flag(UpdateMode::Notify),
                CMD_UPDATE_MODE_NOTIFY,
                "Notify Only",
            );
            append_sub(
                mode_flag(UpdateMode::Manual),
                CMD_UPDATE_MODE_MANUAL,
                "Manual Only",
            );
            unsafe {
                AppendMenuW(updates_menu, MF_SEPARATOR, 0, std::ptr::null());
            }
            append_sub(
                if snap.include_prereleases {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                },
                CMD_UPDATE_PRERELEASE,
                "Include Beta / Pre-releases",
            );
            let sub_title = match &snap.available_version {
                Some(ver) => format!("Updates (v{ver} available)"),
                None => format!("Updates (v{CURRENT_VERSION})"),
            };
            let sub_w = to_wide_nul(&sub_title);
            unsafe {
                AppendMenuW(menu, MF_POPUP, updates_menu as usize, sub_w.as_ptr());
            }
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
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow().as_ref() {
                        st.worker.enqueue_launch(false, None);
                    }
                });
            }
            CMD_UPDATE_CHECK => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        start_update_check(st, true);
                    }
                });
            }
            CMD_UPDATE_INSTALL => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut()
                        && let Some(rel) = st.available_update.clone()
                    {
                        start_update_download(st, rel, true);
                    }
                });
            }
            CMD_UPDATE_MODE_AUTO => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        set_update_mode(st, UpdateMode::AutoInstall);
                    }
                });
            }
            CMD_UPDATE_MODE_NOTIFY => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        set_update_mode(st, UpdateMode::Notify);
                    }
                });
            }
            CMD_UPDATE_MODE_MANUAL => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        set_update_mode(st, UpdateMode::Manual);
                    }
                });
            }
            CMD_UPDATE_PRERELEASE => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        toggle_prereleases(st);
                    }
                });
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
                    STATE.with(|slot| {
                        if let Some(st) = slot.borrow().as_ref() {
                            st.worker.enqueue_launch(false, Some(id));
                        }
                    });
                }
            }
        }
    }

    fn add_tray_icon(st: &mut DaemonState) {
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
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) != 0 };
        st.tray_added = added || st.tray_added;
    }

    fn update_tray_tooltip(st: &DaemonState) {
        let text = if !st.failed_hotkeys.is_empty() {
            format!("ReCircles Daemon (failed: {})", st.failed_hotkeys.join(", "))
        } else if st.update_busy {
            "ReCircles Daemon (checking/downloading update…)".to_string()
        } else if let Some(rel) = &st.available_update {
            format!("ReCircles Daemon (update v{} available)", rel.version)
        } else {
            format!("ReCircles Daemon ({} hotkeys)", st.hotkeys.len())
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

    fn show_tray_balloon(st: &DaemonState, title: &str, body: &str) {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = st.hwnd;
        nid.uID = TRAY_UID;
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        let title_w = to_wide_nul(title);
        let tn = title_w.len().min(nid.szInfoTitle.len().saturating_sub(1));
        nid.szInfoTitle[..tn].copy_from_slice(&title_w[..tn]);
        let body_w = to_wide_nul(body);
        let bn = body_w.len().min(nid.szInfo.len().saturating_sub(1));
        nid.szInfo[..bn].copy_from_slice(&body_w[..bn]);
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
        let tb_msg = TASKBAR_CREATED_MSG.load(Ordering::Relaxed);
        if tb_msg != 0 && msg == tb_msg {
            STATE.with(|slot| {
                if let Some(st) = slot.borrow_mut().as_mut() {
                    add_tray_icon(st);
                    update_tray_tooltip(st);
                    if let Some(hook) = &st.hook_guard {
                        hook.reinstall();
                    }
                }
            });
            return 0;
        }

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
                    }
                });
                0
            }
            WM_APP_RELOAD_CONFIG => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        apply_runtime_config(st);
                    }
                });
                0
            }
            WM_APP_CHECK_UPDATES => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        start_update_check(st, wparam != 0);
                    }
                });
                0
            }
            WM_APP_UPDATE_RESULT => {
                STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        handle_update_results(st);
                    }
                });
                0
            }
            WM_WTSSESSION_CHANGE => {
                match wparam {
                    WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => {
                        STATE.with(|slot| {
                            if let Some(st) = slot.borrow().as_ref()
                                && let Some(hook) = &st.hook_guard
                            {
                                hook.reset_state();
                            }
                        });
                    }
                    WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                        STATE.with(|slot| {
                            if let Some(st) = slot.borrow().as_ref()
                                && let Some(hook) = &st.hook_guard
                            {
                                hook.reinstall();
                            }
                        });
                    }
                    _ => {}
                }
                0
            }
            WM_POWERBROADCAST => {
                match wparam {
                    PBT_APMSUSPEND => {
                        STATE.with(|slot| {
                            if let Some(st) = slot.borrow().as_ref()
                                && let Some(hook) = &st.hook_guard
                            {
                                hook.reset_state();
                            }
                        });
                    }
                    PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC => {
                        STATE.with(|slot| {
                            if let Some(st) = slot.borrow().as_ref()
                                && let Some(hook) = &st.hook_guard
                            {
                                hook.reinstall();
                            }
                        });
                    }
                    _ => {}
                }
                0
            }
            WM_TIMER => {
                if wparam == TIMER_UPDATE_CHECK {
                    STATE.with(|slot| {
                        if let Some(st) = slot.borrow_mut().as_mut()
                            && st.cfg.updates.mode != UpdateMode::Manual
                        {
                            start_update_check(st, false);
                        }
                    });
                } else if wparam == TIMER_IDLE_APPLY {
                    STATE.with(|slot| {
                        if let Some(st) = slot.borrow_mut().as_mut()
                            && find_ui_hwnd().is_null()
                            && let Some((rel, path)) = st.pending_auto_install.take()
                        {
                            unsafe {
                                KillTimer(st.hwnd, TIMER_IDLE_APPLY);
                            }
                            if try_launch_upgrade(st, &rel, &path).is_ok() {
                                unsafe {
                                    PostMessageW(st.hwnd, WM_APP_QUIT_DAEMON, 0, 0);
                                }
                            }
                        }
                    });
                }
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
                    STATE.with(|slot| {
                        if let Some(st) = slot.borrow().as_ref() {
                            st.worker.enqueue_launch(false, None);
                        }
                    });
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
