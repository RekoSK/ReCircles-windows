use recircles_core::config::{
    AppConfig, AppearanceConfig, ConfigStore, CustomMenuLayout, KeyList, LoadingStyle,
    MenuActionSpec, MenuConfig, MenuItemConfig, OverflowLayout, SubmenuTarget,
    expand_command_template, run_command_capture, spawn_command_detached,
};
use recircles_core::history::History;
use recircles_core::menu::{
    BuiltinIcon, Entry, MenuState, Mode, PAGE_SIZE,
    build_custom_menu_entries, build_main_menu_entries,
};
use recircles_core::modules::{BuiltinSubmenu, InstalledModules};
use recircles_core::settings::{
    self, AppearanceToggle, NavKeyField, SettingsAction, SettingsItem, SettingsView,
    apply_main_menu_move,
};
use recircles_core::themes::ColorsConfig;
use recircles_core::win_keys::{
    Modifiers, format_vk_parts, parse_global_hotkey, parse_key_spec,
    VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_LWIN, VK_NEXT, VK_PRIOR,
    VK_RETURN, VK_RIGHT, VK_RWIN, VK_TAB, VK_UP,
};
use recircles_core::submenu_abi::{ClipboardKind, SubmenuRequest, SubmenuResponse};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crate::layout::{
    ColorDrag, ColorPickerState, ExplorerBounds, InputBoxState, InputPanelKind, RingGeometry,
    SliderState, SurfaceMode,
};
use crate::motion_ring::RingMotion;

#[derive(Debug, Clone)]
pub enum CaptureTarget {
    Nav(NavKeyField),
    MenuHotkey {
        menu_id: Option<String>,
        label: String,
    },
}

#[derive(Debug, Clone)]
pub enum InputSubmitPurpose {
    CustomMenuInput {
        spec: MenuActionSpec,
        label: Option<String>,
        value: Option<String>,
    },
    ImportThemePath,
    ImportMenuPath,
}

#[derive(Debug, Clone)]
pub struct ActiveCustomMenu {
    pub id: String,
    pub config: MenuConfig,
}

pub enum AsyncJobResult {
    AppsLoaded {
        job_gen: u64,
        result: Result<Vec<recircles_core::submenu_abi::AppDto>, String>,
    },
    CustomMenuLoaded {
        job_gen: u64,
        stack_depth: usize,
        id: String,
        result: Result<Vec<MenuItemConfig>, String>,
    },
}

impl AsyncJobResult {
    fn job_gen(&self) -> u64 {
        match self {
            Self::AppsLoaded { job_gen, .. }
            | Self::CustomMenuLoaded { job_gen, .. } => *job_gen,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEffect {
    None,
    CloseApp,
    FocusWindow { hwnd: u64, close_now: bool },
    NotifyDaemonReload,
    PickThemeFile,
    PickMenuFile,
}

pub trait BackendBridge: Send + Sync {
    fn invoke(&self, sub: BuiltinSubmenu, req: &SubmenuRequest) -> Result<SubmenuResponse, String>;
    fn is_installed(&self, sub: BuiltinSubmenu) -> bool;
}

pub struct App {
    pub store: ConfigStore,
    pub installed: InstalledModules,
    pub cfg: AppConfig,
    pub effective_colors: ColorsConfig,
    pub history: History,
    pub mode: Mode,
    pub standalone: bool,
    pub menu: MenuState,
    pub ring_motion: RingMotion,
    pub viewport_size: (f64, f64),
    pub anchor: (f64, f64),
    pub hover_pos: Option<(f64, f64)>,
    pub pressed: Option<(usize, bool)>,
    pub surface: SurfaceMode,
    pub show_help: bool,
    pub notice: Option<String>,
    pub scroll_accum: f64,
    pub scrollbar_dragging: bool,
    pub thumb_grab: Option<f64>,
    pub loading_active: bool,
    pub loading_visible: bool,
    pub loading_is_3d: bool,
    pub async_gen: u64,

    // Submenu states
    pub nav_stack: Vec<(Mode, String)>,
    pub back_stack: Vec<String>,
    pub typing_in_search: bool,
    pub settings_stack: Vec<SettingsView>,
    pub settings_items: Vec<SettingsItem>,
    pub apps_cache: Option<Vec<recircles_core::submenu_abi::AppDto>>,
    pub custom_stack: Vec<ActiveCustomMenu>,
    pub confirming_clipboard_clear: bool,

    // Overlay modes
    pub input_box: Option<InputBoxState>,
    pub input_purpose: Option<InputSubmitPurpose>,
    pub slider: Option<SliderState>,
    pub slider_original_value: Option<u8>,
    pub color_picker: Option<ColorPickerState>,
    pub color_original_colors: Option<ColorsConfig>,
    pub capture: Option<CaptureTarget>,

    // Pending async work
    pub backend: std::sync::Arc<dyn BackendBridge>,
    pub async_tx: Sender<AsyncJobResult>,
    pub async_rx: Receiver<AsyncJobResult>,
    pub closing: bool,
}

impl App {
    pub fn new(
        store: ConfigStore,
        installed: InstalledModules,
        backend: std::sync::Arc<dyn BackendBridge>,
        start_menu: Option<&str>,
    ) -> Result<Self, String> {
        let cfg = store.load_or_create(&installed)?;
        let effective_colors = cfg.effective_colors(&store);
        let history = History::load_in(store.dir().to_path_buf());
        let (async_tx, async_rx) = mpsc::channel();
        let animate = !cfg.appearance.reduced_motion;

        let mut app = Self {
            store,
            installed,
            cfg,
            effective_colors,
            history,
            mode: Mode::Main,
            standalone: start_menu.is_some(),
            menu: MenuState::default(),
            ring_motion: RingMotion::default(),
            viewport_size: (1536.0, 864.0),
            anchor: (768.0, 432.0),
            hover_pos: None,
            pressed: None,
            surface: SurfaceMode::Ring,
            show_help: false,
            notice: None,
            scroll_accum: 0.0,
            scrollbar_dragging: false,
            thumb_grab: None,
            loading_active: false,
            loading_visible: false,
            loading_is_3d: true,
            async_gen: 0,
            nav_stack: Vec::new(),
            back_stack: Vec::new(),
            typing_in_search: false,
            settings_stack: vec![SettingsView::Root],
            settings_items: Vec::new(),
            apps_cache: None,
            custom_stack: Vec::new(),
            confirming_clipboard_clear: false,
            input_box: None,
            input_purpose: None,
            slider: None,
            slider_original_value: None,
            color_picker: None,
            color_original_colors: None,
            capture: None,
            backend,
            async_tx,
            async_rx,
            closing: false,
        };

        app.ring_motion.open(animate);
        match start_menu {
            None => {
                app.enter_main_menu();
            }
            Some(target) => {
                app.open_menu_target(target, true)?;
            }
        }
        Ok(app)
    }

    pub fn set_viewport(&mut self, width: f64, height: f64, anchor_x: f64, anchor_y: f64) {
        self.viewport_size = (width.max(320.0), height.max(240.0));
        self.anchor = (anchor_x, anchor_y);
    }

    pub fn ring_geometry(&self) -> RingGeometry {
        RingGeometry::new(
            self.viewport_size.0,
            self.viewport_size.1,
            self.anchor.0,
            self.anchor.1,
        )
    }

    pub fn explorer_bounds(&self) -> Option<ExplorerBounds> {
        if self.ring_motion.loading_pose().is_some() {
            return None;
        }
        let has_input =
            self.input_box.is_some() || self.slider.is_some() || self.color_picker.is_some();
        let uses_panel = matches!(self.surface, SurfaceMode::Grid | SurfaceMode::List);
        if !has_input && !uses_panel && self.ring_motion.panel_mix() <= 0.001 {
            return None;
        }
        let has_query = !self.menu.query.is_empty();
        let input_kind = if self.color_picker.is_some() {
            InputPanelKind::ColorPicker
        } else if self.slider.is_some() {
            InputPanelKind::Slider
        } else {
            InputPanelKind::Text
        };
        Some(ExplorerBounds::new(
            self.viewport_size,
            self.anchor,
            has_query,
            &self.ring_motion,
            input_kind,
        ))
    }

    pub fn visible_count(&self) -> usize {
        match self.surface {
            SurfaceMode::List => self.menu.list_visible_slice().len(),
            SurfaceMode::Grid => self.menu.grid_visible_slice().len(),
            _ => self.menu.visible_slice(PAGE_SIZE).len(),
        }
    }

    pub fn grid_first_visible(&self) -> usize {
        self.menu.grid_first_visible()
    }

    pub fn selected_vis(&self) -> Option<usize> {
        match self.surface {
            SurfaceMode::List => self.menu.list_selected_visible_index(),
            SurfaceMode::Grid => self.menu.grid_selected_visible_index(),
            _ => self.menu.selected_visible_index(PAGE_SIZE),
        }
    }

    fn select_visible_for_surface(&mut self, vis: usize) {
        match self.surface {
            SurfaceMode::List => self.menu.select_list_visible(vis),
            SurfaceMode::Grid => self.menu.select_grid_visible(vis),
            _ => self.menu.select_visible(vis, PAGE_SIZE),
        }
    }

    pub fn pressed_vis(&self) -> Option<usize> {
        self.pressed.map(|(idx, _)| idx)
    }

    pub fn ring_page_indicator(&self) -> Option<(usize, usize)> {
        if self.surface != SurfaceMode::Ring {
            return None;
        }
        let pages = self.menu.page_count(PAGE_SIZE);
        (pages > 1).then_some((self.menu.page + 1, pages))
    }

    /// Exact 1:1 Linux titles: EMPTY on Main Home ring so only the pure glass circle floats!
    pub fn display_title(&self) -> String {
        match &self.mode {
            Mode::Main => String::new(),
            Mode::Files => "Files".into(),
            Mode::Apps => "Applications".into(),
            Mode::Windows => "Alt-Tab".into(),
            Mode::Clipboard => {
                if self.confirming_clipboard_clear {
                    "Clear Clipboard".into()
                } else {
                    "Clipboard".into()
                }
            }
            Mode::Settings => {
                let view = self
                    .settings_stack
                    .last()
                    .cloned()
                    .unwrap_or(SettingsView::Root);
                settings::view_title(&view).to_string()
            }
            Mode::Custom(id) => self
                .custom_stack
                .last()
                .map(|c| c.config.title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| id.clone()),
        }
    }

    /// Exact 1:1 Linux status: EMPTY on Main Home ring unless a notice/error is active.
    pub fn display_status(&self) -> String {
        if let Some(n) = &self.notice {
            return n.clone();
        }
        if matches!(self.mode, Mode::Main) {
            return String::new();
        }
        if self.loading_active && self.loading_visible {
            return String::new();
        }
        if let Some(entry) = self.menu.selected_entry() {
            if !entry.detail.is_empty() {
                return entry.detail.clone();
            }
            return entry.title.clone();
        }
        if !self.menu.query.is_empty() && self.menu.filtered.is_empty() {
            return "No matches".into();
        }
        match self.mode {
            Mode::Main => String::new(),
            Mode::Files if self.menu.filtered.is_empty() => "Type a folder path to get started".into(),
            Mode::Files if self.menu.query.is_empty() => "Frequent folders · Type a path".into(),
            Mode::Apps if self.menu.query.is_empty() => "Type an application name".into(),
            Mode::Windows if self.menu.filtered.is_empty() => "No open windows".into(),
            Mode::Windows if self.menu.query.is_empty() => "Search by window title or app".into(),
            Mode::Clipboard if self.confirming_clipboard_clear => {
                "Delete all saved clipboard history?".into()
            }
            Mode::Clipboard if self.menu.filtered.is_empty() => "Clipboard history is empty".into(),
            Mode::Clipboard if self.menu.query.is_empty() => "Click to copy · Right-click to delete".into(),
            Mode::Settings => {
                let view = self
                    .settings_stack
                    .last()
                    .cloned()
                    .unwrap_or(SettingsView::Root);
                settings::idle_status(&view).to_string()
            }
            Mode::Custom(_) if self.surface == SurfaceMode::List => {
                "Click to choose · Esc to go back".into()
            }
            Mode::Custom(_) if self.menu.query.is_empty() && !self.menu.filtered.is_empty() => {
                "Type to filter".into()
            }
            _ => String::new(),
        }
    }

    pub fn inline_suggestion(&self) -> Option<String> {
        self.menu
            .suggestion(self.mode == Mode::Files)
            .map(|(_, suffix)| suffix.to_string())
    }

    fn push_back(&mut self, query: &str) {
        if self.back_stack.last().map(String::as_str) != Some(query) {
            self.back_stack.push(query.to_owned());
        }
    }

    fn expand_file_query(query: &str) -> String {
        let q = query.trim();
        let home = std::env::var("USERPROFILE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("HOME").ok().filter(|s| !s.trim().is_empty()))
            .unwrap_or_else(|| "C:\\".to_string());
        if q == "~" || q.starts_with("~/") || q.starts_with("~\\") {
            let rest = q.strip_prefix('~').unwrap_or("").replace('/', "\\");
            let base = home.trim_end_matches(['\\', '/']);
            if rest.is_empty() {
                return format!("{base}\\");
            }
            return format!("{base}{rest}");
        }
        q.replace('/', "\\")
    }

    pub fn parent_file_query(query: &str) -> Option<String> {
        let q = query.trim();
        if q.is_empty() {
            return None;
        }
        let expanded = Self::expand_file_query(q);
        let trimmed = expanded.trim_end_matches(['\\', '/']);
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.len() == 2 && trimmed.as_bytes()[1] == b':' {
            return Some("\\".to_string());
        }
        let parent = std::path::Path::new(trimmed).parent()?;
        if parent.as_os_str().is_empty() {
            return None;
        }
        let mut s = parent.to_string_lossy().into_owned();
        if !s.ends_with(['\\', '/']) {
            s.push('\\');
        }
        Some(s)
    }

    fn current_folder_path(query: &str) -> Option<String> {
        let q = query.trim();
        if q.is_empty() {
            return None;
        }
        let expanded = Self::expand_file_query(q);
        if expanded == "\\" || expanded == "/" {
            return None;
        }
        let path = std::path::Path::new(&expanded);
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else if expanded.ends_with('\\') {
            return None;
        } else {
            let par = path.parent()?;
            if par.as_os_str().is_empty() || !par.is_dir() {
                return None;
            }
            par.to_path_buf()
        };
        let mut s = dir.to_string_lossy().into_owned();
        if !s.ends_with(['\\', '/']) {
            s.push('\\');
        }
        Some(s)
    }

    fn open_current_folder(&mut self) -> UiEffect {
        let Some(dir) = Self::current_folder_path(&self.menu.query) else {
            return UiEffect::None;
        };
        self.history.bump_path(&dir);
        let _ = self.history.save();
        match self.backend.invoke(
            BuiltinSubmenu::Files,
            &SubmenuRequest::FilesOpenFolder { path: dir },
        ) {
            Ok(SubmenuResponse::ActionOutcome { ok: false, message }) => {
                self.notice = Some(message);
                UiEffect::None
            }
            Err(e) => {
                self.notice = Some(e);
                UiEffect::None
            }
            _ => self.begin_close(),
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.scrollbar_dragging
            || self.slider.as_ref().is_some_and(|s| s.dragging)
            || self
                .color_picker
                .as_ref()
                .is_some_and(|cp| cp.dragging != ColorDrag::None)
    }

    pub fn is_animating(&self) -> bool {
        self.loading_active
            || self.ring_motion.is_animating()
            || self.slider.as_ref().is_some_and(|s| s.is_animating())
            || (self.closing && self.ring_motion.master_alpha() > 0.01)
    }

    pub fn tick(&mut self, dt: f64) -> Vec<UiEffect> {
        let mut effects = Vec::new();
        while let Ok(msg) = self.async_rx.try_recv() {
            self.handle_async_result(msg);
        }
        self.ring_motion.advance(dt);
        if let Some(s) = &mut self.slider {
            s.advance(dt, self.cfg.appearance.reduced_motion);
        }
        if let Some((hx, hy)) = self.hover_pos
            && !self.is_dragging()
            && !self.closing
        {
            self.sync_hover_at(hx, hy);
        }
        if self.closing && self.ring_motion.master_alpha() <= 0.02 {
            effects.push(UiEffect::CloseApp);
        }
        effects
    }

    pub fn begin_close(&mut self) -> UiEffect {
        if self.closing {
            return if self.cfg.appearance.reduced_motion {
                UiEffect::CloseApp
            } else {
                UiEffect::None
            };
        }
        let _ = self.history.save();
        self.pressed = None;
        if self.cfg.appearance.reduced_motion {
            self.closing = true;
            return UiEffect::CloseApp;
        }
        self.closing = true;
        self.ring_motion.close(true);
        UiEffect::None
    }

    pub fn cancel_active_pointer_press(&mut self) {
        self.pressed = None;
        self.thumb_grab = None;
        self.scrollbar_dragging = false;
        if let Some(cp) = &mut self.color_picker {
            cp.dragging = ColorDrag::None;
            cp.pressed_button = None;
        }
        if let Some(s) = &mut self.slider {
            s.end_drag(self.cfg.appearance.reduced_motion);
            s.pressed_button = None;
        }
        if let Some(inp) = &mut self.input_box {
            inp.pressed_button = None;
        }
    }

    pub fn rescale_hover(&mut self, ratio: f64) {
        if let Some((hx, hy)) = self.hover_pos
            && ratio.is_finite()
            && ratio > 0.0
        {
            self.hover_pos = Some((hx * ratio, hy * ratio));
        }
    }

    fn bump_async_gen(&mut self) -> u64 {
        self.async_gen = self.async_gen.wrapping_add(1);
        self.async_gen
    }

    fn start_loading(&mut self, is_3d: bool) {
        self.loading_active = true;
        self.loading_visible = true;
        self.loading_is_3d = is_3d;
        let animate = !self.cfg.appearance.reduced_motion;
        self.ring_motion.set_loading(is_3d, animate);
        self.ring_motion.set_empty(!is_3d, !is_3d, animate);
    }

    fn stop_loading(&mut self) {
        self.loading_active = false;
        self.loading_visible = false;
        let animate = !self.cfg.appearance.reduced_motion;
        self.ring_motion.set_loading(false, animate);
    }

    fn handle_async_result(&mut self, res: AsyncJobResult) {
        let is_current = res.job_gen() == self.async_gen;
        if is_current {
            self.stop_loading();
        }
        match res {
            AsyncJobResult::AppsLoaded { result: Ok(apps), .. } => {
                let unchanged = self.apps_cache.as_deref() == Some(apps.as_slice());
                self.apps_cache = Some(apps.clone());
                if is_current && self.mode == Mode::Apps && !unchanged {
                    self.apply_apps_list(apps);
                }
            }
            AsyncJobResult::AppsLoaded { result: Err(e), .. } => {
                if is_current && self.mode == Mode::Apps && self.apps_cache.is_none() {
                    self.notice = Some(e);
                    self.sync_items_and_motion(Vec::new(), false);
                }
            }
            AsyncJobResult::CustomMenuLoaded {
                stack_depth,
                id,
                result,
                ..
            } => {
                if is_current
                    && self.custom_stack.len() == stack_depth
                    && let Some(top) = self.custom_stack.last_mut()
                    && top.id == id
                {
                    match result {
                        Ok(items) => {
                            top.config.items = items;
                            let entries = build_custom_menu_entries(&top.config.items);
                            self.sync_items_and_motion(entries, true);
                        }
                        Err(e) => {
                            self.notice = Some(e);
                            self.sync_items_and_motion(Vec::new(), true);
                        }
                    }
                }
            }
        }
    }

    pub fn open_menu_target(&mut self, raw_id: &str, check_installed: bool) -> Result<(), String> {
        let id = raw_id.trim();
        if let Some(sub) = BuiltinSubmenu::from_id(id) {
            if check_installed && !self.backend.is_installed(sub) {
                return Err(format!(
                    "Submenu '{}' is not installed on this system. Run recircles-setup.exe to install it.",
                    sub.title()
                ));
            }
            match sub {
                BuiltinSubmenu::Files => self.enter_files(),
                BuiltinSubmenu::Apps => self.enter_apps(),
                BuiltinSubmenu::Windows => self.enter_windows(),
                BuiltinSubmenu::Clipboard => self.enter_clipboard(),
                BuiltinSubmenu::Settings => self.enter_settings(SettingsView::Root),
            }
            return Ok(());
        }

        if let Some(menu_cfg) = self.cfg.menus.get(id).cloned() {
            self.enter_custom_menu(id.to_string(), menu_cfg, true);
            return Ok(());
        }
        Err(format!("Unknown menu '{id}'"))
    }

    pub fn enter_main_menu(&mut self) {
        self.bump_async_gen();
        self.stop_loading();
        self.hover_pos = None;
        self.mode = Mode::Main;
        self.notice = None;
        self.clear_overlays();
        self.nav_stack.clear();
        self.custom_stack.clear();
        let entries = build_main_menu_entries(&self.cfg, &self.installed);
        self.menu.set_items(entries, true);
        // Exact 1:1 Linux Home behavior: no sector pre-selected on open until hovered or stepped!
        self.menu.selected = None;
        self.refresh_surface_and_motion();
    }

    fn enter_files(&mut self) {
        self.bump_async_gen();
        self.stop_loading();
        self.hover_pos = None;
        self.mode = Mode::Files;
        self.notice = None;
        self.clear_overlays();
        self.menu.query.clear();
        self.reload_files_for_query();
    }

    fn reload_files_for_query(&mut self) {
        let q = self.menu.query.trim().to_string();
        let is_path = q.starts_with('~')
            || q.starts_with('/')
            || q.starts_with('\\')
            || (q.len() >= 2 && q.as_bytes()[1] == b':');
        let req = if is_path {
            SubmenuRequest::FilesCurrentFolder {
                query: q,
                config_dir: self.store.dir().to_string_lossy().into_owned(),
            }
        } else {
            SubmenuRequest::FilesFrequentFolders {
                usage_keys: self.history.recent_path_keys(),
                config_dir: self.store.dir().to_string_lossy().into_owned(),
            }
        };
        match self.backend.invoke(BuiltinSubmenu::Files, &req) {
            Ok(SubmenuResponse::FilesCurrentFolder { entries, .. }) => {
                let items = entries.into_iter().map(file_dto_to_entry).collect();
                // Files directory already filtered by backend path query; preserve items order
                self.menu.items = items;
                self.menu.filtered = (0..self.menu.items.len()).collect();
                self.menu.page = 0;
                self.menu.grid_scroll = 0;
                self.menu.list_scroll = 0;
                self.menu.selected = if self.menu.filtered.is_empty() { None } else { Some(0) };
                self.refresh_surface_and_motion();
            }
            Ok(SubmenuResponse::FilesList { entries }) => {
                let items = entries.into_iter().map(file_dto_to_entry).collect();
                self.sync_items_and_motion(items, true);
            }
            Ok(_) => {}
            Err(e) => {
                self.notice = Some(e);
                self.sync_items_and_motion(Vec::new(), true);
            }
        }
    }

    fn apps_disk_cache_path(&self) -> std::path::PathBuf {
        self.store.dir().join("cache").join("apps_v1.json")
    }

    fn load_apps_disk_cache(&self) -> Option<Vec<recircles_core::submenu_abi::AppDto>> {
        let bytes = std::fs::read(self.apps_disk_cache_path()).ok()?;
        let apps: Vec<recircles_core::submenu_abi::AppDto> = serde_json::from_slice(&bytes).ok()?;
        (!apps.is_empty()).then_some(apps)
    }

    fn enter_apps(&mut self) {
        let job_gen = self.bump_async_gen();
        self.stop_loading();
        self.hover_pos = None;
        self.mode = Mode::Apps;
        self.notice = None;
        self.clear_overlays();
        self.menu.query.clear();

        if let Some(cached) = self.apps_cache.clone() {
            self.apply_apps_list(cached);
            return;
        }

        let cache_path = self.apps_disk_cache_path();
        let seeded_from_disk = if let Some(disk_apps) = self.load_apps_disk_cache() {
            self.apps_cache = Some(disk_apps.clone());
            self.apply_apps_list(disk_apps);
            true
        } else {
            false
        };

        if !seeded_from_disk {
            // Preserve existing Main Ring sectors while the first app scan runs so icons spatially
            // morph directly from the ring into the 4x3 grid (1:1 with Linux). Only show the 3D
            // loading ring when opened standalone with no prior ring sectors.
            let has_ring_sectors = self.ring_motion.sectors().next().is_some();
            if has_ring_sectors {
                self.loading_active = true;
                self.loading_visible = false;
            } else {
                self.sync_items_and_motion(Vec::new(), false);
                self.start_loading(matches!(
                    self.cfg.appearance.loading_animation,
                    recircles_core::config::LoadingStyle::ThreeD
                ));
            }
        }
        let backend = self.backend.clone();
        let tx = self.async_tx.clone();
        let prev_disk = self.apps_cache.clone();
        std::thread::spawn(move || {
            let res = match backend.invoke(BuiltinSubmenu::Apps, &SubmenuRequest::AppsList) {
                Ok(SubmenuResponse::AppsList { apps }) => {
                    if !apps.is_empty() && prev_disk.as_deref() != Some(apps.as_slice()) {
                        save_apps_disk_cache(&cache_path, &apps);
                    }
                    Ok(apps)
                }
                Ok(SubmenuResponse::Error { message }) => Err(message),
                Ok(_) => Err("Unexpected response from apps module".into()),
                Err(e) => Err(e),
            };
            let _ = tx.send(AsyncJobResult::AppsLoaded { job_gen, result: res });
        });
    }

    fn apply_apps_list(&mut self, mut apps: Vec<recircles_core::submenu_abi::AppDto>) {
        apps.sort_by_cached_key(|a| {
            (
                std::cmp::Reverse(self.history.app_score(&a.id)),
                a.name.to_ascii_lowercase(),
            )
        });
        let entries = apps
            .into_iter()
            .map(|a| {
                let active = self.history.app_score(&a.id) > 0;
                Entry {
                    key: a.id,
                    title: a.name,
                    detail: String::new(),
                    icon: BuiltinIcon::App,
                    icon_path: None,
                    exe_icon_path: if a.icon_path.is_empty() {
                        None
                    } else {
                        Some(a.icon_path)
                    },
                    accent_override: None,
                    active,
                    completion: None,
                    payload: a.launch_target,
                    secondary_payload: None,
                }
            })
            .collect();
        self.sync_items_and_motion(entries, false);
    }

    fn enter_windows(&mut self) {
        self.bump_async_gen();
        self.stop_loading();
        self.hover_pos = None;
        self.mode = Mode::Windows;
        self.notice = None;
        self.clear_overlays();
        self.menu.query.clear();
        match self.backend.invoke(BuiltinSubmenu::Windows, &SubmenuRequest::WindowsList) {
            Ok(SubmenuResponse::WindowsList { windows }) => {
                let entries = windows
                    .into_iter()
                    .map(|w| Entry {
                        key: format!("hwnd:{}", w.hwnd),
                        title: w.title,
                        detail: if w.minimized {
                            format!("{} · minimized", w.app_id)
                        } else {
                            w.app_id
                        },
                        icon: BuiltinIcon::Window,
                        icon_path: None,
                        exe_icon_path: if w.exe_path.is_empty() {
                            None
                        } else {
                            Some(w.exe_path)
                        },
                        accent_override: None,
                        active: !w.minimized,
                        completion: None,
                        payload: w.hwnd.to_string(),
                        secondary_payload: None,
                    })
                    .collect();
                self.sync_items_and_motion(entries, true);
            }
            Ok(_) => self.sync_items_and_motion(Vec::new(), true),
            Err(e) => {
                self.notice = Some(e);
                self.sync_items_and_motion(Vec::new(), true);
            }
        }
    }

    pub fn enter_clipboard(&mut self) {
        self.bump_async_gen();
        self.stop_loading();
        self.hover_pos = None;
        self.mode = Mode::Clipboard;
        self.notice = None;
        self.clear_overlays();
        self.menu.query.clear();
        self.reload_clipboard_entries();
    }

    fn reload_clipboard_entries(&mut self) {
        let data_dir = self.store.dir().to_string_lossy().into_owned();
        match self.backend.invoke(
            BuiltinSubmenu::Clipboard,
            &SubmenuRequest::ClipboardList { data_dir },
        ) {
            Ok(SubmenuResponse::ClipboardItems { items }) => {
                let mut entries: Vec<Entry> = items
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let icon = match c.kind {
                            ClipboardKind::Text => BuiltinIcon::Clipboard,
                            ClipboardKind::Files => BuiltinIcon::Folder,
                            ClipboardKind::Image => BuiltinIcon::Image,
                        };
                        Entry {
                            key: c.id.clone(),
                            title: c.label,
                            detail: format!("{} · R-click delete", c.detail),
                            icon,
                            icon_path: c.image_path,
                            exe_icon_path: None,
                            accent_override: None,
                            active: i == 0,
                            completion: None,
                            payload: format!("clip_copy:{}", c.id),
                            secondary_payload: Some(format!("clip_del:{}", c.id)),
                        }
                    })
                    .collect();
                if !entries.is_empty() {
                    entries.push(Entry {
                        key: "clip:clear".into(),
                        title: "Clear Clipboard History".into(),
                        detail: "Delete all saved clips".into(),
                        icon: BuiltinIcon::Forget,
                        icon_path: None,
                        exe_icon_path: None,
                        accent_override: None,
                        active: false,
                        completion: None,
                        payload: "clip_clear".into(),
                        secondary_payload: None,
                    });
                }
                self.sync_items_and_motion(entries, true);
            }
            Ok(_) => self.sync_items_and_motion(Vec::new(), true),
            Err(e) => {
                self.notice = Some(e);
                self.sync_items_and_motion(Vec::new(), true);
            }
        }
    }

    pub fn enter_settings(&mut self, view: SettingsView) {
        self.bump_async_gen();
        self.stop_loading();
        self.hover_pos = None;
        self.mode = Mode::Settings;
        self.notice = None;
        self.clear_overlays();
        self.menu.query.clear();
        self.settings_stack = vec![view];
        self.rebuild_settings_entries();
    }

    fn rebuild_settings_entries(&mut self) {
        self.rebuild_settings_entries_inner(false);
    }

    fn refresh_current_settings_entries(&mut self) {
        self.rebuild_settings_entries_inner(true);
    }

    fn rebuild_settings_entries_inner(&mut self, preserve_position: bool) {
        let prev_selected = self.menu.selected;
        let prev_page = self.menu.page;
        let prev_grid_scroll = self.menu.grid_scroll;
        let prev_list_scroll = self.menu.list_scroll;

        let view = self.settings_stack.last().cloned().unwrap_or(SettingsView::Root);
        let items = settings::build_items(&self.cfg, &self.store, &self.installed, &view);
        self.settings_items = items.clone();
        let entries = items
            .into_iter()
            .enumerate()
            .map(|(i, s)| Entry {
                key: format!("set:{i}:{}", s.label),
                title: s.label,
                detail: s.detail,
                icon: s.icon,
                icon_path: None,
                exe_icon_path: None,
                accent_override: s.accent,
                active: s.active,
                completion: None,
                payload: i.to_string(),
                secondary_payload: s.secondary_action.as_ref().map(|_| format!("sec:{i}")),
            })
            .collect();
        self.menu.set_items(entries, true);
        if preserve_position && !self.menu.filtered.is_empty() {
            let max_idx = self.menu.filtered.len() - 1;
            self.menu.selected = prev_selected.map(|s| s.min(max_idx));
            let pages = self.menu.page_count(PAGE_SIZE);
            self.menu.page = prev_page.min(pages.saturating_sub(1));
            self.menu.grid_scroll = prev_grid_scroll.min(self.menu.grid_max_scroll());
            self.menu.list_scroll = prev_list_scroll.min(self.menu.list_max_scroll());
        }
        self.refresh_surface_and_motion();
    }

    fn enter_custom_menu(&mut self, id: String, cfg: MenuConfig, push: bool) {
        let job_gen = self.bump_async_gen();
        self.stop_loading();
        self.hover_pos = None;
        self.mode = Mode::Custom(id.clone());
        self.notice = None;
        self.clear_overlays();
        self.menu.query.clear();
        if push {
            self.custom_stack.push(ActiveCustomMenu {
                id: id.clone(),
                config: cfg.clone(),
            });
        }

        if cfg.layout == CustomMenuLayout::Input {
            let prompt = cfg
                .input_prompt
                .clone()
                .unwrap_or_else(|| cfg.title.clone());
            self.input_box = Some(InputBoxState::new(
                prompt,
                cfg.input_placeholder.clone().unwrap_or_default(),
                cfg.secret,
            ));
            self.input_purpose = Some(InputSubmitPurpose::CustomMenuInput {
                spec: cfg.on_submit.clone().unwrap_or_default(),
                label: None,
                value: None,
            });
            self.refresh_surface_and_motion();
            return;
        }

        if let Some(cmd) = cfg.on_open.clone() {
            self.sync_items_and_motion(Vec::new(), true);
            let use_3d = cfg
                .loading_animation
                .unwrap_or(self.cfg.appearance.loading_animation)
                == recircles_core::config::LoadingStyle::ThreeD;
            self.start_loading(use_3d);
            let depth = self.custom_stack.len();
            let tx = self.async_tx.clone();
            std::thread::spawn(move || {
                let res = match run_command_capture(&cmd, Duration::from_secs(5)) {
                    Ok(stdout) => parse_dynamic_menu_stdout(&stdout),
                    Err(e) => Err(e),
                };
                let _ = tx.send(AsyncJobResult::CustomMenuLoaded {
                    job_gen,
                    stack_depth: depth,
                    id,
                    result: res,
                });
            });
        } else {
            let entries = build_custom_menu_entries(&cfg.items);
            self.sync_items_and_motion(entries, true);
        }
    }

    fn sync_items_and_motion(&mut self, items: Vec<Entry>, preserve_order: bool) {
        self.menu.set_items(items, preserve_order);
        self.refresh_surface_and_motion();
    }

    fn refresh_surface_and_motion(&mut self) {
        let animate = !self.cfg.appearance.reduced_motion;
        let has_input =
            self.input_box.is_some() || self.slider.is_some() || self.color_picker.is_some();

        if self.input_box.is_some() {
            self.surface = SurfaceMode::Input;
        } else if self.slider.is_some() {
            self.surface = SurfaceMode::Slider;
        } else if self.color_picker.is_some() {
            self.surface = SurfaceMode::ColorPicker;
        } else {
            let is_custom_list = matches!(self.mode, Mode::Custom(_))
                && self
                    .custom_stack
                    .last()
                    .is_some_and(|c| c.config.layout == CustomMenuLayout::List);
            let is_settings_list = self.mode == Mode::Settings
                && matches!(self.settings_stack.last(), Some(SettingsView::ColorTheme));
            let is_list = is_custom_list || is_settings_list;

            let want_box = is_list
                || (self.cfg.appearance.layout == recircles_core::config::OverflowLayout::Box
                    && self.menu.filtered.len() > PAGE_SIZE
                    && !self.menu.filtered.is_empty());

            self.surface = if is_list {
                SurfaceMode::List
            } else if want_box {
                SurfaceMode::Grid
            } else {
                SurfaceMode::Ring
            };
        }

        self.menu.grid_scroll = self.menu.grid_first_visible();
        self.menu.list_scroll = self.menu.list_first_visible();

        let loading = !has_input && self.loading_active && self.loading_visible;
        let loading_3d = loading && self.loading_is_3d;
        let loading_2d = loading && !self.loading_is_3d;

        let list_active = !has_input
            && self.surface == SurfaceMode::List
            && !loading
            && (!self.menu.filtered.is_empty() || !self.menu.query.is_empty());
        let box_active = !has_input && !list_active && self.surface == SurfaceMode::Grid && !loading;
        let is_empty = !has_input && !list_active && !box_active && !loading && self.menu.filtered.is_empty();

        let vis_entries: Vec<Entry> = match self.surface {
            SurfaceMode::List => self
                .menu
                .list_visible_slice()
                .iter()
                .filter_map(|&idx| self.menu.items.get(idx).cloned())
                .collect(),
            SurfaceMode::Grid => self
                .menu
                .grid_visible_entries()
                .into_iter()
                .cloned()
                .collect(),
            _ => self
                .menu
                .visible_entries(PAGE_SIZE)
                .into_iter()
                .cloned()
                .collect(),
        };
        let list_rows = vis_entries.len().max(1);
        let selected = if has_input { None } else { self.selected_vis() };

        if has_input {
            self.ring_motion.set_input(true);
            let sel_btn = self
                .color_picker
                .as_ref()
                .and_then(|c| c.selected_button)
                .or_else(|| self.slider.as_ref().and_then(|s| s.selected_button))
                .or_else(|| self.input_box.as_ref().and_then(|i| i.selected_button));
            self.ring_motion.select_input_button(sel_btn);
        } else {
            if list_active {
                self.ring_motion.set_list(true, list_rows);
            } else {
                self.ring_motion.set_box(box_active);
                self.ring_motion.set_list(false, 1);
            }
            self.ring_motion.set_input(false);
        }

        self.ring_motion.set_loading(loading_3d, animate);
        self.ring_motion.set_empty(is_empty || loading_2d, loading_2d, animate);
        self.ring_motion.update(vis_entries, selected, animate);
    }

    fn clear_overlays(&mut self) {
        self.input_box = None;
        self.input_purpose = None;
        self.slider = None;
        self.slider_original_value = None;
        self.color_picker = None;
        self.color_original_colors = None;
        self.capture = None;
        self.show_help = false;
        self.confirming_clipboard_clear = false;
        self.back_stack.clear();
        self.typing_in_search = false;
        self.menu.clear_completion();
    }

    pub fn handle_key_down(&mut self, mods: Modifiers, vk: u32, ch: Option<char>) -> UiEffect {
        // Keyboard input immediately takes selection ownership so a stationary mouse cursor
        // cannot overwrite keyboard/search selection on the next animation tick.
        self.hover_pos = None;
        // Key capture mode intercepts the next key press directly!
        if let Some(target) = self.capture.take() {
            if vk == VK_ESCAPE {
                self.notice = Some("Keybind change cancelled".into());
                return UiEffect::None;
            }
            let Some(formatted) = format_vk_parts(mods, vk) else {
                // Modifier alone: wait cleanly for non-modifier key
                self.capture = Some(target);
                return UiEffect::None;
            };

            match target {
                CaptureTarget::MenuHotkey { menu_id, label } => {
                    if let Err(e) = parse_global_hotkey(&formatted) {
                        self.notice = Some(e);
                        self.capture = Some(CaptureTarget::MenuHotkey { menu_id, label });
                        return UiEffect::None;
                    }
                    if let Err(e) = self
                        .cfg
                        .keybinds
                        .check_global_conflict(menu_id.as_deref(), &formatted)
                    {
                        self.notice = Some(e);
                        self.capture = Some(CaptureTarget::MenuHotkey { menu_id, label });
                        return UiEffect::None;
                    }
                    match menu_id {
                        None => self.cfg.keybinds.main_menu = KeyList::one(formatted.clone()),
                        Some(ref id) => {
                            self.cfg
                                .keybinds
                                .set_menu_hotkey(id, KeyList::one(formatted.clone()))
                        }
                    }
                    let _ = self.store.save(&self.cfg);
                    self.refresh_current_settings_entries();
                    self.notice = Some(format!("Bound {label} to {formatted}"));
                    return UiEffect::NotifyDaemonReload;
                }
                CaptureTarget::Nav(field) => {
                    if vk == VK_LWIN || vk == VK_RWIN {
                        self.capture = Some(CaptureTarget::Nav(field));
                        return UiEffect::None;
                    }
                    if let Err(e) = parse_key_spec(&formatted) {
                        self.notice = Some(e);
                        return UiEffect::None;
                    }
                    field.set(&mut self.cfg.keybinds, KeyList::one(formatted.clone()));
                    let _ = self.store.save(&self.cfg);
                    self.refresh_current_settings_entries();
                    self.notice = Some(format!("{} -> {formatted}", field.label()));
                    return UiEffect::None;
                }
            }
        }

        // Slider submenu keyboard input (Left/Right ±1% or Shift ±10%, PageUp/Down ±10%, Home/End 0/100%, Tab, Enter/Esc, Ctrl+1/2)
        if let Some(s) = &mut self.slider {
            let field = s.field;
            let go_home = self.cfg.keybinds.home.matches_vk(mods, vk, ch);
            if go_home {
                if let Some(orig) = self.slider_original_value.take() {
                    field.set(&mut self.cfg.appearance, orig);
                }
                self.slider = None;
                self.notice = None;
                if self.standalone {
                    return self.begin_close();
                }
                self.enter_main_menu();
                return UiEffect::None;
            }
            let cancel_slider = self.cfg.keybinds.back.matches_vk(mods, vk, ch)
                || vk == VK_ESCAPE
                || (vk == VK_RETURN && s.selected_button == Some(1))
                || (self.cfg.keybinds.number_shortcuts
                    && (mods.ctrl ^ mods.alt)
                    && matches!(vk, 0x32 | 0x62));
            if cancel_slider {
                if let Some(orig) = self.slider_original_value.take() {
                    field.set(&mut self.cfg.appearance, orig);
                }
                self.slider = None;
                self.notice = None;
                self.refresh_surface_and_motion();
                return UiEffect::None;
            }
            let save_slider = vk == VK_RETURN
                || (self.cfg.keybinds.number_shortcuts
                    && (mods.ctrl ^ mods.alt)
                    && matches!(vk, 0x31 | 0x61));
            if save_slider {
                let val = s.value;
                field.set(&mut self.cfg.appearance, val);
                let _ = self.store.save(&self.cfg);
                self.slider = None;
                self.slider_original_value = None;
                self.refresh_current_settings_entries();
                self.notice = Some(format!("{} saved ({val}%)", field.label()));
                return UiEffect::None;
            }
            if matches!(vk, VK_TAB | VK_UP | VK_DOWN) {
                let reverse = (vk == VK_TAB && mods.shift) || vk == VK_UP;
                let next_btn = match (s.selected_button, reverse) {
                    (None, false) => Some(0),
                    (Some(0), false) => Some(1),
                    (Some(1), false) => None,
                    (None, true) => Some(1),
                    (Some(1), true) => Some(0),
                    (Some(0), true) => None,
                    _ => None,
                };
                s.selected_button = next_btn;
                self.ring_motion.select_input_button(next_btn);
                if self.cfg.appearance.reduced_motion {
                    self.ring_motion.advance(10.0);
                }
                return UiEffect::None;
            }
            let reduced = self.cfg.appearance.reduced_motion;
            let step: Option<i16> = match vk {
                VK_LEFT => Some(if mods.shift { -10 } else { -1 }),
                VK_RIGHT => Some(if mods.shift { 10 } else { 1 }),
                VK_NEXT => Some(-10),
                VK_PRIOR => Some(10),
                _ => None,
            };
            let changed = if let Some(d) = step {
                s.step_by(d, reduced)
            } else if vk == VK_HOME {
                if s.value != 0 {
                    s.set_stepped(0, reduced);
                    true
                } else {
                    false
                }
            } else if vk == VK_END {
                if s.value != 100 {
                    s.set_stepped(100, reduced);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if changed {
                let val = s.value;
                if s.selected_button.is_some() {
                    s.selected_button = None;
                    self.ring_motion.select_input_button(None);
                }
                field.set(&mut self.cfg.appearance, val);
            }
            return UiEffect::None;
        }

        // Color picker keyboard input (Escape / Enter / Ctrl+1 / Ctrl+2 / Tab / Ctrl+A / arrows / Delete / BackSpace / Hex)
        if let Some(cp) = &mut self.color_picker {
            let go_home = self.cfg.keybinds.home.matches_vk(mods, vk, ch);
            if go_home {
                if let Some(orig) = self.color_original_colors.take() {
                    self.cfg.colors = orig;
                    self.effective_colors = self.cfg.effective_colors(&self.store);
                }
                self.color_picker = None;
                self.notice = None;
                if self.standalone {
                    return self.begin_close();
                }
                self.enter_main_menu();
                return UiEffect::None;
            }
            let cancel_picker = self.cfg.keybinds.back.matches_vk(mods, vk, ch)
                || vk == VK_ESCAPE
                || (vk == VK_RETURN && cp.selected_button == Some(1))
                || (self.cfg.keybinds.number_shortcuts
                    && (mods.ctrl ^ mods.alt)
                    && matches!(vk, 0x32 | 0x62));
            if cancel_picker {
                if let Some(orig) = self.color_original_colors.take() {
                    self.cfg.colors = orig;
                    self.effective_colors = self.cfg.effective_colors(&self.store);
                }
                self.color_picker = None;
                self.notice = None;
                self.refresh_surface_and_motion();
                return UiEffect::None;
            }
            let save_picker = vk == VK_RETURN
                || (self.cfg.keybinds.number_shortcuts
                    && (mods.ctrl ^ mods.alt)
                    && matches!(vk, 0x31 | 0x61));
            if save_picker {
                if !cp.try_apply_hex_input() {
                    // Show inline error next to hex input so user knows why Save was rejected
                    cp.error = Some("Enter a valid #RRGGBB or #RRGGBBAA hex color".into());
                    return UiEffect::None;
                }
                let hex = cp.color().to_hex();
                let field = cp.field;
                field.set(&mut self.cfg.colors, hex.clone());
                self.effective_colors = self.cfg.effective_colors(&self.store);
                let _ = self.store.save(&self.cfg);
                self.color_picker = None;
                self.color_original_colors = None;
                self.refresh_current_settings_entries();
                self.notice = Some(format!("{} saved ({hex})", field.label()));
                return UiEffect::None;
            }
            if matches!(vk, VK_TAB | VK_UP | VK_DOWN) {
                let reverse = (vk == VK_TAB && mods.shift) || vk == VK_UP;
                let next_btn = match (cp.selected_button, reverse) {
                    (None, false) => Some(0),
                    (Some(0), false) => Some(1),
                    (Some(1), false) => None,
                    (None, true) => Some(1),
                    (Some(1), true) => Some(0),
                    (Some(0), true) => None,
                    _ => None,
                };
                cp.selected_button = next_btn;
                self.ring_motion.select_input_button(next_btn);
                if self.cfg.appearance.reduced_motion {
                    self.ring_motion.advance(10.0);
                }
                return UiEffect::None;
            }
            if mods.ctrl && !mods.alt && vk == 0x41 {
                cp.selected_button = None;
                self.ring_motion.select_input_button(None);
                cp.select_all_text();
                return UiEffect::None;
            }
            let mut mutated = false;
            let mut clear_btn = false;
            match vk {
                VK_LEFT => {
                    cp.move_left();
                    clear_btn = true;
                }
                VK_RIGHT => {
                    cp.move_right();
                    clear_btn = true;
                }
                VK_HOME => {
                    cp.move_home();
                    clear_btn = true;
                }
                VK_END => {
                    cp.move_end();
                    clear_btn = true;
                }
                VK_DELETE => {
                    mutated = cp.delete_forward();
                    clear_btn = true;
                }
                VK_BACK => {
                    mutated = cp.backspace(mods.ctrl && !mods.alt);
                    clear_btn = true;
                }
                _ => {
                    let is_plain_or_altgr = (!mods.ctrl && !mods.alt) || (mods.ctrl && mods.alt);
                    if let Some(c) = ch
                        && is_plain_or_altgr
                        && !mods.win
                    {
                        mutated = cp.insert_hex_char(c);
                        clear_btn = mutated;
                    }
                }
            }
            if clear_btn && cp.selected_button.is_some() {
                cp.selected_button = None;
                self.ring_motion.select_input_button(None);
            }
            if mutated && cp.is_hex_valid() {
                let hex = cp.color().to_hex();
                cp.field.set(&mut self.cfg.colors, hex);
                self.effective_colors = self.cfg.effective_colors(&self.store);
            }
            return UiEffect::None;
        }

        // Input box keyboard handling (full cursor navigation, Tab button cycling, Delete, Ctrl+A, Ctrl+1/2)
        if let Some(inp) = &mut self.input_box {
            let go_home = self.cfg.keybinds.home.matches_vk(mods, vk, ch);
            if go_home {
                self.input_box = None;
                self.input_purpose = None;
                self.notice = None;
                if self.standalone {
                    return self.begin_close();
                }
                self.enter_main_menu();
                return UiEffect::None;
            }
            let cancel_inp = self.cfg.keybinds.back.matches_vk(mods, vk, ch)
                || vk == VK_ESCAPE
                || (vk == VK_RETURN && inp.selected_button == Some(1))
                || (self.cfg.keybinds.number_shortcuts
                    && (mods.ctrl ^ mods.alt)
                    && matches!(vk, 0x32 | 0x62));
            if cancel_inp {
                self.input_box = None;
                self.input_purpose = None;
                self.notice = None;
                self.refresh_surface_and_motion();
                return UiEffect::None;
            }
            if self.cfg.keybinds.clear_query.matches_vk(mods, vk, ch) {
                inp.text.clear();
                inp.cursor = 0;
                inp.select_all = false;
                inp.selected_button = None;
                self.ring_motion.select_input_button(None);
                return UiEffect::None;
            }
            let submit_inp = vk == VK_RETURN
                || (self.cfg.keybinds.number_shortcuts
                    && (mods.ctrl ^ mods.alt)
                    && matches!(vk, 0x31 | 0x61));
            if submit_inp {
                return self.submit_input_box();
            }
            if matches!(vk, VK_TAB | VK_UP | VK_DOWN) {
                let reverse = (vk == VK_TAB && mods.shift) || vk == VK_UP;
                let next_btn = match (inp.selected_button, reverse) {
                    (None, false) => Some(0),
                    (Some(0), false) => Some(1),
                    (Some(1), false) => None,
                    (None, true) => Some(1),
                    (Some(1), true) => Some(0),
                    (Some(0), true) => None,
                    _ => None,
                };
                inp.selected_button = next_btn;
                self.ring_motion.select_input_button(next_btn);
                if self.cfg.appearance.reduced_motion {
                    self.ring_motion.advance(10.0);
                }
                return UiEffect::None;
            }
            if mods.ctrl && !mods.alt && vk == 0x41 {
                inp.selected_button = None;
                self.ring_motion.select_input_button(None);
                inp.select_all_text();
                return UiEffect::None;
            }
            let mut clear_btn = false;
            match vk {
                VK_LEFT => {
                    inp.move_left();
                    clear_btn = true;
                }
                VK_RIGHT => {
                    inp.move_right();
                    clear_btn = true;
                }
                VK_HOME => {
                    inp.move_home();
                    clear_btn = true;
                }
                VK_END => {
                    inp.move_end();
                    clear_btn = true;
                }
                VK_DELETE => {
                    inp.delete_forward();
                    clear_btn = true;
                }
                VK_BACK => {
                    inp.backspace(mods.ctrl && !mods.alt);
                    clear_btn = true;
                }
                _ => {
                    let is_plain_or_altgr = (!mods.ctrl && !mods.alt) || (mods.ctrl && mods.alt);
                    if let Some(c) = ch
                        && !c.is_control()
                        && is_plain_or_altgr
                        && !mods.win
                    {
                        inp.insert_char(c);
                        clear_btn = true;
                    }
                }
            }
            if clear_btn && inp.selected_button.is_some() {
                inp.selected_button = None;
                self.ring_motion.select_input_button(None);
            }
            return UiEffect::None;
        }

        let kb = self.cfg.keybinds.clone();
        if kb.help.matches_vk(mods, vk, ch) {
            self.show_help = !self.show_help;
            return UiEffect::None;
        }
        if self.show_help && (vk == VK_ESCAPE || kb.back.matches_vk(mods, vk, ch)) {
            self.show_help = false;
            return UiEffect::None;
        }

        if kb.accept_completion.matches_vk(mods, vk, ch) && self.mode != Mode::Main {
            if let Some((fi, _)) = self.menu.suggestion(self.mode == Mode::Files) {
                let prev = self.menu.query.clone();
                if let Some(completed) = self.menu.accept_completion(fi) {
                    self.notice = None;
                    if completed != prev {
                        self.push_back(&prev);
                    }
                    self.typing_in_search = false;
                    if self.mode == Mode::Files
                        && (completed.ends_with('\\') || completed.ends_with('/'))
                    {
                        self.reload_files_for_query();
                    } else {
                        self.refresh_surface_and_motion();
                    }
                    return UiEffect::None;
                }
            }
            return UiEffect::None;
        }

        if kb.undo_completion.matches_vk(mods, vk, ch) && self.mode != Mode::Main {
            if self.mode == Mode::Files {
                if let Some(parent) = Self::parent_file_query(&self.menu.query)
                    && parent != self.menu.query
                {
                    self.notice = None;
                    let prev = self.menu.query.clone();
                    self.push_back(&prev);
                    self.menu.query = parent;
                    self.typing_in_search = false;
                    self.reload_files_for_query();
                }
                return UiEffect::None;
            }
            if let Some(restored) = self.menu.undo_completion() {
                self.notice = None;
                if self.back_stack.last().map(String::as_str) == Some(restored.as_str()) {
                    self.back_stack.pop();
                }
                self.typing_in_search = !restored.is_empty();
                self.refresh_surface_and_motion();
            }
            return UiEffect::None;
        }

        if kb.number_shortcuts && !mods.win {
            let digit_slot = match vk {
                0x31..=0x38 => Some((vk - 0x31) as usize),
                0x61..=0x68 => Some((vk - 0x61) as usize),
                _ => None,
            };
            if let Some(slot_idx) = digit_slot {
                let plain_ok =
                    !mods.shift && (self.mode == Mode::Main || !self.typing_in_search);
                if mods.ctrl || mods.alt || plain_ok {
                    if slot_idx < self.visible_count() {
                        self.notice = None;
                        self.pressed = None;
                        self.select_visible_for_surface(slot_idx);
                        self.refresh_surface_and_motion();
                        if mods.ctrl {
                            return self.activate_selected(mods.shift);
                        }
                    }
                    return UiEffect::None;
                }
            }
        }

        if kb.home.matches_vk(mods, vk, ch) {
            self.notice = None;
            if self.mode == Mode::Main || self.standalone {
                return self.begin_close();
            }
            self.enter_main_menu();
            return UiEffect::None;
        }
        if kb.back.matches_vk(mods, vk, ch) {
            return self.step_back();
        }
        if kb.open_current_folder.matches_vk(mods, vk, ch) {
            if self.mode == Mode::Files {
                return self.open_current_folder();
            }
            return UiEffect::None;
        }
        if kb.secondary_activate.matches_vk(mods, vk, ch) {
            return self.activate_selected(true);
        }
        if kb.activate.matches_vk(mods, vk, ch) {
            return self.activate_selected(false);
        }
        if kb.page_next.matches_vk(mods, vk, ch) {
            self.notice = None;
            match self.surface {
                SurfaceMode::Grid => {
                    self.menu.turn_grid_page(1);
                }
                SurfaceMode::List => {
                    self.menu.turn_list_page(1);
                }
                _ => {
                    self.menu.next_page(PAGE_SIZE);
                }
            }
            self.refresh_surface_and_motion();
            return UiEffect::None;
        }
        if kb.page_previous.matches_vk(mods, vk, ch) {
            self.notice = None;
            match self.surface {
                SurfaceMode::Grid => {
                    self.menu.turn_grid_page(-1);
                }
                SurfaceMode::List => {
                    self.menu.turn_list_page(-1);
                }
                _ => {
                    self.menu.prev_page(PAGE_SIZE);
                }
            }
            self.refresh_surface_and_motion();
            return UiEffect::None;
        }
        if kb.previous.matches_vk(mods, vk, ch) {
            self.notice = None;
            match self.surface {
                SurfaceMode::Grid => self.menu.move_grid(-1, 0),
                SurfaceMode::List => self.menu.move_list(-1),
                SurfaceMode::Ring => self.menu.step_selection(-1, PAGE_SIZE),
                _ => {}
            }
            self.refresh_surface_and_motion();
            return UiEffect::None;
        }
        if kb.next.matches_vk(mods, vk, ch) {
            self.notice = None;
            match self.surface {
                SurfaceMode::Grid => self.menu.move_grid(1, 0),
                SurfaceMode::List => self.menu.move_list(1),
                SurfaceMode::Ring => self.menu.step_selection(1, PAGE_SIZE),
                _ => {}
            }
            self.refresh_surface_and_motion();
            return UiEffect::None;
        }
        if kb.clear_query.matches_vk(mods, vk, ch) {
            if self.mode != Mode::Main && !self.menu.query.is_empty() {
                self.notice = None;
                let prev = self.menu.query.clone();
                self.push_back(&prev);
                self.menu.set_query(String::new());
                self.typing_in_search = false;
                if self.mode == Mode::Files {
                    self.reload_files_for_query();
                } else {
                    self.refresh_surface_and_motion();
                }
            }
            return UiEffect::None;
        }

        if self.mode == Mode::Main {
            return UiEffect::None;
        }

        if vk == VK_BACK && !mods.alt && !mods.win {
            self.notice = None;
            let changed = if mods.ctrl {
                self.menu.delete_word_backward()
            } else {
                self.menu.pop_char()
            };
            if changed {
                self.typing_in_search = !self.menu.query.is_empty();
                if self.mode == Mode::Files {
                    self.reload_files_for_query();
                } else {
                    self.refresh_surface_and_motion();
                }
            }
            return UiEffect::None;
        }

        let is_plain_or_altgr = (!mods.ctrl && !mods.alt) || (mods.ctrl && mods.alt);
        if let Some(c) = ch
            && !c.is_control()
            && is_plain_or_altgr
            && !mods.win
        {
            self.notice = None;
            if !self.typing_in_search {
                let prev = self.menu.query.clone();
                self.push_back(&prev);
                self.typing_in_search = true;
            }
            self.menu.push_char(c);
            if self.mode == Mode::Files {
                self.reload_files_for_query();
            } else {
                self.refresh_surface_and_motion();
            }
        }
        UiEffect::None
    }

    pub fn paste_text(&mut self, raw: &str) {
        self.hover_pos = None;
        let clean: String = raw
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect();
        if clean.is_empty() {
            return;
        }
        if let Some(cp) = &mut self.color_picker {
            if cp.paste_hex(&clean) && recircles_core::colors::Color::from_hex(&cp.hex_input).is_ok()
            {
                let hex = cp.color().to_hex();
                cp.field.set(&mut self.cfg.colors, hex);
                self.effective_colors = self.cfg.effective_colors(&self.store);
            }
            return;
        }
        if let Some(inp) = &mut self.input_box {
            inp.insert_str(&clean);
            return;
        }
        if self.capture.is_none() && self.mode != Mode::Main {
            self.notice = None;
            if !self.typing_in_search {
                let prev = self.menu.query.clone();
                self.push_back(&prev);
                self.typing_in_search = true;
            }
            for c in clean.chars() {
                self.menu.push_char(c);
            }
            if self.mode == Mode::Files {
                self.reload_files_for_query();
            } else {
                self.refresh_surface_and_motion();
            }
        }
    }

    fn ring_hit_angle(&self, lx: f64, ly: f64) -> Option<f64> {
        let geom = self.ring_geometry();
        let (inner, outer) = match self.ring_motion.empty_ring() {
            Some(empty) => (
                crate::layout::INNER
                    + (crate::layout::EMPTY_INNER - crate::layout::INNER) * empty.mix,
                crate::layout::OUTER
                    + (crate::layout::EMPTY_OUTER - crate::layout::OUTER) * empty.mix,
            ),
            None => (crate::layout::INNER, crate::layout::OUTER),
        };
        geom.angle_with_radii(lx, ly, self.ring_motion.scale(), inner, outer)
    }

    fn hit_visible(&self, lx: f64, ly: f64) -> Option<usize> {
        if self.input_box.is_some() || self.slider.is_some() || self.color_picker.is_some() {
            return None;
        }
        let total = self.menu.filtered.len();
        if let Some(bounds) = self.explorer_bounds() {
            let box_mix = self.ring_motion.box_mix();
            let list_mix = self.ring_motion.list_mix();
            if self.surface == SurfaceMode::List || (list_mix > box_mix && list_mix > 0.12) {
                return bounds.hit_list((lx, ly), total, &self.ring_motion);
            }
            if self.surface == SurfaceMode::Grid || box_mix > 0.12 {
                return bounds.hit((lx, ly), &self.ring_motion);
            }
        }
        let angle = self.ring_hit_angle(lx, ly)?;
        self.ring_motion.hit(angle)
    }

    fn dismiss_at(&self, lx: f64, ly: f64) -> bool {
        if let Some(bounds) = self.explorer_bounds() {
            return !bounds.contains((lx, ly));
        }
        self.ring_hit_angle(lx, ly).is_none() || self.visible_count() == 0
    }

    fn sync_hover_at(&mut self, lx: f64, ly: f64) -> bool {
        if self.color_picker.is_some() || self.slider.is_some() || self.input_box.is_some() {
            let bounds = self.explorer_bounds();
            let hit_btn = bounds.and_then(|b| b.hit_input_button((lx, ly)));
            let hit_track = bounds.is_some_and(|b| b.hit_slider_track((lx, ly)));
            let mut changed = false;
            let mut btn_changed = false;
            if let Some(cp) = &mut self.color_picker {
                if cp.selected_button != hit_btn {
                    cp.selected_button = hit_btn;
                    changed = true;
                    btn_changed = true;
                }
            } else if let Some(s) = &mut self.slider {
                if s.selected_button != hit_btn {
                    s.selected_button = hit_btn;
                    changed = true;
                    btn_changed = true;
                }
                if s.hovered != hit_track {
                    s.hovered = hit_track;
                    changed = true;
                }
            } else if let Some(inp) = &mut self.input_box
                && inp.selected_button != hit_btn
            {
                inp.selected_button = hit_btn;
                changed = true;
                btn_changed = true;
            }
            if btn_changed {
                self.ring_motion.select_input_button(hit_btn);
                if self.cfg.appearance.reduced_motion {
                    self.ring_motion.advance(10.0);
                }
            }
            return changed;
        }

        let hit = self.hit_visible(lx, ly);
        let cur_vis = self.selected_vis();
        if cur_vis == hit {
            return false;
        }
        match hit {
            Some(vis) => {
                self.notice = None;
                self.select_visible_for_surface(vis);
            }
            None => {
                self.menu.selected = None;
            }
        }
        self.ring_motion.select(hit);
        if self.cfg.appearance.reduced_motion {
            self.ring_motion.advance(10.0);
        }
        true
    }

    pub fn handle_pointer_move(&mut self, lx: f64, ly: f64) -> bool {
        self.hover_pos = Some((lx, ly));

        if self.slider.as_ref().is_some_and(|s| s.dragging) {
            if let Some(bounds) = self.explorer_bounds() {
                let reduced = self.cfg.appearance.reduced_motion;
                if let Some(s) = &mut self.slider {
                    let (val, exact, overshoot) =
                        bounds.slider_drag_value((lx, ly), s.grab_offset_x);
                    s.set_dragged(val, exact, overshoot, reduced);
                    s.field.set(&mut self.cfg.appearance, val);
                }
                return true;
            }
            return false;
        }

        if let Some(cp) = &mut self.color_picker
            && cp.dragging != ColorDrag::None
        {
            let drag = cp.dragging;
            if let Some(bounds) = self.explorer_bounds() {
                match drag {
                    ColorDrag::Sv => {
                        let (s, v) = bounds.color_sv_at((lx, ly));
                        if let Some(cp) = &mut self.color_picker {
                            cp.s = s;
                            cp.v = v;
                            cp.sync_hex();
                            let hex = cp.color().to_hex();
                            cp.field.set(&mut self.cfg.colors, hex);
                        }
                    }
                    ColorDrag::Hue => {
                        let h = bounds.color_hue_at((lx, ly));
                        if let Some(cp) = &mut self.color_picker {
                            cp.h = h;
                            cp.sync_hex();
                            let hex = cp.color().to_hex();
                            cp.field.set(&mut self.cfg.colors, hex);
                        }
                    }
                    ColorDrag::None => {}
                }
                self.effective_colors = self.cfg.effective_colors(&self.store);
                return true;
            }
            return false;
        }

        if let Some(grab) = self.thumb_grab
            && let Some(bounds) = self.explorer_bounds()
        {
            let total = self.menu.filtered.len();
            if self.surface == SurfaceMode::List {
                let next = bounds.dragged_list_first((lx, ly), grab, total);
                if self.menu.scroll_list_to(next) {
                    self.refresh_surface_and_motion();
                    return true;
                }
            } else if self.surface == SurfaceMode::Grid {
                let first = bounds.dragged_first((lx, ly), grab, total);
                if self.menu.scroll_grid_to(first) {
                    self.refresh_surface_and_motion();
                    return true;
                }
            }
            return false;
        }

        self.sync_hover_at(lx, ly)
    }

    pub fn handle_pointer_down(&mut self, lx: f64, ly: f64, right_click: bool) -> UiEffect {
        self.hover_pos = Some((lx, ly));

        if self.slider.is_some() {
            if !right_click
                && let Some(bounds) = self.explorer_bounds()
            {
                if bounds.hit_slider_track((lx, ly)) {
                    let reduced = self.cfg.appearance.reduced_motion;
                    if let Some(s) = &mut self.slider {
                        let grab_offset = bounds.slider_grab_offset((lx, ly), s.visual_value);
                        s.dragging = true;
                        s.hovered = true;
                        s.grab_offset_x = grab_offset;
                        s.selected_button = None;
                        s.pressed_button = None;
                        let (val, exact, overshoot) =
                            bounds.slider_drag_value((lx, ly), grab_offset);
                        s.set_dragged(val, exact, overshoot, reduced);
                        s.field.set(&mut self.cfg.appearance, val);
                    }
                    self.ring_motion.select_input_button(None);
                    return UiEffect::None;
                }
                let hit_btn = bounds.hit_input_button((lx, ly));
                if let Some(s) = &mut self.slider {
                    s.selected_button = hit_btn;
                    s.pressed_button = hit_btn;
                }
                self.ring_motion.select_input_button(hit_btn);
            }
            return UiEffect::None;
        }

        if self.color_picker.is_some() {
            if !right_click
                && let Some(bounds) = self.explorer_bounds()
            {
                if let Some((s, v)) = bounds.hit_color_sv((lx, ly)) {
                    if let Some(cp) = &mut self.color_picker {
                        cp.dragging = ColorDrag::Sv;
                        cp.s = s;
                        cp.v = v;
                        cp.sync_hex();
                        let hex = cp.color().to_hex();
                        cp.field.set(&mut self.cfg.colors, hex);
                    }
                    self.effective_colors = self.cfg.effective_colors(&self.store);
                    return UiEffect::None;
                }
                if let Some(h) = bounds.hit_color_hue((lx, ly)) {
                    if let Some(cp) = &mut self.color_picker {
                        cp.dragging = ColorDrag::Hue;
                        cp.h = h;
                        cp.sync_hex();
                        let hex = cp.color().to_hex();
                        cp.field.set(&mut self.cfg.colors, hex);
                    }
                    self.effective_colors = self.cfg.effective_colors(&self.store);
                    return UiEffect::None;
                }
                if bounds.hit_input_field((lx, ly), true) {
                    if let Some(cp) = &mut self.color_picker {
                        cp.select_all = false;
                        cp.cursor = cp.char_len();
                        cp.selected_button = None;
                        cp.pressed_button = None;
                    }
                    self.ring_motion.select_input_button(None);
                    return UiEffect::None;
                }
                let hit_btn = bounds.hit_input_button((lx, ly));
                if let Some(cp) = &mut self.color_picker {
                    cp.selected_button = hit_btn;
                    cp.pressed_button = hit_btn;
                }
                self.ring_motion.select_input_button(hit_btn);
            }
            return UiEffect::None;
        }

        if self.input_box.is_some() {
            if !right_click
                && let Some(bounds) = self.explorer_bounds()
            {
                if self.input_box.as_ref().is_some_and(|i| i.secret) && bounds.hit_input_eye((lx, ly))
                {
                    if let Some(inp) = &mut self.input_box {
                        inp.reveal_secret = !inp.reveal_secret;
                    }
                    return UiEffect::None;
                }
                if bounds.hit_input_field((lx, ly), false) {
                    if let Some(inp) = &mut self.input_box {
                        inp.select_all = false;
                        inp.cursor = inp.char_len();
                        inp.selected_button = None;
                        inp.pressed_button = None;
                    }
                    self.ring_motion.select_input_button(None);
                    return UiEffect::None;
                }
                let hit_btn = bounds.hit_input_button((lx, ly));
                if let Some(inp) = &mut self.input_box {
                    inp.selected_button = hit_btn;
                    inp.pressed_button = hit_btn;
                }
                self.ring_motion.select_input_button(hit_btn);
            }
            return UiEffect::None;
        }

        if self.show_help {
            self.show_help = false;
            return UiEffect::None;
        }

        if !right_click
            && let Some(bounds) = self.explorer_bounds()
        {
            let total = self.menu.filtered.len();
            let grab = match self.surface {
                SurfaceMode::List => bounds.grab_list_thumb((lx, ly), self.menu.list_first_visible(), total),
                SurfaceMode::Grid => bounds.grab_thumb((lx, ly), self.grid_first_visible(), total),
                _ => None,
            };
            if let Some(g) = grab {
                self.thumb_grab = Some(g);
                self.scrollbar_dragging = true;
                self.pressed = None;
                self.hover_pos = None;
                let changed = match self.surface {
                    SurfaceMode::List => {
                        let first = bounds.dragged_list_first((lx, ly), g, total);
                        self.menu.scroll_list_to(first)
                    }
                    SurfaceMode::Grid => {
                        let first = bounds.dragged_first((lx, ly), g, total);
                        self.menu.scroll_grid_to(first)
                    }
                    _ => false,
                };
                if changed {
                    self.refresh_surface_and_motion();
                }
                return UiEffect::None;
            }
        }

        let hit = self.hit_visible(lx, ly);
        self.pressed = hit.map(|vis| (vis, right_click));
        if let Some(vis) = hit {
            self.notice = None;
            self.select_visible_for_surface(vis);
            self.ring_motion.select(Some(vis));
        }
        UiEffect::None
    }

    pub fn handle_pointer_up(&mut self, lx: f64, ly: f64, right_click: bool) -> (bool, UiEffect) {
        self.hover_pos = Some((lx, ly));

        if self.slider.is_some() {
            let was_dragging = self.slider.as_ref().is_some_and(|s| s.dragging);
            if was_dragging {
                let reduced = self.cfg.appearance.reduced_motion;
                let hovered = self
                    .explorer_bounds()
                    .is_some_and(|b| b.hit_slider_track((lx, ly)));
                if let Some(s) = &mut self.slider {
                    s.hovered = hovered;
                    s.end_drag(reduced);
                }
                return (true, UiEffect::None);
            }
            if !right_click {
                let hit_btn = self
                    .explorer_bounds()
                    .and_then(|b| b.hit_input_button((lx, ly)));
                let pressed_btn = self
                    .slider
                    .as_mut()
                    .and_then(|s| s.pressed_button.take());
                if let Some(btn) = pressed_btn {
                    if hit_btn == Some(btn) {
                        if btn == 0 {
                            if let Some(s) = self.slider.take() {
                                let val = s.value;
                                s.field.set(&mut self.cfg.appearance, val);
                                let _ = self.store.save(&self.cfg);
                                self.slider_original_value = None;
                                self.refresh_current_settings_entries();
                                self.notice = Some(format!("{} saved ({val}%)", s.field.label()));
                            }
                            return (true, UiEffect::None);
                        } else {
                            if let Some(s) = self.slider.take()
                                && let Some(orig) = self.slider_original_value.take()
                            {
                                s.field.set(&mut self.cfg.appearance, orig);
                            }
                            self.notice = None;
                            self.refresh_surface_and_motion();
                            return (true, UiEffect::None);
                        }
                    }
                    return (true, UiEffect::None);
                } else if self.dismiss_at(lx, ly) {
                    if let Some(s) = self.slider.take()
                        && let Some(orig) = self.slider_original_value.take()
                    {
                        s.field.set(&mut self.cfg.appearance, orig);
                    }
                    return (true, self.begin_close());
                }
            }
            return (false, UiEffect::None);
        }

        if let Some(cp) = &mut self.color_picker {
            if cp.dragging != ColorDrag::None {
                cp.dragging = ColorDrag::None;
                return (true, UiEffect::None);
            }
            if !right_click {
                let hit_btn = self
                    .explorer_bounds()
                    .and_then(|b| b.hit_input_button((lx, ly)));
                let pressed_btn = self
                    .color_picker
                    .as_mut()
                    .and_then(|c| c.pressed_button.take());
                if let Some(btn) = pressed_btn {
                    if hit_btn == Some(btn) {
                        if btn == 0 {
                            let valid = self
                                .color_picker
                                .as_mut()
                                .is_some_and(|cp| cp.try_apply_hex_input());
                            if !valid {
                                if let Some(cp) = &mut self.color_picker {
                                    cp.error = Some(
                                        "Enter a valid #RRGGBB or #RRGGBBAA hex color".into(),
                                    );
                                }
                                return (true, UiEffect::None);
                            }
                            if let Some(cp) = self.color_picker.take() {
                                let hex = cp.color().to_hex();
                                let field = cp.field;
                                field.set(&mut self.cfg.colors, hex.clone());
                                self.effective_colors = self.cfg.effective_colors(&self.store);
                                let _ = self.store.save(&self.cfg);
                                self.color_original_colors = None;
                                self.refresh_current_settings_entries();
                                self.notice = Some(format!("{} saved ({hex})", field.label()));
                            }
                            return (true, UiEffect::None);
                        } else {
                            self.color_picker = None;
                            if let Some(orig) = self.color_original_colors.take() {
                                self.cfg.colors = orig;
                                self.effective_colors = self.cfg.effective_colors(&self.store);
                            }
                            self.notice = None;
                            self.refresh_surface_and_motion();
                            return (true, UiEffect::None);
                        }
                    }
                    return (true, UiEffect::None);
                } else if self.dismiss_at(lx, ly) {
                    self.color_picker = None;
                    if let Some(orig) = self.color_original_colors.take() {
                        self.cfg.colors = orig;
                        self.effective_colors = self.cfg.effective_colors(&self.store);
                    }
                    return (true, self.begin_close());
                }
            }
            return (false, UiEffect::None);
        }

        if self.input_box.is_some() {
            if !right_click {
                let hit_btn = self
                    .explorer_bounds()
                    .and_then(|b| b.hit_input_button((lx, ly)));
                let pressed_btn = self
                    .input_box
                    .as_mut()
                    .and_then(|i| i.pressed_button.take());
                if let Some(btn) = pressed_btn {
                    if hit_btn == Some(btn) {
                        if btn == 0 {
                            return (true, self.submit_input_box());
                        } else {
                            self.input_box = None;
                            self.input_purpose = None;
                            self.notice = None;
                            self.refresh_surface_and_motion();
                            return (true, UiEffect::None);
                        }
                    }
                    return (true, UiEffect::None);
                } else if self.dismiss_at(lx, ly) {
                    self.input_box = None;
                    self.input_purpose = None;
                    return (true, self.begin_close());
                }
            }
            return (false, UiEffect::None);
        }

        if self.thumb_grab.take().is_some() {
            self.scrollbar_dragging = false;
            return (true, UiEffect::None);
        }
        self.scrollbar_dragging = false;

        let pressed = self.pressed.take();
        let hit = self.hit_visible(lx, ly);
        if let Some(vis) = hit {
            if pressed == Some((vis, right_click)) {
                self.select_visible_for_surface(vis);
                let eff = self.activate_selected(right_click);
                return (true, eff);
            }
            return (pressed.is_some(), UiEffect::None);
        }

        if pressed.is_none() && self.dismiss_at(lx, ly) {
            let eff = if right_click {
                self.step_back()
            } else {
                self.begin_close()
            };
            return (true, eff);
        }
        (pressed.is_some(), UiEffect::None)
    }

    pub fn handle_wheel(&mut self, delta: f64) -> bool {
        self.hover_pos = None;
        if let Some(s) = &mut self.slider {
            let dir: i16 = if delta > 0.0 { 5 } else { -5 };
            let reduced = self.cfg.appearance.reduced_motion;
            if s.step_by(dir, reduced) {
                let val = s.value;
                s.field.set(&mut self.cfg.appearance, val);
                return true;
            }
            return false;
        }
        if let Some(cp) = &mut self.color_picker {
            let dir = if delta > 0.0 { 1.0 } else { -1.0 };
            cp.h = (cp.h + dir * 6.0).rem_euclid(360.0);
            cp.sync_hex();
            let hex = cp.color().to_hex();
            cp.field.set(&mut self.cfg.colors, hex);
            self.effective_colors = self.cfg.effective_colors(&self.store);
            return true;
        }
        let mut changed = false;
        self.scroll_accum += delta;
        let threshold = 120.0;
        while self.scroll_accum.abs() >= threshold {
            let dir = if self.scroll_accum > 0.0 { -1 } else { 1 };
            self.scroll_accum -= if self.scroll_accum > 0.0 {
                threshold
            } else {
                -threshold
            };
            match self.surface {
                SurfaceMode::List => {
                    self.notice = None;
                    if self.menu.list_max_scroll() > 0 {
                        if self.menu.scroll_list_rows(dir) {
                            self.refresh_surface_and_motion();
                            changed = true;
                        }
                    } else {
                        self.menu.move_list(dir);
                        self.refresh_surface_and_motion();
                        changed = true;
                    }
                }
                SurfaceMode::Grid => {
                    self.notice = None;
                    if self.menu.scroll_grid_rows(dir) {
                        self.refresh_surface_and_motion();
                        changed = true;
                    }
                }
                SurfaceMode::Ring => {
                    self.notice = None;
                    if self.menu.page_count(PAGE_SIZE) > 1 {
                        if dir > 0 {
                            self.menu.next_page(PAGE_SIZE);
                        } else {
                            self.menu.prev_page(PAGE_SIZE);
                        }
                    } else {
                        self.menu.step_selection(dir, PAGE_SIZE);
                    }
                    self.refresh_surface_and_motion();
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }

    pub fn step_back(&mut self) -> UiEffect {
        self.notice = None;
        if self.mode == Mode::Main {
            return self.begin_close();
        }
        if self.mode != Mode::Files
            && let Some(restored) = self.menu.undo_completion()
        {
            if self.back_stack.last().map(String::as_str) == Some(restored.as_str()) {
                self.back_stack.pop();
            }
            self.typing_in_search = !restored.is_empty();
            self.refresh_surface_and_motion();
            return UiEffect::None;
        }
        if let Some(prev) = self.back_stack.pop() {
            if prev != self.menu.query {
                self.menu.query = prev;
                self.menu.clear_completion();
                self.typing_in_search = false;
                if self.mode == Mode::Files {
                    self.reload_files_for_query();
                } else {
                    self.menu.refilter(true);
                    self.refresh_surface_and_motion();
                }
            }
            return UiEffect::None;
        }
        match self.mode.clone() {
            Mode::Main => self.begin_close(),
            Mode::Settings => {
                if self.settings_stack.len() > 1 {
                    self.settings_stack.pop();
                    self.back_stack.clear();
                    self.typing_in_search = false;
                    self.menu.set_query(String::new());
                    self.rebuild_settings_entries();
                    return UiEffect::None;
                }
                if self.standalone {
                    return self.begin_close();
                }
                self.enter_main_menu();
                UiEffect::None
            }
            Mode::Custom(_) => {
                if self.custom_stack.len() > 1 {
                    self.custom_stack.pop();
                    if let Some(prev) = self.custom_stack.last().cloned() {
                        self.enter_custom_menu(prev.id, prev.config, false);
                        return UiEffect::None;
                    }
                }
                if self.standalone {
                    return self.begin_close();
                }
                self.enter_main_menu();
                UiEffect::None
            }
            Mode::Files | Mode::Apps | Mode::Windows | Mode::Clipboard => {
                if self.mode == Mode::Clipboard && self.confirming_clipboard_clear {
                    self.confirming_clipboard_clear = false;
                    self.back_stack.clear();
                    self.typing_in_search = false;
                    self.menu.set_query(String::new());
                    self.reload_clipboard_entries();
                    return UiEffect::None;
                }
                if self.standalone {
                    return self.begin_close();
                }
                self.enter_main_menu();
                UiEffect::None
            }
        }
    }

    pub fn activate_selected(&mut self, secondary: bool) -> UiEffect {
        let Some(entry) = self.menu.selected_entry().cloned() else {
            return UiEffect::None;
        };

        match self.mode.clone() {
            Mode::Main => {
                if !secondary
                    && let Err(e) = self.open_menu_target(&entry.payload, true)
                {
                    self.notice = Some(e);
                }
                UiEffect::None
            }
            Mode::Files => {
                if secondary {
                    let p = std::path::Path::new(&entry.payload);
                    if p.is_dir() {
                        self.history.bump_path(&entry.payload);
                        let _ = self.history.save();
                    } else if let Some(parent) = p.parent()
                        && !parent.as_os_str().is_empty()
                    {
                        self.history.bump_path(&parent.to_string_lossy());
                        let _ = self.history.save();
                    }
                    let _ = self.backend.invoke(
                        BuiltinSubmenu::Files,
                        &SubmenuRequest::FilesOpenFolder { path: entry.payload },
                    );
                    return self.begin_close();
                }
                if entry.completion.as_ref().is_some_and(|c| c.ends_with('\\') || c.ends_with('/')) {
                    self.history.bump_path(&entry.payload);
                    let _ = self.history.save();
                    let next_query = entry.completion.unwrap();
                    if self.menu.query != next_query {
                        let current = self.menu.query.clone();
                        self.push_back(&current);
                        self.menu.query = next_query;
                    }
                    self.typing_in_search = false;
                    self.reload_files_for_query();
                    return UiEffect::None;
                }
                self.history.bump_path(&entry.payload);
                let _ = self.history.save();
                let _ = self.backend.invoke(
                    BuiltinSubmenu::Files,
                    &SubmenuRequest::FilesOpen { path: entry.payload },
                );
                self.begin_close()
            }
            Mode::Apps => {
                self.history.bump_app(&entry.key);
                let _ = self.history.save();
                match self.backend.invoke(
                    BuiltinSubmenu::Apps,
                    &SubmenuRequest::AppsLaunch {
                        id: entry.key,
                        launch_target: entry.payload,
                    },
                ) {
                    Ok(SubmenuResponse::ActionOutcome { ok: false, message }) => {
                        self.notice = Some(message);
                        UiEffect::None
                    }
                    Err(e) => {
                        self.notice = Some(e);
                        UiEffect::None
                    }
                    _ => self.begin_close(),
                }
            }
            Mode::Windows => {
                let hwnd = entry.payload.parse::<u64>().unwrap_or(0);
                let close_now = matches!(self.begin_close(), UiEffect::CloseApp);
                // Defer WindowsFocus to UiEffect so SetForegroundWindow runs AFTER the
                // Win32 WndProc releases its RefCell borrow on UiRuntime and mouse capture.
                UiEffect::FocusWindow { hwnd, close_now }
            }
            Mode::Clipboard => {
                let data_dir = self.store.dir().to_string_lossy().into_owned();
                if entry.payload == "clip_clear" {
                    self.confirming_clipboard_clear = true;
                    self.back_stack.clear();
                    self.typing_in_search = false;
                    self.menu.set_query(String::new());
                    self.sync_items_and_motion(
                        vec![
                            Entry {
                                key: "clip:clear_confirm".into(),
                                title: "Clear All".into(),
                                detail: "Permanently delete all saved clips".into(),
                                icon: BuiltinIcon::Forget,
                                icon_path: None,
                                exe_icon_path: None,
                                accent_override: None,
                                active: false,
                                completion: None,
                                payload: "clip_clear_confirm".into(),
                                secondary_payload: None,
                            },
                            Entry {
                                key: "clip:clear_cancel".into(),
                                title: "Cancel".into(),
                                detail: "Keep saved clipboard history".into(),
                                icon: BuiltinIcon::Disconnect,
                                icon_path: None,
                                exe_icon_path: None,
                                accent_override: None,
                                active: false,
                                completion: None,
                                payload: "clip_clear_cancel".into(),
                                secondary_payload: None,
                            },
                        ],
                        true,
                    );
                    return UiEffect::None;
                }
                if entry.payload == "clip_clear_cancel" {
                    self.confirming_clipboard_clear = false;
                    self.reload_clipboard_entries();
                    return UiEffect::None;
                }
                if entry.payload == "clip_clear_confirm" {
                    self.confirming_clipboard_clear = false;
                    match self.backend.invoke(
                        BuiltinSubmenu::Clipboard,
                        &SubmenuRequest::ClipboardClear { data_dir },
                    ) {
                        Ok(SubmenuResponse::ActionOutcome { ok: true, message }) => {
                            self.notice = Some(message);
                            self.reload_clipboard_entries();
                        }
                        Ok(SubmenuResponse::ActionOutcome { ok: false, message }) => {
                            self.notice = Some(message);
                        }
                        Err(e) => self.notice = Some(e),
                        _ => {}
                    }
                    return UiEffect::None;
                }
                if secondary {
                    if let Some(id) = entry
                        .secondary_payload
                        .as_deref()
                        .and_then(|s| s.strip_prefix("clip_del:"))
                    {
                        match self.backend.invoke(
                            BuiltinSubmenu::Clipboard,
                            &SubmenuRequest::ClipboardDelete {
                                data_dir,
                                id: id.to_string(),
                            },
                        ) {
                            Ok(SubmenuResponse::ActionOutcome { ok: true, message }) => {
                                self.notice = Some(message);
                                self.reload_clipboard_entries();
                            }
                            Ok(SubmenuResponse::ActionOutcome { ok: false, message }) => {
                                self.notice = Some(message);
                            }
                            Err(e) => self.notice = Some(e),
                            _ => {}
                        }
                    }
                    return UiEffect::None;
                }
                if let Some(id) = entry.payload.strip_prefix("clip_copy:") {
                    match self.backend.invoke(
                        BuiltinSubmenu::Clipboard,
                        &SubmenuRequest::ClipboardCopyToClipboard {
                            data_dir,
                            id: id.to_string(),
                        },
                    ) {
                        Ok(SubmenuResponse::ActionOutcome { ok: true, .. }) => {
                            return self.begin_close();
                        }
                        Ok(SubmenuResponse::ActionOutcome { ok: false, message }) => {
                            self.notice = Some(message);
                        }
                        Err(e) => self.notice = Some(e),
                        _ => {}
                    }
                }
                UiEffect::None
            }
            Mode::Settings => {
                let idx = entry.payload.parse::<usize>().unwrap_or(usize::MAX);
                if let Some(item) = self.settings_items.get(idx).cloned() {
                    let act = if secondary {
                        match item.secondary_action {
                            Some(sec) => sec,
                            None => return UiEffect::None,
                        }
                    } else {
                        item.action
                    };
                    return self.handle_settings_action(act);
                }
                UiEffect::None
            }
            Mode::Custom(_) => self.activate_custom_entry(&entry, secondary),
        }
    }

    fn activate_custom_entry(&mut self, entry: &Entry, secondary: bool) -> UiEffect {
        let Some(top) = self.custom_stack.last().cloned() else {
            return UiEffect::None;
        };
        let idx = entry.payload.parse::<usize>().unwrap_or(usize::MAX);
        let item_opt = top.config.items.get(idx);
        let spec_opt = if let Some(item) = item_opt {
            if secondary {
                item.right_click.clone()
            } else {
                item.primary_action()
            }
        } else {
            None
        };
        let item_val = item_opt
            .and_then(|i| i.value.as_deref())
            .filter(|v| !v.is_empty())
            .unwrap_or(&entry.detail);

        if let Some(spec) = spec_opt {
            return self.execute_custom_action_spec(&spec, Some(&entry.title), Some(item_val), None);
        }
        UiEffect::None
    }

    fn execute_custom_action_spec(
        &mut self,
        spec: &MenuActionSpec,
        label: Option<&str>,
        value: Option<&str>,
        input: Option<&str>,
    ) -> UiEffect {
        if input.is_none()
            && let Some(prompt) = &spec.input
        {
            let mut next_spec = spec.clone();
            next_spec.input = None;
            self.input_box = Some(InputBoxState::new(
                prompt.clone(),
                spec.input_placeholder
                    .clone()
                    .unwrap_or_else(|| "Type value...".into()),
                spec.secret,
            ));
            self.input_purpose = Some(InputSubmitPurpose::CustomMenuInput {
                spec: next_spec,
                label: label.map(ToOwned::to_owned),
                value: value.map(ToOwned::to_owned),
            });
            self.refresh_surface_and_motion();
            return UiEffect::None;
        }
        if let Some(open_id) = &spec.open_menu {
            let _ = self.open_menu_target(open_id, true);
            return UiEffect::None;
        }
        if let Some(sub) = &spec.submenu {
            match sub {
                SubmenuTarget::Inline(sub_cfg) => {
                    self.enter_custom_menu(format!("inline_{}", self.custom_stack.len()), (**sub_cfg).clone(), true);
                    return UiEffect::None;
                }
                SubmenuTarget::Ref(ref_id) => {
                    let _ = self.open_menu_target(ref_id, true);
                    return UiEffect::None;
                }
            }
        }
        if let Some(cmd_tmpl) = &spec.command {
            let expanded = expand_command_template(cmd_tmpl, input, label, value);
            if spec.reload {
                if let Err(e) = spawn_command_detached(&expanded) {
                    self.notice = Some(e);
                } else if let Some(top) = self.custom_stack.last().cloned() {
                    self.enter_custom_menu(top.id, top.config, false);
                }
                return UiEffect::None;
            }
            if let Err(e) = spawn_command_detached(&expanded) {
                self.notice = Some(e);
                return UiEffect::None;
            }
            if !spec.keep_open {
                return self.begin_close();
            }
        }
        UiEffect::None
    }

    pub fn import_theme_from_path(&mut self, path: &std::path::Path) {
        self.input_box = None;
        self.input_purpose = None;
        self.refresh_surface_and_motion();
        match self.store.import_theme_file(path) {
            Ok(theme_id) => {
                self.cfg.appearance.theme = theme_id.clone();
                let fresh = self
                    .store
                    .resolve_theme_colors(&theme_id)
                    .unwrap_or_default();
                self.cfg.colors = fresh.clone();
                self.effective_colors = fresh;
                let _ = self.store.save(&self.cfg);
                self.rebuild_settings_entries();
                self.notice = Some(format!("Imported & applied '{theme_id}'"));
            }
            Err(e) => self.notice = Some(e),
        }
    }

    pub fn import_menu_from_path(&mut self, path: &std::path::Path) -> UiEffect {
        self.input_box = None;
        self.input_purpose = None;
        self.refresh_surface_and_motion();
        match self.store.import_menu_file(path, &self.installed) {
            Ok(menu_id) => {
                if let Ok(reloaded) = self.store.load_or_create(&self.installed) {
                    self.cfg = reloaded;
                    self.effective_colors = self.cfg.effective_colors(&self.store);
                }
                self.rebuild_settings_entries();
                self.notice = Some(format!("Imported menu '{menu_id}'"));
                UiEffect::NotifyDaemonReload
            }
            Err(e) => {
                self.notice = Some(e);
                UiEffect::None
            }
        }
    }

    fn submit_input_box(&mut self) -> UiEffect {
        let Some(mut inp) = self.input_box.take() else {
            return UiEffect::None;
        };
        let Some(purpose) = self.input_purpose.take() else {
            return UiEffect::None;
        };
        let text = inp.text.clone();
        match purpose {
            InputSubmitPurpose::CustomMenuInput { spec, label, value } => {
                self.refresh_surface_and_motion();
                self.execute_custom_action_spec(
                    &spec,
                    label.as_deref(),
                    value.as_deref().or(Some(&text)),
                    Some(&text),
                )
            }
            InputSubmitPurpose::ImportThemePath => {
                let clean = text.trim().trim_matches('"');
                if clean.is_empty() {
                    inp.error = Some("Enter a valid .json or .zip theme path".into());
                    self.input_box = Some(inp);
                    self.input_purpose = Some(InputSubmitPurpose::ImportThemePath);
                    return UiEffect::None;
                }
                let path = std::path::PathBuf::from(clean);
                match self.store.import_theme_file(&path) {
                    Ok(theme_id) => {
                        self.cfg.appearance.theme = theme_id.clone();
                        let fresh = self
                            .store
                            .resolve_theme_colors(&theme_id)
                            .unwrap_or_default();
                        self.cfg.colors = fresh.clone();
                        self.effective_colors = fresh;
                        let _ = self.store.save(&self.cfg);
                        self.rebuild_settings_entries();
                        self.notice = Some(format!("Imported & applied '{theme_id}'"));
                    }
                    Err(e) => {
                        inp.error = Some(e);
                        self.input_box = Some(inp);
                        self.input_purpose = Some(InputSubmitPurpose::ImportThemePath);
                    }
                }
                UiEffect::None
            }
            InputSubmitPurpose::ImportMenuPath => {
                let clean = text.trim().trim_matches('"');
                if clean.is_empty() {
                    inp.error = Some("Enter a valid .json or .zip menu path".into());
                    self.input_box = Some(inp);
                    self.input_purpose = Some(InputSubmitPurpose::ImportMenuPath);
                    return UiEffect::None;
                }
                let path = std::path::PathBuf::from(clean);
                match self.store.import_menu_file(&path, &self.installed) {
                    Ok(menu_id) => {
                        if let Ok(reloaded) = self.store.load_or_create(&self.installed) {
                            self.cfg = reloaded;
                            self.effective_colors = self.cfg.effective_colors(&self.store);
                        }
                        self.rebuild_settings_entries();
                        self.notice = Some(format!("Imported menu '{menu_id}'"));
                        UiEffect::NotifyDaemonReload
                    }
                    Err(e) => {
                        inp.error = Some(e);
                        self.input_box = Some(inp);
                        self.input_purpose = Some(InputSubmitPurpose::ImportMenuPath);
                        UiEffect::None
                    }
                }
            }
        }
    }

    pub fn handle_settings_action(&mut self, action: SettingsAction) -> UiEffect {
        match action {
            SettingsAction::OpenView(view) => {
                self.notice = None;
                self.settings_stack.push(view);
                self.rebuild_settings_entries();
                UiEffect::None
            }
            SettingsAction::GoBack => self.step_back(),
            SettingsAction::SetTheme(id) => {
                self.cfg.appearance.theme = id.clone();
                let fresh = self
                    .store
                    .resolve_theme_colors(&id)
                    .unwrap_or_default();
                self.cfg.colors = fresh.clone();
                self.effective_colors = fresh;
                let _ = self.store.save(&self.cfg);
                self.refresh_current_settings_entries();
                self.notice = Some(format!("Theme set to {id}"));
                UiEffect::None
            }
            SettingsAction::ImportTheme => {
                self.input_box = Some(InputBoxState::new(
                    "Import Theme (.json or .zip)".into(),
                    "C:\\path\\to\\theme.json".into(),
                    false,
                ));
                self.input_purpose = Some(InputSubmitPurpose::ImportThemePath);
                self.refresh_surface_and_motion();
                UiEffect::PickThemeFile
            }
            SettingsAction::ImportMenu => {
                self.input_box = Some(InputBoxState::new(
                    "Import Menu (.json or .zip)".into(),
                    "C:\\path\\to\\menu.json".into(),
                    false,
                ));
                self.input_purpose = Some(InputSubmitPurpose::ImportMenuPath);
                self.refresh_surface_and_motion();
                UiEffect::PickMenuFile
            }
            SettingsAction::ToggleAppearance(toggle) => {
                let notice = match toggle {
                    AppearanceToggle::OverflowLayout => {
                        self.cfg.appearance.layout = self.cfg.appearance.layout.toggle();
                        if self.cfg.appearance.layout == OverflowLayout::Box {
                            "Overflow layout: Box"
                        } else {
                            "Overflow layout: Rings"
                        }
                    }
                    AppearanceToggle::Animations => {
                        self.cfg.appearance.reduced_motion = !self.cfg.appearance.reduced_motion;
                        if !self.cfg.appearance.reduced_motion {
                            "Animations: On"
                        } else {
                            "Animations: Off"
                        }
                    }
                    AppearanceToggle::Animation3d => {
                        self.cfg.appearance.loading_animation =
                            self.cfg.appearance.loading_animation.toggle();
                        if self.cfg.appearance.loading_animation == LoadingStyle::ThreeD {
                            "3D Loading: On"
                        } else {
                            "3D Loading: Off"
                        }
                    }
                    AppearanceToggle::HighContrast => {
                        self.cfg.appearance.high_contrast = !self.cfg.appearance.high_contrast;
                        if self.cfg.appearance.high_contrast {
                            "High Contrast: On"
                        } else {
                            "High Contrast: Off"
                        }
                    }
                };
                let _ = self.store.save(&self.cfg);
                self.refresh_current_settings_entries();
                self.notice = Some(notice.into());
                UiEffect::None
            }
            SettingsAction::AdjustSlider(field) => {
                let current = field.get(&self.cfg.appearance);
                self.slider_original_value = Some(current);
                self.slider = Some(SliderState::new(field, current));
                self.refresh_surface_and_motion();
                self.notice = Some(format!("Adjusting {}", field.label()));
                UiEffect::None
            }
            SettingsAction::ResetAppearance => {
                let theme = self.cfg.appearance.theme.clone();
                let font = self.cfg.appearance.font.clone();
                self.cfg.appearance = AppearanceConfig {
                    theme,
                    font,
                    ..AppearanceConfig::default()
                };
                let _ = self.store.save(&self.cfg);
                self.refresh_current_settings_entries();
                self.notice = Some("Appearance reset to defaults".into());
                UiEffect::None
            }
            SettingsAction::PickColor(field) => {
                self.color_original_colors = Some(self.cfg.colors.clone());
                let orig = field.get(&self.cfg.colors).to_string();
                self.color_picker = Some(ColorPickerState::new(field, &orig));
                self.refresh_surface_and_motion();
                self.notice = Some(format!("Editing {}", field.label()));
                UiEffect::None
            }
            SettingsAction::ResetColors => {
                let fresh = self
                    .store
                    .resolve_theme_colors(&self.cfg.appearance.theme)
                    .unwrap_or_default();
                self.cfg.colors = fresh.clone();
                self.effective_colors = fresh;
                let _ = self.store.save(&self.cfg);
                self.refresh_current_settings_entries();
                self.notice = Some("Colors reset to theme defaults".into());
                UiEffect::None
            }
            SettingsAction::ToggleNumberShortcuts => {
                self.cfg.keybinds.number_shortcuts = !self.cfg.keybinds.number_shortcuts;
                let _ = self.store.save(&self.cfg);
                self.refresh_current_settings_entries();
                self.notice = Some(if self.cfg.keybinds.number_shortcuts {
                    "Number shortcuts enabled".into()
                } else {
                    "Number shortcuts disabled".into()
                });
                UiEffect::None
            }
            SettingsAction::CaptureMenuHotkey { menu_id, label } => {
                self.notice = Some(format!("Press global hotkey for {label} (Esc cancels)"));
                self.capture = Some(CaptureTarget::MenuHotkey { menu_id, label });
                UiEffect::None
            }
            SettingsAction::ClearMenuHotkey(menu_id) => {
                match menu_id {
                    None => self.cfg.keybinds.main_menu = KeyList::default(),
                    Some(ref id) => self.cfg.keybinds.set_menu_hotkey(id, KeyList::default()),
                }
                let _ = self.store.save(&self.cfg);
                self.refresh_current_settings_entries();
                self.notice = Some("Global hotkey cleared".into());
                UiEffect::NotifyDaemonReload
            }
            SettingsAction::CaptureNavKeybind(field) => {
                self.notice = Some(format!("Press key for {} (Esc cancels)", field.label()));
                self.capture = Some(CaptureTarget::Nav(field));
                UiEffect::None
            }
            SettingsAction::ClearNavKeybind(field) => {
                field.set(&mut self.cfg.keybinds, KeyList::default());
                let _ = self.store.save(&self.cfg);
                self.refresh_current_settings_entries();
                self.notice = Some(format!("{} cleared", field.label()));
                UiEffect::None
            }
            SettingsAction::ResetKeybinds => {
                self.cfg.keybinds = recircles_core::config::KeybindsConfig::default();
                let _ = self.store.save(&self.cfg);
                self.refresh_current_settings_entries();
                self.notice = Some("Keybinds reset to defaults".into());
                UiEffect::NotifyDaemonReload
            }
            SettingsAction::AddMainMenuEntry(id) => {
                if !self.cfg.main_menu.iter().any(|e| e.menu == id) {
                    self.cfg
                        .main_menu
                        .push(recircles_core::config::MainMenuEntry::new(id));
                    let _ = self.store.save(&self.cfg);
                }
                if self.settings_stack.len() > 1 {
                    self.settings_stack.pop();
                }
                self.rebuild_settings_entries();
                self.notice = Some("Pinned to main menu".into());
                UiEffect::None
            }
            SettingsAction::RemoveMainMenuEntry(idx) => {
                if idx < self.cfg.main_menu.len() && self.cfg.main_menu[idx].menu != "settings" {
                    self.cfg.main_menu.remove(idx);
                    let _ = self.store.save(&self.cfg);
                    if matches!(self.settings_stack.last(), Some(SettingsView::MainMenuEntry(_)))
                        && self.settings_stack.len() > 1
                    {
                        self.settings_stack.pop();
                    }
                    self.rebuild_settings_entries();
                    self.notice = Some("Removed from main menu".into());
                }
                UiEffect::None
            }
            SettingsAction::MoveMainMenuEntry { index, delta } => {
                if apply_main_menu_move(&mut self.cfg.main_menu, index, delta) {
                    if let Some(SettingsView::MainMenuEntry(cur_idx)) =
                        self.settings_stack.last_mut()
                        && !self.cfg.main_menu.is_empty()
                    {
                        let len = self.cfg.main_menu.len() as isize;
                        *cur_idx = (index as isize + delta).rem_euclid(len) as usize;
                    }
                    let _ = self.store.save(&self.cfg);
                    self.rebuild_settings_entries();
                }
                UiEffect::None
            }
            SettingsAction::ResetMainMenu => {
                self.cfg.main_menu = AppConfig::default_for_installed(&self.installed).main_menu;
                let _ = self.store.save(&self.cfg);
                self.rebuild_settings_entries();
                self.notice = Some("Main menu reset to defaults".into());
                UiEffect::None
            }
            SettingsAction::ResetMemory => {
                match self.history.reset_in_place() {
                    Ok(()) => self.notice = Some("Usage memory cleared".into()),
                    Err(e) => self.notice = Some(format!("Reset failed: {e}")),
                }
                if self.settings_stack.len() > 1 {
                    self.settings_stack.pop();
                }
                self.rebuild_settings_entries();
                UiEffect::None
            }
            SettingsAction::ResetAllSettings => {
                self.cfg = AppConfig::default_for_installed(&self.installed);
                self.effective_colors = self.cfg.effective_colors(&self.store);
                let _ = self.store.save(&self.cfg);
                self.settings_stack = vec![SettingsView::Root];
                self.rebuild_settings_entries();
                self.notice = Some("All settings reset to defaults".into());
                UiEffect::NotifyDaemonReload
            }
        }
    }

    #[allow(dead_code)]
    pub fn active_accent(&self) -> recircles_core::colors::Color {
        if let Some(e) = self.menu.selected_entry()
            && let Some(hex) = &e.accent_override
            && let Ok(c) = recircles_core::colors::Color::from_hex(hex)
        {
            return c;
        }
        match &self.mode {
            Mode::Main => {
                if let Some(e) = self.menu.selected_entry() {
                    return self.effective_colors.accent_for_mode(&e.key);
                }
                self.effective_colors.accent_color()
            }
            Mode::Custom(id) => {
                if let Some(hex) = self.cfg.menus.get(id).and_then(|m| m.accent.as_deref())
                    && let Ok(c) = recircles_core::colors::Color::from_hex(hex)
                {
                    return c;
                }
                self.effective_colors.accent_color()
            }
            other => self.effective_colors.accent_for_mode(other.id()),
        }
    }
}

fn save_apps_disk_cache(path: &std::path::Path, apps: &[recircles_core::submenu_abi::AppDto]) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(apps) else {
        return;
    };
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::write(&tmp, &bytes).is_ok() {
        if std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(path);
            if std::fs::rename(&tmp, path).is_err() {
                let _ = std::fs::remove_file(&tmp);
                let _ = std::fs::write(path, &bytes);
            }
        }
    }
}

fn file_dto_to_entry(dto: recircles_core::submenu_abi::FileEntryDto) -> Entry {
    Entry {
        key: dto.path.clone(),
        title: dto.label,
        detail: dto.path.clone(),
        icon: if dto.is_home {
            BuiltinIcon::Home
        } else if dto.is_pin {
            BuiltinIcon::Pin
        } else if dto.is_dir {
            BuiltinIcon::Folder
        } else {
            BuiltinIcon::File
        },
        icon_path: None,
        exe_icon_path: None,
        accent_override: None,
        active: dto.is_pin || dto.is_home,
        completion: Some(dto.completion),
        payload: dto.path,
        secondary_payload: None,
    }
}

fn parse_dynamic_menu_stdout(stdout: &str) -> Result<Vec<MenuItemConfig>, String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.starts_with('[')
        && let Ok(items) = serde_json::from_str::<Vec<MenuItemConfig>>(trimmed)
    {
        return Ok(items);
    }
    if trimmed.starts_with('{')
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(items_val) = val.get("items")
        && let Ok(items) = serde_json::from_value::<Vec<MenuItemConfig>>(items_val.clone())
    {
        return Ok(items);
    }
    Ok(trimmed
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let clean = line.trim().to_string();
            MenuItemConfig {
                label: clean.clone(),
                value: Some(clean.clone()),
                command: Some(clean),
                ..Default::default()
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct MockBackend {
        installed: InstalledModules,
        calls: Mutex<Vec<String>>,
    }

    impl BackendBridge for MockBackend {
        fn invoke(&self, sub: BuiltinSubmenu, req: &SubmenuRequest) -> Result<SubmenuResponse, String> {
            let s = serde_json::to_string(req).unwrap();
            self.calls.lock().unwrap().push(s);
            match (sub, req) {
                (BuiltinSubmenu::Windows, SubmenuRequest::WindowsList) => {
                    Ok(SubmenuResponse::WindowsList {
                        windows: vec![recircles_core::submenu_abi::WindowDto {
                            hwnd: 0x1234_5678,
                            title: "Notepad".into(),
                            app_id: "notepad".into(),
                            exe_path: String::new(),
                            minimized: false,
                        }],
                    })
                }
                (BuiltinSubmenu::Clipboard, SubmenuRequest::ClipboardList { .. }) => {
                    Ok(SubmenuResponse::ClipboardItems {
                        items: vec![recircles_core::submenu_abi::ClipboardItemDto {
                            id: "clip1".into(),
                            kind: ClipboardKind::Text,
                            label: "hello clip".into(),
                            detail: "10 chars".into(),
                            text: Some("hello clip".into()),
                            files: Vec::new(),
                            image_path: None,
                            width: 0,
                            height: 0,
                            hash: "h1".into(),
                            timestamp_secs: 100,
                        }],
                    })
                }
                (BuiltinSubmenu::Clipboard, SubmenuRequest::ClipboardDelete { .. }) => {
                    Ok(SubmenuResponse::ActionOutcome {
                        ok: true,
                        message: "Deleted".into(),
                    })
                }
                _ => Ok(SubmenuResponse::ActionOutcome {
                    ok: true,
                    message: "ok".into(),
                }),
            }
        }

        fn is_installed(&self, sub: BuiltinSubmenu) -> bool {
            self.installed.is_installed(sub)
        }
    }

    fn temp_app(with_clipboard: bool) -> (PathBuf, App, Arc<MockBackend>) {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("recircles_app_test_{nanos}"));
        let sub_dir = dir.join("submenus");
        std::fs::create_dir_all(&sub_dir).unwrap();
        if with_clipboard {
            std::fs::write(sub_dir.join("recircles_clipboard.dll"), b"dll").unwrap();
        }
        let installed = InstalledModules::detect_in(sub_dir);
        let store = ConfigStore::in_dir(dir.clone());
        let backend = Arc::new(MockBackend {
            installed: installed.clone(),
            calls: Mutex::new(Vec::new()),
        });
        let app = App::new(store, installed, backend.clone(), None).unwrap();
        (dir, app, backend)
    }

    #[test]
    fn refuses_uninstalled_optional_submenu_and_handles_reset_memory_and_keybind_reload() {
        let (dir, mut app, _backend) = temp_app(false);
        // Opening uninstalled clipboard fails cleanly!
        assert!(app.open_menu_target("clipboard", true).is_err());

        // Add usage history, open Settings -> Reset Memory -> Confirm -> clears history
        app.history.bump_app("test.exe");
        app.history.save().unwrap();
        assert_eq!(app.history.app_score("test.exe"), 1);

        app.enter_settings(SettingsView::ConfirmResetMemory);
        let eff = app.handle_settings_action(SettingsAction::ResetMemory);
        assert_eq!(eff, UiEffect::None);
        assert_eq!(app.history.app_score("test.exe"), 0);

        // Capture global hotkey for Main Ring emits NotifyDaemonReload and updates config!
        app.enter_settings(SettingsView::Keybinds);
        app.handle_settings_action(SettingsAction::CaptureMenuHotkey {
            menu_id: None,
            label: "Main Ring".into(),
        });
        let eff2 = app.handle_key_down(
            Modifiers {
                ctrl: true,
                alt: true,
                shift: false,
                win: false,
            },
            0x4B, // 'K'
            Some('k'),
        );
        assert_eq!(eff2, UiEffect::NotifyDaemonReload);
        assert_eq!(app.cfg.keybinds.main_menu.primary(), Some("Ctrl+Alt+k"));

        // Capture Win+L is rejected and stays in capture mode
        app.handle_settings_action(SettingsAction::CaptureMenuHotkey {
            menu_id: None,
            label: "Main Ring".into(),
        });
        let eff_locked = app.handle_key_down(
            Modifiers {
                win: true,
                ..Default::default()
            },
            b'L' as u32,
            None,
        );
        assert_eq!(eff_locked, UiEffect::None);
        assert!(app.capture.is_some());

        // Capture Win+B succeeds and emits NotifyDaemonReload
        let eff_win_b = app.handle_key_down(
            Modifiers {
                win: true,
                ..Default::default()
            },
            b'B' as u32,
            None,
        );
        assert_eq!(eff_win_b, UiEffect::NotifyDaemonReload);
        assert_eq!(app.cfg.keybinds.main_menu.primary(), Some("Win+b"));

        // Capture standalone Win tap (VK_LWIN with empty mods) succeeds
        app.handle_settings_action(SettingsAction::CaptureMenuHotkey {
            menu_id: None,
            label: "Main Ring".into(),
        });
        let eff_win_alone = app.handle_key_down(Modifiers::default(), VK_LWIN, None);
        assert_eq!(eff_win_alone, UiEffect::NotifyDaemonReload);
        assert_eq!(app.cfg.keybinds.main_menu.primary(), Some("Win"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clipboard_secondary_click_deletes_and_reloads_entries() {
        let (dir, mut app, backend) = temp_app(true);
        app.enter_clipboard();
        assert_eq!(app.mode, Mode::Clipboard);
        assert_eq!(app.menu.items.len(), 2); // clip1 + Clear Clipboard History

        app.activate_selected(true);
        let calls = backend.calls.lock().unwrap();
        assert!(calls.iter().any(|c| c.contains("clipboard_delete")));
        assert!(calls.iter().filter(|c| c.contains("clipboard_list")).count() >= 2);
        drop(calls);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn supports_dynamic_on_open_custom_items_input_actions_conflicts_and_number_shortcuts() {
        let (dir, mut app, _backend) = temp_app(true);

        // Hotkey conflict rejection in Settings capture
        app.enter_settings(SettingsView::Keybinds);
        app.handle_settings_action(SettingsAction::CaptureMenuHotkey {
            menu_id: Some("clipboard".into()),
            label: "Clipboard".into(),
        });
        let eff_conflict = app.handle_key_down(
            Modifiers {
                ctrl: false,
                alt: false,
                shift: false,
                win: true,
            },
            b'E' as u32, // Win+e (already bound to Main Ring by default)
            Some('e'),
        );
        assert_eq!(eff_conflict, UiEffect::None);
        assert!(app.capture.is_some());
        assert!(app.notice.as_deref().unwrap_or("").contains("already bound"));
        app.capture = None;

        // Dynamic on_open JSON { "items": [...] } with input prompt & secondary action
        let dyn_json = r#"{"items":[{"label":"Dyn A","detail":"valA","left_click":{"input":"Enter note","command":"echo {input}"},"right_click":{"command":"del {value}","update":true}}]}"#;
        let parsed = parse_dynamic_menu_stdout(dyn_json).unwrap();
        assert_eq!(parsed.len(), 1);
        app.enter_custom_menu(
            "dyn_test".into(),
            MenuConfig {
                title: "DynTest".into(),
                ..Default::default()
            },
            true,
        );
        let job_gen = app.async_gen;
        app.handle_async_result(AsyncJobResult::CustomMenuLoaded {
            job_gen,
            stack_depth: app.custom_stack.len(),
            id: "dyn_test".into(),
            result: Ok(parsed),
        });
        assert_eq!(app.menu.items.len(), 1);
        // Primary click opens input box (instead of previously hitting dead None stub)
        app.activate_selected(false);
        assert!(app.input_box.is_some());
        assert_eq!(app.input_box.as_ref().unwrap().prompt, "Enter note");
        // Paste & escape cleanly closes input box
        app.paste_text("hello");
        assert_eq!(app.input_box.as_ref().unwrap().text, "hello");
        app.handle_key_down(Modifiers::default(), VK_ESCAPE, None);
        assert!(app.input_box.is_none());

        // Number shortcut '2' on main menu focuses the 2nd entry without opening; Ctrl+2 opens it
        app.enter_main_menu();
        assert!(app.menu.items.len() >= 2);
        let second_id = app.menu.items[1].key.clone();
        app.handle_key_down(Modifiers::default(), 0x32, Some('2'));
        assert_eq!(app.mode, Mode::Main);
        assert_eq!(app.menu.selected, Some(1));
        app.handle_key_down(
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
            0x32,
            Some('2'),
        );
        assert_eq!(app.mode.id(), second_id);

        // Settings notice must not clobber item detail and must clear cleanly when stepping back
        app.enter_settings(SettingsView::Root);
        assert!(app.notice.is_none());
        app.handle_settings_action(SettingsAction::ToggleNumberShortcuts);
        assert!(!app.cfg.keybinds.number_shortcuts);
        assert!(app.notice.is_some());
        app.step_back();
        assert!(app.notice.is_none());

        // Files BackSpace from Drives root ("\\") must pop back to Frequent Folders instead of looping
        app.open_menu_target("files", false).unwrap();
        app.menu.query = "\\".into();
        app.handle_key_down(Modifiers::default(), 0x08, None);
        assert!(app.menu.query.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn main_ring_opens_cleanly_without_capsules_and_commits_on_pointer_up_with_cancel_drag() {
        let (dir, mut app, _backend) = temp_app(true);
        app.set_viewport(1200.0, 800.0, 600.0, 400.0);
        // Settle opening spring
        app.ring_motion.advance(5.0);

        // Main Home ring has zero title/status capsules and no pre-selected sector (1:1 Linux)
        assert_eq!(app.mode, Mode::Main);
        assert!(app.display_title().is_empty());
        assert!(app.display_status().is_empty());
        assert_eq!(app.menu.selected, None);

        // Keyboard stepping from unselected Main ring lands cleanly on 0 (forward) or last (backward)
        app.menu.step_selection(1, PAGE_SIZE);
        assert_eq!(app.menu.selected, Some(0));
        app.menu.selected = None;
        app.menu.step_selection(-1, PAGE_SIZE);
        assert_eq!(app.menu.selected, Some(app.menu.filtered.len() - 1));
        app.menu.selected = None;

        // Pointer down on top sector (12 o'clock) arms press without activating yet;
        // dragging into the center hole and releasing cancels the press cleanly!
        let geom = app.ring_geometry();
        let top_x = geom.center_x;
        let top_y = geom.center_y - 123.5;
        let eff_down = app.handle_pointer_down(top_x, top_y, false);
        assert_eq!(eff_down, UiEffect::None);
        assert_eq!(app.mode, Mode::Main);
        assert_eq!(app.pressed, Some((0, false)));

        let (_dirty, eff_cancel) = app.handle_pointer_up(geom.center_x, geom.center_y, false);
        assert_eq!(eff_cancel, UiEffect::None);
        assert_eq!(app.mode, Mode::Main);
        assert!(!app.closing);

        // Pointer down + release on the same top sector activates it cleanly
        app.handle_pointer_down(top_x, top_y, false);
        let target_id = app.menu.items[0].key.clone();
        let (_dirty, eff_commit) = app.handle_pointer_up(top_x, top_y, false);
        assert_eq!(eff_commit, UiEffect::None);
        assert_eq!(app.mode.id(), target_id);

        // Clicking outside / center hole on Main closes cleanly
        app.enter_main_menu();
        app.ring_motion.advance(5.0);
        app.handle_pointer_down(geom.center_x, geom.center_y, false);
        let (_dirty, _eff) = app.handle_pointer_up(geom.center_x, geom.center_y, false);
        assert!(app.closing);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn loading_keeps_pacer_awake_even_with_reduced_motion() {
        let (dir, mut app, _backend) = temp_app(true);
        app.cfg.appearance.reduced_motion = true;
        app.ring_motion.advance(10.0);
        assert!(!app.is_animating());

        app.start_loading(true);
        assert!(app.is_animating());
        app.stop_loading();
        app.ring_motion.advance(10.0);
        assert!(!app.is_animating());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stationary_mouse_does_not_wipe_keyboard_or_search_selection() {
        let (dir, mut app, _backend) = temp_app(true);
        app.set_viewport(1200.0, 800.0, 600.0, 400.0);
        app.ring_motion.advance(5.0);

        // Move mouse outside the ring (stationary background pointer)
        app.handle_pointer_move(10.0, 10.0);
        assert_eq!(app.menu.selected, None);

        // Press Down arrow (VK_DOWN = 0x28): must select item 0 and STAY selected across ticks!
        app.handle_key_down(Modifiers::default(), 0x28, None);
        assert_eq!(app.menu.selected, Some(0));
        for _ in 0..30 {
            app.tick(0.016);
        }
        assert_eq!(
            app.menu.selected,
            Some(0),
            "stationary mouse must not wipe keyboard selection on tick()"
        );

        // Typing a search query in Settings must also preserve selection across animation ticks
        app.enter_settings(SettingsView::Root);
        app.handle_pointer_move(10.0, 10.0);
        app.handle_key_down(Modifiers::default(), 0x41, Some('a')); // 'a' -> Appearance
        assert_eq!(app.menu.selected, Some(0));
        for _ in 0..30 {
            app.tick(0.016);
        }
        assert_eq!(app.menu.selected, Some(0));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_async_result_does_not_cancel_current_loader_and_apps_preserves_ring_morph_and_caches() {
        let (dir, mut app, _backend) = temp_app(true);
        app.set_viewport(1200.0, 800.0, 600.0, 400.0);
        app.ring_motion.advance(5.0);

        // Entering Apps from Main Ring preserves ring sectors (no 3D flash) for spatial morph
        let before_sectors = app.ring_motion.sectors().count();
        assert!(before_sectors >= 2);
        app.enter_apps();
        assert!(app.loading_active);
        assert!(!app.loading_visible);
        assert_eq!(app.ring_motion.sectors().count(), before_sectors);

        let apps_gen = app.async_gen;
        let fake_apps: Vec<recircles_core::submenu_abi::AppDto> = (0..14)
            .map(|i| recircles_core::submenu_abi::AppDto {
                id: format!("app_{i}"),
                name: format!("App {i}"),
                launch_target: format!("app_{i}.exe"),
                icon_path: String::new(),
            })
            .collect();
        app.handle_async_result(AsyncJobResult::AppsLoaded {
            job_gen: apps_gen,
            result: Ok(fake_apps),
        });
        assert!(!app.loading_active);
        assert_eq!(app.surface, SurfaceMode::Grid);
        assert!(app.apps_cache.is_some());

        // Second open of Apps uses cache immediately without spawning a loader
        app.enter_main_menu();
        app.enter_apps();
        assert!(!app.loading_active);
        assert_eq!(app.surface, SurfaceMode::Grid);
        assert_eq!(app.menu.items.len(), 14);

        // Stale async result from an older generation must NOT cancel active loading
        let stale_gen = app.async_gen;
        let _active_gen = app.bump_async_gen();
        app.start_loading(true);
        assert!(app.loading_active);
        app.handle_async_result(AsyncJobResult::AppsLoaded {
            job_gen: stale_gen,
            result: Ok(Vec::new()),
        });
        assert!(
            app.loading_active,
            "stale async result must not stop newer active loading spinner"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn color_picker_cancel_restores_derived_colors_and_paste_replaces_hex() {
        let (dir, mut app, _backend) = temp_app(true);
        let orig_colors = app.cfg.colors.clone();
        assert!(orig_colors.ring_top.is_some());
        assert!(orig_colors.ring_bottom.is_some());

        app.enter_settings(SettingsView::ColorGroupEdit(
            recircles_core::settings::ColorGroup::Surface,
        ));
        app.handle_settings_action(SettingsAction::PickColor(
            recircles_core::settings::ColorField::Ring,
        ));
        assert!(app.color_picker.is_some());

        // Pasting a hex color replaces hex_input instead of appending onto existing hex
        app.paste_text("#11aa22");
        assert_eq!(app.color_picker.as_ref().unwrap().hex_input, "#11aa22");
        assert!(app.cfg.colors.ring_top.is_none());

        // Pressing Escape restores the full ColorsConfig snapshot including ring_top & ring_bottom!
        app.handle_key_down(Modifiers::default(), VK_ESCAPE, None);
        assert!(app.color_picker.is_none());
        assert_eq!(app.cfg.colors, orig_colors);
        assert_eq!(app.effective_colors, orig_colors);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn input_box_cursor_editing_and_settings_subview_titles() {
        let (dir, mut app, _backend) = temp_app(true);

        // Settings sub-views show their own titles instead of hardcoded "Settings"
        app.enter_settings(SettingsView::Root);
        assert_eq!(app.display_title(), "Settings");
        app.handle_settings_action(SettingsAction::OpenView(SettingsView::Appearance));
        assert_eq!(app.display_title(), "Appearance");
        app.handle_settings_action(SettingsAction::OpenView(SettingsView::ColorTheme));
        assert_eq!(app.display_title(), "Theme");

        // Input box supports Left/Right/Home/End/Delete/Ctrl+A
        app.input_box = Some(InputBoxState::new("Prompt".into(), "Hint".into(), false));
        app.handle_key_down(Modifiers::default(), 0x41, Some('a'));
        app.handle_key_down(Modifiers::default(), 0x42, Some('b'));
        app.handle_key_down(Modifiers::default(), 0x44, Some('d'));
        app.handle_key_down(Modifiers::default(), VK_LEFT, None);
        app.handle_key_down(Modifiers::default(), 0x43, Some('c'));
        assert_eq!(app.input_box.as_ref().unwrap().text, "abcd");

        app.handle_key_down(Modifiers::default(), VK_HOME, None);
        app.handle_key_down(Modifiers::default(), VK_DELETE, None);
        assert_eq!(app.input_box.as_ref().unwrap().text, "bcd");

        app.handle_key_down(
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
            0x41,
            Some('a'),
        );
        assert!(app.input_box.as_ref().unwrap().select_all);
        app.handle_key_down(Modifiers::default(), 0x5A, Some('z'));
        assert_eq!(app.input_box.as_ref().unwrap().text, "z");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_color_theme_uses_list_layout_and_returns_to_ring() {
        let (dir, mut app, _backend) = temp_app(true);
        app.enter_settings(SettingsView::Root);
        assert_eq!(app.surface, SurfaceMode::Ring);

        app.handle_settings_action(SettingsAction::OpenView(SettingsView::Appearance));
        assert_eq!(app.surface, SurfaceMode::Ring);

        app.handle_settings_action(SettingsAction::OpenView(SettingsView::ColorTheme));
        assert_eq!(app.surface, SurfaceMode::List);
        assert_eq!(app.display_title(), "Theme");
        assert!(app.menu.items.len() >= 6);

        // Mouse wheel scrolls list cleanly when > LIST_ROWS items exist, and applying a scrolled
        // theme keeps list_scroll & selected position instead of jumping back to item 0!
        if app.menu.items.len() > recircles_core::menu::LIST_ROWS {
            assert!(app.handle_wheel(-120.0));
            assert_eq!(app.menu.list_first_visible(), 1);
            app.menu.select_list_visible(2);
            let sel_before = app.menu.selected;
            app.activate_selected(false);
            assert_eq!(app.menu.list_first_visible(), 1);
            assert_eq!(app.menu.selected, sel_before);
        }

        // Stepping back morphs smoothly back to Ring
        app.step_back();
        assert_eq!(app.surface, SurfaceMode::Ring);
        assert_eq!(app.display_title(), "Appearance");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn grid_wheel_scrollbar_drag_and_page_keys_scroll_row_wise_without_overflow() {
        let (dir, mut app, _backend) = temp_app(true);
        app.set_viewport(1536.0, 864.0, 768.0, 432.0);
        app.ring_motion.advance(5.0);
        app.enter_apps();
        let apps_gen = app.async_gen;

        // 15 items (4 rows -> max_scroll = 4): scrollbar drag & wheel MUST work!
        let apps_15: Vec<recircles_core::submenu_abi::AppDto> = (0..15)
            .map(|i| recircles_core::submenu_abi::AppDto {
                id: format!("a{i}"),
                name: format!("App {i:02}"),
                launch_target: format!("a{i}.exe"),
                icon_path: String::new(),
            })
            .collect();
        app.handle_async_result(AsyncJobResult::AppsLoaded {
            job_gen: apps_gen,
            result: Ok(apps_15),
        });
        app.ring_motion.advance(5.0);
        assert_eq!(app.surface, SurfaceMode::Grid);
        assert_eq!(app.grid_first_visible(), 0);

        // Mouse wheel scrolls 1 row (4 items) down and back up
        assert!(app.handle_wheel(-120.0));
        assert_eq!(app.grid_first_visible(), 4);
        assert_eq!(app.visible_count(), 11);
        assert!(!app.handle_wheel(-120.0)); // already at bottom
        assert_eq!(app.grid_first_visible(), 4);
        assert!(app.handle_wheel(120.0));
        assert_eq!(app.grid_first_visible(), 0);

        // Dragging scrollbar to bottom on 15 items moves grid_scroll from 0 to 4!
        let bounds = app.explorer_bounds().unwrap();
        let track_x = bounds.cx + (bounds.width * 0.5 - 8.0) * bounds.scale;
        let track_top = bounds.cy + (bounds.grid_top() + 8.0) * bounds.scale;
        let track_bot = bounds.cy + (bounds.grid_top() + crate::layout::GRID_H - 8.0) * bounds.scale;
        app.handle_pointer_down(track_x, track_top, false);
        assert!(app.scrollbar_dragging);
        assert!(app.handle_pointer_move(track_x, track_bot));
        assert_eq!(app.grid_first_visible(), 4);
        app.handle_pointer_up(track_x, track_bot, false);
        assert!(!app.scrollbar_dragging);

        // 111 items (28 rows -> max_scroll = 100): PageUp from 0 lands on 100, NOT 108!
        let apps_111: Vec<recircles_core::submenu_abi::AppDto> = (0..111)
            .map(|i| recircles_core::submenu_abi::AppDto {
                id: format!("b{i}"),
                name: format!("App {i:03}"),
                launch_target: format!("b{i}.exe"),
                icon_path: String::new(),
            })
            .collect();
        app.apply_apps_list(apps_111);
        app.ring_motion.advance(5.0);
        app.handle_key_down(Modifiers::default(), 0x21, None); // VK_PRIOR (PageUp)
        assert_eq!(app.grid_first_visible(), 100);
        let bounds111 = app.explorer_bounds().unwrap();
        let (thumb_top, thumb_h) = bounds111.thumb(app.grid_first_visible(), 111).unwrap();
        let track_bot_local = bounds111.grid_top() + 4.0 + (crate::layout::GRID_H - 8.0);
        assert!(thumb_top + thumb_h <= track_bot_local + 1e-6);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn color_picker_supports_ctrl_a_cursor_navigation_delete_and_live_hex_replacement() {
        let (dir, mut app, _backend) = temp_app(true);
        app.enter_settings(SettingsView::ColorGroupEdit(
            recircles_core::settings::ColorGroup::Surface,
        ));
        app.handle_settings_action(SettingsAction::PickColor(
            recircles_core::settings::ColorField::Ring,
        ));
        assert_eq!(app.color_picker.as_ref().unwrap().hex_input, "#26160de8");

        // Ctrl+A selects all 9 chars and typing a brand-new hex color works!
        app.handle_key_down(
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
            0x41,
            Some('a'),
        );
        assert!(app.color_picker.as_ref().unwrap().select_all);

        for c in "#33cc77".chars() {
            let (mods, vk) = if c == '#' {
                (
                    Modifiers {
                        shift: true,
                        ..Default::default()
                    },
                    0x33, // Shift+3 produces '#' on Windows keyboards (0x23 is VK_END!)
                )
            } else {
                (Modifiers::default(), c.to_ascii_uppercase() as u32)
            };
            app.handle_key_down(mods, vk, Some(c));
        }
        assert_eq!(app.color_picker.as_ref().unwrap().hex_input, "#33cc77");
        assert!(!app.color_picker.as_ref().unwrap().select_all);
        assert_eq!(app.cfg.colors.ring, "#33cc77");

        // Home -> Right -> Delete -> insert 'f' -> live preview updates to "#f3cc77"
        app.handle_key_down(Modifiers::default(), VK_HOME, None);
        app.handle_key_down(Modifiers::default(), VK_RIGHT, None);
        app.handle_key_down(Modifiers::default(), VK_DELETE, None);
        assert_eq!(app.color_picker.as_ref().unwrap().hex_input, "#3cc77");
        app.handle_key_down(Modifiers::default(), 0x46, Some('f'));
        assert_eq!(app.color_picker.as_ref().unwrap().hex_input, "#f3cc77");
        assert_eq!(app.cfg.colors.ring, "#f3cc77");

        // End -> Left -> BackSpace -> insert 'a' -> "#f3cca7"
        app.handle_key_down(Modifiers::default(), VK_END, None);
        app.handle_key_down(Modifiers::default(), VK_LEFT, None);
        app.handle_key_down(Modifiers::default(), VK_BACK, None);
        app.handle_key_down(Modifiers::default(), 0x41, Some('a'));
        assert_eq!(app.color_picker.as_ref().unwrap().hex_input, "#f3cca7");
        assert_eq!(app.cfg.colors.ring, "#f3cca7");

        // Incomplete/invalid hex refuses Save on Enter and surfaces an inline error message
        app.handle_key_down(Modifiers::default(), VK_BACK, None);
        assert_eq!(app.color_picker.as_ref().unwrap().hex_input, "#f3cc7");
        app.handle_key_down(Modifiers::default(), VK_RETURN, None);
        assert!(app.color_picker.is_some());
        assert_eq!(
            app.color_picker.as_ref().unwrap().error.as_deref(),
            Some("Enter a valid #RRGGBB or #RRGGBBAA hex color")
        );

        // Tab -> Save (0) -> Tab -> Cancel (1) -> Enter cancels and restores original colors!
        app.handle_key_down(Modifiers::default(), VK_TAB, None);
        assert_eq!(app.color_picker.as_ref().unwrap().selected_button, Some(0));
        app.handle_key_down(Modifiers::default(), VK_TAB, None);
        assert_eq!(app.color_picker.as_ref().unwrap().selected_button, Some(1));
        app.handle_key_down(Modifiers::default(), VK_RETURN, None);
        assert!(app.color_picker.is_none());
        assert_eq!(app.cfg.colors.ring, "#26160de8");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn linux_keybind_parity_ring_grid_files_completion_back_and_alt_enter() {
        let (dir, mut app, backend) = temp_app(true);
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };

        // 1. Ring: Down & Tab circle clockwise (+1), Up & Shift+Tab circle anticlockwise (-1)
        app.enter_main_menu();
        app.menu.set_items(
            (0..4)
                .map(|i| Entry::simple(format!("m{i}"), format!("Item {i}"), BuiltinIcon::Circle))
                .collect(),
            true,
        );
        app.refresh_surface_and_motion();
        assert_eq!(app.surface, SurfaceMode::Ring);
        let n = app.menu.filtered.len();
        assert_eq!(n, 4);
        app.menu.selected = Some(0);
        app.handle_key_down(Modifiers::default(), VK_DOWN, None);
        assert_eq!(app.menu.selected, Some(1));
        app.handle_key_down(Modifiers::default(), VK_TAB, None);
        assert_eq!(app.menu.selected, Some(2));
        app.handle_key_down(shift, VK_TAB, None);
        assert_eq!(app.menu.selected, Some(1));
        app.handle_key_down(Modifiers::default(), VK_UP, None);
        assert_eq!(app.menu.selected, Some(0));
        app.handle_key_down(shift, VK_TAB, None);
        assert_eq!(app.menu.selected, Some(n - 1));

        // 2. Grid: Down & Tab circle linearly (+1), Up & Shift+Tab circle linearly (-1)
        let apps: Vec<recircles_core::submenu_abi::AppDto> = (0..14)
            .map(|i| recircles_core::submenu_abi::AppDto {
                id: format!("app{i}"),
                name: if i == 0 {
                    "Firefox".into()
                } else {
                    format!("App {i:02}")
                },
                launch_target: format!("a{i}.exe"),
                icon_path: String::new(),
            })
            .collect();
        app.mode = Mode::Apps;
        app.apply_apps_list(apps);
        assert_eq!(app.surface, SurfaceMode::Grid);
        assert_eq!(app.menu.selected, Some(0));
        app.handle_key_down(Modifiers::default(), VK_DOWN, None);
        assert_eq!(app.menu.selected, Some(1));
        app.handle_key_down(Modifiers::default(), VK_TAB, None);
        assert_eq!(app.menu.selected, Some(2));
        app.handle_key_down(Modifiers::default(), VK_UP, None);
        assert_eq!(app.menu.selected, Some(1));
        app.handle_key_down(shift, VK_TAB, None);
        assert_eq!(app.menu.selected, Some(0));

        // Non-Files autocomplete with Right, undo with Left, and clear with Ctrl+u
        app.handle_key_down(Modifiers::default(), 0x46, Some('f'));
        app.handle_key_down(Modifiers::default(), 0x49, Some('i'));
        assert_eq!(app.menu.query, "fi");
        assert_eq!(app.inline_suggestion().as_deref(), Some("refox"));
        app.handle_key_down(Modifiers::default(), VK_RIGHT, None);
        assert_eq!(app.menu.query, "Firefox");
        app.handle_key_down(Modifiers::default(), VK_LEFT, None);
        assert_eq!(app.menu.query, "fi");
        app.handle_key_down(ctrl, 0x55, Some('u')); // Ctrl+u clears query
        assert_eq!(app.menu.query, "");
        app.handle_key_down(Modifiers::default(), VK_ESCAPE, None); // Esc steps back to "fi"
        assert_eq!(app.menu.query, "fi");

        // 3. Files mode: parent_file_query, Left -> parent dir, Right -> autocomplete, Esc -> step back (not parent), Alt+Enter -> open current folder
        let sub1 = dir.join("smth");
        let sub2 = sub1.join("smth2");
        std::fs::create_dir_all(&sub2).unwrap();
        let sub1_str = format!("{}\\", sub1.display());
        let sub2_str = format!("{}\\", sub2.display());

        assert_eq!(
            App::parent_file_query(&format!("{}/smth2", sub1.to_string_lossy().replace('\\', "/"))),
            Some(sub1_str.clone())
        );
        assert_eq!(App::parent_file_query(&sub2_str), Some(sub1_str.clone()));
        assert_eq!(App::parent_file_query("C:\\"), Some("\\".to_string()));
        assert_eq!(App::parent_file_query("\\"), None);

        app.open_menu_target("files", false).unwrap();
        // Enter sub2 directly from Frequent Folders (empty query)
        app.menu.items = vec![Entry {
            key: sub2_str.clone(),
            title: "smth2".into(),
            detail: sub2_str.clone(),
            icon: BuiltinIcon::Folder,
            icon_path: None,
            exe_icon_path: None,
            accent_override: None,
            active: false,
            completion: Some(sub2_str.clone()),
            payload: sub2_str.clone(),
            secondary_payload: None,
        }];
        app.menu.filtered = vec![0];
        app.menu.selected = Some(0);
        app.handle_key_down(Modifiers::default(), VK_RETURN, None);
        assert_eq!(app.menu.query, sub2_str);

        // Esc goes one step back to "" (Frequent Folders), NOT to parent directory sub1!
        app.handle_key_down(Modifiers::default(), VK_ESCAPE, None);
        assert_eq!(app.menu.query, "");

        // Re-enter sub2, then Left goes to parent directory sub1
        app.push_back("");
        app.menu.query = sub2_str.clone();
        app.handle_key_down(Modifiers::default(), VK_LEFT, None);
        assert_eq!(app.menu.query, sub1_str);

        // Esc steps back one step to sub2!
        app.handle_key_down(Modifiers::default(), VK_ESCAPE, None);
        assert_eq!(app.menu.query, sub2_str);

        // Right finishes autocomplete for a partial path
        app.menu.query = format!("{}sm", sub1_str);
        app.menu.items = vec![Entry {
            key: sub2_str.clone(),
            title: "smth2".into(),
            detail: sub2_str.clone(),
            icon: BuiltinIcon::Folder,
            icon_path: None,
            exe_icon_path: None,
            accent_override: None,
            active: false,
            completion: Some(sub2_str.clone()),
            payload: sub2_str.clone(),
            secondary_payload: None,
        }];
        app.menu.filtered = vec![0];
        app.menu.selected = Some(0);
        assert_eq!(app.inline_suggestion().as_deref(), Some("th2\\"));
        app.handle_key_down(Modifiers::default(), VK_RIGHT, None);
        assert_eq!(app.menu.query, sub2_str);

        // Alt+Enter opens current folder (sub2) and closes
        let eff = app.handle_key_down(alt, VK_RETURN, None);
        assert!(matches!(eff, UiEffect::CloseApp) || app.closing);
        let calls = backend.calls.lock().unwrap().clone();
        assert!(calls
            .iter()
            .any(|c| c.contains("files_open_folder") && c.contains("smth2")));

        // Shift+Escape (home) jumps directly back to Main Menu
        app.closing = false;
        app.open_menu_target("files", false).unwrap();
        app.push_back("a");
        app.push_back("b");
        app.handle_key_down(shift, VK_ESCAPE, None);
        assert_eq!(app.mode, Mode::Main);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apps_disk_cache_opens_immediately_and_skips_redundant_refresh() {
        let (dir, mut app, backend) = temp_app(true);
        app.set_viewport(1200.0, 800.0, 600.0, 400.0);
        app.ring_motion.advance(5.0);

        let fake_apps: Vec<recircles_core::submenu_abi::AppDto> = (0..20)
            .map(|i| recircles_core::submenu_abi::AppDto {
                id: format!("app:{i}"),
                name: format!("App {i:02}"),
                launch_target: format!("app_{i}.exe"),
                icon_path: format!("app_{i}.exe"),
            })
            .collect();
        save_apps_disk_cache(&app.apps_disk_cache_path(), &fake_apps);

        // Fresh App pointing to the same ConfigStore loads disk cache immediately on enter_apps()
        let store2 = ConfigStore::in_dir(dir.clone());
        let installed2 = InstalledModules::all_for_tests(PathBuf::from("."));
        let mut app2 = App::new(store2, installed2, backend, None).unwrap();
        app2.set_viewport(1200.0, 800.0, 600.0, 400.0);
        app2.ring_motion.advance(5.0);

        app2.enter_apps();
        assert!(!app2.loading_active, "disk cache should populate grid without loading delay");
        assert_eq!(app2.surface, SurfaceMode::Grid);
        assert_eq!(app2.menu.items.len(), 20);

        // Scroll down 1 row; an unchanged background refresh must NOT reset scroll!
        assert!(app2.handle_wheel(-120.0));
        assert_eq!(app2.menu.grid_first_visible(), 4);
        let job_gen = app2.async_gen;
        app2.handle_async_result(AsyncJobResult::AppsLoaded {
            job_gen,
            result: Ok(fake_apps),
        });
        assert_eq!(app2.menu.grid_first_visible(), 4);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn windows_activation_defers_focus_to_ui_effect_without_sync_invoke() {
        let (dir, mut app, backend) = temp_app(true);
        app.set_viewport(1200.0, 800.0, 600.0, 400.0);
        app.ring_motion.advance(5.0);
        app.open_menu_target("windows", false).unwrap();
        assert_eq!(app.mode, Mode::Windows);
        assert!(!app.menu.filtered.is_empty());

        let expected_hwnd: u64 = app
            .menu
            .selected_entry()
            .expect("window entry")
            .payload
            .parse()
            .unwrap();
        backend.calls.lock().unwrap().clear();

        let eff = app.activate_selected(false);
        assert!(app.closing);
        assert_eq!(
            eff,
            UiEffect::FocusWindow {
                hwnd: expected_hwnd,
                close_now: app.cfg.appearance.reduced_motion,
            }
        );
        // Must NOT invoke WindowsFocus synchronously inside activate_selected (prevents Win32 WndProc RefCell re-entrancy).
        let calls = backend.calls.lock().unwrap().clone();
        assert!(
            !calls.iter().any(|c| c.contains("windows_focus")),
            "WindowsFocus should be deferred to UiEffect::FocusWindow, got calls: {calls:?}"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clipboard_clear_requires_confirmation_and_input_box_shows_inline_error() {
        let (dir, mut app, backend) = temp_app(true);
        app.enter_clipboard();
        assert_eq!(app.mode, Mode::Clipboard);
        assert_eq!(app.menu.items.len(), 2); // clip1 + Clear Clipboard History

        // Select "Clear Clipboard History" (item 1) and activate: must enter confirmation view without calling ClipboardClear yet
        app.menu.selected = Some(1);
        backend.calls.lock().unwrap().clear();
        app.activate_selected(false);
        assert!(app.confirming_clipboard_clear);
        assert_eq!(app.display_title(), "Clear Clipboard");
        assert_eq!(app.menu.items.len(), 2);
        assert_eq!(app.menu.items[0].payload, "clip_clear_confirm");
        assert_eq!(app.menu.items[1].payload, "clip_clear_cancel");
        assert!(
            !backend
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.contains("clipboard_clear"))
        );

        // Escape / step_back cancels confirmation and restores clipboard items
        app.step_back();
        assert!(!app.confirming_clipboard_clear);
        assert_eq!(app.display_title(), "Clipboard");
        assert_eq!(app.menu.items.len(), 2);

        // Re-open confirmation and activate "Clear All" (item 0) -> executes ClipboardClear
        app.menu.selected = Some(1);
        app.activate_selected(false);
        assert!(app.confirming_clipboard_clear);
        app.menu.selected = Some(0);
        app.activate_selected(false);
        assert!(!app.confirming_clipboard_clear);
        assert!(
            backend
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.contains("clipboard_clear"))
        );

        // Submitting an empty ImportThemePath keeps input_box open with an inline error
        app.enter_settings(SettingsView::ColorTheme);
        app.handle_settings_action(SettingsAction::ImportTheme);
        assert!(app.input_box.is_some());
        app.handle_key_down(Modifiers::default(), VK_RETURN, None);
        assert!(app.input_box.is_some());
        assert_eq!(
            app.input_box.as_ref().unwrap().error.as_deref(),
            Some("Enter a valid .json or .zip theme path")
        );
        // Typing clears the inline error immediately
        app.handle_key_down(Modifiers::default(), 0x41, Some('a'));
        assert!(app.input_box.as_ref().unwrap().error.is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn slider_submenu_updates_live_saves_to_config_and_cancels_cleanly() {
        let (dir, mut app, _backend) = temp_app(true);
        app.enter_settings(SettingsView::Appearance);

        // Open Translucency slider
        app.handle_settings_action(SettingsAction::AdjustSlider(
            recircles_core::settings::SliderField::Translucency,
        ));
        assert!(app.slider.is_some());
        assert_eq!(app.surface, SurfaceMode::Slider);
        let initial_t = app.cfg.appearance.translucency;
        assert_eq!(app.slider.as_ref().unwrap().value, initial_t);

        // Shift+Right steps +10% with immediate live preview
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        app.handle_key_down(shift, VK_RIGHT, None);
        assert_eq!(app.cfg.appearance.translucency, (initial_t + 10).min(100));

        // Escape cancels and rolls back live preview
        app.handle_key_down(Modifiers::default(), VK_ESCAPE, None);
        assert!(app.slider.is_none());
        assert_eq!(app.cfg.appearance.translucency, initial_t);

        // Open Blur slider, adjust with Home + wheel + Right, then Save via Enter
        app.handle_settings_action(SettingsAction::AdjustSlider(
            recircles_core::settings::SliderField::Blur,
        ));
        app.handle_key_down(Modifiers::default(), VK_HOME, None);
        assert_eq!(app.cfg.appearance.blur, 0);
        app.handle_wheel(120.0);
        assert_eq!(app.cfg.appearance.blur, 5);
        app.handle_key_down(Modifiers::default(), VK_RIGHT, None);
        assert_eq!(app.cfg.appearance.blur, 6);
        app.handle_key_down(Modifiers::default(), VK_RETURN, None);
        assert!(app.slider.is_none());
        assert_eq!(app.cfg.appearance.blur, 6);

        // Verify persisted to recircles.json
        let reloaded = app.store.load_or_create(&app.installed).unwrap();
        assert_eq!(reloaded.appearance.blur, 6);

        let _ = std::fs::remove_dir_all(dir);
    }
}
