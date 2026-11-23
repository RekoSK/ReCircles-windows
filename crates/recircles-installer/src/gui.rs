#[cfg(windows)]
pub mod win_gui {
    use crate::d2d::{D2dCanvas, D2dColor, D2dGradStop};
    use crate::engine::{
        InstallOptions, InstallSummary, NavPreset, SUBMENU_EXCLUSION_WARNING, UpgradeOptions,
        UpgradeProgress, enforce_mandatory_submenus, execute_install, execute_uninstall,
        execute_upgrade, load_existing_manifest,
    };
    use crate::icons::{UiGlyph, glyph_cmds};
    use crate::spring::{
        AppleSpring, LiveRingMotion, LoadingPose3D, PaletteTransition, Rgb, ThemePalette, lerp,
        lerp_rgb,
    };
    use crate::vsync::{VsyncPacer, WM_APP_INSTALL_DONE, WM_APP_TICK};
    use recircles_core::config::{
        AppearanceConfig, KeyList, KeybindsConfig, LoadingStyle, OverflowLayout, UpdateMode,
    };
    use recircles_core::ipc::{WM_APP_CAPTURE_KEY, win::to_wide_nul};
    use recircles_core::modules::BuiltinSubmenu;
    use recircles_core::settings::NavKeyField;
    use recircles_core::themes::{BuiltinTheme, builtin_theme, builtin_themes};
    use recircles_core::win_hook::win::CaptureHook;
    use recircles_core::win_keys::{
        Modifiers, VK_BACK, VK_ESCAPE, VK_LWIN as CORE_VK_LWIN, VK_RWIN as CORE_VK_RWIN,
        format_vk_parts, parse_global_hotkey, parse_key_spec,
    };
    use std::borrow::Cow;
    use std::cell::RefCell;
    use std::f64::consts::{PI, TAU};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::ptr;
    use std::sync::{Arc, LazyLock, Mutex, OnceLock};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ValidateRect,
    };
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
        SetProcessDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, GetKeyboardState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT,
        ToUnicode, TrackMouseEvent, VK_CONTROL, VK_LEFT, VK_LWIN, VK_MENU, VK_RETURN, VK_RIGHT,
        VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
        GetMessageW, GetWindowRect, IDC_ARROW, IDC_HAND, IDC_IBEAM, IsIconic, KillTimer,
        LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassExW,
        SPI_GETCLIENTAREAANIMATION, SW_MINIMIZE, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOSIZE,
        SWP_NOZORDER, SetCursor, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow,
        SystemParametersInfoW, TranslateMessage, WM_ACTIVATE, WM_CAPTURECHANGED, WM_DESTROY,
        WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
        WM_MOUSEWHEEL, WM_NCCALCSIZE, WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_SYSKEYDOWN, WM_TIMER,
        WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MINIMIZEBOX, WS_POPUP,
        WS_SYSMENU,
    };

    const WS_EX_NOREDIRECTIONBITMAP: u32 = 0x0020_0000;
    const WM_MOUSELEAVE: u32 = 0x02A3;
    const SIZE_RESTORED_VAL: usize = 0;
    const SIZE_MINIMIZED_VAL: usize = 1;
    const SIZE_MAXIMIZED_VAL: usize = 2;
    const WA_INACTIVE_VAL: usize = 0;
    const TIMER_CARET: usize = 1;
    const TIMER_PULSE: usize = 2;
    const CARET_BLINK_MS: u32 = 530;
    const PULSE_INTERVAL_MS: u32 = 33;

    mod perf {
        use super::*;
        use std::io::Write;

        struct FrameSample {
            t_ms: f64,
            render_cpu_us: u64,
            present_us: u64,
            interval_us: u64,
            page: usize,
            animating: bool,
        }

        struct PerfState {
            path: PathBuf,
            launch_t0: Instant,
            prev_frame_t0: Option<Instant>,
            phases: Vec<(&'static str, f64)>,
            frames: Vec<FrameSample>,
        }

        static PERF: OnceLock<Option<Mutex<PerfState>>> = OnceLock::new();

        fn state() -> Option<&'static Mutex<PerfState>> {
            PERF.get_or_init(|| {
                let p = std::env::var_os("RECIRCLES_PERF_LOG")?;
                if p.is_empty() {
                    return None;
                }
                Some(Mutex::new(PerfState {
                    path: PathBuf::from(p),
                    launch_t0: Instant::now(),
                    prev_frame_t0: None,
                    phases: Vec::with_capacity(16),
                    frames: Vec::with_capacity(4096),
                }))
            })
            .as_ref()
        }

        pub fn init_launch() {
            let _ = state();
        }

        pub fn is_enabled() -> bool {
            state().is_some()
        }

        pub fn record_phase(name: &'static str, ms: f64) {
            if let Some(m) = state()
                && let Ok(mut g) = m.lock()
            {
                g.phases.push((name, ms));
            }
        }

        pub fn next_interval_us(now: Instant) -> u64 {
            if let Some(m) = state()
                && let Ok(mut g) = m.lock()
            {
                let iv = g
                    .prev_frame_t0
                    .map(|p| now.duration_since(p).as_micros() as u64)
                    .unwrap_or(0);
                g.prev_frame_t0 = Some(now);
                iv
            } else {
                0
            }
        }

        pub fn record_frame(
            now: Instant,
            render_cpu_us: u64,
            present_us: u64,
            interval_us: u64,
            page: usize,
            animating: bool,
        ) {
            if let Some(m) = state()
                && let Ok(mut g) = m.lock()
            {
                let t_ms = now.duration_since(g.launch_t0).as_secs_f64() * 1000.0;
                g.frames.push(FrameSample {
                    t_ms,
                    render_cpu_us,
                    present_us,
                    interval_us,
                    page,
                    animating,
                });
            }
        }

        fn stats(vals: &[u64]) -> (f64, u64, u64, u64) {
            if vals.is_empty() {
                return (0.0, 0, 0, 0);
            }
            let mut s = vals.to_vec();
            s.sort_unstable();
            let mean = s.iter().map(|&v| v as f64).sum::<f64>() / s.len() as f64;
            let p50 = s[s.len() * 50 / 100];
            let p95 = s[(s.len() * 95 / 100).min(s.len() - 1)];
            let p99 = s[(s.len() * 99 / 100).min(s.len() - 1)];
            (mean, p50, p95, p99)
        }

        pub fn flush() {
            let Some(m) = state() else {
                return;
            };
            let Ok(g) = m.lock() else {
                return;
            };
            if let Some(parent) = g.path.parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = std::fs::create_dir_all(parent);
            }
            let Ok(mut f) = std::fs::File::create(&g.path) else {
                return;
            };
            for (name, ms) in &g.phases {
                let _ = writeln!(f, "PHASE {name}={ms:.3}ms");
            }
            let r_us: Vec<u64> = g.frames.iter().map(|x| x.render_cpu_us).collect();
            let p_us: Vec<u64> = g.frames.iter().map(|x| x.present_us).collect();
            let iv_us: Vec<u64> = g
                .frames
                .iter()
                .map(|x| x.interval_us)
                .filter(|&v| v > 0 && v < 100_000)
                .collect();
            let (rm, r50, r95, r99) = stats(&r_us);
            let (pm, p50, p95, p99) = stats(&p_us);
            let (im, i50, i95, i99) = stats(&iv_us);
            let _ = writeln!(f, "FRAMES_TOTAL={}", g.frames.len());
            let _ = writeln!(
                f,
                "RENDER_CPU_US mean={rm:.1} p50={r50} p95={r95} p99={r99}"
            );
            let _ = writeln!(
                f,
                "PRESENT_US mean={pm:.1} p50={p50} p95={p95} p99={p99}"
            );
            let _ = writeln!(
                f,
                "INTERVAL_US mean={im:.1} p50={i50} p95={i95} p99={i99}"
            );
            for fr in &g.frames {
                let _ = writeln!(
                    f,
                    "FRAME t={:.2} render_us={} present_us={} interval_us={} page={} anim={}",
                    fr.t_ms,
                    fr.render_cpu_us,
                    fr.present_us,
                    fr.interval_us,
                    fr.page,
                    fr.animating as u8
                );
            }
        }
    }

    // Floating glass window logical geometry
    const PAD: f64 = 20.0;
    const SHELL_W: f64 = 900.0;
    const SHELL_H: f64 = 616.0;
    const SHELL_R: f64 = 24.0;
    const WIN_W: f64 = SHELL_W + PAD * 2.0;
    const WIN_H: f64 = SHELL_H + PAD * 2.0;

    const LEFT_W: f64 = 288.0;
    const RIGHT_X: f64 = PAD + LEFT_W;
    const RIGHT_W: f64 = SHELL_W - LEFT_W;

    // Baseline typographic scale
    const FONT_TITLE: f64 = 22.0;
    const FONT_BRAND: f64 = 18.0;
    const FONT_SUBHEAD: f64 = 14.0;
    const FONT_BODY: f64 = 13.0;
    const FONT_SECONDARY: f64 = 12.0;
    const FONT_CAPTION: f64 = 11.0;

    // Live ReCircles radial ring stage geometry
    const RING_CX: f64 = PAD + LEFT_W * 0.5;
    const RING_CY: f64 = PAD + 190.0;
    const RING_OUTER: f64 = 96.0;
    const RING_INNER: f64 = 40.0;
    const RING_ITEM_R: f64 = 68.0;
    const RING_SEGMENTS: usize = 64;
    const RING_HALF_THICKNESS: f64 = 12.0;
    const PERSPECTIVE_D: f64 = 520.0;

    const PAGE_SETUP: usize = 0;
    const PAGE_SUBMENUS: usize = 1;
    const PAGE_THEME: usize = 2;
    const PAGE_APPEARANCE: usize = 3;
    const PAGE_KEYBINDS: usize = 4;
    const PAGE_INSTALL: usize = 5;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum KeybindCapture {
        MainRing,
        Submenu(String),
        Nav(NavKeyField),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum SliderDragKind {
        None,
        Translucency,
        Blur,
    }

    static RING_COS_SIN: LazyLock<[(f64, f64); RING_SEGMENTS]> = LazyLock::new(|| {
        std::array::from_fn(|i| {
            let a = TAU * i as f64 / RING_SEGMENTS as f64;
            (a.cos(), a.sin())
        })
    });

    static RING_MID_COS_SIN: LazyLock<[(f64, f64); RING_SEGMENTS]> = LazyLock::new(|| {
        std::array::from_fn(|i| {
            let a = TAU * (i as f64 + 0.5) / RING_SEGMENTS as f64;
            (a.cos(), a.sin())
        })
    });

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum ClickAction {
        MinimizeWindow,
        CloseWindow,
        GoPage(usize),
        Next,
        Back,
        FocusInstallDir,
        BrowseInstallDir,
        ResetInstallDir,
        ToggleStartup,
        ToggleShortcuts,
        ToggleLaunchDaemon,
        SetUpdateMode(UpdateMode),
        UninstallExisting,
        ConfirmUninstall,
        CancelUninstall,
        ToggleSubmenu(BuiltinSubmenu),
        SelectTheme(&'static str),
        SetOverflowLayout(OverflowLayout),
        ToggleAnimations,
        ToggleLoading3d,
        ToggleHighContrast,
        ResetAppearance,
        ToggleNumberShortcuts,
        SetNavPreset(NavPreset),
        ResetCustomKeybinds,
        CaptureMainHotkey,
        ClearMainHotkey,
        CaptureSubmenuHotkey(String),
        ClearSubmenuHotkey(String),
        CaptureNavKeybind(NavKeyField),
        ClearNavKeybind(NavKeyField),
        RunInstall,
        Finish,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CursorKind {
        Arrow,
        Hand,
        IBeam,
    }

    #[derive(Debug, Clone)]
    struct HitRegion {
        id: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        action: ClickAction,
        cursor: CursorKind,
        hover_submenu: Option<BuiltinSubmenu>,
    }

    impl HitRegion {
        fn contains(&self, px: f64, py: f64, pad: f64) -> bool {
            px >= self.x - pad
                && px <= self.x + self.w + pad
                && py >= self.y - pad
                && py <= self.y + self.h + pad
        }
    }

    struct DragWindow {
        start_screen_x: i32,
        start_screen_y: i32,
        win_start_x: i32,
        win_start_y: i32,
    }

    struct WizardState {
        hwnd: HWND,
        canvas: D2dCanvas,
        pacer: VsyncPacer,
        scale: f64,
        reduced_motion: bool,
        last_tick: Instant,
        clock: f64,
        minimized: bool,
        focused: bool,
        closing: bool,
        has_capture: bool,
        last_animating: bool,
        recreating: bool,
        caret_visible: bool,
        caret_timer_on: bool,
        pulse_timer_on: bool,
        capture_hook: Option<CaptureHook>,
        last_mouse_phys: (i32, i32),
        backdrop_mon_origin: (i32, i32),
        last_blur_pct: u8,

        // Wizard domain state
        page: usize,
        prev_page: usize,
        page_dir: f64,
        opts: InstallOptions,
        install_dir_str: String,
        dir_error: Option<String>,
        editing_dir: bool,
        existing_installed: bool,
        confirm_uninstall_open: bool,
        custom_menus: Vec<(String, String)>,
        capturing_hotkey: Option<KeybindCapture>,
        status_msg: Option<(String, bool)>,
        installing: bool,
        install_result: Arc<Mutex<Option<Result<InstallSummary, String>>>>,
        summary: Option<InstallSummary>,

        // Pointer & hit state (Apple Design §1, §2, §10)
        mouse_lx: f64,
        mouse_ly: f64,
        hits: Vec<HitRegion>,
        hover_id: Option<u64>,
        pressed_action: Option<(u64, ClickAction)>,
        pressed_ring_sub: Option<BuiltinSubmenu>,
        drag_win: Option<DragWindow>,
        slider_drag: SliderDragKind,
        slider_grab_offset: f64,
        slider_rects: [(f64, f64, f64, f64); 2],
        cursor_kind: CursorKind,
        tracking_mouse: bool,

        // Fluid spring motion state (Apple Design §3, §4, §7, §9)
        palette: PaletteTransition,
        ring: LiveRingMotion,
        page_mix: AppleSpring,
        step_pill_y: AppleSpring,
        nav_seg_x: AppleSpring,
        layout_seg_x: AppleSpring,
        update_seg_x: AppleSpring,
        primary_press: AppleSpring,
        back_press: AppleSpring,
        toggle_startup: AppleSpring,
        toggle_shortcuts: AppleSpring,
        toggle_daemon: AppleSpring,
        toggle_animations: AppleSpring,
        toggle_loading_3d: AppleSpring,
        toggle_high_contrast: AppleSpring,
        toggle_number_shortcuts: AppleSpring,
        submenu_toggles: [AppleSpring; 5],
        status_spring: AppleSpring,
        keybind_scroll: AppleSpring,
        keybind_max_scroll: f64,
        themes: Vec<BuiltinTheme>,
        theme_palettes: Vec<ThemePalette>,
    }

    thread_local! {
        static WIZ: RefCell<Option<WizardState>> = const { RefCell::new(None) };
    }

    fn with_wiz<R>(f: impl FnOnce(&mut WizardState) -> R) -> Option<R> {
        WIZ.with(|slot| {
            let Ok(mut guard) = slot.try_borrow_mut() else {
                return None;
            };
            let st = guard.as_mut()?;
            if st.closing {
                return None;
            }
            Some(f(st))
        })
    }

    #[derive(Default)]
    struct DispatchEffects {
        set_capture: bool,
        release_capture: bool,
        move_window: Option<(i32, i32)>,
        show_minimize: bool,
        destroy_window: bool,
        browse_dir: bool,
        caret_timer: Option<bool>,
        pulse_timer: Option<bool>,
    }

    fn sync_timers_effects(st: &mut WizardState, eff: &mut DispatchEffects) {
        let (want_caret, want_pulse) = compute_timer_flags(
            st.editing_dir && st.page == PAGE_SETUP && !st.confirm_uninstall_open,
            st.capturing_hotkey.is_some()
                && st.page == PAGE_KEYBINDS
                && !st.reduced_motion
                && !st.confirm_uninstall_open,
            st.focused,
            st.minimized,
            st.closing,
        );
        if want_caret != st.caret_timer_on {
            st.caret_timer_on = want_caret;
            eff.caret_timer = Some(want_caret);
        }
        if want_pulse != st.pulse_timer_on {
            st.pulse_timer_on = want_pulse;
            eff.pulse_timer = Some(want_pulse);
        }
        sync_capture_hook_state(st, false);
    }

    fn sync_capture_hook_state(st: &mut WizardState, force_drop: bool) {
        let can_hold_hook = !force_drop
            && st.page == PAGE_KEYBINDS
            && st.focused
            && !st.minimized
            && !st.closing
            && !st.confirm_uninstall_open;
        if !can_hold_hook {
            st.capture_hook = None;
            return;
        }
        match &st.capturing_hotkey {
            Some(cap) => {
                let allow_lone_win = matches!(
                    cap,
                    KeybindCapture::MainRing | KeybindCapture::Submenu(_)
                );
                if let Some(hook) = &st.capture_hook {
                    hook.set_active(true, allow_lone_win);
                } else {
                    st.capture_hook =
                        CaptureHook::install(st.hwnd, WM_APP_CAPTURE_KEY, allow_lone_win);
                }
            }
            None => {
                if let Some(hook) = &st.capture_hook {
                    if hook.has_suppressed() {
                        hook.set_active(false, false);
                    } else {
                        st.capture_hook = None;
                    }
                }
            }
        }
    }

    fn compute_timer_flags(
        editing_dir: bool,
        capturing_hotkey: bool,
        focused: bool,
        minimized: bool,
        closing: bool,
    ) -> (bool, bool) {
        if closing || minimized || !focused {
            (false, false)
        } else {
            (editing_dir, capturing_hotkey)
        }
    }

    fn should_keep_awake(
        still_anim: bool,
        installing: bool,
        minimized: bool,
        closing: bool,
    ) -> bool {
        !closing && !minimized && (still_anim || installing)
    }

    fn run_dispatch_effects(hwnd: HWND, eff: DispatchEffects) {
        if eff.release_capture {
            unsafe { ReleaseCapture() };
        }
        if eff.set_capture {
            unsafe { SetCapture(hwnd) };
        }
        if let Some((nx, ny)) = eff.move_window {
            unsafe {
                SetWindowPos(
                    hwnd,
                    ptr::null_mut(),
                    nx,
                    ny,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        if let Some(on) = eff.caret_timer {
            unsafe {
                if on {
                    SetTimer(hwnd, TIMER_CARET, CARET_BLINK_MS, None);
                } else {
                    KillTimer(hwnd, TIMER_CARET);
                }
            }
        }
        if let Some(on) = eff.pulse_timer {
            unsafe {
                if on {
                    SetTimer(hwnd, TIMER_PULSE, PULSE_INTERVAL_MS, None);
                } else {
                    KillTimer(hwnd, TIMER_PULSE);
                }
            }
        }
        if eff.show_minimize {
            unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
        }
        if eff.destroy_window {
            unsafe { DestroyWindow(hwnd) };
        }
        if eff.browse_dir
            && let Some(picked) = pick_folder_dialog(hwnd, "Select ReCircles Install Folder")
        {
            with_wiz(|st| {
                st.install_dir_str = picked.to_string_lossy().into_owned();
                st.dir_error = None;
                st.editing_dir = false;
                wake_and_paint(st);
            });
        }
    }

    fn clear_pointer_state(st: &mut WizardState) -> bool {
        let had = st.drag_win.is_some()
            || st.slider_drag != SliderDragKind::None
            || st.pressed_ring_sub.is_some()
            || st.ring.pressed.is_some()
            || st.pressed_action.is_some()
            || st.hover_id.is_some()
            || st.ring.hovered.is_some()
            || st.ring.highlighted_hint.is_some();
        st.drag_win = None;
        st.slider_drag = SliderDragKind::None;
        st.slider_grab_offset = 0.0;
        st.pressed_ring_sub = None;
        st.ring.pressed = None;
        st.hover_id = None;
        st.ring.set_hover(None);
        st.ring.highlighted_hint = None;
        st.tracking_mouse = false;
        st.cursor_kind = CursorKind::Arrow;
        if let Some((_, act)) = st.pressed_action.take() {
            update_press_spring(st, &act, false);
        }
        had
    }

    fn system_reduced_motion() -> bool {
        let mut anim: i32 = 1;
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut anim as *mut i32).cast(),
                0,
            )
        };
        ok != 0 && anim == 0
    }

    pub fn run_wizard() -> i32 {
        perf::init_launch();
        let t_launch = Instant::now();
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        let t_cfg0 = Instant::now();
        let mut opts = InstallOptions::default();
        let mut existing_installed = false;
        let mut custom_menus = Vec::new();
        if let Some(manifest) = load_existing_manifest(&opts.config_dir) {
            existing_installed = true;
            opts.install_dir = PathBuf::from(&manifest.install_dir);
            opts.theme = manifest.theme;
            opts.selected_submenus.clear();
            for id in &manifest.submenus {
                if let Some(s) = BuiltinSubmenu::from_id(id) {
                    opts.selected_submenus.insert(s);
                }
            }
            enforce_mandatory_submenus(&mut opts.selected_submenus);
            let sub_dir = opts.install_dir.join("submenus");
            let detected = recircles_core::modules::InstalledModules::detect_in(sub_dir);
            if let Ok(cfg) = recircles_core::config::ConfigStore::in_dir(opts.config_dir.clone())
                .load_or_create(&detected)
            {
                opts.appearance = cfg.appearance.clone();
                opts.keybinds = cfg.keybinds.clone();
                opts.update_mode = cfg.updates.mode;
                if let Some(m) = cfg.keybinds.main_menu.primary() {
                    opts.main_hotkey = m.to_string();
                }
                for (id, kl) in &cfg.keybinds.menu_keybinds {
                    if let Some(p) = kl.primary() {
                        opts.submenu_hotkeys.insert(id.clone(), p.to_string());
                    }
                }
                for (cm_id, cm) in &cfg.menus {
                    let title = if cm.title.trim().is_empty() {
                        cm_id.clone()
                    } else {
                        cm.title.clone()
                    };
                    custom_menus.push((cm_id.clone(), title));
                }
                // Detect Vim preset if existing navigation uses H/J/K/L
                if cfg.keybinds.previous.0.iter().any(|k| k.eq_ignore_ascii_case("K"))
                    && cfg.keybinds.next.0.iter().any(|k| k.eq_ignore_ascii_case("J"))
                {
                    opts.nav_preset = NavPreset::Vim;
                }
            }
        }
        let dir_str = opts.install_dir.to_string_lossy().into_owned();
        perf::record_phase("config_detect", t_cfg0.elapsed().as_secs_f64() * 1000.0);

        let t_win0 = Instant::now();
        let hinst = unsafe { GetModuleHandleW(ptr::null()) };
        let cls = to_wide_nul("ReCirclesSetupGlassWizard");
        let title = to_wide_nul("ReCircles Setup");
        let (app_icon, app_icon_sm) = recircles_core::app_icon::win::load_or_create_app_icons();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: app_icon,
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: app_icon_sm,
        };
        unsafe { RegisterClassExW(&wc) };

        // Center on the monitor containing the cursor
        let mut pt = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut pt) };
        let hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        unsafe { GetMonitorInfoW(hmon, &mut mi) };
        let work = mi.rcWork;

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_APPWINDOW,
                cls.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU,
                work.left + 80,
                work.top + 60,
                WIN_W as i32,
                WIN_H as i32,
                ptr::null_mut(),
                ptr::null_mut(),
                hinst,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return 1;
        }

        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f64;
        let scale = (dpi / 96.0).clamp(1.0, 4.0);
        let phys_w = (WIN_W * scale).round() as i32;
        let phys_h = (WIN_H * scale).round() as i32;
        let cx = work.left + ((work.right - work.left) - phys_w) / 2;
        let cy = work.top + ((work.bottom - work.top) - phys_h) / 2;
        unsafe {
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                cx,
                cy,
                phys_w,
                phys_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        perf::record_phase("window_create", t_win0.elapsed().as_secs_f64() * 1000.0);

        let t_d2d0 = Instant::now();
        let Some(mut canvas) = D2dCanvas::new(hwnd, phys_w, phys_h, scale) else {
            unsafe { DestroyWindow(hwnd) };
            return 1;
        };
        perf::record_phase("d2d_create", t_d2d0.elapsed().as_secs_f64() * 1000.0);

        // Capture monitor desktop BEFORE showing the wizard so GPU frosted glass
        // stays strictly clipped to the rounded shell and tracks window drags.
        let mon = mi.rcMonitor;
        let mon_w = (mon.right - mon.left).max(1);
        let mon_h = (mon.bottom - mon.top).max(1);
        let init_blur = opts.appearance.blur;
        let blur_t = f64::from(init_blur) / 100.0;
        let blur_sigma = ((0.35 + blur_t.powf(1.35) * 37.65) * scale).clamp(0.25, 64.0);
        let blur_sat = 1.0 + 0.24 * blur_t;
        if canvas.capture_backdrop(mon.left, mon.top, mon_w, mon_h, blur_sigma) {
            let _ = canvas.set_backdrop_blur(blur_sigma, blur_sat);
            canvas.set_backdrop_origin((cx - mon.left) as f64, (cy - mon.top) as f64);
        }

        let reduced_motion = system_reduced_motion() || opts.appearance.reduced_motion;
        let initial_colors = builtin_theme(&opts.theme)
            .map(|b| b.colors)
            .unwrap_or_default();
        let initial_palette = ThemePalette::from_colors(&initial_colors);
        let mut ring = LiveRingMotion::new(&opts.selected_submenus, reduced_motion);
        if !reduced_motion {
            ring.open_alpha.value = 0.06;
        }
        let themes = builtin_themes();
        let theme_palettes = themes
            .iter()
            .map(|bt| ThemePalette::from_colors(&bt.colors))
            .collect();

        let submenu_toggles = std::array::from_fn(|i| {
            let sub = BuiltinSubmenu::ALL[i];
            let on = opts.selected_submenus.contains(&sub) || sub.is_mandatory();
            AppleSpring::new(if on { 1.0 } else { 0.0 }, 1.0, 0.16)
        });

        let pacer = VsyncPacer::start(hwnd);
        let mut st = WizardState {
            hwnd,
            canvas,
            pacer,
            scale,
            reduced_motion,
            last_tick: Instant::now(),
            clock: 0.0,
            minimized: false,
            focused: true,
            closing: false,
            has_capture: false,
            last_animating: true,
            recreating: false,
            caret_visible: true,
            caret_timer_on: false,
            pulse_timer_on: false,
            capture_hook: None,
            last_mouse_phys: (-1, -1),
            backdrop_mon_origin: (mon.left, mon.top),
            last_blur_pct: init_blur,
            page: PAGE_SETUP,
            prev_page: PAGE_SETUP,
            page_dir: 1.0,
            toggle_startup: AppleSpring::new(if opts.start_with_windows { 1.0 } else { 0.0 }, 1.0, 0.16),
            toggle_shortcuts: AppleSpring::new(if opts.create_shortcuts { 1.0 } else { 0.0 }, 1.0, 0.16),
            toggle_daemon: AppleSpring::new(if opts.launch_daemon { 1.0 } else { 0.0 }, 1.0, 0.16),
            toggle_animations: AppleSpring::new(if !opts.appearance.reduced_motion { 1.0 } else { 0.0 }, 1.0, 0.16),
            toggle_loading_3d: AppleSpring::new(if opts.appearance.loading_animation == LoadingStyle::ThreeD { 1.0 } else { 0.0 }, 1.0, 0.16),
            toggle_high_contrast: AppleSpring::new(if opts.appearance.high_contrast { 1.0 } else { 0.0 }, 1.0, 0.16),
            toggle_number_shortcuts: AppleSpring::new(if opts.keybinds.number_shortcuts { 1.0 } else { 0.0 }, 1.0, 0.16),
            nav_seg_x: AppleSpring::new(if opts.nav_preset == NavPreset::Vim { 1.0 } else { 0.0 }, 1.0, 0.18),
            layout_seg_x: AppleSpring::new(if opts.appearance.layout == OverflowLayout::Ring { 1.0 } else { 0.0 }, 1.0, 0.18),
            update_seg_x: AppleSpring::new(update_mode_index(opts.update_mode), 1.0, 0.18),
            opts,
            install_dir_str: dir_str,
            dir_error: None,
            editing_dir: false,
            existing_installed,
            confirm_uninstall_open: false,
            custom_menus,
            capturing_hotkey: None,
            status_msg: None,
            installing: false,
            install_result: Arc::new(Mutex::new(None)),
            summary: None,
            mouse_lx: -1.0,
            mouse_ly: -1.0,
            hits: Vec::with_capacity(64),
            hover_id: None,
            pressed_action: None,
            pressed_ring_sub: None,
            drag_win: None,
            slider_drag: SliderDragKind::None,
            slider_grab_offset: 0.0,
            slider_rects: [(0.0, 0.0, 0.0, 0.0); 2],
            cursor_kind: CursorKind::Arrow,
            tracking_mouse: false,
            palette: PaletteTransition::new(initial_palette),
            ring,
            page_mix: AppleSpring::new(1.0, 1.0, 0.28),
            step_pill_y: AppleSpring::new(0.0, 1.0, 0.22),
            primary_press: AppleSpring::new(0.0, 1.0, 0.14),
            back_press: AppleSpring::new(0.0, 1.0, 0.14),
            submenu_toggles,
            status_spring: AppleSpring::new(0.0, 1.0, 0.18),
            keybind_scroll: AppleSpring::new(0.0, 1.0, 0.18),
            keybind_max_scroll: 0.0,
            themes,
            theme_palettes,
        };

        let t_f0 = Instant::now();
        render_frame(&mut st);
        perf::record_phase("first_frame", t_f0.elapsed().as_secs_f64() * 1000.0);

        WIZ.with(|slot| *slot.borrow_mut() = Some(st));

        unsafe {
            ShowWindow(hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(hwnd);
        }
        perf::record_phase("total_to_visible", t_launch.elapsed().as_secs_f64() * 1000.0);

        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        WIZ.with(|slot| {
            if let Ok(mut g) = slot.try_borrow_mut() {
                drop(g.take());
            }
        });
        perf::flush();
        0
    }

    fn wake_and_paint(st: &mut WizardState) {
        if st.closing || st.minimized || unsafe { IsIconic(st.hwnd) } != 0 {
            st.last_animating = false;
            st.pacer.set_animating(false);
            return;
        }
        st.last_tick = Instant::now();
        st.last_animating = true;
        st.pacer.set_animating(true);
        render_frame(st);
    }

    fn schedule_paint(st: &mut WizardState) {
        if st.closing || st.minimized || unsafe { IsIconic(st.hwnd) } != 0 {
            st.last_animating = false;
            st.pacer.set_animating(false);
            return;
        }
        if !st.last_animating {
            st.last_tick = Instant::now();
            st.last_animating = true;
            st.pacer.set_animating(true);
            render_frame(st);
        } else {
            st.pacer.set_animating(true);
        }
    }

    fn validate_install_dir(st: &mut WizardState) -> bool {
        if st.install_dir_str.trim().is_empty() {
            st.dir_error = Some("Install location cannot be empty.".into());
            return false;
        }
        st.dir_error = None;
        true
    }

    fn navigate_to(st: &mut WizardState, next: usize) {
        let next = next.min(PAGE_INSTALL);
        if next == st.page || st.installing || st.confirm_uninstall_open {
            return;
        }
        if st.page == PAGE_SETUP && next > PAGE_SETUP && !validate_install_dir(st) {
            return;
        }
        st.prev_page = st.page;
        st.page_dir = if next > st.page { 1.0 } else { -1.0 };
        st.page = next;
        st.editing_dir = false;
        st.capturing_hotkey = None;
        if st.reduced_motion {
            st.page_mix.snap(1.0);
        } else {
            st.page_mix.value = 0.0;
            st.page_mix.velocity = 0.0;
            st.page_mix.set_target(1.0);
        }
        st.step_pill_y.set_target(next as f64);
    }

    fn set_status(st: &mut WizardState, msg: impl Into<String>, is_err: bool) {
        st.status_msg = Some((msg.into(), is_err));
        st.status_spring.set_target(1.0);
    }

    fn clear_status(st: &mut WizardState) {
        st.status_msg = None;
        st.status_spring.set_target(0.0);
    }

    fn select_theme_live(st: &mut WizardState, theme_id: &str) {
        st.opts.theme = theme_id.to_string();
        if let Some(idx) = st.themes.iter().position(|bt| bt.id == theme_id) {
            st.palette
                .transition_to(st.theme_palettes[idx], st.reduced_motion);
        } else if let Some(bt) = builtin_theme(theme_id) {
            let next_pal = ThemePalette::from_colors(&bt.colors);
            st.palette.transition_to(next_pal, st.reduced_motion);
        }
    }

    fn toggle_submenu(st: &mut WizardState, sub: BuiltinSubmenu) {
        if sub.is_mandatory() {
            set_status(st, "Settings is required and cannot be unselected.", true);
            return;
        }
        if st.opts.selected_submenus.contains(&sub) {
            st.opts.selected_submenus.remove(&sub);
        } else {
            st.opts.selected_submenus.insert(sub);
        }
        enforce_mandatory_submenus(&mut st.opts.selected_submenus);
        st.ring.sync_selected(&st.opts.selected_submenus);
        for (i, &s) in BuiltinSubmenu::ALL.iter().enumerate() {
            let on = st.opts.selected_submenus.contains(&s) || s.is_mandatory();
            st.submenu_toggles[i].set_target(if on { 1.0 } else { 0.0 });
        }
    }

    fn start_background_install(st: &mut WizardState) {
        if st.installing {
            return;
        }
        st.opts.install_dir = PathBuf::from(st.install_dir_str.trim());
        enforce_mandatory_submenus(&mut st.opts.selected_submenus);
        st.installing = true;
        st.summary = None;
        clear_status(st);
        st.ring.set_installing(true);

        let opts_clone = st.opts.clone();
        let slot = st.install_result.clone();
        let hwnd_raw = st.hwnd as usize;
        std::thread::spawn(move || {
            let started = Instant::now();
            let res = execute_install(&opts_clone, None);
            // Keep the 3D tumbling ring visible for at least 550ms so completion never flashes jarringly
            let min_dur = std::time::Duration::from_millis(550);
            if let Some(rem) = min_dur.checked_sub(started.elapsed()) {
                std::thread::sleep(rem);
            }
            if let Ok(mut guard) = slot.lock() {
                *guard = Some(res);
            }
            unsafe {
                PostMessageW(hwnd_raw as HWND, WM_APP_INSTALL_DONE, 0, 0);
            }
        });
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_ERASEBKGND => 1,
            WM_NCCALCSIZE => {
                if wparam != 0 {
                    0
                } else {
                    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
                }
            }
            WM_SIZE => {
                let kind = wparam;
                let pw = (lparam & 0xFFFF) as i16 as i32;
                let ph = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                let mut eff = DispatchEffects::default();
                with_wiz(|st| {
                    if kind == SIZE_MINIMIZED_VAL {
                        st.minimized = true;
                        st.last_animating = false;
                        clear_pointer_state(st);
                        if st.has_capture {
                            st.has_capture = false;
                            eff.release_capture = true;
                        }
                        st.pacer.set_animating(false);
                        sync_timers_effects(st, &mut eff);
                    } else if kind == SIZE_RESTORED_VAL || kind == SIZE_MAXIMIZED_VAL {
                        let was_min = st.minimized;
                        st.minimized = false;
                        st.last_tick = Instant::now();
                        clear_pointer_state(st);
                        if pw > 0 && ph > 0 {
                            st.canvas.resize(pw, ph, st.scale);
                        }
                        sync_timers_effects(st, &mut eff);
                        if was_min {
                            wake_and_paint(st);
                        }
                    }
                });
                run_dispatch_effects(hwnd, eff);
                0
            }
            WM_ACTIVATE => {
                let state = wparam & 0xFFFF;
                let mut eff = DispatchEffects::default();
                if state == WA_INACTIVE_VAL {
                    with_wiz(|st| {
                        st.focused = false;
                        let changed = clear_pointer_state(st);
                        if st.has_capture {
                            st.has_capture = false;
                            eff.release_capture = true;
                        }
                        sync_timers_effects(st, &mut eff);
                        if changed && !st.minimized && unsafe { IsIconic(hwnd) } == 0 {
                            schedule_paint(st);
                        }
                    });
                } else {
                    with_wiz(|st| {
                        st.focused = true;
                        st.last_tick = Instant::now();
                        sync_timers_effects(st, &mut eff);
                        if (st.editing_dir || st.capturing_hotkey.is_some())
                            && !st.minimized
                            && unsafe { IsIconic(hwnd) } == 0
                        {
                            st.caret_visible = true;
                            render_frame(st);
                        }
                    });
                }
                run_dispatch_effects(hwnd, eff);
                0
            }
            WM_TIMER => {
                let timer_id = wparam;
                with_wiz(|st| {
                    if st.minimized || unsafe { IsIconic(hwnd) } != 0 {
                        return;
                    }
                    if timer_id == TIMER_CARET && st.editing_dir {
                        st.caret_visible = !st.caret_visible;
                        render_frame(st);
                    } else if timer_id == TIMER_PULSE && st.capturing_hotkey.is_some() {
                        let now = Instant::now();
                        let dt = now.duration_since(st.last_tick).as_secs_f64().clamp(0.0, 0.1);
                        st.last_tick = now;
                        st.clock += dt;
                        render_frame(st);
                    }
                });
                0
            }
            WM_PAINT => {
                unsafe { ValidateRect(hwnd, ptr::null()) };
                with_wiz(|st| {
                    if !st.minimized && unsafe { IsIconic(hwnd) } == 0 {
                        render_frame(st);
                    }
                });
                0
            }
            WM_MOUSELEAVE => {
                with_wiz(|st| {
                    st.tracking_mouse = false;
                    st.last_mouse_phys = (-1, -1);
                    if st.hover_id.is_some()
                        || st.ring.hovered.is_some()
                        || st.ring.highlighted_hint.is_some()
                    {
                        st.hover_id = None;
                        st.ring.set_hover(None);
                        st.ring.highlighted_hint = None;
                        schedule_paint(st);
                    }
                });
                0
            }
            WM_APP_TICK => {
                with_wiz(|st| {
                    st.pacer.ack_tick();
                    if st.minimized || unsafe { IsIconic(hwnd) } != 0 {
                        st.last_animating = false;
                        st.pacer.set_animating(false);
                        return;
                    }
                    let now = Instant::now();
                    let dt = now.duration_since(st.last_tick).as_secs_f64().clamp(0.0, 0.05);
                    st.last_tick = now;
                    st.clock += dt;

                    let still_anim = step_animations(st, dt);
                    render_frame(st);
                    let keep_awake =
                        should_keep_awake(still_anim, st.installing, st.minimized, st.closing);
                    st.last_animating = keep_awake;
                    st.pacer.set_animating(keep_awake);
                });
                0
            }
            WM_APP_INSTALL_DONE => {
                with_wiz(|st| {
                    st.installing = false;
                    st.ring.set_installing(false);
                    let res = st.install_result.lock().ok().and_then(|mut g| g.take());
                    if let Some(outcome) = res {
                        match outcome {
                            Ok(sum) => {
                                st.summary = Some(sum);
                                st.existing_installed = true;
                                st.ring.trigger_complete_bloom();
                                set_status(st, "ReCircles installed successfully!", false);
                            }
                            Err(e) => {
                                set_status(st, e, true);
                            }
                        }
                    }
                    wake_and_paint(st);
                });
                0
            }
            WM_DPICHANGED => {
                let new_dpi = (wparam & 0xFFFF) as f64;
                let new_scale = (new_dpi / 96.0).clamp(1.0, 4.0);
                let suggested = lparam as *const RECT;
                let mut pos_req: Option<(i32, i32, i32, i32)> = None;
                with_wiz(|st| {
                    st.scale = new_scale;
                    let phys_w = (WIN_W * new_scale).round() as i32;
                    let phys_h = (WIN_H * new_scale).round() as i32;
                    let (nx, ny) = if !suggested.is_null() {
                        unsafe { ((*suggested).left, (*suggested).top) }
                    } else {
                        let mut wr: RECT = unsafe { std::mem::zeroed() };
                        unsafe { GetWindowRect(hwnd, &mut wr) };
                        (wr.left, wr.top)
                    };
                    pos_req = Some((nx, ny, phys_w, phys_h));
                });
                if let Some((nx, ny, phys_w, phys_h)) = pos_req {
                    unsafe {
                        SetWindowPos(
                            hwnd,
                            ptr::null_mut(),
                            nx,
                            ny,
                            phys_w,
                            phys_h,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                    with_wiz(|st| {
                        st.canvas.resize(phys_w, phys_h, new_scale);
                        wake_and_paint(st);
                    });
                }
                0
            }
            WM_SETCURSOR => {
                let kind = WIZ
                    .with(|slot| {
                        slot.try_borrow()
                            .ok()
                            .and_then(|g| g.as_ref().map(|s| s.cursor_kind))
                    })
                    .unwrap_or(CursorKind::Arrow);
                let idc = match kind {
                    CursorKind::Arrow => IDC_ARROW,
                    CursorKind::Hand => IDC_HAND,
                    CursorKind::IBeam => IDC_IBEAM,
                };
                unsafe {
                    SetCursor(LoadCursorW(ptr::null_mut(), idc));
                }
                1
            }
            WM_MOUSEMOVE => {
                let px_i = (lparam & 0xFFFF) as i16 as i32;
                let py_i = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                let mut eff = DispatchEffects::default();
                with_wiz(|st| {
                    if st.minimized {
                        return;
                    }
                    if let Some(drag) = &st.drag_win {
                        let mut pt = POINT { x: 0, y: 0 };
                        unsafe { GetCursorPos(&mut pt) };
                        let nx = drag.win_start_x + (pt.x - drag.start_screen_x);
                        let ny = drag.win_start_y + (pt.y - drag.start_screen_y);
                        st.canvas.set_backdrop_origin(
                            (nx - st.backdrop_mon_origin.0) as f64,
                            (ny - st.backdrop_mon_origin.1) as f64,
                        );
                        eff.move_window = Some((nx, ny));
                        wake_and_paint(st);
                        return;
                    }

                    if st.slider_drag != SliderDragKind::None {
                        let lx = px_i as f64 / st.scale;
                        let ly = py_i as f64 / st.scale;
                        st.mouse_lx = lx;
                        st.mouse_ly = ly;
                        if update_appearance_slider(st, st.slider_drag, lx) {
                            wake_and_paint(st);
                        }
                        return;
                    }

                    if (px_i, py_i) == st.last_mouse_phys {
                        return;
                    }
                    st.last_mouse_phys = (px_i, py_i);

                    if !st.tracking_mouse {
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        unsafe { TrackMouseEvent(&mut tme) };
                        st.tracking_mouse = true;
                    }

                    let lx = px_i as f64 / st.scale;
                    let ly = py_i as f64 / st.scale;
                    st.mouse_lx = lx;
                    st.mouse_ly = ly;

                    let hit = st.hits.iter().rposition(|h| h.contains(lx, ly, 0.0));
                    let next_hover = hit.map(|i| st.hits[i].id);
                    let next_cursor = hit
                        .map(|i| st.hits[i].cursor)
                        .unwrap_or(CursorKind::Arrow);
                    let hover_hint = hit.and_then(|i| st.hits[i].hover_submenu);

                    let ring_hit = if st.installing || st.confirm_uninstall_open {
                        None
                    } else {
                        st.ring.hit_test_sector(
                            lx - RING_CX,
                            ly - RING_CY,
                            RING_INNER,
                            RING_OUTER,
                        )
                    };

                    let effective_cursor = if ring_hit.is_some()
                        || (st.page == PAGE_APPEARANCE
                            && hit_test_appearance_slider(st, lx, ly).is_some())
                    {
                        CursorKind::Hand
                    } else {
                        next_cursor
                    };

                    let changed = next_hover != st.hover_id
                        || ring_hit != st.ring.hovered
                        || hover_hint != st.ring.highlighted_hint;

                    st.hover_id = next_hover;
                    st.cursor_kind = effective_cursor;
                    st.ring.set_hover(ring_hit);
                    st.ring.highlighted_hint = hover_hint;

                    if let Some((pid, is_primary, is_back)) = st.pressed_action.as_ref().map(|(id, a)| {
                        (
                            *id,
                            matches!(a, ClickAction::Next | ClickAction::RunInstall | ClickAction::Finish),
                            matches!(a, ClickAction::Back),
                        )
                    }) {
                        let inside = st
                            .hits
                            .iter()
                            .find(|h| h.id == pid)
                            .is_some_and(|h| h.contains(lx, ly, 10.0));
                        let v = if inside { 1.0 } else { 0.0 };
                        if is_primary {
                            st.primary_press.set_target(v);
                        } else if is_back {
                            st.back_press.set_target(v);
                        }
                    }

                    if changed {
                        schedule_paint(st);
                    }
                });
                run_dispatch_effects(hwnd, eff);
                0
            }
            WM_LBUTTONDOWN => {
                let px = (lparam & 0xFFFF) as i16 as f64;
                let py = ((lparam >> 16) & 0xFFFF) as i16 as f64;
                let mut eff = DispatchEffects::default();
                with_wiz(|st| {
                    if st.minimized {
                        return;
                    }
                    let lx = px / st.scale;
                    let ly = py / st.scale;
                    st.mouse_lx = lx;
                    st.mouse_ly = ly;

                    if !st.installing
                        && !st.confirm_uninstall_open
                        && st.page == PAGE_APPEARANCE
                        && let Some((kind, grab_off)) = hit_test_appearance_slider(st, lx, ly)
                    {
                        st.editing_dir = false;
                        st.slider_drag = kind;
                        st.slider_grab_offset = grab_off;
                        let _ = update_appearance_slider(st, kind, lx);
                        st.has_capture = true;
                        eff.set_capture = true;
                        sync_timers_effects(st, &mut eff);
                        wake_and_paint(st);
                        return;
                    }

                    if !st.installing
                        && !st.confirm_uninstall_open
                        && let Some(sub) = st.ring.hit_test_sector(
                            lx - RING_CX,
                            ly - RING_CY,
                            RING_INNER,
                            RING_OUTER,
                        )
                    {
                        st.pressed_ring_sub = Some(sub);
                        st.ring.pressed = Some(sub);
                        st.has_capture = true;
                        eff.set_capture = true;
                        wake_and_paint(st);
                        return;
                    }

                    if let Some(idx) = st.hits.iter().rposition(|h| h.contains(lx, ly, 0.0)) {
                        let id = st.hits[idx].id;
                        let act = st.hits[idx].action.clone();
                        if !matches!(act, ClickAction::FocusInstallDir) {
                            st.editing_dir = false;
                        }
                        st.pressed_action = Some((id, act.clone()));
                        update_press_spring(st, &act, true);
                        st.has_capture = true;
                        eff.set_capture = true;
                        sync_timers_effects(st, &mut eff);
                        wake_and_paint(st);
                        return;
                    }

                    st.editing_dir = false;
                    sync_timers_effects(st, &mut eff);

                    let in_shell = (PAD..=PAD + SHELL_W).contains(&lx)
                        && (PAD..=PAD + SHELL_H).contains(&ly);
                    if in_shell {
                        let mut pt = POINT { x: 0, y: 0 };
                        let mut wr: RECT = unsafe { std::mem::zeroed() };
                        unsafe {
                            GetCursorPos(&mut pt);
                            GetWindowRect(hwnd, &mut wr);
                        }
                        st.has_capture = true;
                        eff.set_capture = true;
                        st.drag_win = Some(DragWindow {
                            start_screen_x: pt.x,
                            start_screen_y: pt.y,
                            win_start_x: wr.left,
                            win_start_y: wr.top,
                        });
                    }
                    wake_and_paint(st);
                });
                run_dispatch_effects(hwnd, eff);
                0
            }
            WM_LBUTTONUP => {
                let px = (lparam & 0xFFFF) as i16 as f64;
                let py = ((lparam >> 16) & 0xFFFF) as i16 as f64;
                let mut commit_action = None;
                let mut commit_ring_sub = None;
                let mut eff = DispatchEffects::default();

                with_wiz(|st| {
                    let lx = px / st.scale;
                    let ly = py / st.scale;
                    st.mouse_lx = lx;
                    st.mouse_ly = ly;

                    if st.slider_drag != SliderDragKind::None {
                        st.slider_drag = SliderDragKind::None;
                        st.slider_grab_offset = 0.0;
                        if st.has_capture {
                            st.has_capture = false;
                            eff.release_capture = true;
                        }
                        wake_and_paint(st);
                        return;
                    }

                    if st.drag_win.take().is_some() {
                        if st.has_capture {
                            st.has_capture = false;
                            eff.release_capture = true;
                        }
                        return;
                    }

                    if let Some(sub) = st.pressed_ring_sub.take() {
                        st.ring.pressed = None;
                        if st.has_capture {
                            st.has_capture = false;
                            eff.release_capture = true;
                        }
                        let still_over = st.ring.hit_test_sector(
                            lx - RING_CX,
                            ly - RING_CY,
                            RING_INNER - 6.0,
                            RING_OUTER + 10.0,
                        );
                        if still_over == Some(sub) {
                            commit_ring_sub = Some(sub);
                        } else {
                            wake_and_paint(st);
                        }
                        return;
                    }

                    if let Some((pid, act)) = st.pressed_action.take() {
                        update_press_spring(st, &act, false);
                        if st.has_capture {
                            st.has_capture = false;
                            eff.release_capture = true;
                        }
                        let still_inside = st
                            .hits
                            .iter()
                            .find(|h| h.id == pid)
                            .is_some_and(|h| h.contains(lx, ly, 10.0));
                        if still_inside {
                            commit_action = Some(act);
                        } else {
                            wake_and_paint(st);
                        }
                    }
                });

                run_dispatch_effects(hwnd, eff);

                if let Some(sub) = commit_ring_sub {
                    with_wiz(|st| {
                        if st.page == 1 && st.summary.is_none() {
                            toggle_submenu(st, sub);
                        } else if let Some(slot_ref) =
                            st.ring.slots.iter_mut().find(|s| s.submenu == sub)
                        {
                            slot_ref.tilt.velocity += 2.6;
                        }
                        wake_and_paint(st);
                    });
                }

                if let Some(act) = commit_action {
                    handle_click(hwnd, act);
                }
                0
            }
            WM_CAPTURECHANGED => {
                with_wiz(|st| {
                    st.has_capture = false;
                    let had_slider = st.slider_drag != SliderDragKind::None;
                    st.slider_drag = SliderDragKind::None;
                    st.slider_grab_offset = 0.0;
                    let had_drag_or_press = had_slider
                        || st.drag_win.take().is_some()
                        || st.pressed_ring_sub.take().is_some()
                        || st.ring.pressed.take().is_some()
                        || st.pressed_action.is_some();
                    if let Some((_, act)) = st.pressed_action.take() {
                        update_press_spring(st, &act, false);
                    }
                    if had_drag_or_press && !st.minimized && unsafe { IsIconic(hwnd) } == 0 {
                        wake_and_paint(st);
                    }
                });
                0
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam >> 16) as i16) as f64 / 120.0;
                with_wiz(|st| {
                    if st.page == PAGE_KEYBINDS && st.keybind_max_scroll > 0.0 {
                        let next = (st.keybind_scroll.target - delta * 46.0)
                            .clamp(0.0, st.keybind_max_scroll);
                        st.keybind_scroll.set_target(next);
                        schedule_paint(st);
                    }
                });
                0
            }
            WM_APP_CAPTURE_KEY => {
                let vk = wparam as u32;
                let mut eff = DispatchEffects::default();
                with_wiz(|st| {
                    if vk == 0 {
                        // Drain-complete sentinel after captured key release
                        sync_capture_hook_state(st, false);
                    } else if st.capturing_hotkey.is_some() {
                        let mods = Modifiers::from_bits(lparam as u8);
                        apply_captured_key(st, mods, vk, &mut eff);
                    } else {
                        sync_capture_hook_state(st, false);
                    }
                });
                run_dispatch_effects(hwnd, eff);
                0
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = wparam as u32;
                let down = |k: u16| unsafe { (GetKeyState(k as i32) as u16 & 0x8000) != 0 };
                let mods = Modifiers {
                    ctrl: down(VK_CONTROL),
                    alt: down(VK_MENU),
                    shift: down(VK_SHIFT),
                    win: down(VK_LWIN) || down(VK_RWIN),
                };
                let mut trigger_action = None;
                let mut eff = DispatchEffects::default();
                with_wiz(|st| {
                    if st.confirm_uninstall_open {
                        if vk == VK_ESCAPE {
                            trigger_action = Some(ClickAction::CancelUninstall);
                        } else if vk == VK_RETURN as u32 {
                            trigger_action = Some(ClickAction::ConfirmUninstall);
                        }
                        return;
                    }
                    if st.capturing_hotkey.is_some() {
                        if st.capture_hook.is_some() {
                            return;
                        }
                        apply_captured_key(st, mods, vk, &mut eff);
                    } else if st.editing_dir {
                        if vk == VK_BACK {
                            if mods.ctrl {
                                let trimmed = st.install_dir_str.trim_end_matches(['\\', '/']);
                                if let Some(idx) = trimmed.rfind(['\\', '/']) {
                                    st.install_dir_str.truncate(idx + 1);
                                } else {
                                    st.install_dir_str.clear();
                                }
                            } else {
                                st.install_dir_str.pop();
                            }
                            if !st.install_dir_str.trim().is_empty() {
                                st.dir_error = None;
                            }
                        } else if vk == VK_ESCAPE || vk == VK_RETURN as u32 {
                            st.editing_dir = false;
                            validate_install_dir(st);
                        } else if mods.ctrl && !mods.alt && !mods.win && vk == b'V' as u32 {
                            if let Some(clip) = read_clipboard_unicode_text(hwnd) {
                                let clean = clip.replace(['\r', '\n', '\0'], "");
                                let clean = clean.trim();
                                if !clean.is_empty() {
                                    st.install_dir_str.push_str(clean);
                                    st.dir_error = None;
                                }
                            }
                        } else if let Some(ch) = key_to_char(vk, lparam)
                            && !ch.is_control()
                        {
                            st.install_dir_str.push(ch);
                            st.dir_error = None;
                        }
                        st.caret_visible = true;
                        sync_timers_effects(st, &mut eff);
                        wake_and_paint(st);
                    } else if !st.installing {
                        if vk == VK_RETURN as u32 {
                            trigger_action = Some(if st.summary.is_some() {
                                ClickAction::Finish
                            } else if st.page == PAGE_INSTALL {
                                ClickAction::RunInstall
                            } else {
                                ClickAction::Next
                            });
                        } else if mods.alt && vk == VK_LEFT as u32 && st.page > 0 && st.summary.is_none() {
                            trigger_action = Some(ClickAction::Back);
                        } else if mods.alt && vk == VK_RIGHT as u32 && st.page < PAGE_INSTALL && st.summary.is_none() {
                            trigger_action = Some(ClickAction::Next);
                        }
                    }
                });
                run_dispatch_effects(hwnd, eff);
                if let Some(act) = trigger_action {
                    handle_click(hwnd, act);
                }
                0
            }
            WM_DESTROY => {
                WIZ.with(|slot| {
                    if let Ok(mut guard) = slot.try_borrow_mut()
                        && let Some(st) = guard.as_mut()
                    {
                        st.closing = true;
                        if st.caret_timer_on {
                            st.caret_timer_on = false;
                            unsafe { KillTimer(hwnd, TIMER_CARET) };
                        }
                        if st.pulse_timer_on {
                            st.pulse_timer_on = false;
                            unsafe { KillTimer(hwnd, TIMER_PULSE) };
                        }
                        sync_capture_hook_state(st, true);
                        st.pacer.stop();
                    }
                });
                perf::flush();
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    fn apply_captured_key(
        st: &mut WizardState,
        mods: Modifiers,
        vk: u32,
        eff: &mut DispatchEffects,
    ) {
        if let Some(cap) = st.capturing_hotkey.take() {
            if vk == VK_ESCAPE {
                set_status(st, "Keybind capture cancelled.", false);
            } else if let Some(formatted) = format_vk_parts(mods, vk) {
                match &cap {
                    KeybindCapture::MainRing | KeybindCapture::Submenu(_) => {
                        match parse_global_hotkey(&formatted) {
                            Ok(parsed) => {
                                let conflict = find_hotkey_conflict(st, &cap, &parsed);
                                if let Some(other) = conflict {
                                    set_status(
                                        st,
                                        format!("'{formatted}' is already assigned to {other}"),
                                        true,
                                    );
                                    st.capturing_hotkey = Some(cap);
                                } else {
                                    match cap {
                                        KeybindCapture::MainRing => {
                                            st.opts.keybinds.main_menu =
                                                KeyList::one(formatted.clone());
                                            st.opts.main_hotkey = formatted.clone();
                                        }
                                        KeybindCapture::Submenu(id) => {
                                            st.opts.keybinds.menu_keybinds.insert(
                                                id.clone(),
                                                KeyList::one(formatted.clone()),
                                            );
                                            st.opts
                                                .submenu_hotkeys
                                                .insert(id, formatted.clone());
                                        }
                                        KeybindCapture::Nav(_) => {}
                                    }
                                    set_status(st, format!("Hotkey set to {formatted}"), false);
                                }
                            }
                            Err(e) => {
                                set_status(st, e, true);
                                st.capturing_hotkey = Some(cap);
                            }
                        }
                    }
                    KeybindCapture::Nav(field) => {
                        if vk == CORE_VK_LWIN || vk == CORE_VK_RWIN {
                            st.capturing_hotkey = Some(cap);
                        } else {
                            match parse_key_spec(&formatted) {
                                Ok(_) => {
                                    field.set(
                                        &mut st.opts.keybinds,
                                        KeyList::one(formatted.clone()),
                                    );
                                    set_status(
                                        st,
                                        format!("{} set to {formatted}", field.label()),
                                        false,
                                    );
                                }
                                Err(e) => {
                                    set_status(st, e, true);
                                    st.capturing_hotkey = Some(cap);
                                }
                            }
                        }
                    }
                }
            } else {
                st.capturing_hotkey = Some(cap);
            }
            sync_timers_effects(st, eff);
            wake_and_paint(st);
        }
    }

    fn update_press_spring(st: &mut WizardState, act: &ClickAction, pressed: bool) {
        let v = if pressed { 1.0 } else { 0.0 };
        match act {
            ClickAction::Next | ClickAction::RunInstall | ClickAction::Finish => {
                st.primary_press.set_target(v);
            }
            ClickAction::Back => {
                st.back_press.set_target(v);
            }
            _ => {}
        }
    }

    fn hit_test_appearance_slider(
        st: &WizardState,
        lx: f64,
        ly: f64,
    ) -> Option<(SliderDragKind, f64)> {
        let sliders = [
            (
                SliderDragKind::Translucency,
                st.slider_rects[0],
                st.opts.appearance.translucency,
            ),
            (
                SliderDragKind::Blur,
                st.slider_rects[1],
                st.opts.appearance.blur,
            ),
        ];
        for (kind, (sx, sy, sw, sh), val) in sliders {
            if sw <= 1.0 || sh <= 1.0 {
                continue;
            }
            if lx >= sx - 12.0 && lx <= sx + sw + 12.0 && ly >= sy - 14.0 && ly <= sy + sh + 14.0 {
                let thumb_x = sx + (f64::from(val) / 100.0) * sw;
                let grab_off = if (lx - thumb_x).abs() <= 14.0 {
                    lx - thumb_x
                } else {
                    0.0
                };
                return Some((kind, grab_off));
            }
        }
        None
    }

    fn update_appearance_slider(st: &mut WizardState, kind: SliderDragKind, lx: f64) -> bool {
        let idx = match kind {
            SliderDragKind::Translucency => 0,
            SliderDragKind::Blur => 1,
            SliderDragKind::None => return false,
        };
        let (sx, _, sw, _) = st.slider_rects[idx];
        if sw <= 1.0 {
            return false;
        }
        let t = ((lx - st.slider_grab_offset - sx) / sw).clamp(0.0, 1.0);
        let pct = (t * 100.0).round() as u8;
        match kind {
            SliderDragKind::Translucency => {
                if st.opts.appearance.translucency != pct {
                    st.opts.appearance.translucency = pct;
                    st.opts.appearance.sync_material();
                    return true;
                }
            }
            SliderDragKind::Blur => {
                if st.opts.appearance.blur != pct {
                    st.opts.appearance.blur = pct;
                    return true;
                }
            }
            SliderDragKind::None => {}
        }
        false
    }

    fn find_hotkey_conflict(
        st: &WizardState,
        cap: &KeybindCapture,
        parsed: &recircles_core::win_keys::ParsedKey,
    ) -> Option<String> {
        let eq = |v: &str| {
            parse_global_hotkey(v).ok().is_some_and(|ex| {
                ex.mods.to_register_hotkey_flags() == parsed.mods.to_register_hotkey_flags()
                    && ex.vk == parsed.vk
            })
        };
        match cap {
            KeybindCapture::MainRing => st
                .opts
                .keybinds
                .menu_keybinds
                .iter()
                .find_map(|(k, kl)| kl.0.iter().any(|v| eq(v)).then(|| k.clone())),
            KeybindCapture::Submenu(target_id) => {
                if st.opts.keybinds.main_menu.0.iter().any(|v| eq(v)) {
                    return Some("Main Ring".to_string());
                }
                st.opts
                    .keybinds
                    .menu_keybinds
                    .iter()
                    .find_map(|(k, kl)| {
                        (k != target_id && kl.0.iter().any(|v| eq(v))).then(|| k.clone())
                    })
            }
            KeybindCapture::Nav(_) => None,
        }
    }

    fn step_animations(st: &mut WizardState, dt: f64) -> bool {
        let rm = st.reduced_motion;
        let mut active = false;
        active |= st.palette.step(dt, rm);
        active |= st.ring.step(dt, rm);
        active |= st.page_mix.step(dt, rm);
        active |= st.step_pill_y.step(dt, rm);
        active |= st.nav_seg_x.step(dt, rm);
        active |= st.layout_seg_x.step(dt, rm);
        active |= st.update_seg_x.step(dt, rm);
        active |= st.primary_press.step(dt, rm);
        active |= st.back_press.step(dt, rm);
        active |= st.toggle_startup.step(dt, rm);
        active |= st.toggle_shortcuts.step(dt, rm);
        active |= st.toggle_daemon.step(dt, rm);
        active |= st.toggle_animations.step(dt, rm);
        active |= st.toggle_loading_3d.step(dt, rm);
        active |= st.toggle_high_contrast.step(dt, rm);
        active |= st.toggle_number_shortcuts.step(dt, rm);
        for t in &mut st.submenu_toggles {
            active |= t.step(dt, rm);
        }
        active |= st.status_spring.step(dt, rm);
        active |= st.keybind_scroll.step(dt, rm);
        active
    }

    fn apply_click_state(st: &mut WizardState, act: ClickAction) -> DispatchEffects {
        let mut eff = DispatchEffects::default();
        if !matches!(
            act,
            ClickAction::CaptureMainHotkey
                | ClickAction::CaptureSubmenuHotkey(_)
                | ClickAction::CaptureNavKeybind(_)
        ) {
            clear_status(st);
        }
        match act {
            ClickAction::MinimizeWindow => {
                clear_pointer_state(st);
                if st.has_capture {
                    st.has_capture = false;
                    eff.release_capture = true;
                }
                st.pacer.set_animating(false);
                eff.show_minimize = true;
                sync_timers_effects(st, &mut eff);
                return eff;
            }
            ClickAction::CloseWindow | ClickAction::Finish => {
                if !st.installing {
                    st.closing = true;
                    if st.has_capture {
                        st.has_capture = false;
                        eff.release_capture = true;
                    }
                    st.pacer.stop();
                    eff.destroy_window = true;
                }
                sync_timers_effects(st, &mut eff);
                return eff;
            }
            ClickAction::GoPage(p) => {
                if st.summary.is_none() {
                    navigate_to(st, p);
                }
            }
            ClickAction::Next => {
                if st.page < PAGE_INSTALL {
                    let target = st.page + 1;
                    navigate_to(st, target);
                }
            }
            ClickAction::Back => {
                if st.page > 0 && st.summary.is_none() {
                    let target = st.page - 1;
                    navigate_to(st, target);
                }
            }
            ClickAction::FocusInstallDir => {
                st.editing_dir = true;
                st.caret_visible = true;
            }
            ClickAction::BrowseInstallDir => {
                st.editing_dir = false;
                eff.browse_dir = true;
            }
            ClickAction::ResetInstallDir => {
                st.install_dir_str = crate::engine::default_install_dir()
                    .to_string_lossy()
                    .into_owned();
                st.dir_error = None;
            }
            ClickAction::ToggleStartup => {
                st.opts.start_with_windows = !st.opts.start_with_windows;
                st.toggle_startup
                    .set_target(if st.opts.start_with_windows { 1.0 } else { 0.0 });
            }
            ClickAction::ToggleShortcuts => {
                st.opts.create_shortcuts = !st.opts.create_shortcuts;
                st.toggle_shortcuts
                    .set_target(if st.opts.create_shortcuts { 1.0 } else { 0.0 });
            }
            ClickAction::ToggleLaunchDaemon => {
                st.opts.launch_daemon = !st.opts.launch_daemon;
                st.toggle_daemon
                    .set_target(if st.opts.launch_daemon { 1.0 } else { 0.0 });
            }
            ClickAction::SetUpdateMode(mode) => {
                st.opts.update_mode = mode;
                st.update_seg_x.set_target(update_mode_index(mode));
            }
            ClickAction::UninstallExisting => {
                st.editing_dir = false;
                st.confirm_uninstall_open = true;
            }
            ClickAction::CancelUninstall => {
                st.confirm_uninstall_open = false;
            }
            ClickAction::ConfirmUninstall => {
                st.confirm_uninstall_open = false;
                let idir = PathBuf::from(st.install_dir_str.trim());
                let cdir = st.opts.config_dir.clone();
                match execute_uninstall(&idir, &cdir, false) {
                    Ok(()) => {
                        st.existing_installed = false;
                        set_status(st, "ReCircles uninstalled cleanly.", false);
                    }
                    Err(e) => {
                        set_status(st, e, true);
                    }
                }
            }
            ClickAction::ToggleSubmenu(sub) => {
                toggle_submenu(st, sub);
            }
            ClickAction::SelectTheme(id) => {
                select_theme_live(st, id);
            }
            ClickAction::SetOverflowLayout(layout) => {
                st.opts.appearance.layout = layout;
                st.layout_seg_x
                    .set_target(if layout == OverflowLayout::Ring { 1.0 } else { 0.0 });
            }
            ClickAction::ToggleAnimations => {
                st.opts.appearance.reduced_motion = !st.opts.appearance.reduced_motion;
                st.reduced_motion = system_reduced_motion() || st.opts.appearance.reduced_motion;
                st.toggle_animations.set_target(if !st.opts.appearance.reduced_motion {
                    1.0
                } else {
                    0.0
                });
            }
            ClickAction::ToggleLoading3d => {
                st.opts.appearance.loading_animation =
                    if st.opts.appearance.loading_animation == LoadingStyle::ThreeD {
                        LoadingStyle::TwoD
                    } else {
                        LoadingStyle::ThreeD
                    };
                st.toggle_loading_3d.set_target(
                    if st.opts.appearance.loading_animation == LoadingStyle::ThreeD {
                        1.0
                    } else {
                        0.0
                    },
                );
            }
            ClickAction::ToggleHighContrast => {
                st.opts.appearance.high_contrast = !st.opts.appearance.high_contrast;
                st.toggle_high_contrast
                    .set_target(if st.opts.appearance.high_contrast { 1.0 } else { 0.0 });
            }
            ClickAction::ResetAppearance => {
                let keep_theme = st.opts.appearance.theme.clone();
                st.opts.appearance = AppearanceConfig::default();
                st.opts.appearance.theme = keep_theme;
                st.reduced_motion = system_reduced_motion() || st.opts.appearance.reduced_motion;
                st.layout_seg_x.set_target(
                    if st.opts.appearance.layout == OverflowLayout::Ring {
                        1.0
                    } else {
                        0.0
                    },
                );
                st.toggle_animations.set_target(1.0);
                st.toggle_loading_3d.set_target(1.0);
                st.toggle_high_contrast.set_target(0.0);
                set_status(st, "Restored default appearance.", false);
            }
            ClickAction::ToggleNumberShortcuts => {
                st.opts.keybinds.number_shortcuts = !st.opts.keybinds.number_shortcuts;
                st.toggle_number_shortcuts.set_target(if st.opts.keybinds.number_shortcuts {
                    1.0
                } else {
                    0.0
                });
            }
            ClickAction::SetNavPreset(preset) => {
                st.opts.nav_preset = preset;
                st.nav_seg_x
                    .set_target(if preset == NavPreset::Vim { 1.0 } else { 0.0 });
                match preset {
                    NavPreset::Standard => {
                        let def = KeybindsConfig::default();
                        st.opts.keybinds.previous = def.previous;
                        st.opts.keybinds.next = def.next;
                        st.opts.keybinds.undo_completion = def.undo_completion;
                        st.opts.keybinds.accept_completion = def.accept_completion;
                    }
                    NavPreset::Vim => {
                        st.opts.keybinds.previous = KeyList::many(&["Up", "Shift+Tab", "K"]);
                        st.opts.keybinds.next = KeyList::many(&["Down", "Tab", "J"]);
                        st.opts.keybinds.undo_completion = KeyList::many(&["Left", "H"]);
                        st.opts.keybinds.accept_completion = KeyList::many(&["Right", "L"]);
                    }
                }
            }
            ClickAction::ResetCustomKeybinds => {
                st.capturing_hotkey = None;
                st.opts.keybinds = KeybindsConfig::default();
                st.opts.main_hotkey = "Win+e".into();
                st.opts.submenu_hotkeys.clear();
                st.opts.custom_keybinds = false;
                st.opts.nav_preset = NavPreset::Standard;
                st.nav_seg_x.set_target(0.0);
                st.toggle_number_shortcuts.set_target(1.0);
                set_status(st, "Restored default keybinds.", false);
            }
            ClickAction::CaptureMainHotkey => {
                st.capturing_hotkey = Some(KeybindCapture::MainRing);
                set_status(
                    st,
                    "Press Main Ring shortcut (e.g. Win+E, Win, Ctrl+Space, or Esc to cancel)…",
                    false,
                );
            }
            ClickAction::ClearMainHotkey => {
                st.opts.keybinds.main_menu = KeyList::default();
                st.opts.main_hotkey.clear();
            }
            ClickAction::CaptureSubmenuHotkey(id) => {
                st.capturing_hotkey = Some(KeybindCapture::Submenu(id.clone()));
                let title = BuiltinSubmenu::from_id(&id)
                    .map(|s| s.title().to_string())
                    .unwrap_or(id);
                set_status(
                    st,
                    format!("Press shortcut for {title} (e.g. Win+B, Win, or Esc to cancel)…"),
                    false,
                );
            }
            ClickAction::ClearSubmenuHotkey(id) => {
                st.opts.keybinds.menu_keybinds.remove(&id);
                st.opts.submenu_hotkeys.remove(&id);
            }
            ClickAction::CaptureNavKeybind(field) => {
                st.capturing_hotkey = Some(KeybindCapture::Nav(field));
                set_status(
                    st,
                    format!("Press key for {} (or Esc to cancel)…", field.label()),
                    false,
                );
            }
            ClickAction::ClearNavKeybind(field) => {
                field.set(&mut st.opts.keybinds, KeyList::default());
            }
            ClickAction::RunInstall => {
                start_background_install(st);
            }
        }
        sync_timers_effects(st, &mut eff);
        wake_and_paint(st);
        eff
    }

    fn handle_click(hwnd: HWND, act: ClickAction) {
        let eff = with_wiz(|st| apply_click_state(st, act)).unwrap_or_default();
        run_dispatch_effects(hwnd, eff);
    }

    // ==================== GPU DIRECT2D RENDERING ====================

    fn draw_scene(st: &mut WizardState) {
        let pal = st.palette.current();
        let master_a = st.ring.open_alpha.value.clamp(0.0, 1.0);
        let open_s = st.ring.open_scale.value.clamp(0.84, 1.04);

        // Live GPU backdrop blur sync when Blur slider changes
        if st.canvas.has_backdrop()
            && !st.opts.appearance.high_contrast
            && st.opts.appearance.translucency > 0
            && st.opts.appearance.blur != st.last_blur_pct
        {
            st.last_blur_pct = st.opts.appearance.blur;
            let blur_t = f64::from(st.last_blur_pct) / 100.0;
            let blur_sigma = if st.last_blur_pct == 0 {
                0.0
            } else {
                ((0.35 + blur_t.powf(1.35) * 37.65) * st.scale).clamp(0.25, 64.0)
            };
            let blur_sat = 1.0 + 0.24 * blur_t;
            let _ = st.canvas.set_backdrop_blur(blur_sigma, blur_sat);
        }

        st.hits.clear();
        st.canvas.begin_frame(st.scale);

        if master_a > 0.004 {
            // Subtle floating arrival scale centered on window shell
            let cx = PAD + SHELL_W * 0.5;
            let cy = PAD + SHELL_H * 0.5;
            let need_scale = (open_s - 1.0).abs() > 0.0005;
            if need_scale {
                st.canvas.save();
                st.canvas.translate(cx, cy);
                st.canvas.scale(open_s, open_s);
                st.canvas.translate(-cx, -cy);
            }

            draw_glass_window_shell(st, &pal, master_a);
            draw_left_stage(st, &pal, master_a);
            draw_right_stage(st, &pal, master_a);

            if st.confirm_uninstall_open {
                draw_uninstall_alert_dialog(st, &pal, master_a);
            }

            // Outer border over the complete shell
            st.canvas.stroke_rounded_rect(
                PAD,
                PAD,
                SHELL_W,
                SHELL_H,
                SHELL_R,
                1.0,
                D2dColor::from_rgb(pal.rim_top, 0.36 * master_a),
            );

            if need_scale {
                st.canvas.restore();
            }
        }
    }

    fn render_frame(st: &mut WizardState) {
        let perf_on = perf::is_enabled();
        let t0 = if perf_on { Some(Instant::now()) } else { None };
        let iv_us = t0.map(perf::next_interval_us).unwrap_or(0);

        draw_scene(st);

        let t_draw = t0.map(|_| Instant::now());
        if !st.canvas.end_frame(0)
            && !st.recreating
            && st.canvas.recreate()
        {
            st.recreating = true;
            draw_scene(st);
            let _ = st.canvas.end_frame(0);
            st.recreating = false;
        }
        if let (Some(t0), Some(t_draw)) = (t0, t_draw) {
            let t_end = Instant::now();
            perf::record_frame(
                t0,
                t_draw.duration_since(t0).as_micros() as u64,
                t_end.duration_since(t_draw).as_micros() as u64,
                iv_us,
                st.page,
                st.last_animating,
            );
        }
    }

    fn draw_glass_window_shell(st: &WizardState, pal: &ThemePalette, alpha: f64) {
        // Soft graduated outer shadow strictly outside the rounded shell
        st.canvas.fill_rounded_rect_shadow(
            PAD,
            PAD,
            SHELL_W,
            SHELL_H,
            SHELL_R,
            D2dColor::from_rgb(pal.shadow, 1.0),
            alpha * 0.90,
        );

        // Live frosted glass shell driven by Appearance settings (defaults: 60% translucency, 20% blur)
        let hc = st.opts.appearance.high_contrast;
        let trans_pct = if hc { 0 } else { st.opts.appearance.translucency };
        let blur_pct = if hc { 0 } else { st.opts.appearance.blur };
        let uses_backdrop = st.canvas.has_backdrop() && trans_pct > 0 && blur_pct > 0;
        let fade_layer = uses_backdrop && alpha < 0.995;
        let local_a = if fade_layer {
            st.canvas
                .push_opacity_layer(PAD - 1.0, PAD - 1.0, SHELL_W + 2.0, SHELL_H + 2.0, alpha);
            1.0
        } else {
            alpha
        };

        if uses_backdrop {
            st.canvas
                .fill_rounded_rect_backdrop(PAD, PAD, SHELL_W, SHELL_H, SHELL_R, 0.0, local_a);
        }

        let base = if hc {
            1.0
        } else {
            (1.0 - f64::from(trans_pct) / 100.0).clamp(0.08, 1.0)
        };
        let panel_base = if hc {
            1.0
        } else {
            (base.powf(0.84) + lerp(0.0, 0.08, pal.is_light)).min(1.0)
        };
        let stops = [
            D2dGradStop {
                pos: 0.0,
                color: D2dColor::from_rgb(pal.ring_top, (panel_base * 1.06).min(1.0) * local_a),
            },
            D2dGradStop {
                pos: 0.45,
                color: D2dColor::from_rgb(pal.ring_mid, (panel_base * 0.96).min(1.0) * local_a),
            },
            D2dGradStop {
                pos: 1.0,
                color: D2dColor::from_rgb(pal.ring_bottom, (panel_base * 1.08).min(1.0) * local_a),
            },
        ];
        st.canvas.fill_rounded_rect_linear(
            PAD,
            PAD,
            SHELL_W,
            SHELL_H,
            SHELL_R,
            0.0,
            PAD,
            0.0,
            PAD + SHELL_H,
            &stops,
        );

        // Subtle Apple-style specular glass top sheen catching light along the upper rim
        let sheen_a = 0.08 * (1.0 - base) * local_a;
        st.canvas.fill_top_sheen(
            PAD,
            PAD,
            SHELL_W,
            SHELL_H,
            SHELL_R,
            34.0,
            D2dColor::from_rgb(pal.rim_top, sheen_a),
        );

        // Left stage distinct surface tone for clear sidebar hierarchy
        let left_depth_tint = if pal.is_light > 0.5 {
            lerp_rgb(pal.ring_bottom, pal.capsule_bottom, 0.45)
        } else {
            lerp_rgb(pal.ring_bottom, pal.shadow, 0.34)
        };
        st.canvas.push_clip_rect(PAD, PAD, LEFT_W, SHELL_H);
        st.canvas.fill_rounded_rect_solid(
            PAD,
            PAD,
            LEFT_W + SHELL_R,
            SHELL_H,
            SHELL_R,
            D2dColor::from_rgb(left_depth_tint, lerp(0.28, 0.22, pal.is_light) * local_a),
        );
        st.canvas.pop_clip_rect();

        if fade_layer {
            st.canvas.pop_layer();
        }

        // Crisp 1px vertical divider between left stage and right content
        let div_x = PAD + LEFT_W;
        st.canvas.draw_line(
            div_x,
            PAD + 16.0,
            div_x,
            PAD + SHELL_H - 16.0,
            1.0,
            D2dColor::from_rgb(pal.divider, 0.20 * alpha),
            false,
        );
    }

    fn draw_left_stage(st: &mut WizardState, pal: &ThemePalette, alpha: f64) {
        // Brand Header
        let lx = PAD + 24.0;
        st.canvas.draw_text(
            "ReCircles",
            FONT_BRAND,
            true,
            lx,
            PAD + 20.0,
            148.0,
            24.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        // Version pill
        const VER_TEXT: &str = concat!("v", env!("CARGO_PKG_VERSION"));
        let pill_w = 78.0;
        let vx = PAD + LEFT_W - 24.0 - pill_w;
        let vy = PAD + 22.0;
        st.canvas.fill_rounded_rect_solid(
            vx,
            vy,
            pill_w,
            20.0,
            10.0,
            D2dColor::from_rgb(pal.accent, 0.16 * alpha),
        );
        st.canvas.stroke_rounded_rect(
            vx,
            vy,
            pill_w,
            20.0,
            10.0,
            1.0,
            D2dColor::from_rgb(pal.accent, 0.42 * alpha),
        );
        st.canvas.draw_text(
            VER_TEXT,
            FONT_CAPTION,
            true,
            vx + 6.0,
            vy,
            pill_w - 12.0,
            20.0,
            1,
            3,
            1,
            D2dColor::from_rgb(pal.accent, 0.96 * alpha),
            None,
        );
        st.canvas.draw_text(
            "Windows Setup",
            FONT_CAPTION,
            false,
            lx,
            PAD + 44.0,
            LEFT_W - 48.0,
            16.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.88 * alpha),
            None,
        );

        // Live Interactive ReCircles Radial Ring (or 3D Tumbling Glass Loading Ring while installing!)
        if let Some(pose) = st.ring.loading_pose(st.reduced_motion) {
            draw_loading_ring_3d(st, pal, pose, alpha);
        } else {
            draw_live_radial_ring(st, pal, alpha);
        }

        // Authentic ReCircles Caption Capsule beneath the ring
        let cap_text = compute_ring_caption(st);
        draw_glass_capsule(st, &cap_text, RING_CX, RING_CY + RING_OUTER + 24.0, LEFT_W - 36.0, pal, alpha);

        // Vertical 6-Step Navigation Rail with spring-gliding active selection pill
        let rail_x = PAD + 18.0;
        let rail_w = LEFT_W - 36.0;
        let rail_top = PAD + 344.0;
        let row_h = 38.0;
        let row_gap = 4.0;
        let stride = row_h + row_gap;

        // Gliding active selection pill (solid fill + 1px accent border)
        let pill_y = rail_top + st.step_pill_y.value * stride;
        st.canvas.fill_rounded_rect_solid(
            rail_x,
            pill_y,
            rail_w,
            row_h,
            12.0,
            D2dColor::from_rgb(pal.accent, lerp(0.16, 0.14, pal.is_light) * alpha),
        );
        st.canvas.stroke_rounded_rect(
            rail_x,
            pill_y,
            rail_w,
            row_h,
            12.0,
            1.0,
            D2dColor::from_rgb(pal.accent, 0.56 * alpha),
        );

        let steps = [
            ("Setup", UiGlyph::Sliders),
            ("Submenus", UiGlyph::Submenu(BuiltinSubmenu::Apps)),
            ("Theme", UiGlyph::Palette),
            ("Appearance", UiGlyph::Sparkles),
            ("Keybinds", UiGlyph::Keyboard),
            ("Install", UiGlyph::Download),
        ];

        for (i, (label, glyph)) in steps.iter().enumerate() {
            let y = rail_top + i as f64 * stride;
            let hit_id = 100 + i as u64;
            let active = st.page == i;
            let hovered = st.hover_id == Some(hit_id);
            let done = i < st.page || st.summary.is_some();

            if hovered && !active {
                st.canvas.fill_rounded_rect_solid(
                    rail_x,
                    y,
                    rail_w,
                    row_h,
                    12.0,
                    D2dColor::from_rgb(pal.rim_top, lerp(0.06, 0.12, pal.is_light) * alpha),
                );
            }

            // Icon plate
            let ix = rail_x + 12.0;
            let iy = y + (row_h - 24.0) * 0.5;
            let icon_tint = if active {
                pal.accent
            } else if done {
                lerp_rgb(pal.text_primary, pal.accent, 0.45)
            } else {
                pal.text_muted
            };
            st.canvas.fill_rounded_rect_solid(
                ix,
                iy,
                24.0,
                24.0,
                8.0,
                D2dColor::from_rgb(icon_tint, if active { 0.22 } else { 0.10 } * alpha),
            );
            draw_glyph(
                &st.canvas,
                *glyph,
                ix + 12.0,
                iy + 12.0,
                14.5,
                D2dColor::from_rgb(icon_tint, if active { 1.0 } else { 0.82 } * alpha),
                1.65,
            );

            // Step label
            let text_col = if active {
                pal.text_primary
            } else if hovered {
                lerp_rgb(pal.text_muted, pal.text_primary, 0.72)
            } else {
                pal.text_muted
            };
            st.canvas.draw_text(
                label,
                FONT_BODY,
                active,
                ix + 34.0,
                y,
                rail_w - 78.0,
                row_h,
                0,
                3,
                1,
                D2dColor::from_rgb(text_col, alpha),
                None,
            );

            // Subtle right status dot / checkmark
            let dot_cx = rail_x + rail_w - 18.0;
            let dot_cy = y + row_h * 0.5;
            if done && !active {
                st.canvas.fill_circle(
                    dot_cx,
                    dot_cy,
                    7.5,
                    D2dColor::from_rgb(pal.accent, 0.20 * alpha),
                );
                draw_glyph(
                    &st.canvas,
                    UiGlyph::Check,
                    dot_cx,
                    dot_cy,
                    10.5,
                    D2dColor::from_rgb(pal.accent, 0.95 * alpha),
                    2.0,
                );
            } else if active {
                st.canvas.fill_circle(
                    dot_cx,
                    dot_cy,
                    3.5,
                    D2dColor::from_rgb(pal.accent, 0.95 * alpha),
                );
            }

            if !st.installing && st.summary.is_none() {
                st.hits.push(HitRegion {
                    id: hit_id,
                    x: rail_x,
                    y,
                    w: rail_w,
                    h: row_h,
                    action: ClickAction::GoPage(i),
                    cursor: CursorKind::Hand,
                    hover_submenu: None,
                });
            }
        }
    }

    fn compute_ring_caption(st: &WizardState) -> Cow<'static, str> {
        if st.installing {
            return Cow::Borrowed("Installing ReCircles…");
        }
        if st.summary.is_some() {
            let hk = st
                .opts
                .keybinds
                .main_menu
                .primary()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Win+e");
            return Cow::Owned(format!("Ready · {hk}"));
        }
        if let Some(sub) = st.ring.hovered.or(st.ring.highlighted_hint) {
            if st.page == PAGE_KEYBINDS
                && let Some(hk) = st
                    .opts
                    .keybinds
                    .menu_keybinds
                    .get(sub.id())
                    .and_then(|kl| kl.primary())
                && !hk.trim().is_empty()
            {
                return Cow::Owned(format!("{} · {hk}", sub.title()));
            }
            if st.page == PAGE_SUBMENUS && !sub.is_mandatory() {
                return Cow::Owned(format!("{} · Click to toggle", sub.title()));
            }
            return Cow::Borrowed(sub.title());
        }
        if st.page == PAGE_THEME
            && let Some(bt) = st.themes.iter().find(|t| t.id.eq_ignore_ascii_case(&st.opts.theme))
        {
            return Cow::Borrowed(bt.label);
        }
        if st.page == PAGE_APPEARANCE {
            if st.opts.appearance.high_contrast {
                return Cow::Borrowed("High Contrast");
            }
            return Cow::Owned(format!(
                "{}% glass · {}% blur",
                st.opts.appearance.translucency, st.opts.appearance.blur
            ));
        }
        match st.opts.selected_submenus.len() {
            1 => Cow::Borrowed("1 module active"),
            2 => Cow::Borrowed("2 modules active"),
            3 => Cow::Borrowed("3 modules active"),
            4 => Cow::Borrowed("4 modules active"),
            5 => Cow::Borrowed("5 modules active"),
            n => Cow::Owned(format!("{n} modules active")),
        }
    }

    fn draw_live_radial_ring(st: &mut WizardState, pal: &ThemePalette, alpha: f64) {
        st.canvas.save();
        st.canvas.translate(RING_CX, RING_CY);

        // Soft graduated outer shadow strictly outside outer rim
        st.canvas.fill_annulus_shadow(
            1.5,
            RING_OUTER,
            8.5,
            D2dColor::from_rgb(pal.shadow, 0.24 * alpha),
        );

        // 3-stop vertical ReCircles annulus glass gradient driven live by Appearance settings
        let ring_base = if st.opts.appearance.high_contrast {
            1.0
        } else {
            (1.0 - f64::from(st.opts.appearance.translucency) / 100.0).clamp(0.14, 1.0)
        };
        let stops = [
            D2dGradStop {
                pos: 0.0,
                color: D2dColor::from_rgb(pal.ring_top, (ring_base * 1.08).min(1.0) * alpha),
            },
            D2dGradStop {
                pos: 0.42,
                color: D2dColor::from_rgb(pal.ring_mid, (ring_base * 0.96).min(1.0) * alpha),
            },
            D2dGradStop {
                pos: 1.0,
                color: D2dColor::from_rgb(pal.ring_bottom, (ring_base * 1.12).min(1.0) * alpha),
            },
        ];
        st.canvas.fill_annulus_linear(
            RING_INNER,
            RING_OUTER,
            0.0,
            -RING_OUTER,
            0.0,
            RING_OUTER,
            &stops,
        );

        let mut spans = [(BuiltinSubmenu::Files, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0); 5];
        let count = st.ring.sector_spans_into(&mut spans);
        let plate_sz = if count <= 4 { 28.0 } else { 24.5 };
        let icon_sz = if count <= 4 { 16.5 } else { 14.5 };

        for &(sub, start, end, weight, focus, press, tilt) in &spans[..count] {
            let tint = pal.accent_for_submenu(sub);
            let span = end - start;

            // Focused / pressed sector accent fill + glowing outer rim arc (exact ReCircles!)
            if focus > 0.005 || press > 0.005 {
                let fill_a = (0.14 * focus + 0.14 * press).min(0.28) * weight * alpha;
                st.canvas
                    .fill_sector(RING_INNER, RING_OUTER, start, end, D2dColor::from_rgb(tint, fill_a));

                let rim_a = (0.88 * focus + 0.20 * press).min(0.98) * weight * alpha;
                let inset = 0.03_f64.min(span * 0.25);
                st.canvas.stroke_arc(
                    0.0,
                    0.0,
                    RING_OUTER - 2.3,
                    start + inset,
                    end - inset,
                    2.4,
                    D2dColor::from_rgb(tint, rim_a),
                    true,
                );
            }

            // Crisp 1px radial sector dividers
            if span < TAU - 0.001 {
                let (sa, ca) = start.sin_cos();
                st.canvas.draw_line(
                    (RING_INNER + 1.0) * ca,
                    (RING_INNER + 1.0) * sa,
                    (RING_OUTER - 1.0) * ca,
                    (RING_OUTER - 1.0) * sa,
                    1.0,
                    D2dColor::from_rgb(pal.divider, 0.22 * weight * alpha),
                    false,
                );
            }

            // Icon plate inside sector
            let mid = (start + end) * 0.5;
            let ix = RING_ITEM_R * mid.cos();
            let iy = RING_ITEM_R * mid.sin();
            let zoom = (1.0 + 0.08 * focus - 0.07 * press) * (0.65 + 0.35 * weight);
            let item_a = weight * alpha;

            st.canvas.save();
            st.canvas.translate(ix, iy);
            st.canvas.scale(zoom, zoom);
            if tilt.abs() > 0.002 {
                st.canvas.rotate(tilt * 0.14);
            }
            draw_icon_plate(&st.canvas, UiGlyph::Submenu(sub), 0.0, 0.0, plate_sz, icon_sz, tint, item_a);
            st.canvas.restore();
        }

        // Authentic inner + outer luminous glass rims
        st.canvas.stroke_annulus_rim(
            RING_INNER,
            RING_OUTER,
            1.1,
            0.0,
            -RING_OUTER,
            0.0,
            RING_OUTER,
            D2dColor::from_rgb(pal.rim_top, 0.74 * alpha),
            D2dColor::from_rgb(pal.rim_bottom, 0.30 * alpha),
        );

        // Subtle celebratory completion rim pulse
        if st.ring.complete_bloom.value > 0.01 {
            let b = st.ring.complete_bloom.value;
            st.canvas.stroke_circle(
                0.0,
                0.0,
                RING_OUTER + (1.0 - b) * 14.0,
                2.2 * b,
                D2dColor::from_rgb(pal.accent, 0.65 * b * alpha),
            );
        }

        st.canvas.restore();
    }

    // Exact ReCircles 3D Tumbling Glass Loading Ring (adapted from recircles-ui/src/renderer.rs)
    fn draw_loading_ring_3d(
        st: &WizardState,
        pal: &ThemePalette,
        pose: LoadingPose3D,
        alpha: f64,
    ) {
        #[derive(Clone, Copy)]
        struct Vec3(f64, f64, f64);
        impl Vec3 {
            fn dot(self, o: Self) -> f64 {
                self.0 * o.0 + self.1 * o.1 + self.2 * o.2
            }
            fn normalize(self) -> Self {
                let l = self.dot(self).sqrt();
                if l > 0.0 {
                    Self(self.0 / l, self.1 / l, self.2 / l)
                } else {
                    Self(0.0, 0.0, 1.0)
                }
            }
        }

        let r = pose.rotation;
        let rot = |v: Vec3| {
            Vec3(
                r[0][0] * v.0 + r[0][1] * v.1 + r[0][2] * v.2,
                r[1][0] * v.0 + r[1][1] * v.1 + r[1][2] * v.2,
                r[2][0] * v.0 + r[2][1] * v.1 + r[2][2] * v.2,
            )
        };
        let proj = |p: Vec3| {
            let w = rot(p);
            let persp = PERSPECTIVE_D / (PERSPECTIVE_D - w.2);
            (w.0 * persp, w.1 * persp, w.2)
        };
        let cam = Vec3(
            r[2][0] * PERSPECTIVE_D,
            r[2][1] * PERSPECTIVE_D,
            r[2][2] * PERSPECTIVE_D,
        );

        let half_h = RING_HALF_THICKNESS * pose.depth;
        let r_scale = 1.0 - 0.03 * pose.depth;
        let outer_r = RING_OUTER * r_scale;
        let inner_r = RING_INNER * r_scale;

        let mut outer_f = [(0.0, 0.0, 0.0); RING_SEGMENTS];
        let mut outer_b = [(0.0, 0.0, 0.0); RING_SEGMENTS];
        let mut inner_f = [(0.0, 0.0, 0.0); RING_SEGMENTS];
        let mut inner_b = [(0.0, 0.0, 0.0); RING_SEGMENTS];
        for (i, &(ca, sa)) in RING_COS_SIN.iter().enumerate() {
            outer_f[i] = proj(Vec3(outer_r * ca, outer_r * sa, half_h));
            outer_b[i] = proj(Vec3(outer_r * ca, outer_r * sa, -half_h));
            inner_f[i] = proj(Vec3(inner_r * ca, inner_r * sa, half_h));
            inner_b[i] = proj(Vec3(inner_r * ca, inner_r * sa, -half_h));
        }

        let light = Vec3(-0.42, -0.72, 0.80).normalize();
        let half_vec = Vec3(light.0, light.1, light.2 + 1.0).normalize();

        let shade_outer = |ca: f64, sa: f64| -> Rgb {
            let n = rot(Vec3(ca, sa, 0.0));
            let diff = n.dot(light).max(0.0);
            let spec = n.dot(half_vec).max(0.0).powf(12.0);
            let base = lerp_rgb(pal.ring_bottom, pal.ring_top, 0.20 + 0.80 * diff);
            let lit = lerp_rgb(base, pal.rim_top, 0.28 * diff + 0.48 * spec);
            lerp_rgb(lit, pal.accent, (0.24 * diff + 0.46 * spec) * pose.depth)
        };
        let shade_inner = |ca: f64, sa: f64| -> Rgb {
            let n = rot(Vec3(-ca, -sa, 0.0));
            let diff = n.dot(light).max(0.0);
            let spec = n.dot(half_vec).max(0.0).powf(10.0);
            let base = lerp_rgb(pal.ring_mid, pal.ring_top, 0.12 + 0.72 * diff);
            let lit = lerp_rgb(base, pal.rim_bottom, 0.20 * diff + 0.34 * spec);
            lerp_rgb(lit, pal.accent, (0.18 * diff + 0.34 * spec) * pose.depth)
        };

        st.canvas.save();
        st.canvas.translate(RING_CX, RING_CY);

        if half_h > 0.05 {
            #[derive(Clone, Copy)]
            struct WallQuad {
                z: f64,
                pts: [(f64, f64); 4],
                rgb: Rgb,
            }
            let mut quads = [WallQuad {
                z: 0.0,
                pts: [(0.0, 0.0); 4],
                rgb: (0.0, 0.0, 0.0),
            }; RING_SEGMENTS * 2];
            let mut quad_len = 0usize;
            for (i, &(ca, sa)) in RING_MID_COS_SIN.iter().enumerate() {
                let j = (i + 1) % RING_SEGMENTS;
                if cam.0 * ca + cam.1 * sa - outer_r > -4.0 {
                    let (p0, p1, p2, p3) = (outer_f[i], outer_f[j], outer_b[j], outer_b[i]);
                    quads[quad_len] = WallQuad {
                        z: 0.25 * (p0.2 + p1.2 + p2.2 + p3.2),
                        pts: [(p0.0, p0.1), (p1.0, p1.1), (p2.0, p2.1), (p3.0, p3.1)],
                        rgb: shade_outer(ca, sa),
                    };
                    quad_len += 1;
                }
                if -(cam.0 * ca + cam.1 * sa) + inner_r > -4.0 {
                    let (p0, p1, p2, p3) = (inner_f[i], inner_f[j], inner_b[j], inner_b[i]);
                    quads[quad_len] = WallQuad {
                        z: 0.25 * (p0.2 + p1.2 + p2.2 + p3.2),
                        pts: [(p0.0, p0.1), (p1.0, p1.1), (p2.0, p2.1), (p3.0, p3.1)],
                        rgb: shade_inner(ca, sa),
                    };
                    quad_len += 1;
                }
            }
            let active_quads = &mut quads[..quad_len];
            active_quads.sort_by(|a, b| a.z.total_cmp(&b.z));
            if !active_quads.is_empty() {
                st.canvas.push_opacity_layer(
                    -RING_OUTER - 18.0,
                    -RING_OUTER - 18.0,
                    RING_OUTER * 2.0 + 36.0,
                    RING_OUTER * 2.0 + 36.0,
                    0.88 * alpha,
                );
                for q in active_quads {
                    st.canvas.fill_polygon(&q.pts, D2dColor::from_rgb(q.rgb, 1.0));
                }
                st.canvas.pop_layer();
            }
        }

        let front_pos = cam.2 >= 0.0;
        let src_outer = if front_pos { &outer_f } else { &outer_b };
        let src_inner = if front_pos { &inner_f } else { &inner_b };
        let mut face_outer = [(0.0, 0.0); RING_SEGMENTS];
        let mut face_inner = [(0.0, 0.0); RING_SEGMENTS];
        for i in 0..RING_SEGMENTS {
            face_outer[i] = (src_outer[i].0, src_outer[i].1);
            face_inner[i] = (src_inner[i].0, src_inner[i].1);
        }

        let (mut gy0, mut gy1) = (f64::INFINITY, f64::NEG_INFINITY);
        for &(_, y) in &face_outer {
            gy0 = gy0.min(y);
            gy1 = gy1.max(y);
        }
        let stops = [
            D2dGradStop {
                pos: 0.0,
                color: D2dColor::from_rgb(pal.ring_top, 0.86 * alpha),
            },
            D2dGradStop {
                pos: 0.5,
                color: D2dColor::from_rgb(pal.ring_mid, 0.78 * alpha),
            },
            D2dGradStop {
                pos: 1.0,
                color: D2dColor::from_rgb(pal.ring_bottom, 0.88 * alpha),
            },
        ];
        st.canvas.fill_polygon_ring_linear(
            &face_outer,
            &face_inner,
            0.0,
            gy0,
            0.0,
            gy1,
            &stops,
        );
        st.canvas.stroke_polygon_ring_linear(
            &face_outer,
            &face_inner,
            1.1,
            0.0,
            gy0,
            0.0,
            gy1,
            D2dColor::from_rgb(pal.rim_top, 0.75 * alpha),
            D2dColor::from_rgb(pal.rim_bottom, 0.30 * alpha),
        );
        st.canvas.stroke_polygon(
            &face_outer,
            1.6,
            D2dColor::from_rgb(pal.accent, 0.58 * pose.depth * alpha),
        );

        st.canvas.restore();
    }

    fn draw_glass_capsule(
        st: &mut WizardState,
        text: &str,
        cx: f64,
        cy: f64,
        max_w: f64,
        pal: &ThemePalette,
        alpha: f64,
    ) {
        let (tw, _) = st.canvas.measure_text(text, FONT_CAPTION, true, max_w - 24.0, 1);
        let h = 26.0;
        let w = (tw + 26.0).clamp(72.0, max_w);
        let x = cx - w * 0.5;
        let y = cy - h * 0.5;
        let r = h * 0.5;

        st.canvas.fill_rounded_rect_solid(
            x,
            y,
            w,
            h,
            r,
            D2dColor::from_rgb(pal.capsule_bottom, 0.88 * alpha),
        );
        st.canvas.stroke_rounded_rect(
            x,
            y,
            w,
            h,
            r,
            1.0,
            D2dColor::from_rgb(pal.capsule_border, 0.36 * alpha),
        );
        st.canvas.draw_text(
            text,
            FONT_CAPTION,
            true,
            x + 10.0,
            y,
            w - 20.0,
            h,
            1,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
    }

    // ==================== RIGHT CONTENT STAGE ====================

    fn draw_right_stage(st: &mut WizardState, pal: &ThemePalette, alpha: f64) {
        // Top header strip (window controls + step badge)
        let hx = RIGHT_X + 28.0;
        let hy = PAD + 16.0;

        let badge_txt: &str = if st.summary.is_some() {
            "Setup Complete"
        } else if st.existing_installed {
            match st.page {
                PAGE_SETUP => "Step 1 of 6  ·  Modifying Existing Install",
                PAGE_SUBMENUS => "Step 2 of 6  ·  Modifying Existing Install",
                PAGE_THEME => "Step 3 of 6  ·  Modifying Existing Install",
                PAGE_APPEARANCE => "Step 4 of 6  ·  Modifying Existing Install",
                PAGE_KEYBINDS => "Step 5 of 6  ·  Modifying Existing Install",
                _ => "Step 6 of 6  ·  Modifying Existing Install",
            }
        } else {
            match st.page {
                PAGE_SETUP => "Step 1 of 6",
                PAGE_SUBMENUS => "Step 2 of 6",
                PAGE_THEME => "Step 3 of 6",
                PAGE_APPEARANCE => "Step 4 of 6",
                PAGE_KEYBINDS => "Step 5 of 6",
                _ => "Step 6 of 6",
            }
        };
        st.canvas.draw_text(
            badge_txt,
            FONT_CAPTION,
            true,
            hx,
            hy + 4.0,
            360.0,
            20.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.85 * alpha),
            None,
        );

        // Circular glass Minimize & Close buttons top-right
        let close_x = PAD + SHELL_W - 44.0;
        let min_x = close_x - 36.0;
        let ctl_y = PAD + 14.0;
        draw_window_control(
            st,
            901,
            min_x,
            ctl_y,
            UiGlyph::Minimize,
            ClickAction::MinimizeWindow,
            false,
            pal,
            alpha,
        );
        draw_window_control(
            st,
            902,
            close_x,
            ctl_y,
            UiGlyph::Close,
            ClickAction::CloseWindow,
            true,
            pal,
            alpha,
        );

        // Main Page Viewport (with smooth directional slide + opacity cross-fade)
        let vp_x = RIGHT_X + 28.0;
        let vp_y = PAD + 48.0;
        let vp_w = RIGHT_W - 56.0;
        let vp_h = SHELL_H - 126.0;

        let mix = st.page_mix.value.clamp(0.0, 1.0);
        st.canvas.push_clip_rect(vp_x - 6.0, vp_y - 4.0, vp_w + 12.0, vp_h + 8.0);

        if mix < 0.995 && !st.reduced_motion {
            // Outgoing page fading + sliding smoothly away
            let out_a = (1.0 - mix).powf(1.4) * alpha;
            if out_a > 0.01 {
                let out_dx = -st.page_dir * mix * 30.0;
                st.canvas.save();
                st.canvas.translate(out_dx, 0.0);
                st.canvas
                    .push_opacity_layer(vp_x - 6.0, vp_y - 4.0, vp_w + 12.0, vp_h + 8.0, out_a);
                let prev = st.prev_page;
                draw_page(st, prev, vp_x, vp_y, vp_w, vp_h, pal, 1.0, false);
                st.canvas.pop_layer();
                st.canvas.restore();
            }

            // Incoming page sliding smoothly into place
            let in_dx = st.page_dir * (1.0 - mix) * 34.0;
            let in_a = mix.clamp(0.0, 1.0) * alpha;
            st.canvas.save();
            st.canvas.translate(in_dx, 0.0);
            st.canvas
                .push_opacity_layer(vp_x - 6.0, vp_y - 4.0, vp_w + 12.0, vp_h + 8.0, in_a);
            let cur = st.page;
            draw_page(st, cur, vp_x, vp_y, vp_w, vp_h, pal, 1.0, true);
            st.canvas.pop_layer();
            st.canvas.restore();
        } else {
            let cur = st.page;
            draw_page(st, cur, vp_x, vp_y, vp_w, vp_h, pal, alpha, true);
        }

        st.canvas.pop_clip_rect();

        // Bottom Action Dock
        draw_bottom_dock(st, pal, alpha);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_window_control(
        st: &mut WizardState,
        id: u64,
        x: f64,
        y: f64,
        glyph: UiGlyph,
        action: ClickAction,
        is_close: bool,
        pal: &ThemePalette,
        alpha: f64,
    ) {
        let sz = 28.0;
        let hovered = st.hover_id == Some(id);
        let pressed = st
            .pressed_action
            .as_ref()
            .is_some_and(|(pid, _)| *pid == id);
        let scale = if pressed { 0.90 } else { 1.0 };

        let bg_rgb = if is_close && (hovered || pressed) {
            (0.92, 0.30, 0.34)
        } else {
            pal.rim_top
        };
        let bg_a = if is_close && (hovered || pressed) {
            if pressed { 0.85 } else { 0.70 }
        } else if pressed {
            0.22
        } else if hovered {
            0.14
        } else {
            0.06
        };
        let fg_rgb = if is_close && (hovered || pressed) {
            (1.0, 1.0, 1.0)
        } else if hovered {
            pal.text_primary
        } else {
            pal.text_muted
        };

        st.canvas.save();
        st.canvas.translate(x + sz * 0.5, y + sz * 0.5);
        st.canvas.scale(scale, scale);
        st.canvas
            .fill_circle(0.0, 0.0, sz * 0.5, D2dColor::from_rgb(bg_rgb, bg_a * alpha));
        st.canvas.stroke_circle(
            0.0,
            0.0,
            sz * 0.5,
            0.8,
            D2dColor::from_rgb(pal.rim_top, if hovered { 0.28 } else { 0.14 } * alpha),
        );
        draw_glyph(
            &st.canvas,
            glyph,
            0.0,
            0.0,
            12.5,
            D2dColor::from_rgb(fg_rgb, alpha),
            1.8,
        );
        st.canvas.restore();

        st.hits.push(HitRegion {
            id,
            x,
            y,
            w: sz,
            h: sz,
            action,
            cursor: CursorKind::Hand,
            hover_submenu: None,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_page(
        st: &mut WizardState,
        page: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        match page {
            PAGE_SETUP => draw_page_welcome(st, x, y, w, pal, alpha, interactive),
            PAGE_SUBMENUS => draw_page_submenus(st, x, y, w, pal, alpha, interactive),
            PAGE_THEME => draw_page_themes(st, x, y, w, pal, alpha, interactive),
            PAGE_APPEARANCE => draw_page_appearance(st, x, y, w, pal, alpha, interactive),
            PAGE_KEYBINDS => draw_page_keybinds(st, x, y, w, h, pal, alpha, interactive),
            _ => draw_page_install(st, x, y, w, pal, alpha, interactive),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_page_header(
        st: &mut WizardState,
        x: f64,
        y: f64,
        w: f64,
        title: &str,
        subtitle: &str,
        pal: &ThemePalette,
        alpha: f64,
    ) {
        st.canvas.draw_text(
            title,
            FONT_TITLE,
            true,
            x,
            y,
            w,
            28.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        st.canvas.draw_text(
            subtitle,
            FONT_SECONDARY,
            false,
            x,
            y + 30.0,
            w,
            20.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.92 * alpha),
            None,
        );
    }

    // ---- PAGE 0: WELCOME & SETUP ----
    fn draw_page_welcome(
        st: &mut WizardState,
        x: f64,
        y: f64,
        w: f64,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        let title = if st.existing_installed {
            "Modify ReCircles"
        } else {
            "Welcome to ReCircles"
        };
        let subtitle = if st.existing_installed {
            "Adjust installed submenus, glass theme, or global hotkeys cleanly."
        } else {
            "Fluid radial launcher, window switcher, system controls & native clipboard history."
        };
        draw_page_header(st, x, y, w, title, subtitle, pal, alpha);

        // Install Folder section
        let fy = y + 68.0;
        st.canvas.draw_text(
            "Install Location",
            FONT_SECONDARY,
            true,
            x + 2.0,
            fy,
            220.0,
            18.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, 0.92 * alpha),
            None,
        );
        st.canvas.draw_text(
            "Click to type or Ctrl+V",
            FONT_CAPTION,
            false,
            x + w - 160.0,
            fy + 1.0,
            160.0,
            16.0,
            2,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.72 * alpha),
            None,
        );

        let input_y = fy + 22.0;
        let input_h = 40.0;
        let browse_w = 96.0;
        let reset_w = 78.0;
        let gap = 8.0;
        let dir_w = w - browse_w - reset_w - gap * 2.0;

        // Input well
        let focused = st.editing_dir;
        let has_dir_err = st.dir_error.is_some();
        draw_glass_well(
            &st.canvas,
            x,
            input_y,
            dir_w,
            input_h,
            10.0,
            focused,
            st.hover_id == Some(201),
            has_dir_err,
            pal,
            alpha,
        );
        let text_pad_x = x + 13.0;
        let text_avail = (dir_w - 26.0).max(20.0);
        st.canvas.draw_text(
            &st.install_dir_str,
            FONT_SECONDARY,
            false,
            text_pad_x,
            input_y,
            text_avail,
            input_h,
            0,
            1, // start-ellipsis so folder tail stays visible
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        // Blinking caret when focused (driven by 530ms TIMER_CARET)
        if focused && st.caret_visible {
            let end_px = st
                .canvas
                .hit_test_text_end(&st.install_dir_str, FONT_SECONDARY, false);
            let cx = text_pad_x + end_px.min(text_avail - 2.0) + 1.0;
            st.canvas.draw_line(
                cx,
                input_y + 10.0,
                cx,
                input_y + input_h - 10.0,
                1.6,
                D2dColor::from_rgb(pal.accent, 0.95 * alpha),
                true,
            );
        }
        if interactive {
            st.hits.push(HitRegion {
                id: 201,
                x,
                y: input_y,
                w: dir_w,
                h: input_h,
                action: ClickAction::FocusInstallDir,
                cursor: CursorKind::IBeam,
                hover_submenu: None,
            });
        }

        // Inline error right next to the Install Location field
        if let Some(ref err_msg) = st.dir_error {
            let err_col = (0.95, 0.42, 0.44);
            let ey = input_y + input_h + 6.0;
            draw_glyph(
                &st.canvas,
                UiGlyph::Alert,
                x + 9.0,
                ey + 8.0,
                12.5,
                D2dColor::from_rgb(err_col, 0.96 * alpha),
                1.8,
            );
            st.canvas.draw_text(
                err_msg,
                FONT_CAPTION,
                true,
                x + 20.0,
                ey,
                dir_w - 20.0,
                16.0,
                0,
                3,
                1,
                D2dColor::from_rgb(err_col, 0.98 * alpha),
                None,
            );
        }

        // Browse button
        let bx = x + dir_w + gap;
        draw_secondary_button(
            st,
            202,
            bx,
            input_y,
            browse_w,
            input_h,
            "Browse…",
            Some(UiGlyph::FolderOpen),
            ClickAction::BrowseInstallDir,
            pal,
            alpha,
            interactive,
        );

        // Reset button
        let rx = bx + browse_w + gap;
        draw_secondary_button(
            st,
            203,
            rx,
            input_y,
            reset_w,
            input_h,
            "Reset",
            Some(UiGlyph::RotateCcw),
            ClickAction::ResetInstallDir,
            pal,
            alpha,
            interactive,
        );

        // Integration Toggles
        let ty0 = input_y + 52.0;
        st.canvas.draw_text(
            "Windows Integration",
            FONT_SECONDARY,
            true,
            x + 2.0,
            ty0,
            240.0,
            18.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, 0.92 * alpha),
            None,
        );

        let toggles = [
            (
                210,
                "Start with Windows",
                "Run the lightweight ReCircles hotkey daemon at sign-in",
                st.toggle_startup.value,
                ClickAction::ToggleStartup,
            ),
            (
                211,
                "Create Start Menu Shortcuts",
                "Add ReCircles, Settings, and Setup shortcuts to Start",
                st.toggle_shortcuts.value,
                ClickAction::ToggleShortcuts,
            ),
            (
                212,
                "Launch Immediately After Setup",
                "Start ReCircles right away so your hotkeys work instantly",
                st.toggle_daemon.value,
                ClickAction::ToggleLaunchDaemon,
            ),
        ];

        for (i, (id, lbl, desc, anim, act)) in toggles.into_iter().enumerate() {
            let ry = ty0 + 22.0 + i as f64 * 52.0;
            draw_toggle_card(
                st,
                id,
                x,
                ry,
                w,
                46.0,
                lbl,
                desc,
                anim,
                act,
                pal,
                alpha,
                interactive,
            );
        }

        // Automatic Updates mode selector (Auto-install | Notify | Manual)
        let upd_y = ty0 + 22.0 + 3.0 * 52.0 + 10.0;
        st.canvas.draw_text(
            "Automatic Updates",
            FONT_SECONDARY,
            true,
            x + 2.0,
            upd_y,
            150.0,
            18.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, 0.92 * alpha),
            None,
        );
        st.canvas.draw_text(
            st.opts.update_mode.description(),
            FONT_CAPTION,
            false,
            x + 150.0,
            upd_y + 1.0,
            w - 150.0,
            16.0,
            2,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.78 * alpha),
            None,
        );
        let seg_y = upd_y + 22.0;
        let seg_h = 34.0;
        draw_segmented_control_3(
            st,
            215,
            x,
            seg_y,
            w,
            seg_h,
            ["Auto-install", "Notify Only", "Manual Only"],
            st.update_seg_x.value,
            [
                ClickAction::SetUpdateMode(UpdateMode::AutoInstall),
                ClickAction::SetUpdateMode(UpdateMode::Notify),
                ClickAction::SetUpdateMode(UpdateMode::Manual),
            ],
            pal,
            alpha,
            interactive,
        );

        // Destructive uninstall action (only if existing install detected)
        if st.existing_installed {
            let uy = seg_y + seg_h + 10.0;
            let uw = 198.0;
            let uh = 32.0;
            let hovered = st.hover_id == Some(220);
            let danger = (0.95, 0.42, 0.44);
            st.canvas.fill_rounded_rect_solid(
                x,
                uy,
                uw,
                uh,
                10.0,
                D2dColor::from_rgb(danger, if hovered { 0.22 } else { 0.12 } * alpha),
            );
            st.canvas.stroke_rounded_rect(
                x,
                uy,
                uw,
                uh,
                10.0,
                1.0,
                D2dColor::from_rgb(danger, if hovered { 0.72 } else { 0.42 } * alpha),
            );
            draw_glyph(
                &st.canvas,
                UiGlyph::Trash,
                x + 20.0,
                uy + uh * 0.5,
                14.0,
                D2dColor::from_rgb(danger, 0.95 * alpha),
                1.7,
            );
            st.canvas.draw_text(
                "Uninstall Existing…",
                FONT_SECONDARY,
                true,
                x + 36.0,
                uy,
                uw - 44.0,
                uh,
                0,
                3,
                1,
                D2dColor::from_rgb(danger, 0.98 * alpha),
                None,
            );
            if interactive {
                st.hits.push(HitRegion {
                    id: 220,
                    x,
                    y: uy,
                    w: uw,
                    h: uh,
                    action: ClickAction::UninstallExisting,
                    cursor: CursorKind::Hand,
                    hover_submenu: None,
                });
            }
        }
    }

    // ---- PAGE 1: SUBMENUS ----
    fn draw_page_submenus(
        st: &mut WizardState,
        x: f64,
        y: f64,
        w: f64,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        draw_page_header(
            st,
            x,
            y,
            w,
            "Choose Your Submenus",
            "Toggle modules below or click slices directly on the live ReCircles ring.",
            pal,
            alpha,
        );

        let list_y = y + 66.0;
        let row_h = 52.0;
        let row_gap = 8.0;
        // Single accent color per view on the right panel
        let tint = pal.accent;

        for (i, &sub) in BuiltinSubmenu::ALL.iter().enumerate() {
            let ry = list_y + i as f64 * (row_h + row_gap);
            let id = 300 + i as u64;
            let hovered = st.hover_id == Some(id) || st.ring.hovered == Some(sub);
            let anim = st.submenu_toggles[i].value.clamp(0.0, 1.0);
            let mandatory = sub.is_mandatory();

            // Clean card surface
            let card_bg = lerp_rgb(pal.capsule_bottom, tint, 0.06 * anim + if hovered { 0.04 } else { 0.0 });
            st.canvas.fill_rounded_rect_solid(
                x,
                ry,
                w,
                row_h,
                12.0,
                D2dColor::from_rgb(card_bg, lerp(0.56, 0.70, pal.is_light) * alpha),
            );
            let brd_col = if hovered {
                tint
            } else if anim > 0.5 {
                lerp_rgb(pal.capsule_border, tint, 0.40)
            } else {
                pal.capsule_border
            };
            let brd_a = if hovered { 0.58 } else if anim > 0.5 { 0.32 } else { 0.18 };
            st.canvas.stroke_rounded_rect(
                x,
                ry,
                w,
                row_h,
                12.0,
                1.0,
                D2dColor::from_rgb(brd_col, brd_a * alpha),
            );

            // Tinted vector icon plate
            draw_icon_plate(
                &st.canvas,
                UiGlyph::Submenu(sub),
                x + 26.0,
                ry + row_h * 0.5,
                32.0,
                16.5,
                tint,
                (0.45 + 0.55 * anim) * alpha,
            );

            // Title & Description
            let text_x = x + 52.0;
            let right_w = if mandatory { 104.0 } else { 62.0 };
            let text_w = w - 60.0 - right_w;
            st.canvas.draw_text(
                sub.title(),
                FONT_BODY,
                true,
                text_x,
                ry + 8.0,
                text_w,
                18.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, (0.60 + 0.40 * anim) * alpha),
                None,
            );
            st.canvas.draw_text(
                sub.description(),
                FONT_CAPTION,
                false,
                text_x,
                ry + 27.0,
                text_w,
                17.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_muted, 0.88 * alpha),
                None,
            );

            if mandatory {
                let bx = x + w - 98.0;
                let by = ry + (row_h - 24.0) * 0.5;
                st.canvas.fill_rounded_rect_solid(
                    bx,
                    by,
                    84.0,
                    24.0,
                    12.0,
                    D2dColor::from_rgb(tint, 0.16 * alpha),
                );
                st.canvas.stroke_rounded_rect(
                    bx,
                    by,
                    84.0,
                    24.0,
                    12.0,
                    1.0,
                    D2dColor::from_rgb(tint, 0.46 * alpha),
                );
                draw_glyph(
                    &st.canvas,
                    UiGlyph::Lock,
                    bx + 14.0,
                    by + 12.0,
                    11.5,
                    D2dColor::from_rgb(tint, 0.95 * alpha),
                    1.8,
                );
                st.canvas.draw_text(
                    "Required",
                    FONT_CAPTION,
                    true,
                    bx + 23.0,
                    by,
                    56.0,
                    24.0,
                    0,
                    3,
                    1,
                    D2dColor::from_rgb(tint, 0.98 * alpha),
                    None,
                );
            } else {
                draw_pill_switch(
                    &st.canvas,
                    x + w - 54.0,
                    ry + (row_h - 22.0) * 0.5,
                    anim,
                    tint,
                    pal,
                    alpha,
                );
            }

            if interactive {
                st.hits.push(HitRegion {
                    id,
                    x,
                    y: ry,
                    w,
                    h: row_h,
                    action: ClickAction::ToggleSubmenu(sub),
                    cursor: CursorKind::Hand,
                    hover_submenu: Some(sub),
                });
            }
        }

        // Amber warning callout positioned directly below the 5 submenu rows (no dead 7-row gap)
        let wy = list_y + (BuiltinSubmenu::ALL.len() as f64) * (row_h + row_gap) + 12.0;
        let wh = 62.0;
        let warn_accent = if pal.is_light > 0.5 {
            (0.76, 0.42, 0.04)
        } else {
            (0.98, 0.68, 0.22)
        };
        st.canvas.fill_rounded_rect_solid(
            x,
            wy,
            w,
            wh,
            12.0,
            D2dColor::from_rgb(warn_accent, lerp(0.14, 0.16, pal.is_light) * alpha),
        );
        st.canvas.stroke_rounded_rect(
            x,
            wy,
            w,
            wh,
            12.0,
            1.0,
            D2dColor::from_rgb(warn_accent, 0.55 * alpha),
        );
        draw_glyph(
            &st.canvas,
            UiGlyph::Alert,
            x + 22.0,
            wy + wh * 0.5,
            18.0,
            D2dColor::from_rgb(warn_accent, 0.96 * alpha),
            1.8,
        );
        let warn_txt_col = if pal.is_light > 0.5 {
            (0.38, 0.20, 0.02)
        } else {
            (0.99, 0.91, 0.72)
        };
        st.canvas.draw_text(
            SUBMENU_EXCLUSION_WARNING,
            FONT_CAPTION,
            true,
            x + 42.0,
            wy + 8.0,
            w - 54.0,
            wh - 14.0,
            0,
            3,
            3,
            D2dColor::from_rgb(warn_txt_col, alpha),
            None,
        );
    }

    // ---- PAGE 2: THEME ----
    fn draw_page_themes(
        st: &mut WizardState,
        x: f64,
        y: f64,
        w: f64,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        draw_page_header(
            st,
            x,
            y,
            w,
            "Choose a Glass Theme",
            "Select a palette to morph the installer and live ReCircles ring in real time.",
            pal,
            alpha,
        );

        let grid_y = y + 66.0;
        let col_gap = 12.0;
        let row_gap = 12.0;
        let card_w = (w - col_gap) * 0.5;
        let card_h = 88.0;

        let themes_len = st.themes.len();
        for i in 0..themes_len {
            let col = i % 2;
            let row = i / 2;
            let cx = x + col as f64 * (card_w + col_gap);
            let cy = grid_y + row as f64 * (card_h + row_gap);
            let id = 400 + i as u64;

            let theme_id = st.themes[i].id;
            let theme_label = st.themes[i].label;
            let theme_desc = st.themes[i].description;
            let t_pal = st.theme_palettes[i];

            let active = st.opts.theme.eq_ignore_ascii_case(theme_id);
            let hovered = st.hover_id == Some(id);
            let pressed = st
                .pressed_action
                .as_ref()
                .is_some_and(|(pid, _)| *pid == id);
            let card_scale: f64 = if pressed { 0.975 } else { 1.0 };

            st.canvas.save();
            if (card_scale - 1.0).abs() > 0.001 {
                st.canvas.translate(cx + card_w * 0.5, cy + card_h * 0.5);
                st.canvas.scale(card_scale, card_scale);
                st.canvas.translate(-(cx + card_w * 0.5), -(cy + card_h * 0.5));
            }

            // Solid card surface
            let fill_rgb = if active {
                lerp_rgb(pal.capsule_top, t_pal.accent, 0.14)
            } else if hovered {
                lerp_rgb(pal.capsule_top, pal.rim_top, 0.06)
            } else {
                pal.capsule_bottom
            };
            st.canvas.fill_rounded_rect_solid(
                cx,
                cy,
                card_w,
                card_h,
                14.0,
                D2dColor::from_rgb(fill_rgb, lerp(0.58, 0.70, pal.is_light) * alpha),
            );
            let brd_col = if active {
                t_pal.accent
            } else if hovered {
                pal.rim_top
            } else {
                pal.capsule_border
            };
            st.canvas.stroke_rounded_rect(
                cx,
                cy,
                card_w,
                card_h,
                14.0,
                if active { 1.5 } else { 1.0 },
                D2dColor::from_rgb(brd_col, if active { 0.92 } else if hovered { 0.42 } else { 0.22 } * alpha),
            );

            // Mini ReCircles ring swatch
            let sx = cx + 34.0;
            let sy = cy + card_h * 0.5;
            st.canvas.save();
            st.canvas.translate(sx, sy);
            let sw_stops = [
                D2dGradStop {
                    pos: 0.0,
                    color: D2dColor::from_rgb(t_pal.ring_top, 0.96 * alpha),
                },
                D2dGradStop {
                    pos: 1.0,
                    color: D2dColor::from_rgb(t_pal.ring_bottom, 0.96 * alpha),
                },
            ];
            st.canvas
                .fill_annulus_linear(9.0, 22.0, 0.0, -22.0, 0.0, 22.0, &sw_stops);
            st.canvas.stroke_annulus_rim(
                9.0,
                22.0,
                1.0,
                0.0,
                -22.0,
                0.0,
                22.0,
                D2dColor::from_rgb(t_pal.rim_top, 0.80 * alpha),
                D2dColor::from_rgb(t_pal.rim_bottom, 0.40 * alpha),
            );
            st.canvas.stroke_arc(
                0.0,
                0.0,
                19.5,
                -PI * 0.78,
                -PI * 0.18,
                2.6,
                D2dColor::from_rgb(t_pal.accent, 0.98 * alpha),
                true,
            );
            st.canvas.restore();

            // Theme title & description
            let tx = cx + 66.0;
            let tw = card_w - 78.0;
            st.canvas.draw_text(
                theme_label,
                FONT_BODY,
                true,
                tx,
                cy + 14.0,
                tw - if active { 22.0 } else { 0.0 },
                19.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, alpha),
                None,
            );
            st.canvas.draw_text(
                theme_desc,
                FONT_CAPTION,
                false,
                tx,
                cy + 36.0,
                tw,
                40.0,
                0,
                3,
                2,
                D2dColor::from_rgb(pal.text_muted, 0.88 * alpha),
                None,
            );

            if active {
                let ck_x = cx + card_w - 19.0;
                let ck_y = cy + 22.0;
                st.canvas.fill_circle(
                    ck_x,
                    ck_y,
                    9.0,
                    D2dColor::from_rgb(t_pal.accent, 0.96 * alpha),
                );
                draw_glyph(
                    &st.canvas,
                    UiGlyph::Check,
                    ck_x,
                    ck_y,
                    11.0,
                    D2dColor::from_rgb(t_pal.accent_fg, alpha),
                    2.2,
                );
            }

            if interactive {
                st.hits.push(HitRegion {
                    id,
                    x: cx,
                    y: cy,
                    w: card_w,
                    h: card_h,
                    action: ClickAction::SelectTheme(theme_id),
                    cursor: CursorKind::Hand,
                    hover_submenu: None,
                });
            }
            st.canvas.restore();
        }

        // Balanced 8th grid slot: Custom JSON Themes info card
        if themes_len == 7 {
            let cx = x + (card_w + col_gap);
            let cy = grid_y + 3.0 * (card_h + row_gap);
            st.canvas.fill_rounded_rect_solid(
                cx,
                cy,
                card_w,
                card_h,
                14.0,
                D2dColor::from_rgb(pal.capsule_bottom, lerp(0.32, 0.44, pal.is_light) * alpha),
            );
            st.canvas.stroke_rounded_rect(
                cx,
                cy,
                card_w,
                card_h,
                14.0,
                1.0,
                D2dColor::from_rgb(pal.capsule_border, 0.20 * alpha),
            );
            draw_icon_plate(
                &st.canvas,
                UiGlyph::Sparkles,
                cx + 34.0,
                cy + card_h * 0.5,
                34.0,
                18.0,
                pal.accent,
                0.85 * alpha,
            );
            let tx = cx + 64.0;
            let tw = card_w - 76.0;
            st.canvas.draw_text(
                "Custom Themes",
                FONT_BODY,
                true,
                tx,
                cy + 14.0,
                tw,
                19.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, 0.90 * alpha),
                None,
            );
            st.canvas.draw_text(
                "Import your own JSON themes anytime in Settings or via `recircles add`.",
                FONT_CAPTION,
                false,
                tx,
                cy + 36.0,
                tw,
                40.0,
                0,
                3,
                2,
                D2dColor::from_rgb(pal.text_muted, 0.84 * alpha),
                None,
            );
        }
    }

    // ---- PAGE 3: APPEARANCE ----
    #[allow(clippy::too_many_arguments)]
    fn draw_page_appearance(
        st: &mut WizardState,
        x: f64,
        y: f64,
        w: f64,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        draw_page_header(
            st,
            x,
            y,
            w - 96.0,
            "Appearance",
            "Configure translucency, blur, overflow layout, and motion before installing.",
            pal,
            alpha,
        );
        draw_secondary_button(
            st,
            450,
            x + w - 88.0,
            y + 2.0,
            88.0,
            32.0,
            "Reset",
            Some(UiGlyph::RotateCcw),
            ClickAction::ResetAppearance,
            pal,
            alpha,
            interactive,
        );

        let hc = st.opts.appearance.high_contrast;
        let slider_alpha = if hc { alpha * 0.48 } else { alpha };

        // 1. Translucency slider (0-100%, default 60%)
        let s1_y = y + 62.0;
        let s_h = 66.0;
        draw_appearance_slider(
            st,
            0,
            x,
            s1_y,
            w,
            s_h,
            "Translucency",
            "0% solid  ·  100% full glass",
            st.opts.appearance.translucency,
            st.slider_drag == SliderDragKind::Translucency,
            pal,
            slider_alpha,
            interactive,
        );

        // 2. Blur slider (0-100%, default 20%)
        let s2_y = s1_y + s_h + 8.0;
        draw_appearance_slider(
            st,
            1,
            x,
            s2_y,
            w,
            s_h,
            "Blur",
            "0% sharp glass  ·  100% deep frost",
            st.opts.appearance.blur,
            st.slider_drag == SliderDragKind::Blur,
            pal,
            slider_alpha,
            interactive,
        );

        // 3. Overflow Layout segmented control (Box | Rings)
        let lay_y = s2_y + s_h + 12.0;
        let lay_h = 36.0;
        let lbl_w = 132.0;
        st.canvas.draw_text(
            "Overflow Layout",
            FONT_SECONDARY,
            true,
            x + 2.0,
            lay_y,
            lbl_w - 6.0,
            lay_h,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, 0.94 * alpha),
            None,
        );
        draw_segmented_control(
            st,
            460,
            x + lbl_w,
            lay_y,
            w - lbl_w,
            lay_h,
            "Box",
            "Rings",
            st.layout_seg_x.value,
            ClickAction::SetOverflowLayout(OverflowLayout::Box),
            ClickAction::SetOverflowLayout(OverflowLayout::Ring),
            pal,
            alpha,
            interactive,
        );

        // 4. Compact toggle cards matching Settings -> Appearance
        let toggles = [
            (
                470,
                "Animations",
                "Smooth spring transitions and fluid ring motion",
                st.toggle_animations.value,
                ClickAction::ToggleAnimations,
            ),
            (
                471,
                "3D Loading",
                "Tumbling 3D glass ring while loading heavy menus",
                st.toggle_loading_3d.value,
                ClickAction::ToggleLoading3d,
            ),
            (
                472,
                "High Contrast",
                "Solid high-contrast surfaces with crisp borders",
                st.toggle_high_contrast.value,
                ClickAction::ToggleHighContrast,
            ),
        ];
        let t0_y = lay_y + lay_h + 12.0;
        let t_h = 48.0;
        let t_gap = 8.0;
        for (i, (id, lbl, desc, anim, act)) in toggles.into_iter().enumerate() {
            let ry = t0_y + i as f64 * (t_h + t_gap);
            draw_toggle_card(
                st,
                id,
                x,
                ry,
                w,
                t_h,
                lbl,
                desc,
                anim,
                act,
                pal,
                alpha,
                interactive,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_appearance_slider(
        st: &mut WizardState,
        slot: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        label: &str,
        hint: &str,
        value_pct: u8,
        dragging: bool,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        let track_x = x + 16.0;
        let track_w = w - 32.0;
        let track_y = y + h - 18.0;
        let track_h = 6.0;
        if interactive {
            st.slider_rects[slot] = (track_x, track_y, track_w, track_h);
        }

        let hovered = st.mouse_lx >= track_x - 12.0
            && st.mouse_lx <= track_x + track_w + 12.0
            && st.mouse_ly >= track_y - 14.0
            && st.mouse_ly <= track_y + track_h + 14.0;

        st.canvas.fill_rounded_rect_solid(
            x,
            y,
            w,
            h,
            12.0,
            D2dColor::from_rgb(pal.capsule_bottom, lerp(0.56, 0.70, pal.is_light) * alpha),
        );
        st.canvas.stroke_rounded_rect(
            x,
            y,
            w,
            h,
            12.0,
            if dragging { 1.3 } else { 1.0 },
            D2dColor::from_rgb(
                if dragging { pal.accent } else { pal.capsule_border },
                if dragging { 0.65 } else { 0.24 } * alpha,
            ),
        );

        // Label + subtle hint + right-aligned percentage pill
        st.canvas.draw_text(
            label,
            FONT_BODY,
            true,
            x + 16.0,
            y + 9.0,
            150.0,
            18.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        st.canvas.draw_text(
            hint,
            FONT_CAPTION,
            false,
            x + 124.0,
            y + 10.0,
            w - 204.0,
            17.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.80 * alpha),
            None,
        );
        let pct_str = format!("{value_pct}%");
        st.canvas.draw_text(
            &pct_str,
            FONT_BODY,
            true,
            x + w - 72.0,
            y + 9.0,
            56.0,
            18.0,
            2,
            3,
            1,
            D2dColor::from_rgb(pal.accent, alpha),
            None,
        );

        // Track background
        st.canvas.fill_rounded_rect_solid(
            track_x,
            track_y,
            track_w,
            track_h,
            track_h * 0.5,
            D2dColor::from_rgb(pal.rim_top, lerp(0.16, 0.22, pal.is_light) * alpha),
        );

        // Filled active track
        let frac = (f64::from(value_pct) / 100.0).clamp(0.0, 1.0);
        let fill_w = (track_w * frac).max(track_h);
        if value_pct > 0 {
            st.canvas.fill_rounded_rect_solid(
                track_x,
                track_y,
                fill_w,
                track_h,
                track_h * 0.5,
                D2dColor::from_rgb(pal.accent, 0.92 * alpha),
            );
        }

        // Apple-style circular thumb with subtle press growth
        let thumb_cx = track_x + track_w * frac;
        let thumb_cy = track_y + track_h * 0.5;
        let thumb_r = if dragging {
            9.5
        } else if hovered {
            8.5
        } else {
            7.8
        };
        st.canvas.fill_circle(
            thumb_cx,
            thumb_cy + 1.0,
            thumb_r + 1.5,
            D2dColor::from_rgb(pal.shadow, 0.28 * alpha),
        );
        st.canvas.fill_circle(
            thumb_cx,
            thumb_cy,
            thumb_r,
            D2dColor::from_rgb((1.0, 1.0, 1.0), 0.98 * alpha),
        );
        st.canvas.stroke_circle(
            thumb_cx,
            thumb_cy,
            thumb_r,
            if dragging { 1.8 } else { 1.1 },
            D2dColor::from_rgb(
                if dragging { pal.accent } else { pal.capsule_border },
                if dragging { 0.95 } else { 0.42 } * alpha,
            ),
        );
    }

    // ---- PAGE 4: KEYBINDS ----
    #[allow(clippy::too_many_arguments)]
    fn draw_page_keybinds(
        st: &mut WizardState,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        draw_page_header(
            st,
            x,
            y,
            w,
            "Keybinds",
            "Scroll all global and in-ring shortcuts. Click any binding to change it.",
            pal,
            alpha,
        );

        let body_y = y + 60.0;
        let nav_h = 36.0;
        let nav_y = y + h - nav_h - 2.0;
        let list_h = (nav_y - body_y - 12.0).max(160.0);

        enum KbItem {
            ToggleNumberShortcuts,
            Section(&'static str),
            Row {
                label: String,
                value: String,
                bold_label: bool,
                capturing: bool,
                hover_sub: Option<BuiltinSubmenu>,
                cap_act: ClickAction,
                clr_act: ClickAction,
            },
        }

        let fmt_keylist = |kl: &KeyList| -> String {
            if kl.is_empty() {
                String::new()
            } else {
                kl.display()
            }
        };

        let mut items = Vec::with_capacity(28);
        items.push(KbItem::ToggleNumberShortcuts);
        items.push(KbItem::Section("GLOBAL HOTKEYS"));
        items.push(KbItem::Row {
            label: "Main Radial Ring".into(),
            value: fmt_keylist(&st.opts.keybinds.main_menu),
            bold_label: true,
            capturing: st.capturing_hotkey == Some(KeybindCapture::MainRing),
            hover_sub: None,
            cap_act: ClickAction::CaptureMainHotkey,
            clr_act: ClickAction::ClearMainHotkey,
        });
        for &sub in &BuiltinSubmenu::ALL {
            if !st.opts.selected_submenus.contains(&sub) {
                continue;
            }
            let id = sub.id().to_string();
            let cur = st
                .opts
                .keybinds
                .menu_keybinds
                .get(&id)
                .map(fmt_keylist)
                .unwrap_or_default();
            let cap = st.capturing_hotkey.as_ref() == Some(&KeybindCapture::Submenu(id.clone()));
            items.push(KbItem::Row {
                label: format!("{} Submenu", sub.title()),
                value: cur,
                bold_label: false,
                capturing: cap,
                hover_sub: Some(sub),
                cap_act: ClickAction::CaptureSubmenuHotkey(id.clone()),
                clr_act: ClickAction::ClearSubmenuHotkey(id),
            });
        }
        for (cm_id, cm_title) in &st.custom_menus {
            let cur = st
                .opts
                .keybinds
                .menu_keybinds
                .get(cm_id)
                .map(fmt_keylist)
                .unwrap_or_default();
            let cap = st.capturing_hotkey.as_ref() == Some(&KeybindCapture::Submenu(cm_id.clone()));
            items.push(KbItem::Row {
                label: format!("{cm_title} Menu"),
                value: cur,
                bold_label: false,
                capturing: cap,
                hover_sub: None,
                cap_act: ClickAction::CaptureSubmenuHotkey(cm_id.clone()),
                clr_act: ClickAction::ClearSubmenuHotkey(cm_id.clone()),
            });
        }

        items.push(KbItem::Section("IN-RING CONTROLS"));
        for field in NavKeyField::ALL {
            let cur = fmt_keylist(field.get(&st.opts.keybinds));
            let cap = st.capturing_hotkey == Some(KeybindCapture::Nav(field));
            items.push(KbItem::Row {
                label: field.label().to_string(),
                value: cur,
                bold_label: false,
                capturing: cap,
                hover_sub: None,
                cap_act: ClickAction::CaptureNavKeybind(field),
                clr_act: ClickAction::ClearNavKeybind(field),
            });
        }

        let item_height = |it: &KbItem| -> f64 {
            match it {
                KbItem::ToggleNumberShortcuts => 46.0,
                KbItem::Section(_) => 26.0,
                KbItem::Row { .. } => 36.0,
            }
        };
        let gap = 5.0;
        let total_h: f64 = items.iter().map(|it| item_height(it) + gap).sum::<f64>() - gap;
        st.keybind_max_scroll = (total_h - list_h).max(0.0);
        if st.keybind_scroll.target > st.keybind_max_scroll {
            st.keybind_scroll.set_target(st.keybind_max_scroll);
        }
        let scroll = st.keybind_scroll.value;

        st.canvas.push_clip_rect(x - 2.0, body_y, w + 4.0, list_h);
        let mut cursor_y = body_y - scroll;
        let mut row_idx: u64 = 0;

        for it in items {
            let ih = item_height(&it);
            let is_row = matches!(it, KbItem::Row { .. });
            let visible = cursor_y + ih >= body_y && cursor_y <= body_y + list_h;
            let clickable =
                interactive && cursor_y >= body_y - 4.0 && cursor_y + ih <= body_y + list_h + 4.0;

            if visible {
                match it {
                    KbItem::ToggleNumberShortcuts => {
                        draw_toggle_card(
                            st,
                            510,
                            x,
                            cursor_y,
                            w,
                            ih,
                            "Number Shortcuts (1–8)",
                            "Press 1–8 while the ring is open to launch items directly",
                            st.toggle_number_shortcuts.value,
                            ClickAction::ToggleNumberShortcuts,
                            pal,
                            alpha,
                            clickable,
                        );
                    }
                    KbItem::Section(title) => {
                        st.canvas.draw_text(
                            title,
                            FONT_CAPTION,
                            true,
                            x + 4.0,
                            cursor_y + 6.0,
                            w - 8.0,
                            ih - 6.0,
                            0,
                            3,
                            1,
                            D2dColor::from_rgb(pal.text_muted, 0.86 * alpha),
                            None,
                        );
                    }
                    KbItem::Row {
                        label,
                        value,
                        bold_label,
                        capturing,
                        hover_sub,
                        cap_act,
                        clr_act,
                    } => {
                        let cap_id = 520 + row_idx * 2;
                        let clr_id = cap_id + 1;
                        let hovered = st.hover_id == Some(cap_id);
                        let pulse = if capturing && !st.reduced_motion {
                            0.5 + 0.5 * (st.clock * 5.2).sin()
                        } else {
                            0.0
                        };

                        st.canvas.fill_rounded_rect_solid(
                            x,
                            cursor_y,
                            w,
                            ih,
                            10.0,
                            D2dColor::from_rgb(
                                pal.capsule_bottom,
                                lerp(0.54, 0.68, pal.is_light) * alpha,
                            ),
                        );
                        st.canvas.stroke_rounded_rect(
                            x,
                            cursor_y,
                            w,
                            ih,
                            10.0,
                            if capturing { 1.4 } else { 1.0 },
                            D2dColor::from_rgb(
                                if capturing { pal.accent } else { pal.capsule_border },
                                if capturing {
                                    (0.65 + 0.30 * pulse) * alpha
                                } else {
                                    0.22 * alpha
                                },
                            ),
                        );

                        let has_val = !value.trim().is_empty();
                        st.canvas.fill_circle(
                            x + 15.0,
                            cursor_y + ih * 0.5,
                            3.2,
                            D2dColor::from_rgb(
                                if has_val || capturing {
                                    pal.accent
                                } else {
                                    pal.text_muted
                                },
                                if has_val || capturing { 0.95 } else { 0.45 } * alpha,
                            ),
                        );
                        st.canvas.draw_text(
                            &label,
                            FONT_SECONDARY,
                            bold_label,
                            x + 26.0,
                            cursor_y,
                            w - 254.0,
                            ih,
                            0,
                            3,
                            1,
                            D2dColor::from_rgb(pal.text_primary, alpha),
                            None,
                        );

                        let clr_w = if has_val { 26.0 } else { 0.0 };
                        let cap_w = 186.0;
                        let cap_x = x + w - cap_w - if has_val { clr_w + 11.0 } else { 7.0 };
                        let cap_y = cursor_y + 4.0;
                        let cap_h = ih - 8.0;

                        let cap_fill = if capturing {
                            pal.accent
                        } else if has_val {
                            lerp_rgb(pal.capsule_top, pal.accent, 0.14)
                        } else {
                            pal.capsule_top
                        };
                        st.canvas.fill_rounded_rect_solid(
                            cap_x,
                            cap_y,
                            cap_w,
                            cap_h,
                            8.0,
                            D2dColor::from_rgb(
                                cap_fill,
                                if capturing {
                                    (0.22 + 0.10 * pulse) * alpha
                                } else if hovered {
                                    0.85 * alpha
                                } else {
                                    0.68 * alpha
                                },
                            ),
                        );
                        st.canvas.stroke_rounded_rect(
                            cap_x,
                            cap_y,
                            cap_w,
                            cap_h,
                            8.0,
                            1.0,
                            D2dColor::from_rgb(
                                if capturing || has_val {
                                    pal.accent
                                } else {
                                    pal.capsule_border
                                },
                                if capturing {
                                    0.90
                                } else if has_val {
                                    0.48
                                } else if hovered {
                                    0.45
                                } else {
                                    0.24
                                } * alpha,
                            ),
                        );
                        let disp: Cow<'_, str> = if capturing {
                            Cow::Borrowed("Press key…")
                        } else if has_val {
                            Cow::Borrowed(&value)
                        } else {
                            Cow::Borrowed("None · Click to assign")
                        };
                        st.canvas.draw_text(
                            &disp,
                            FONT_CAPTION,
                            has_val || capturing,
                            cap_x + 8.0,
                            cap_y,
                            cap_w - 16.0,
                            cap_h,
                            1,
                            3,
                            1,
                            D2dColor::from_rgb(
                                if capturing || has_val {
                                    pal.accent
                                } else {
                                    pal.text_muted
                                },
                                alpha,
                            ),
                            None,
                        );
                        if clickable {
                            st.hits.push(HitRegion {
                                id: cap_id,
                                x: cap_x,
                                y: cap_y,
                                w: cap_w,
                                h: cap_h,
                                action: cap_act,
                                cursor: CursorKind::Hand,
                                hover_submenu: hover_sub,
                            });
                        }

                        if has_val {
                            let cx_btn = x + w - clr_w - 6.0;
                            let clr_hov = st.hover_id == Some(clr_id);
                            st.canvas.fill_rounded_rect_solid(
                                cx_btn,
                                cap_y,
                                clr_w,
                                cap_h,
                                8.0,
                                D2dColor::from_rgb(
                                    pal.rim_top,
                                    if clr_hov { 0.14 } else { 0.05 } * alpha,
                                ),
                            );
                            draw_glyph(
                                &st.canvas,
                                UiGlyph::Close,
                                cx_btn + clr_w * 0.5,
                                cap_y + cap_h * 0.5,
                                10.0,
                                D2dColor::from_rgb(
                                    if clr_hov {
                                        pal.text_primary
                                    } else {
                                        pal.text_muted
                                    },
                                    alpha,
                                ),
                                1.8,
                            );
                            if clickable {
                                st.hits.push(HitRegion {
                                    id: clr_id,
                                    x: cx_btn,
                                    y: cap_y,
                                    w: clr_w,
                                    h: cap_h,
                                    action: clr_act,
                                    cursor: CursorKind::Hand,
                                    hover_submenu: hover_sub,
                                });
                            }
                        }
                    }
                }
            }

            if is_row {
                row_idx += 1;
            }
            cursor_y += ih + gap;
        }
        st.canvas.pop_clip_rect();

        // Subtle scrollbar thumb if overflowing
        if st.keybind_max_scroll > 1.0 {
            let frac = (list_h / total_h).clamp(0.14, 1.0);
            let thumb_h = list_h * frac;
            let progress = (scroll / st.keybind_max_scroll).clamp(0.0, 1.0);
            let ty = body_y + progress * (list_h - thumb_h);
            st.canvas.fill_rounded_rect_solid(
                x + w + 4.0,
                ty,
                3.0,
                thumb_h,
                1.5,
                D2dColor::from_rgb(pal.rim_top, 0.36 * alpha),
            );
        }

        // In-Ring Navigation segmented preset bar + Reset Defaults action button
        let lbl_w = 104.0;
        let rst_w = 88.0;
        let bar_gap = 8.0;
        let seg_w = w - lbl_w - rst_w - bar_gap;
        st.canvas.draw_text(
            "In-Ring Nav",
            FONT_SECONDARY,
            true,
            x + 2.0,
            nav_y,
            lbl_w - 6.0,
            nav_h,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.92 * alpha),
            None,
        );
        draw_segmented_control(
            st,
            590,
            x + lbl_w,
            nav_y,
            seg_w,
            nav_h,
            "Standard (Arrows)",
            "Vim (HJKL)",
            st.nav_seg_x.value,
            ClickAction::SetNavPreset(NavPreset::Standard),
            ClickAction::SetNavPreset(NavPreset::Vim),
            pal,
            alpha,
            interactive,
        );
        draw_secondary_button(
            st,
            595,
            x + lbl_w + seg_w + bar_gap,
            nav_y,
            rst_w,
            nav_h,
            "Reset",
            Some(UiGlyph::RotateCcw),
            ClickAction::ResetCustomKeybinds,
            pal,
            alpha,
            interactive,
        );
    }

    // ---- PAGE 4: REVIEW & INSTALL ----
    fn draw_page_install(
        st: &mut WizardState,
        x: f64,
        y: f64,
        w: f64,
        pal: &ThemePalette,
        alpha: f64,
        _interactive: bool,
    ) {
        if st.installing {
            draw_page_header(
                st,
                x,
                y,
                w,
                "Installing ReCircles…",
                "Writing native binaries, selected submenu DLLs, and your configuration.",
                pal,
                alpha,
            );
            let cy = y + 92.0;
            let ch = 160.0;
            st.canvas.fill_rounded_rect_solid(
                x,
                cy,
                w,
                ch,
                14.0,
                D2dColor::from_rgb(pal.capsule_bottom, lerp(0.56, 0.72, pal.is_light) * alpha),
            );
            st.canvas.stroke_rounded_rect(
                x,
                cy,
                w,
                ch,
                14.0,
                1.0,
                D2dColor::from_rgb(pal.accent, 0.42 * alpha),
            );
            st.canvas.draw_text(
                "Setting up your radial workspace",
                FONT_SUBHEAD,
                true,
                x + 24.0,
                cy + 36.0,
                w - 48.0,
                24.0,
                1,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, alpha),
                None,
            );
            st.canvas.draw_text(
                &st.install_dir_str,
                FONT_SECONDARY,
                false,
                x + 24.0,
                cy + 68.0,
                w - 48.0,
                20.0,
                1,
                2,
                1,
                D2dColor::from_rgb(pal.text_muted, 0.90 * alpha),
                None,
            );
            // Progress track (respects prefers-reduced-motion)
            let bar_w = w - 120.0;
            let bar_x = x + 60.0;
            let bar_y = cy + 112.0;
            st.canvas.fill_rounded_rect_solid(
                bar_x,
                bar_y,
                bar_w,
                6.0,
                3.0,
                D2dColor::from_rgb(pal.rim_top, 0.14 * alpha),
            );
            let seg_w = bar_w * 0.34;
            let t = if st.reduced_motion {
                0.5
            } else {
                0.5 - 0.5 * (st.clock * 3.2).cos()
            };
            st.canvas.fill_rounded_rect_solid(
                bar_x + (bar_w - seg_w) * t,
                bar_y,
                seg_w,
                6.0,
                3.0,
                D2dColor::from_rgb(pal.accent, 0.92 * alpha),
            );
            return;
        }

        if let Some(done) = st.summary.clone() {
            draw_page_header(
                st,
                x,
                y,
                w,
                "ReCircles is Ready",
                "Everything is installed and configured. Summon the ring anytime with your hotkey.",
                pal,
                alpha,
            );

            // Success hero card
            let ok_col = (0.28, 0.84, 0.54);
            let sy = y + 66.0;
            st.canvas.fill_rounded_rect_solid(
                x,
                sy,
                w,
                72.0,
                14.0,
                D2dColor::from_rgb(ok_col, lerp(0.14, 0.16, pal.is_light) * alpha),
            );
            st.canvas.stroke_rounded_rect(
                x,
                sy,
                w,
                72.0,
                14.0,
                1.0,
                D2dColor::from_rgb(ok_col, 0.55 * alpha),
            );
            st.canvas.fill_circle(
                x + 34.0,
                sy + 36.0,
                16.0,
                D2dColor::from_rgb(ok_col, 0.24 * alpha),
            );
            draw_glyph(
                &st.canvas,
                UiGlyph::Check,
                x + 34.0,
                sy + 36.0,
                16.0,
                D2dColor::from_rgb(ok_col, 0.98 * alpha),
                2.2,
            );
            st.canvas.draw_text(
                "Installation Complete",
                FONT_SUBHEAD,
                true,
                x + 62.0,
                sy + 14.0,
                w - 82.0,
                22.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, alpha),
                None,
            );
            let primary_hk = done
                .active_hotkeys
                .first()
                .map(|(_, v)| v.as_str())
                .unwrap_or("Win+e");
            st.canvas.draw_text(
                &format!("Press {primary_hk} anywhere to open the ReCircles radial menu."),
                FONT_SECONDARY,
                false,
                x + 62.0,
                sy + 38.0,
                w - 82.0,
                20.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_muted, 0.94 * alpha),
                None,
            );

            // Structured completion details card
            let dy = sy + 86.0;
            let dh = 264.0;
            st.canvas.fill_rounded_rect_solid(
                x,
                dy,
                w,
                dh,
                14.0,
                D2dColor::from_rgb(pal.capsule_bottom, lerp(0.56, 0.72, pal.is_light) * alpha),
            );
            st.canvas.stroke_rounded_rect(
                x,
                dy,
                w,
                dh,
                14.0,
                1.0,
                D2dColor::from_rgb(pal.capsule_border, 0.26 * alpha),
            );

            let dlls_in = if done.installed_dlls.is_empty() {
                "Core only (Settings)".into()
            } else {
                done.installed_dlls.join(", ")
            };
            let dlls_out = if done.omitted_dlls.is_empty() {
                "None".into()
            } else {
                done.omitted_dlls.join(", ")
            };
            let hks = if done.active_hotkeys.is_empty() {
                "None".into()
            } else {
                done.active_hotkeys
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect::<Vec<_>>()
                    .join("   ·   ")
            };
            let rows = [
                ("Install Folder", done.install_dir.to_string_lossy().into_owned()),
                ("Configuration", done.config_path.to_string_lossy().into_owned()),
                ("Installed DLLs", dlls_in),
                ("Excluded DLLs", dlls_out),
                ("Active Hotkeys", hks),
            ];
            for (i, (k, v)) in rows.iter().enumerate() {
                let ry = dy + 16.0 + i as f64 * 48.0;
                if i > 0 {
                    draw_soft_divider(&st.canvas, x + 18.0, ry - 4.0, w - 36.0, pal.divider, 0.14 * alpha);
                }
                st.canvas.draw_text(
                    k,
                    FONT_CAPTION,
                    true,
                    x + 20.0,
                    ry,
                    w - 40.0,
                    16.0,
                    0,
                    3,
                    1,
                    D2dColor::from_rgb(pal.text_muted, 0.86 * alpha),
                    None,
                );
                st.canvas.draw_text(
                    v,
                    FONT_SECONDARY,
                    false,
                    x + 20.0,
                    ry + 17.0,
                    w - 40.0,
                    22.0,
                    0,
                    2,
                    1,
                    D2dColor::from_rgb(pal.text_primary, alpha),
                    None,
                );
            }
            return;
        }

        // Review Summary prior to install
        draw_page_header(
            st,
            x,
            y,
            w,
            "Ready to Install",
            "Review your setup below. You can re-run this installer anytime to modify modules.",
            pal,
            alpha,
        );

        let r0 = y + 66.0;
        // Card 1: Destination & Integration
        draw_summary_card(st, x, r0, w, 78.0, pal, alpha);
        st.canvas.draw_text(
            "DESTINATION",
            FONT_CAPTION,
            true,
            x + 18.0,
            r0 + 12.0,
            160.0,
            15.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.85 * alpha),
            None,
        );
        st.canvas.draw_text(
            &st.install_dir_str,
            FONT_BODY,
            true,
            x + 18.0,
            r0 + 29.0,
            w - 36.0,
            20.0,
            0,
            2,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        let mut flags: Vec<String> = Vec::new();
        if st.opts.start_with_windows {
            flags.push("Start with Windows".into());
        }
        if st.opts.create_shortcuts {
            flags.push("Start Menu Shortcuts".into());
        }
        if st.opts.launch_daemon {
            flags.push("Launch Daemon".into());
        }
        flags.push(format!("Updates: {}", st.opts.update_mode.short_label()));
        let flag_line = flags.join("  ·  ");
        st.canvas.draw_text(
            &flag_line,
            FONT_CAPTION,
            false,
            x + 18.0,
            r0 + 52.0,
            w - 36.0,
            16.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.88 * alpha),
            None,
        );

        // Card 2 & 3 side-by-side: Theme & Appearance | Keybinds
        let r1 = r0 + 90.0;
        let half_w = (w - 12.0) * 0.5;
        draw_summary_card(st, x, r1, half_w, 84.0, pal, alpha);
        st.canvas.draw_text(
            "THEME & APPEARANCE",
            FONT_CAPTION,
            true,
            x + 18.0,
            r1 + 12.0,
            half_w - 36.0,
            15.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.85 * alpha),
            None,
        );
        let theme_lbl: &str = st
            .themes
            .iter()
            .find(|b| b.id.eq_ignore_ascii_case(&st.opts.theme))
            .map(|b| b.label)
            .unwrap_or(&st.opts.theme);
        st.canvas.fill_circle(
            x + 26.0,
            r1 + 43.0,
            6.5,
            D2dColor::from_rgb(pal.accent, 0.96 * alpha),
        );
        st.canvas.stroke_circle(
            x + 26.0,
            r1 + 43.0,
            6.5,
            1.0,
            D2dColor::from_rgb(pal.rim_top, 0.70 * alpha),
        );
        st.canvas.draw_text(
            theme_lbl,
            FONT_BODY,
            true,
            x + 40.0,
            r1 + 32.0,
            half_w - 54.0,
            22.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        let app_summary = if st.opts.appearance.high_contrast {
            format!("High Contrast  ·  {} overflow", st.opts.appearance.layout.label())
        } else {
            format!(
                "{}% glass  ·  {}% blur  ·  {}",
                st.opts.appearance.translucency,
                st.opts.appearance.blur,
                st.opts.appearance.layout.label(),
            )
        };
        st.canvas.draw_text(
            &app_summary,
            FONT_CAPTION,
            false,
            x + 18.0,
            r1 + 56.0,
            half_w - 36.0,
            16.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.88 * alpha),
            None,
        );

        let kx = x + half_w + 12.0;
        draw_summary_card(st, kx, r1, half_w, 84.0, pal, alpha);
        st.canvas.draw_text(
            "MAIN HOTKEY & NAV",
            FONT_CAPTION,
            true,
            kx + 18.0,
            r1 + 12.0,
            half_w - 36.0,
            15.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.85 * alpha),
            None,
        );
        let main_hk_owned = st.opts.keybinds.main_menu.display();
        let main_hk: &str = if main_hk_owned.trim().is_empty() {
            "None"
        } else {
            &main_hk_owned
        };
        st.canvas.draw_text(
            main_hk,
            FONT_BODY,
            true,
            kx + 18.0,
            r1 + 32.0,
            half_w - 36.0,
            22.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.accent, alpha),
            None,
        );
        let nav_lbl = if st.opts.keybinds.number_shortcuts {
            format!("{}  ·  1–8 keys", st.opts.nav_preset.label())
        } else {
            st.opts.nav_preset.label().to_string()
        };
        st.canvas.draw_text(
            &nav_lbl,
            FONT_CAPTION,
            false,
            kx + 18.0,
            r1 + 56.0,
            half_w - 36.0,
            16.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.88 * alpha),
            None,
        );

        // Card 4: Included & Excluded Submenu Module Chips
        let r2 = r1 + 96.0;
        draw_summary_card(st, x, r2, w, 168.0, pal, alpha);
        st.canvas.draw_text(
            "INCLUDED MODULES",
            FONT_CAPTION,
            true,
            x + 18.0,
            r2 + 12.0,
            w - 36.0,
            15.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.85 * alpha),
            None,
        );

        let mut included = [BuiltinSubmenu::Files; 5];
        let mut inc_len = 0usize;
        for &sub in &BuiltinSubmenu::ALL {
            if st.opts.selected_submenus.contains(&sub) {
                included[inc_len] = sub;
                inc_len += 1;
            }
        }
        let mut excluded = [BuiltinSubmenu::Files; 4];
        let mut exc_len = 0usize;
        for &sub in &BuiltinSubmenu::OPTIONAL {
            if !st.opts.selected_submenus.contains(&sub) {
                excluded[exc_len] = sub;
                exc_len += 1;
            }
        }

        // Single accent color per view for module chips
        let chip_tint = pal.accent;
        let mut cx = x + 18.0;
        let mut cy = r2 + 34.0;
        for &sub in &included[..inc_len] {
            let label = sub.title();
            let (tw, _) = st.canvas.measure_text(label, FONT_CAPTION, true, 160.0, 1);
            let cw = tw + 30.0;
            if cx + cw > x + w - 18.0 {
                cx = x + 18.0;
                cy += 30.0;
            }
            st.canvas.fill_rounded_rect_solid(
                cx,
                cy,
                cw,
                24.0,
                12.0,
                D2dColor::from_rgb(chip_tint, 0.16 * alpha),
            );
            st.canvas.stroke_rounded_rect(
                cx,
                cy,
                cw,
                24.0,
                12.0,
                1.0,
                D2dColor::from_rgb(chip_tint, 0.44 * alpha),
            );
            st.canvas
                .fill_circle(cx + 11.0, cy + 12.0, 3.2, D2dColor::from_rgb(chip_tint, 0.96 * alpha));
            st.canvas.draw_text(
                label,
                FONT_CAPTION,
                true,
                cx + 19.0,
                cy,
                tw + 6.0,
                24.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, alpha),
                None,
            );
            cx += cw + 8.0;
        }

        let excl_y = r2 + 106.0;
        draw_soft_divider(&st.canvas, x + 18.0, excl_y - 6.0, w - 36.0, pal.divider, 0.14 * alpha);
        let excl_title: Cow<'static, str> = if exc_len == 0 {
            Cow::Borrowed("All 5 ReCircles modules will be installed.")
        } else {
            let mut s = String::from("Excluded (not installed on disk): ");
            for (i, &sub) in excluded[..exc_len].iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(sub.title());
            }
            Cow::Owned(s)
        };
        st.canvas.draw_text(
            &excl_title,
            FONT_CAPTION,
            false,
            x + 18.0,
            excl_y + 4.0,
            w - 36.0,
            20.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.92 * alpha),
            None,
        );

        let sub_hks: Vec<String> = BuiltinSubmenu::ALL
            .iter()
            .filter(|s| st.opts.selected_submenus.contains(s))
            .filter_map(|s| {
                st.opts
                    .keybinds
                    .menu_keybinds
                    .get(s.id())
                    .filter(|kl| !kl.is_empty())
                    .map(|kl| format!("{}={}", s.title(), kl.display()))
            })
            .collect();
        let hk_summary = if sub_hks.is_empty() {
            "Submenu Hotkeys: None assigned".to_string()
        } else {
            format!("Submenu Hotkeys: {}", sub_hks.join("  ·  "))
        };
        st.canvas.draw_text(
            &hk_summary,
            FONT_CAPTION,
            false,
            x + 18.0,
            excl_y + 28.0,
            w - 36.0,
            18.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.80 * alpha),
            None,
        );
    }

    // ==================== BOTTOM ACTION DOCK ====================

    fn draw_bottom_dock(st: &mut WizardState, pal: &ThemePalette, alpha: f64) {
        let dock_y = PAD + SHELL_H - 72.0;
        let dx = RIGHT_X + 28.0;
        let dw = RIGHT_W - 56.0;

        draw_soft_divider(&st.canvas, dx, dock_y, dw, pal.divider, 0.20 * alpha);

        let btn_y = dock_y + 17.0;
        let btn_h = 38.0;

        // Animated status toast pill (left side of dock when no Back button or centered between Back & Next)
        let show_back = st.page > 0 && st.summary.is_none() && !st.installing;
        let back_w = 102.0;

        if show_back {
            let hovered = st.hover_id == Some(801);
            let press = st.back_press.value.clamp(0.0, 1.0);
            let sc = 1.0 - 0.035 * press;
            st.canvas.save();
            st.canvas.translate(dx + back_w * 0.5, btn_y + btn_h * 0.5);
            st.canvas.scale(sc, sc);
            let bx = -back_w * 0.5;
            let by = -btn_h * 0.5;
            st.canvas.fill_rounded_rect_solid(
                bx,
                by,
                back_w,
                btn_h,
                10.0,
                D2dColor::from_rgb(
                    pal.capsule_top,
                    if hovered { 0.85 } else { 0.64 } * alpha,
                ),
            );
            st.canvas.stroke_rounded_rect(
                bx,
                by,
                back_w,
                btn_h,
                10.0,
                1.0,
                D2dColor::from_rgb(
                    pal.capsule_border,
                    if hovered { 0.50 } else { 0.26 } * alpha,
                ),
            );
            draw_glyph(
                &st.canvas,
                UiGlyph::ChevronLeft,
                bx + 22.0,
                0.0,
                13.5,
                D2dColor::from_rgb(pal.text_primary, 0.90 * alpha),
                1.9,
            );
            st.canvas.draw_text(
                "Back",
                FONT_BODY,
                true,
                bx + 30.0,
                by,
                back_w - 40.0,
                btn_h,
                1,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, alpha),
                None,
            );
            st.canvas.restore();
            st.hits.push(HitRegion {
                id: 801,
                x: dx,
                y: btn_y,
                w: back_w,
                h: btn_h,
                action: ClickAction::Back,
                cursor: CursorKind::Hand,
                hover_submenu: None,
            });
        }

        // Primary CTA button (solid fill, no decorative gradient or sheen)
        let (cta_lbl, cta_glyph, cta_act) = if st.installing {
            ("Installing…", None, None)
        } else if st.summary.is_some() {
            ("Done", Some(UiGlyph::Check), Some(ClickAction::Finish))
        } else if st.page == PAGE_INSTALL {
            (
                "Install ReCircles",
                Some(UiGlyph::Sparkles),
                Some(ClickAction::RunInstall),
            )
        } else {
            ("Continue", Some(UiGlyph::ChevronRight), Some(ClickAction::Next))
        };
        let cta_w = if st.page == PAGE_INSTALL && st.summary.is_none() {
            168.0
        } else {
            138.0
        };
        let cta_x = dx + dw - cta_w;
        let hovered = st.hover_id == Some(802);
        let press = st.primary_press.value.clamp(0.0, 1.0);
        let sc = 1.0 - 0.035 * press;

        st.canvas.save();
        st.canvas.translate(cta_x + cta_w * 0.5, btn_y + btn_h * 0.5);
        st.canvas.scale(sc, sc);
        let px = -cta_w * 0.5;
        let py = -btn_h * 0.5;

        let cta_col = if hovered {
            lerp_rgb(pal.accent, (1.0, 1.0, 1.0), 0.10)
        } else {
            pal.accent
        };
        let cta_a = if st.installing { 0.55 } else { 0.96 } * alpha;
        st.canvas.fill_rounded_rect_solid(
            px,
            py,
            cta_w,
            btn_h,
            10.0,
            D2dColor::from_rgb(cta_col, cta_a),
        );
        st.canvas.stroke_rounded_rect(
            px,
            py,
            cta_w,
            btn_h,
            10.0,
            1.0,
            D2dColor::from_rgb(pal.rim_top, 0.36 * cta_a),
        );

        let fg = pal.accent_fg;
        let lbl_off = if cta_glyph.is_some() { -8.0 } else { 0.0 };
        st.canvas.draw_text(
            cta_lbl,
            FONT_BODY,
            true,
            px + 12.0,
            py,
            cta_w - 24.0 + lbl_off * 2.0,
            btn_h,
            1,
            3,
            1,
            D2dColor::from_rgb(fg, alpha),
            None,
        );
        if let Some(g) = cta_glyph {
            draw_glyph(
                &st.canvas,
                g,
                px + cta_w - 20.0,
                0.0,
                13.5,
                D2dColor::from_rgb(fg, 0.95 * alpha),
                2.0,
            );
        }
        st.canvas.restore();

        if let Some(act) = cta_act {
            st.hits.push(HitRegion {
                id: 802,
                x: cta_x,
                y: btn_y,
                w: cta_w,
                h: btn_h,
                action: act,
                cursor: CursorKind::Hand,
                hover_submenu: None,
            });
        }

        // Inline status message pill (sits cleanly between Back and Primary CTA)
        let st_v = st.status_spring.value.clamp(0.0, 1.0);
        if st_v > 0.02
            && let Some((ref msg, is_err)) = st.status_msg
        {
            let toast_x = if show_back { dx + back_w + 12.0 } else { dx };
            let toast_w = (cta_x - toast_x - 12.0).max(40.0);
            let toast_y = btn_y + 4.0 + (1.0 - st_v) * 6.0;
            let toast_h = 30.0;
            let tint = if is_err {
                (0.96, 0.44, 0.46)
            } else if st.capturing_hotkey.is_some() {
                pal.accent
            } else {
                (0.32, 0.86, 0.56)
            };
            st.canvas.fill_rounded_rect_solid(
                toast_x,
                toast_y,
                toast_w,
                toast_h,
                10.0,
                D2dColor::from_rgb(tint, 0.14 * st_v * alpha),
            );
            st.canvas.stroke_rounded_rect(
                toast_x,
                toast_y,
                toast_w,
                toast_h,
                10.0,
                1.0,
                D2dColor::from_rgb(tint, 0.44 * st_v * alpha),
            );
            st.canvas.draw_text(
                msg,
                FONT_CAPTION,
                true,
                toast_x + 12.0,
                toast_y,
                toast_w - 24.0,
                toast_h,
                0,
                3,
                1,
                D2dColor::from_rgb(tint, 0.98 * st_v * alpha),
                None,
            );
        }
    }

    // ==================== IN-SURFACE ALERT DIALOG & PRIMITIVES ====================

    fn draw_uninstall_alert_dialog(st: &mut WizardState, pal: &ThemePalette, alpha: f64) {
        // Modal backdrop intercepts all background hit targets
        st.hits.clear();
        st.canvas.fill_rounded_rect_solid(
            PAD,
            PAD,
            SHELL_W,
            SHELL_H,
            SHELL_R,
            D2dColor::from_rgb(pal.shadow, 0.62 * alpha),
        );

        let dw = 420.0;
        let dh = 192.0;
        let dx = PAD + (SHELL_W - dw) * 0.5;
        let dy = PAD + (SHELL_H - dh) * 0.5;
        let danger = (0.95, 0.42, 0.44);

        st.canvas.fill_rounded_rect_shadow(
            dx,
            dy,
            dw,
            dh,
            16.0,
            D2dColor::from_rgb(pal.shadow, 1.0),
            0.85 * alpha,
        );
        st.canvas.fill_rounded_rect_solid(
            dx,
            dy,
            dw,
            dh,
            16.0,
            D2dColor::from_rgb(pal.ring_mid, 0.98 * alpha),
        );
        st.canvas.stroke_rounded_rect(
            dx,
            dy,
            dw,
            dh,
            16.0,
            1.0,
            D2dColor::from_rgb(pal.capsule_border, 0.36 * alpha),
        );

        draw_icon_plate(
            &st.canvas,
            UiGlyph::Trash,
            dx + 36.0,
            dy + 38.0,
            36.0,
            18.0,
            danger,
            alpha,
        );
        st.canvas.draw_text(
            "Uninstall ReCircles?",
            FONT_SUBHEAD,
            true,
            dx + 66.0,
            dy + 20.0,
            dw - 86.0,
            22.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        st.canvas.draw_text(
            "This removes ReCircles executables, Start Menu shortcuts, and installed submenu DLLs from your system.",
            FONT_SECONDARY,
            false,
            dx + 66.0,
            dy + 46.0,
            dw - 86.0,
            64.0,
            0,
            3,
            3,
            D2dColor::from_rgb(pal.text_muted, 0.92 * alpha),
            None,
        );

        let btn_w = 112.0;
        let btn_h = 36.0;
        let btn_y = dy + dh - btn_h - 18.0;
        let cancel_x = dx + dw - btn_w * 2.0 - 28.0;
        let confirm_x = dx + dw - btn_w - 18.0;

        draw_secondary_button(
            st,
            950,
            cancel_x,
            btn_y,
            btn_w,
            btn_h,
            "Cancel",
            None,
            ClickAction::CancelUninstall,
            pal,
            alpha,
            true,
        );

        let hov_confirm = st.hover_id == Some(951);
        st.canvas.fill_rounded_rect_solid(
            confirm_x,
            btn_y,
            btn_w,
            btn_h,
            10.0,
            D2dColor::from_rgb(danger, if hov_confirm { 0.96 } else { 0.86 } * alpha),
        );
        st.canvas.stroke_rounded_rect(
            confirm_x,
            btn_y,
            btn_w,
            btn_h,
            10.0,
            1.0,
            D2dColor::from_rgb((1.0, 1.0, 1.0), 0.28 * alpha),
        );
        st.canvas.draw_text(
            "Uninstall",
            FONT_SECONDARY,
            true,
            confirm_x + 10.0,
            btn_y,
            btn_w - 20.0,
            btn_h,
            1,
            3,
            1,
            D2dColor::from_rgb((1.0, 1.0, 1.0), alpha),
            None,
        );
        st.hits.push(HitRegion {
            id: 951,
            x: confirm_x,
            y: btn_y,
            w: btn_w,
            h: btn_h,
            action: ClickAction::ConfirmUninstall,
            cursor: CursorKind::Hand,
            hover_submenu: None,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_icon_plate(
        canvas: &D2dCanvas,
        glyph: UiGlyph,
        cx: f64,
        cy: f64,
        plate: f64,
        icon_size: f64,
        tint: Rgb,
        alpha: f64,
    ) {
        let half = plate * 0.5;
        let rad = plate * 0.28;
        canvas.fill_rounded_rect_solid(
            cx - half,
            cy - half,
            plate,
            plate,
            rad,
            D2dColor::from_rgb(tint, 0.16 * alpha),
        );
        canvas.stroke_rounded_rect(
            cx - half,
            cy - half,
            plate,
            plate,
            rad,
            1.0,
            D2dColor::from_rgb(tint, 0.42 * alpha),
        );
        draw_glyph(
            canvas,
            glyph,
            cx,
            cy,
            icon_size,
            D2dColor::from_rgb(tint, alpha),
            1.65,
        );
    }

    fn draw_glyph(
        canvas: &D2dCanvas,
        glyph: UiGlyph,
        cx: f64,
        cy: f64,
        size: f64,
        col: D2dColor,
        stroke_w: f64,
    ) {
        let cmds = glyph_cmds(glyph);
        canvas.save();
        canvas.translate(cx - size * 0.5, cy - size * 0.5);
        canvas.scale(size / 24.0, size / 24.0);
        canvas.draw_path(cmds, None, Some((col, stroke_w, true)));
        canvas.restore();
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_glass_well(
        canvas: &D2dCanvas,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        focused: bool,
        hovered: bool,
        has_error: bool,
        pal: &ThemePalette,
        alpha: f64,
    ) {
        canvas.fill_rounded_rect_solid(
            x,
            y,
            w,
            h,
            r,
            D2dColor::from_rgb(pal.capsule_bottom, lerp(0.74, 0.84, pal.is_light) * alpha),
        );
        let brd = if has_error {
            (0.95, 0.42, 0.44)
        } else if focused {
            pal.accent
        } else if hovered {
            pal.rim_top
        } else {
            pal.capsule_border
        };
        canvas.stroke_rounded_rect(
            x,
            y,
            w,
            h,
            r,
            if focused || has_error { 1.4 } else { 1.0 },
            D2dColor::from_rgb(
                brd,
                if focused || has_error {
                    0.90
                } else if hovered {
                    0.46
                } else {
                    0.26
                } * alpha,
            ),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_secondary_button(
        st: &mut WizardState,
        id: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        label: &str,
        glyph: Option<UiGlyph>,
        action: ClickAction,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        let hovered = st.hover_id == Some(id);
        let pressed = st
            .pressed_action
            .as_ref()
            .is_some_and(|(pid, _)| *pid == id);
        let sc = if pressed { 0.96 } else { 1.0 };

        st.canvas.save();
        st.canvas.translate(x + w * 0.5, y + h * 0.5);
        st.canvas.scale(sc, sc);
        let bx = -w * 0.5;
        let by = -h * 0.5;
        st.canvas.fill_rounded_rect_solid(
            bx,
            by,
            w,
            h,
            10.0,
            D2dColor::from_rgb(
                pal.capsule_top,
                if hovered { 0.88 } else { 0.66 } * alpha,
            ),
        );
        st.canvas.stroke_rounded_rect(
            bx,
            by,
            w,
            h,
            10.0,
            1.0,
            D2dColor::from_rgb(
                if hovered { pal.rim_top } else { pal.capsule_border },
                if hovered { 0.52 } else { 0.28 } * alpha,
            ),
        );
        if let Some(g) = glyph {
            draw_glyph(
                &st.canvas,
                g,
                bx + 18.0,
                0.0,
                13.5,
                D2dColor::from_rgb(pal.text_primary, 0.88 * alpha),
                1.7,
            );
            st.canvas.draw_text(
                label,
                FONT_SECONDARY,
                true,
                bx + 28.0,
                by,
                w - 34.0,
                h,
                1,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, alpha),
                None,
            );
        } else {
            st.canvas.draw_text(
                label,
                FONT_SECONDARY,
                true,
                bx + 8.0,
                by,
                w - 16.0,
                h,
                1,
                3,
                1,
                D2dColor::from_rgb(pal.text_primary, alpha),
                None,
            );
        }
        st.canvas.restore();

        if interactive {
            st.hits.push(HitRegion {
                id,
                x,
                y,
                w,
                h,
                action,
                cursor: CursorKind::Hand,
                hover_submenu: None,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_toggle_card(
        st: &mut WizardState,
        id: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        title: &str,
        desc: &str,
        anim: f64,
        action: ClickAction,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        let hovered = st.hover_id == Some(id);
        let fill = lerp_rgb(pal.capsule_bottom, pal.accent, 0.06 * anim + if hovered { 0.04 } else { 0.0 });
        st.canvas.fill_rounded_rect_solid(
            x,
            y,
            w,
            h,
            12.0,
            D2dColor::from_rgb(fill, lerp(0.56, 0.70, pal.is_light) * alpha),
        );
        st.canvas.stroke_rounded_rect(
            x,
            y,
            w,
            h,
            12.0,
            1.0,
            D2dColor::from_rgb(
                if hovered { pal.rim_top } else { pal.capsule_border },
                if hovered { 0.44 } else { 0.24 } * alpha,
            ),
        );

        st.canvas.draw_text(
            title,
            FONT_BODY,
            true,
            x + 16.0,
            y + 9.0,
            w - 86.0,
            18.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        st.canvas.draw_text(
            desc,
            FONT_CAPTION,
            false,
            x + 16.0,
            y + 28.0,
            w - 86.0,
            17.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_muted, 0.88 * alpha),
            None,
        );

        draw_pill_switch(
            &st.canvas,
            x + w - 54.0,
            y + (h - 22.0) * 0.5,
            anim,
            pal.accent,
            pal,
            alpha,
        );

        if interactive {
            st.hits.push(HitRegion {
                id,
                x,
                y,
                w,
                h,
                action,
                cursor: CursorKind::Hand,
                hover_submenu: None,
            });
        }
    }

    fn draw_pill_switch(
        canvas: &D2dCanvas,
        x: f64,
        y: f64,
        anim: f64,
        accent: Rgb,
        pal: &ThemePalette,
        alpha: f64,
    ) {
        let w = 40.0;
        let h = 22.0;
        let t = anim.clamp(0.0, 1.0);
        let off_bg = lerp_rgb(pal.capsule_bottom, pal.rim_top, 0.14);
        let track = lerp_rgb(off_bg, accent, t);
        canvas.fill_rounded_rect_solid(
            x,
            y,
            w,
            h,
            h * 0.5,
            D2dColor::from_rgb(track, lerp(0.55, 0.94, t) * alpha),
        );
        canvas.stroke_rounded_rect(
            x,
            y,
            w,
            h,
            h * 0.5,
            1.0,
            D2dColor::from_rgb(
                lerp_rgb(pal.capsule_border, pal.rim_top, t),
                lerp(0.32, 0.58, t) * alpha,
            ),
        );
        let knob_r = 8.2;
        let kx = x + 11.0 + t * (w - 22.0);
        let ky = y + h * 0.5;
        canvas.fill_circle(
            kx,
            ky + 1.0,
            knob_r,
            D2dColor::from_rgb(pal.shadow, 0.28 * alpha),
        );
        canvas.fill_circle(
            kx,
            ky,
            knob_r,
            D2dColor::from_rgb((1.0, 0.99, 0.98), 0.98 * alpha),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_segmented_control(
        st: &mut WizardState,
        base_id: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        left_label: &str,
        right_label: &str,
        pos: f64,
        left_act: ClickAction,
        right_act: ClickAction,
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        let r = 10.0;
        st.canvas.fill_rounded_rect_solid(
            x,
            y,
            w,
            h,
            r,
            D2dColor::from_rgb(pal.capsule_bottom, lerp(0.72, 0.80, pal.is_light) * alpha),
        );
        st.canvas.stroke_rounded_rect(
            x,
            y,
            w,
            h,
            r,
            1.0,
            D2dColor::from_rgb(pal.capsule_border, 0.26 * alpha),
        );

        let inset = 3.0;
        let seg_w = (w - inset * 2.0) * 0.5;
        let seg_h = h - inset * 2.0;
        let pill_x = x + inset + pos.clamp(0.0, 1.0) * seg_w;
        let pill_y = y + inset;

        st.canvas.fill_rounded_rect_solid(
            pill_x,
            pill_y,
            seg_w,
            seg_h,
            r - 2.5,
            D2dColor::from_rgb(pal.accent, 0.20 * alpha),
        );
        st.canvas.stroke_rounded_rect(
            pill_x,
            pill_y,
            seg_w,
            seg_h,
            r - 2.5,
            1.0,
            D2dColor::from_rgb(pal.accent, 0.62 * alpha),
        );

        let left_col = lerp_rgb(pal.text_primary, pal.text_muted, pos.clamp(0.0, 1.0));
        let right_col = lerp_rgb(pal.text_muted, pal.text_primary, pos.clamp(0.0, 1.0));
        st.canvas.draw_text(
            left_label,
            FONT_SECONDARY,
            pos < 0.5,
            x + inset,
            y,
            seg_w,
            h,
            1,
            3,
            1,
            D2dColor::from_rgb(left_col, alpha),
            None,
        );
        st.canvas.draw_text(
            right_label,
            FONT_SECONDARY,
            pos >= 0.5,
            x + inset + seg_w,
            y,
            seg_w,
            h,
            1,
            3,
            1,
            D2dColor::from_rgb(right_col, alpha),
            None,
        );

        if interactive {
            st.hits.push(HitRegion {
                id: base_id,
                x,
                y,
                w: w * 0.5,
                h,
                action: left_act,
                cursor: CursorKind::Hand,
                hover_submenu: None,
            });
            st.hits.push(HitRegion {
                id: base_id + 1,
                x: x + w * 0.5,
                y,
                w: w * 0.5,
                h,
                action: right_act,
                cursor: CursorKind::Hand,
                hover_submenu: None,
            });
        }
    }

    fn update_mode_index(mode: UpdateMode) -> f64 {
        match mode {
            UpdateMode::AutoInstall => 0.0,
            UpdateMode::Notify => 1.0,
            UpdateMode::Manual => 2.0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_segmented_control_3(
        st: &mut WizardState,
        base_id: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        labels: [&str; 3],
        pos: f64,
        actions: [ClickAction; 3],
        pal: &ThemePalette,
        alpha: f64,
        interactive: bool,
    ) {
        let r = 10.0;
        st.canvas.fill_rounded_rect_solid(
            x,
            y,
            w,
            h,
            r,
            D2dColor::from_rgb(pal.capsule_bottom, lerp(0.72, 0.80, pal.is_light) * alpha),
        );
        st.canvas.stroke_rounded_rect(
            x,
            y,
            w,
            h,
            r,
            1.0,
            D2dColor::from_rgb(pal.capsule_border, 0.26 * alpha),
        );

        let inset = 3.0;
        let seg_w = (w - inset * 2.0) / 3.0;
        let seg_h = h - inset * 2.0;
        let clamped = pos.clamp(0.0, 2.0);
        let pill_x = x + inset + clamped * seg_w;
        let pill_y = y + inset;

        st.canvas.fill_rounded_rect_solid(
            pill_x,
            pill_y,
            seg_w,
            seg_h,
            r - 2.5,
            D2dColor::from_rgb(pal.accent, 0.20 * alpha),
        );
        st.canvas.stroke_rounded_rect(
            pill_x,
            pill_y,
            seg_w,
            seg_h,
            r - 2.5,
            1.0,
            D2dColor::from_rgb(pal.accent, 0.62 * alpha),
        );

        for i in 0..3 {
            let dist = (clamped - i as f64).abs().min(1.0);
            let col = lerp_rgb(pal.text_primary, pal.text_muted, dist);
            let sx = x + inset + i as f64 * seg_w;
            st.canvas.draw_text(
                labels[i],
                FONT_SECONDARY,
                dist < 0.5,
                sx,
                y,
                seg_w,
                h,
                1,
                3,
                1,
                D2dColor::from_rgb(col, alpha),
                None,
            );
            if interactive {
                st.hits.push(HitRegion {
                    id: base_id + i as u64,
                    x: x + i as f64 * (w / 3.0),
                    y,
                    w: w / 3.0,
                    h,
                    action: actions[i].clone(),
                    cursor: CursorKind::Hand,
                    hover_submenu: None,
                });
            }
        }
    }

    // -----------------------------------------------------------------------
    // Frameless 3D Rotating Ring + Progress Bar "Updating" Overlay
    // -----------------------------------------------------------------------

    const UPD_W: f64 = 400.0;
    const UPD_H: f64 = 356.0;

    struct UpdaterShared {
        progress: f32,
        status: String,
        done: Option<Result<(), String>>,
    }

    struct UpdaterOverlayState {
        canvas: D2dCanvas,
        pacer: VsyncPacer,
        scale: f64,
        palette: ThemePalette,
        blur_pct: u8,
        translucency_pct: u8,
        high_contrast: bool,
        shared: Arc<Mutex<UpdaterShared>>,
        progress_spring: AppleSpring,
        open_alpha: AppleSpring,
        closing: bool,
        finished_at: Option<Instant>,
        tumble_time: f64,
        last_tick: Instant,
        version_subtitle: String,
        preview_mode: bool,
        exit_code: i32,
    }

    thread_local! {
        static UPD_STATE: RefCell<Option<UpdaterOverlayState>> = const { RefCell::new(None) };
    }

    fn load_theme_for_updater(config_dir: &std::path::Path) -> (ThemePalette, u8, u8, bool) {
        let store = recircles_core::config::ConfigStore::in_dir(config_dir.to_path_buf());
        let detected = recircles_core::modules::InstalledModules::detect_default();
        if let Ok(cfg) = store.load_or_create(&detected) {
            let colors = cfg.effective_colors(&store);
            return (
                ThemePalette::from_colors(&colors),
                cfg.appearance.blur,
                cfg.appearance.translucency,
                cfg.appearance.high_contrast,
            );
        }
        let def = builtin_theme("default").map(|b| b.colors).unwrap_or_default();
        (ThemePalette::from_colors(&def), 20, 60, false)
    }

    pub fn run_updater_preview(
        config_dir: PathBuf,
        progress: f32,
        from_version: Option<String>,
        to_version: Option<String>,
    ) -> i32 {
        let shared = Arc::new(Mutex::new(UpdaterShared {
            progress: progress.clamp(0.0, 1.0),
            status: "Updating core binaries…".into(),
            done: None,
        }));
        let subtitle = match (from_version, to_version) {
            (Some(f), Some(t)) => format!("v{f}  →  v{t}"),
            (None, Some(t)) => format!("v{t}"),
            _ => format!("v{}", env!("CARGO_PKG_VERSION")),
        };
        run_updater_window(config_dir, shared, subtitle, true)
    }

    pub fn run_updater_overlay(
        opts: UpgradeOptions,
        from_version: Option<String>,
        to_version: Option<String>,
    ) -> i32 {
        let config_dir = opts.config_dir.clone();
        let target_ver = to_version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
        let subtitle = match from_version {
            Some(f) if !f.trim().is_empty() && f.trim() != target_ver.trim() => {
                format!("v{}  →  v{}", f.trim(), target_ver.trim())
            }
            _ => format!("v{}", target_ver.trim()),
        };
        let shared = Arc::new(Mutex::new(UpdaterShared {
            progress: 0.06,
            status: "Preparing update…".into(),
            done: None,
        }));
        let shared_bg = shared.clone();
        std::thread::spawn(move || {
            let res = execute_upgrade(&opts, None, |prog: UpgradeProgress| {
                if let Ok(mut g) = shared_bg.lock() {
                    g.progress = prog.fraction.clamp(0.0, 1.0);
                    g.status = prog.status.to_string();
                }
            });
            if let Ok(mut g) = shared_bg.lock() {
                match res {
                    Ok(_) => {
                        g.progress = 1.0;
                        g.status = "Updated!".into();
                        g.done = Some(Ok(()));
                    }
                    Err(e) => {
                        g.status = format!("Update failed: {e}");
                        g.done = Some(Err(e));
                    }
                }
            }
        });
        run_updater_window(config_dir, shared, subtitle, false)
    }

    fn run_updater_window(
        config_dir: PathBuf,
        shared: Arc<Mutex<UpdaterShared>>,
        version_subtitle: String,
        preview_mode: bool,
    ) -> i32 {
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let (palette, blur_pct, translucency_pct, high_contrast) =
            load_theme_for_updater(&config_dir);

        let hinst = unsafe { GetModuleHandleW(ptr::null()) };
        let cls = to_wide_nul("ReCirclesUpdaterRingOverlay");
        let title = to_wide_nul("Updating ReCircles");
        let (app_icon, app_icon_sm) = recircles_core::app_icon::win::load_or_create_app_icons();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(updater_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: app_icon,
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: app_icon_sm,
        };
        unsafe { RegisterClassExW(&wc) };

        let mut pt = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut pt) };
        let hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        unsafe { GetMonitorInfoW(hmon, &mut mi) };
        let work = mi.rcWork;
        let mon = mi.rcMonitor;

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                cls.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                work.left,
                work.top,
                UPD_W as i32,
                UPD_H as i32,
                ptr::null_mut(),
                ptr::null_mut(),
                hinst,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return 1;
        }

        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f64;
        let scale = (dpi / 96.0).clamp(1.0, 4.0);
        let phys_w = (UPD_W * scale).round() as i32;
        let phys_h = (UPD_H * scale).round() as i32;
        let cx = work.left + ((work.right - work.left) - phys_w) / 2;
        let cy = work.top + ((work.bottom - work.top) - phys_h) / 2;
        unsafe {
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                cx,
                cy,
                phys_w,
                phys_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }

        let Some(mut canvas) = D2dCanvas::new(hwnd, phys_w, phys_h, scale) else {
            unsafe { DestroyWindow(hwnd) };
            return 1;
        };

        let mon_w = (mon.right - mon.left).max(1);
        let mon_h = (mon.bottom - mon.top).max(1);
        let blur_t = f64::from(blur_pct) / 100.0;
        let blur_sigma = ((0.35 + blur_t.powf(1.35) * 37.65) * scale).clamp(0.25, 64.0);
        let blur_sat = 1.0 + 0.24 * blur_t;
        if canvas.capture_backdrop(mon.left, mon.top, mon_w, mon_h, blur_sigma) {
            let _ = canvas.set_backdrop_blur(blur_sigma, blur_sat);
            canvas.set_backdrop_origin((cx - mon.left) as f64, (cy - mon.top) as f64);
        }

        let init_prog = shared.lock().map(|g| g.progress as f64).unwrap_or(0.0);
        let pacer = VsyncPacer::start(hwnd);
        pacer.set_animating(true);

        let mut st = UpdaterOverlayState {
            canvas,
            pacer,
            scale,
            palette,
            blur_pct,
            translucency_pct,
            high_contrast,
            shared,
            progress_spring: AppleSpring::new(if preview_mode { init_prog } else { 0.0 }, 1.0, 0.28),
            open_alpha: AppleSpring::new(if preview_mode { 1.0 } else { 0.05 }, 1.0, 0.22),
            closing: false,
            finished_at: None,
            tumble_time: if preview_mode { 0.72 } else { 0.0 },
            last_tick: Instant::now(),
            version_subtitle,
            preview_mode,
            exit_code: 0,
        };
        st.open_alpha.set_target(1.0);
        st.progress_spring.set_target(init_prog);
        paint_updater_overlay(&mut st);

        unsafe {
            ShowWindow(hwnd, SW_SHOWNORMAL);
        }

        UPD_STATE.with(|slot| *slot.borrow_mut() = Some(st));

        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        UPD_STATE.with(|slot| slot.borrow_mut().take().map(|s| s.exit_code).unwrap_or(0))
    }

    fn draw_3d_ring_at(
        canvas: &D2dCanvas,
        pal: &ThemePalette,
        cx: f64,
        cy: f64,
        outer_r: f64,
        inner_r: f64,
        half_thickness: f64,
        tumble_time: f64,
        alpha: f64,
    ) {
        let rot_x = (tumble_time * 2.1).sin() * 0.50;
        let rot_y = (tumble_time * 1.65 + 0.5).cos() * 0.54;
        let rot_z = (tumble_time * 1.45) % TAU;
        let (sx, cx_r) = rot_x.sin_cos();
        let (sy, cy_r) = rot_y.sin_cos();
        let (sz, cz) = rot_z.sin_cos();
        let r = [
            [cz * cy_r, cz * sy * sx - sz * cx_r, cz * sy * cx_r + sz * sx],
            [sz * cy_r, sz * sy * sx + cz * cx_r, sz * sy * cx_r - cz * sx],
            [-sy, cy_r * sx, cy_r * cx_r],
        ];

        #[derive(Clone, Copy)]
        struct Vec3(f64, f64, f64);
        impl Vec3 {
            fn dot(self, o: Self) -> f64 {
                self.0 * o.0 + self.1 * o.1 + self.2 * o.2
            }
            fn normalize(self) -> Self {
                let l = self.dot(self).sqrt();
                if l > 0.0 {
                    Self(self.0 / l, self.1 / l, self.2 / l)
                } else {
                    Self(0.0, 0.0, 1.0)
                }
            }
        }

        let rot = |v: Vec3| {
            Vec3(
                r[0][0] * v.0 + r[0][1] * v.1 + r[0][2] * v.2,
                r[1][0] * v.0 + r[1][1] * v.1 + r[1][2] * v.2,
                r[2][0] * v.0 + r[2][1] * v.1 + r[2][2] * v.2,
            )
        };
        let proj = |p: Vec3| {
            let w = rot(p);
            let persp = PERSPECTIVE_D / (PERSPECTIVE_D - w.2);
            (w.0 * persp, w.1 * persp, w.2)
        };
        let cam = Vec3(
            r[2][0] * PERSPECTIVE_D,
            r[2][1] * PERSPECTIVE_D,
            r[2][2] * PERSPECTIVE_D,
        );

        let mut outer_f = [(0.0, 0.0, 0.0); RING_SEGMENTS];
        let mut outer_b = [(0.0, 0.0, 0.0); RING_SEGMENTS];
        let mut inner_f = [(0.0, 0.0, 0.0); RING_SEGMENTS];
        let mut inner_b = [(0.0, 0.0, 0.0); RING_SEGMENTS];
        for (i, &(ca, sa)) in RING_COS_SIN.iter().enumerate() {
            outer_f[i] = proj(Vec3(outer_r * ca, outer_r * sa, half_thickness));
            outer_b[i] = proj(Vec3(outer_r * ca, outer_r * sa, -half_thickness));
            inner_f[i] = proj(Vec3(inner_r * ca, inner_r * sa, half_thickness));
            inner_b[i] = proj(Vec3(inner_r * ca, inner_r * sa, -half_thickness));
        }

        let light = Vec3(-0.42, -0.72, 0.80).normalize();
        let half_vec = Vec3(light.0, light.1, light.2 + 1.0).normalize();

        let shade_outer = |ca: f64, sa: f64| -> Rgb {
            let n = rot(Vec3(ca, sa, 0.0));
            let diff = n.dot(light).max(0.0);
            let spec = n.dot(half_vec).max(0.0).powf(12.0);
            let base = lerp_rgb(pal.ring_bottom, pal.ring_top, 0.20 + 0.80 * diff);
            let lit = lerp_rgb(base, pal.rim_top, 0.28 * diff + 0.48 * spec);
            lerp_rgb(lit, pal.accent, 0.24 * diff + 0.46 * spec)
        };
        let shade_inner = |ca: f64, sa: f64| -> Rgb {
            let n = rot(Vec3(-ca, -sa, 0.0));
            let diff = n.dot(light).max(0.0);
            let spec = n.dot(half_vec).max(0.0).powf(10.0);
            let base = lerp_rgb(pal.ring_mid, pal.ring_top, 0.12 + 0.72 * diff);
            let lit = lerp_rgb(base, pal.rim_bottom, 0.20 * diff + 0.34 * spec);
            lerp_rgb(lit, pal.accent, 0.18 * diff + 0.34 * spec)
        };

        canvas.save();
        canvas.translate(cx, cy);

        #[derive(Clone, Copy)]
        struct WallQuad {
            z: f64,
            pts: [(f64, f64); 4],
            rgb: Rgb,
        }
        let mut quads = [WallQuad {
            z: 0.0,
            pts: [(0.0, 0.0); 4],
            rgb: (0.0, 0.0, 0.0),
        }; RING_SEGMENTS * 2];
        let mut quad_len = 0usize;
        for (i, &(ca, sa)) in RING_MID_COS_SIN.iter().enumerate() {
            let j = (i + 1) % RING_SEGMENTS;
            if cam.0 * ca + cam.1 * sa - outer_r > -4.0 {
                let (p0, p1, p2, p3) = (outer_f[i], outer_f[j], outer_b[j], outer_b[i]);
                quads[quad_len] = WallQuad {
                    z: 0.25 * (p0.2 + p1.2 + p2.2 + p3.2),
                    pts: [(p0.0, p0.1), (p1.0, p1.1), (p2.0, p2.1), (p3.0, p3.1)],
                    rgb: shade_outer(ca, sa),
                };
                quad_len += 1;
            }
            if -(cam.0 * ca + cam.1 * sa) + inner_r > -4.0 {
                let (p0, p1, p2, p3) = (inner_f[i], inner_f[j], inner_b[j], inner_b[i]);
                quads[quad_len] = WallQuad {
                    z: 0.25 * (p0.2 + p1.2 + p2.2 + p3.2),
                    pts: [(p0.0, p0.1), (p1.0, p1.1), (p2.0, p2.1), (p3.0, p3.1)],
                    rgb: shade_inner(ca, sa),
                };
                quad_len += 1;
            }
        }
        let active_quads = &mut quads[..quad_len];
        active_quads.sort_by(|a, b| a.z.total_cmp(&b.z));
        if !active_quads.is_empty() {
            canvas.push_opacity_layer(
                -outer_r - 20.0,
                -outer_r - 20.0,
                outer_r * 2.0 + 40.0,
                outer_r * 2.0 + 40.0,
                0.90 * alpha,
            );
            for q in active_quads {
                canvas.fill_polygon(&q.pts, D2dColor::from_rgb(q.rgb, 1.0));
            }
            canvas.pop_layer();
        }

        let front_pos = cam.2 >= 0.0;
        let src_outer = if front_pos { &outer_f } else { &outer_b };
        let src_inner = if front_pos { &inner_f } else { &inner_b };
        let mut face_outer = [(0.0, 0.0); RING_SEGMENTS];
        let mut face_inner = [(0.0, 0.0); RING_SEGMENTS];
        for i in 0..RING_SEGMENTS {
            face_outer[i] = (src_outer[i].0, src_outer[i].1);
            face_inner[i] = (src_inner[i].0, src_inner[i].1);
        }

        let (mut gy0, mut gy1) = (f64::INFINITY, f64::NEG_INFINITY);
        for &(_, y) in &face_outer {
            gy0 = gy0.min(y);
            gy1 = gy1.max(y);
        }
        let stops = [
            D2dGradStop {
                pos: 0.0,
                color: D2dColor::from_rgb(pal.ring_top, 0.88 * alpha),
            },
            D2dGradStop {
                pos: 0.5,
                color: D2dColor::from_rgb(pal.ring_mid, 0.82 * alpha),
            },
            D2dGradStop {
                pos: 1.0,
                color: D2dColor::from_rgb(pal.ring_bottom, 0.90 * alpha),
            },
        ];
        canvas.fill_polygon_ring_linear(&face_outer, &face_inner, 0.0, gy0, 0.0, gy1, &stops);
        canvas.stroke_polygon_ring_linear(
            &face_outer,
            &face_inner,
            1.15,
            0.0,
            gy0,
            0.0,
            gy1,
            D2dColor::from_rgb(pal.rim_top, 0.78 * alpha),
            D2dColor::from_rgb(pal.rim_bottom, 0.34 * alpha),
        );
        canvas.stroke_polygon(&face_outer, 1.7, D2dColor::from_rgb(pal.accent, 0.62 * alpha));
        canvas.restore();
    }

    fn paint_updater_overlay(st: &mut UpdaterOverlayState) {
        let alpha = st.open_alpha.value.clamp(0.0, 1.0);
        let pal = st.palette;
        let (status_text, is_err) = st
            .shared
            .lock()
            .map(|g| (g.status.clone(), matches!(g.done, Some(Err(_)))))
            .unwrap_or_else(|_| ("Updating…".into(), false));
        let prog = st.progress_spring.value.clamp(0.0, 1.0);
        let pct = (prog * 100.0).round() as u32;

        st.canvas.begin_frame(st.scale);

        // 1. Frameless 3D rotating glass ring floating cleanly in the upper center
        draw_3d_ring_at(
            &st.canvas,
            &pal,
            UPD_W * 0.5,
            134.0,
            92.0,
            38.0,
            12.5,
            st.tumble_time,
            alpha,
        );

        // 2. Compact frosted glass pill directly underneath: "Updating" + progress bar
        let card_w = 328.0;
        let card_h = 74.0;
        let card_x = (UPD_W - card_w) * 0.5;
        let card_y = 258.0;
        let card_r = 18.0;

        st.canvas.fill_rounded_rect_shadow(
            card_x,
            card_y,
            card_w,
            card_h,
            card_r,
            D2dColor::from_rgb(pal.shadow, 1.0),
            alpha * 0.85,
        );

        let uses_backdrop =
            st.canvas.has_backdrop() && !st.high_contrast && st.translucency_pct > 0 && st.blur_pct > 0;
        if uses_backdrop {
            st.canvas
                .fill_rounded_rect_backdrop(card_x, card_y, card_w, card_h, card_r, 0.0, alpha);
        }

        let panel_a = if st.high_contrast {
            0.96
        } else {
            (1.0 - f64::from(st.translucency_pct) / 100.0 * 0.55).clamp(0.62, 0.94)
        };
        let card_stops = [
            D2dGradStop {
                pos: 0.0,
                color: D2dColor::from_rgb(pal.ring_top, panel_a * alpha),
            },
            D2dGradStop {
                pos: 1.0,
                color: D2dColor::from_rgb(pal.ring_bottom, (panel_a + 0.05).min(1.0) * alpha),
            },
        ];
        st.canvas.fill_rounded_rect_linear(
            card_x,
            card_y,
            card_w,
            card_h,
            card_r,
            0.0,
            card_y,
            0.0,
            card_y + card_h,
            &card_stops,
        );
        st.canvas.stroke_rounded_rect(
            card_x,
            card_y,
            card_w,
            card_h,
            card_r,
            1.0,
            D2dColor::from_rgb(
                if is_err { (0.95, 0.42, 0.44) } else { pal.rim_top },
                0.42 * alpha,
            ),
        );

        let pad_x = 18.0;
        let inner_w = card_w - pad_x * 2.0;

        // Title row: "Updating" on left, percentage on right
        let heading = if is_err {
            "Update Failed"
        } else if pct >= 100 {
            "Updated"
        } else {
            "Updating"
        };
        st.canvas.draw_text(
            heading,
            FONT_SUBHEAD,
            true,
            card_x + pad_x,
            card_y + 11.0,
            inner_w - 46.0,
            20.0,
            0,
            3,
            1,
            D2dColor::from_rgb(pal.text_primary, alpha),
            None,
        );
        let pct_str = format!("{pct}%");
        st.canvas.draw_text(
            &pct_str,
            FONT_CAPTION,
            true,
            card_x + card_w - pad_x - 46.0,
            card_y + 12.0,
            46.0,
            18.0,
            2,
            3,
            1,
            D2dColor::from_rgb(pal.accent, 0.96 * alpha),
            None,
        );

        // Progress bar track + glowing accent fill
        let bar_x = card_x + pad_x;
        let bar_y = card_y + 37.0;
        let bar_w = inner_w;
        let bar_h = 7.0;
        let bar_r = bar_h * 0.5;
        st.canvas.fill_rounded_rect_solid(
            bar_x,
            bar_y,
            bar_w,
            bar_h,
            bar_r,
            D2dColor::from_rgb(pal.capsule_border, 0.24 * alpha),
        );
        let fill_w = (bar_w * prog).clamp(if prog > 0.0 { bar_h } else { 0.0 }, bar_w);
        if fill_w > 0.5 {
            let accent_col = if is_err { (0.95, 0.42, 0.44) } else { pal.accent };
            let hi = lerp_rgb(accent_col, (1.0, 1.0, 1.0), 0.24);
            let bar_stops = [
                D2dGradStop {
                    pos: 0.0,
                    color: D2dColor::from_rgb(accent_col, 0.96 * alpha),
                },
                D2dGradStop {
                    pos: 1.0,
                    color: D2dColor::from_rgb(hi, 0.98 * alpha),
                },
            ];
            st.canvas.fill_rounded_rect_linear(
                bar_x,
                bar_y,
                fill_w,
                bar_h,
                bar_r,
                bar_x,
                0.0,
                bar_x + fill_w,
                0.0,
                &bar_stops,
            );
        }

        // Subtitle line: active phase on left, version transition on right
        if is_err || st.version_subtitle.is_empty() {
            st.canvas.draw_text(
                &status_text,
                FONT_CAPTION,
                false,
                card_x + pad_x,
                card_y + 50.0,
                inner_w,
                16.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_muted, 0.86 * alpha),
                None,
            );
        } else {
            let left_w = inner_w * 0.52;
            let right_w = inner_w - left_w - 6.0;
            st.canvas.draw_text(
                &status_text,
                FONT_CAPTION,
                false,
                card_x + pad_x,
                card_y + 50.0,
                left_w,
                16.0,
                0,
                3,
                1,
                D2dColor::from_rgb(pal.text_muted, 0.86 * alpha),
                None,
            );
            st.canvas.draw_text(
                &st.version_subtitle,
                FONT_CAPTION,
                false,
                card_x + pad_x + left_w + 6.0,
                card_y + 50.0,
                right_w,
                16.0,
                2,
                3,
                1,
                D2dColor::from_rgb(pal.text_muted, 0.78 * alpha),
                None,
            );
        }

        let _ = st.canvas.end_frame(0);
    }

    unsafe extern "system" fn updater_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_ERASEBKGND => 1,
            WM_APP_TICK => {
                let mut should_destroy = false;
                UPD_STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        st.pacer.ack_tick();
                        let now = Instant::now();
                        let dt = (now - st.last_tick).as_secs_f64().clamp(0.0, 0.05);
                        st.last_tick = now;
                        st.tumble_time += dt;

                        let (target_prog, done) = st
                            .shared
                            .lock()
                            .map(|g| (g.progress as f64, g.done.clone()))
                            .unwrap_or((0.0, None));
                        st.progress_spring.set_target(target_prog);
                        st.progress_spring.step(dt, false);
                        st.open_alpha.step(dt, false);

                        if !st.preview_mode
                            && let Some(res) = done
                        {
                            if res.is_err() {
                                st.exit_code = 1;
                            }
                            let hold_ms = if res.is_ok() { 550 } else { 2200 };
                            let started = *st.finished_at.get_or_insert(now);
                            if !st.closing
                                && started.elapsed() >= std::time::Duration::from_millis(hold_ms)
                            {
                                st.closing = true;
                                st.open_alpha.set_target(0.0);
                            }
                        }

                        if st.closing && st.open_alpha.value <= 0.02 {
                            st.pacer.set_animating(false);
                            should_destroy = true;
                        } else {
                            paint_updater_overlay(st);
                        }
                    }
                });
                if should_destroy {
                    unsafe { DestroyWindow(hwnd) };
                }
                0
            }
            WM_KEYDOWN => {
                let vk = wparam as u32;
                let preview = UPD_STATE.with(|slot| {
                    slot.borrow().as_ref().map(|s| s.preview_mode).unwrap_or(false)
                });
                if preview && vk == VK_ESCAPE as u32 {
                    unsafe { DestroyWindow(hwnd) };
                }
                0
            }
            WM_LBUTTONUP => {
                let preview = UPD_STATE.with(|slot| {
                    slot.borrow().as_ref().map(|s| s.preview_mode).unwrap_or(false)
                });
                if preview {
                    unsafe { DestroyWindow(hwnd) };
                }
                0
            }
            WM_DESTROY => {
                UPD_STATE.with(|slot| {
                    if let Some(st) = slot.borrow_mut().as_mut() {
                        st.pacer.set_animating(false);
                    }
                });
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    fn draw_summary_card(
        st: &WizardState,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        pal: &ThemePalette,
        alpha: f64,
    ) {
        st.canvas.fill_rounded_rect_solid(
            x,
            y,
            w,
            h,
            14.0,
            D2dColor::from_rgb(pal.capsule_bottom, lerp(0.56, 0.70, pal.is_light) * alpha),
        );
        st.canvas.stroke_rounded_rect(
            x,
            y,
            w,
            h,
            14.0,
            1.0,
            D2dColor::from_rgb(pal.capsule_border, 0.24 * alpha),
        );
    }

    fn draw_soft_divider(canvas: &D2dCanvas, x: f64, y: f64, w: f64, rgb: Rgb, alpha: f64) {
        canvas.draw_line(
            x,
            y,
            x + w,
            y,
            1.0,
            D2dColor::from_rgb(rgb, alpha),
            false,
        );
    }

    // ==================== WIN32 HELPERS ====================

    fn key_to_char(vk: u32, lparam: isize) -> Option<char> {
        let mut state = [0u8; 256];
        if unsafe { GetKeyboardState(state.as_mut_ptr()) } == 0 {
            return None;
        }
        let ctrl_down = (state[VK_CONTROL as usize] & 0x80) != 0;
        let alt_down = (state[VK_MENU as usize] & 0x80) != 0;
        if ctrl_down && !alt_down {
            return None;
        }
        let scan = ((lparam >> 16) & 0xFF) as u32;
        let mut buf = [0u16; 4];
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
                    while *ptr.add(len) != 0 && len < 4096 {
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

    fn pick_folder_dialog(hwnd: HWND, title: &str) -> Option<PathBuf> {
        #[repr(C)]
        #[allow(non_snake_case)]
        struct BrowseInfoW {
            hwndOwner: HWND,
            pidlRoot: *const core::ffi::c_void,
            pszDisplayName: *mut u16,
            lpszTitle: *const u16,
            ulFlags: u32,
            lpfn: *const core::ffi::c_void,
            lParam: isize,
            iImage: i32,
        }
        #[link(name = "shell32", kind = "raw-dylib")]
        unsafe extern "system" {
            fn SHBrowseForFolderW(lpbi: *mut BrowseInfoW) -> *mut core::ffi::c_void;
            fn SHGetPathFromIDListW(pidl: *const core::ffi::c_void, psz_path: *mut u16) -> i32;
        }
        const BIF_RETURNONLYFSDIRS: u32 = 0x0000_0001;
        const BIF_EDITBOX: u32 = 0x0000_0010;
        const BIF_NEWDIALOGSTYLE: u32 = 0x0000_0040;

        let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        let com_inited = hr >= 0;
        let title_w = to_wide_nul(title);
        let mut disp_buf = [0u16; 260];
        let mut bi = BrowseInfoW {
            hwndOwner: hwnd,
            pidlRoot: ptr::null(),
            pszDisplayName: disp_buf.as_mut_ptr(),
            lpszTitle: title_w.as_ptr(),
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
            lpfn: ptr::null(),
            lParam: 0,
            iImage: 0,
        };
        let pidl = unsafe { SHBrowseForFolderW(&mut bi) };
        let result = if !pidl.is_null() {
            let mut path_buf = [0u16; 4096];
            let ok = unsafe { SHGetPathFromIDListW(pidl, path_buf.as_mut_ptr()) };
            unsafe { CoTaskMemFree(pidl) };
            if ok != 0 {
                let end = path_buf
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(path_buf.len());
                if end > 0 {
                    Some(PathBuf::from(OsString::from_wide(&path_buf[..end])))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        if com_inited {
            unsafe { CoUninitialize() };
        }
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetClientRect, PM_REMOVE, PeekMessageW, SW_RESTORE,
        };

        #[test]
        fn pacer_and_timer_gating_rules() {
            // Editing dir & capturing hotkey must NOT keep VsyncPacer spinning
            assert!(!should_keep_awake(false, false, false, false));
            assert!(should_keep_awake(true, false, false, false));
            assert!(should_keep_awake(false, true, false, false));
            // Minimized or closing always pauses pacer even during install/anim
            assert!(!should_keep_awake(true, true, true, false));
            assert!(!should_keep_awake(true, true, false, true));

            // Caret/pulse timers run only when focused, unminimized, and not closing
            assert_eq!(compute_timer_flags(true, false, true, false, false), (true, false));
            assert_eq!(compute_timer_flags(false, true, true, false, false), (false, true));
            assert_eq!(compute_timer_flags(true, true, false, false, false), (false, false));
            assert_eq!(compute_timer_flags(true, true, true, true, false), (false, false));
            assert_eq!(compute_timer_flags(true, true, true, false, true), (false, false));
        }

        #[test]
        fn non_reentrant_click_minimize_restore_and_close_lifecycle() {
            let hinst = unsafe { GetModuleHandleW(ptr::null()) };
            let cls = to_wide_nul("ReCirclesInstallerTestWnd");
            let title = to_wide_nul("ReCircles Test");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: ptr::null_mut(),
                hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
                hbrBackground: ptr::null_mut(),
                lpszMenuName: ptr::null(),
                lpszClassName: cls.as_ptr(),
                hIconSm: ptr::null_mut(),
            };
            unsafe { RegisterClassExW(&wc) };

            let phys_w = WIN_W as i32;
            let phys_h = WIN_H as i32;
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_NOREDIRECTIONBITMAP,
                    cls.as_ptr(),
                    title.as_ptr(),
                    WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU,
                    40,
                    40,
                    phys_w,
                    phys_h,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    hinst,
                    ptr::null(),
                )
            };
            assert!(!hwnd.is_null());

            // Verify WM_NCCALCSIZE preserves zero non-client border even with WS_MINIMIZEBOX | WS_SYSMENU
            let mut wr: RECT = unsafe { std::mem::zeroed() };
            let mut cr: RECT = unsafe { std::mem::zeroed() };
            unsafe {
                GetWindowRect(hwnd, &mut wr);
                GetClientRect(hwnd, &mut cr);
            }
            assert_eq!(wr.right - wr.left, cr.right - cr.left);
            assert_eq!(wr.bottom - wr.top, cr.bottom - cr.top);

            let canvas = D2dCanvas::new(hwnd, phys_w, phys_h, 1.0).expect("d2d canvas");
            let opts = InstallOptions::default();
            let themes = builtin_themes();
            let theme_palettes: Vec<ThemePalette> = themes
                .iter()
                .map(|bt| ThemePalette::from_colors(&bt.colors))
                .collect();
            assert_eq!(themes.len(), theme_palettes.len());
            let initial_palette = theme_palettes[0];

            let mut st = WizardState {
                hwnd,
                canvas,
                pacer: VsyncPacer::start(hwnd),
                scale: 1.0,
                reduced_motion: true,
                last_tick: Instant::now(),
                clock: 0.0,
                minimized: false,
                focused: true,
                closing: false,
                has_capture: false,
                last_animating: false,
                recreating: false,
                caret_visible: true,
                caret_timer_on: false,
                pulse_timer_on: false,
                capture_hook: None,
                last_mouse_phys: (-1, -1),
                backdrop_mon_origin: (0, 0),
                last_blur_pct: opts.appearance.blur,
                page: PAGE_SETUP,
                prev_page: PAGE_SETUP,
                page_dir: 1.0,
                toggle_startup: AppleSpring::new(1.0, 0.85, 0.26),
                toggle_shortcuts: AppleSpring::new(1.0, 0.85, 0.26),
                toggle_daemon: AppleSpring::new(1.0, 0.85, 0.26),
                toggle_animations: AppleSpring::new(1.0, 0.85, 0.26),
                toggle_loading_3d: AppleSpring::new(1.0, 0.85, 0.26),
                toggle_high_contrast: AppleSpring::new(0.0, 0.85, 0.26),
                toggle_number_shortcuts: AppleSpring::new(1.0, 0.85, 0.26),
                nav_seg_x: AppleSpring::new(0.0, 0.92, 0.28),
                layout_seg_x: AppleSpring::new(0.0, 0.92, 0.28),
                update_seg_x: AppleSpring::new(0.0, 0.92, 0.28),
                opts,
                install_dir_str: "C:\\Temp\\ReCircles".into(),
                dir_error: None,
                editing_dir: false,
                existing_installed: false,
                confirm_uninstall_open: false,
                custom_menus: Vec::new(),
                capturing_hotkey: None,
                status_msg: None,
                installing: false,
                install_result: Arc::new(Mutex::new(None)),
                summary: None,
                mouse_lx: -1.0,
                mouse_ly: -1.0,
                hits: Vec::with_capacity(64),
                hover_id: None,
                pressed_action: None,
                pressed_ring_sub: None,
                drag_win: None,
                slider_drag: SliderDragKind::None,
                slider_grab_offset: 0.0,
                slider_rects: [(0.0, 0.0, 0.0, 0.0); 2],
                cursor_kind: CursorKind::Arrow,
                tracking_mouse: false,
                palette: PaletteTransition::new(initial_palette),
                ring: LiveRingMotion::new(&InstallOptions::default().selected_submenus, true),
                page_mix: AppleSpring::new(1.0, 1.0, 0.34),
                step_pill_y: AppleSpring::new(0.0, 1.0, 0.32),
                primary_press: AppleSpring::new(0.0, 0.82, 0.20),
                back_press: AppleSpring::new(0.0, 0.82, 0.20),
                submenu_toggles: std::array::from_fn(|_| AppleSpring::new(1.0, 0.85, 0.26)),
                status_spring: AppleSpring::new(0.0, 0.92, 0.28),
                keybind_scroll: AppleSpring::new(0.0, 0.96, 0.26),
                keybind_max_scroll: 0.0,
                themes,
                theme_palettes,
            };
            render_frame(&mut st);
            WIZ.with(|slot| *slot.borrow_mut() = Some(st));

            let lparam = |x: i32, y: i32| -> isize { ((y << 16) | (x & 0xFFFF)) as isize };

            // 1. Click Continue (820, 600) — exercises SetCapture + ReleaseCapture + WM_CAPTURECHANGED
            unsafe {
                wnd_proc(hwnd, WM_LBUTTONDOWN, 1, lparam(820, 600));
                wnd_proc(hwnd, WM_LBUTTONUP, 0, lparam(820, 600));
            }
            let page_after_next = with_wiz(|s| s.page).unwrap();
            assert_eq!(page_after_next, 1, "Continue should advance to page 1 without panic");

            // 2. Focus / click Minimize button (854, 48) -> ShowWindow(SW_MINIMIZE) -> WM_SIZE(SIZE_MINIMIZED)
            unsafe {
                wnd_proc(hwnd, WM_LBUTTONDOWN, 1, lparam(854, 48));
                wnd_proc(hwnd, WM_LBUTTONUP, 0, lparam(854, 48));
            }
            assert_ne!(unsafe { IsIconic(hwnd) }, 0, "Minimize click must iconify window");
            let min_state = with_wiz(|s| (s.minimized, s.has_capture, s.pressed_action.is_some())).unwrap();
            assert_eq!(min_state, (true, false, false));

            // 3. Restore window -> WM_SIZE(SIZE_RESTORED) -> clears minimized & stays responsive
            unsafe { ShowWindow(hwnd, SW_RESTORE) };
            assert_eq!(unsafe { IsIconic(hwnd) }, 0, "Restore must un-iconify window");
            let restored_min = with_wiz(|s| s.minimized).unwrap();
            assert!(!restored_min);

            // 4. Click Continue again after restore -> advances to page 2 (Themes)
            unsafe {
                wnd_proc(hwnd, WM_LBUTTONDOWN, 1, lparam(820, 600));
                wnd_proc(hwnd, WM_LBUTTONUP, 0, lparam(820, 600));
            }
            assert_eq!(with_wiz(|s| s.page).unwrap(), 2);

            // 5. Return to Page 0, simulate FocusInstallDir + timer tick + AlertDialog + CloseWindow
            handle_click(hwnd, ClickAction::GoPage(0));
            assert_eq!(with_wiz(|s| s.page).unwrap(), 0);
            handle_click(hwnd, ClickAction::FocusInstallDir);
            assert!(with_wiz(|s| s.caret_timer_on).unwrap());
            let c0 = with_wiz(|s| s.caret_visible).unwrap();
            unsafe { wnd_proc(hwnd, WM_TIMER, TIMER_CARET, 0) };
            let c1 = with_wiz(|s| s.caret_visible).unwrap();
            assert_ne!(c0, c1);

            handle_click(hwnd, ClickAction::UninstallExisting);
            assert!(with_wiz(|s| s.confirm_uninstall_open).unwrap());
            handle_click(hwnd, ClickAction::CancelUninstall);
            assert!(!with_wiz(|s| s.confirm_uninstall_open).unwrap());

            handle_click(hwnd, ClickAction::CloseWindow);
            WIZ.with(|slot| {
                if let Ok(mut g) = slot.try_borrow_mut() {
                    if let Some(s) = g.as_ref() {
                        assert!(s.closing);
                    }
                    drop(g.take());
                }
            });

            // Drain WM_QUIT posted by WM_DESTROY so test runner thread stays clean
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {}
        }
    }
}
