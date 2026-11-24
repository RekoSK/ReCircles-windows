#[cfg(windows)]
pub mod win_render {
    use crate::app::App;
    use crate::d2d::{D2dCanvas, D2dColor, D2dGradStop, PathBuilder};
    use crate::icon_loader::ShellIconLoader;
    use crate::layout::{
        CELL_H, CELL_W, EMPTY_INNER, EMPTY_INNER_PULSE, EMPTY_OUTER, EMPTY_OUTER_PULSE,
        ExplorerBounds, GRID_FOOTER, GRID_H, GRID_LEFT, GRID_WIDTH, INNER, ITEM_RADIUS,
        LIST_FOOTER, LIST_ROW_H, OUTER, PERSPECTIVE_D, RING_HALF_THICKNESS, RING_SEGMENTS,
        RingGeometry, SurfaceMode, lerp, smoothstep,
    };
    use crate::motion_ring::{LoadingPose3D, RingMotion, RingSector};
    use crate::svg::{PathSeg, SvgDoc, parse_svg};
    use recircles_core::colors::Color;
    use recircles_core::config::{BUILTIN_CLIPBOARD_SVG, ConfigStore, linear_blur_params};
    use recircles_core::menu::{BuiltinIcon, Entry, GRID_COLUMNS, GRID_SIZE, LIST_ROWS, Mode, PAGE_SIZE};
    use recircles_core::settings::SliderField;
    use recircles_core::themes::ColorsConfig;
    use std::collections::HashMap;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
    use std::sync::LazyLock;
    use windows_sys::Win32::Foundation::HWND;

    type Rgb = (f64, f64, f64);

    // Standardized UI typography scale tokens (DIP)
    const FONT_TITLE: f64 = 16.0;
    const FONT_BODY: f64 = 14.0;
    const FONT_CONTROL: f64 = 13.0;
    const FONT_CAPTION: f64 = 12.0;
    const FONT_BADGE: f64 = 10.0;

    #[derive(Clone, Copy)]
    struct SurfaceMaterial {
        translucency: f64,
        backdrop_alpha: f64,
        blur_t: f64,
        high_contrast: bool,
    }

    fn blur_params(blur_pct: u8, dpi_scale: f64) -> Option<(f64, f64)> {
        linear_blur_params(blur_pct, dpi_scale)
    }

    impl SurfaceMaterial {
        fn resolve(
            translucency_pct: u8,
            blur_pct: u8,
            has_backdrop: bool,
            high_contrast: bool,
        ) -> Self {
            if high_contrast || translucency_pct == 0 {
                return Self {
                    translucency: 0.0,
                    backdrop_alpha: 0.0,
                    blur_t: 0.0,
                    high_contrast,
                };
            }
            let translucency = f64::from(translucency_pct.min(100)) / 100.0;
            let blur_t = if has_backdrop {
                f64::from(blur_pct.min(100)) / 100.0
            } else {
                0.0
            };
            let backdrop_alpha = if blur_pct > 0 && has_backdrop { 1.0 } else { 0.0 };
            Self {
                translucency,
                backdrop_alpha,
                blur_t,
                high_contrast: false,
            }
        }

        fn uses_backdrop(self) -> bool {
            self.backdrop_alpha > 0.002 && self.translucency > 0.001 && !self.high_contrast
        }

        /// Continuous tint opacity for the ring & floating capsules:
        /// 1.0 at 0% translucency (solid), ~0.33-0.39 at default 65%, 0.0 at 100% (pure glass).
        fn ring_opacity(self, weight: f64) -> f64 {
            if self.high_contrast || self.translucency <= 0.001 {
                return 1.0;
            }
            let base = (1.0 - self.translucency).clamp(0.0, 1.0);
            let unblurred_factor = if self.uses_backdrop() {
                (1.0 - self.blur_t / 0.15).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let unblurred_lift = 0.20 * base * (1.0 - base) * unblurred_factor;
            ((base + unblurred_lift) * weight).clamp(0.0, 1.0)
        }

        /// Slightly thicker glass tint for larger panels (Apple §12: bigger surfaces read thicker),
        /// smoothly spanning 1.0 at 0% translucency to 0.0 at 100%.
        fn panel_opacity(self, morph_t: f64, ring_weight: f64, panel_weight: f64) -> f64 {
            if self.high_contrast || self.translucency <= 0.001 {
                return 1.0;
            }
            let ring_a = self.ring_opacity(ring_weight);
            let base = (1.0 - self.translucency).clamp(0.0, 1.0);
            let thicker = base.powf(0.84);
            let unblurred_factor = if self.uses_backdrop() {
                (1.0 - self.blur_t / 0.15).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let unblurred_lift = 0.24 * base * (1.0 - base) * unblurred_factor;
            let panel_a = ((thicker + unblurred_lift) * panel_weight).clamp(0.0, 1.0);
            lerp(ring_a, panel_a, morph_t.clamp(0.0, 1.0))
        }

        /// Recessed interactive control wells (search bar, text inputs, slider track) retain a
        /// soft glass floor even at 100% translucency so controls always remain legible.
        fn control_opacity(self, floor: f64, ceiling: f64) -> f64 {
            if self.high_contrast || self.translucency <= 0.001 {
                return ceiling;
            }
            let base = (1.0 - self.translucency).clamp(0.0, 1.0);
            lerp(floor, ceiling, base)
        }

        /// Heavier material opacity for 3D side walls and modal help overlay.
        fn heavy_opacity(self, floor: f64) -> f64 {
            if self.high_contrast || self.translucency <= 0.001 {
                return 1.0;
            }
            let base = (1.0 - self.translucency).clamp(0.0, 1.0);
            lerp(floor, 1.0, base.powf(0.75))
        }
    }

    #[derive(Clone, Copy)]
    struct Palette {
        ring_top: Rgb,
        ring_mid: Rgb,
        ring_bottom: Rgb,
        rim_top: Rgb,
        rim_bottom: Rgb,
        divider: Rgb,
        shadow: Rgb,
        capsule_top: Rgb,
        capsule_bottom: Rgb,
        capsule_border: Rgb,
        text_primary: Rgb,
        text_capsule: Rgb,
        text_suggestion: Rgb,
        text_shadow: Rgb,
        accent_files: Rgb,
        accent_apps: Rgb,
        accent_windows: Rgb,
        accent_clipboard: Rgb,
        accent_settings: Rgb,
        accent_toggle_on: Rgb,
        accent_action: Rgb,
    }

    impl Palette {
        fn from_colors(c: &ColorsConfig) -> Self {
            let rgb = |col: Color| (col.r, col.g, col.b);
            let primary = rgb(c.text_color());
            let primary_luma = 0.2126 * primary.0 + 0.7152 * primary.1 + 0.0722 * primary.2;
            let accent_toggle_on = if primary_luma < 0.45 {
                (0.08, 0.49, 0.55) // #147d8c (Linux light theme toggle-on accent)
            } else {
                (0.36, 0.84, 0.70) // #5cd6b3 (Linux default toggle-on accent)
            };
            Self {
                ring_top: rgb(c.ring_top_color()),
                ring_mid: rgb(c.ring_color()),
                ring_bottom: rgb(c.ring_bottom_color()),
                rim_top: rgb(c.ring_border_color()),
                rim_bottom: rgb(c.rim_bottom_color()),
                divider: rgb(c.divider_color()),
                shadow: rgb(c.shadow_color()),
                capsule_top: rgb(c.capsule_color()),
                capsule_bottom: rgb(c.capsule_bottom_color()),
                capsule_border: rgb(c.capsule_border_color()),
                text_primary: primary,
                text_capsule: primary,
                text_suggestion: rgb(c.muted_color()),
                text_shadow: rgb(c.text_shadow_color()),
                accent_files: rgb(c.accent_for_mode("files")),
                accent_apps: rgb(c.accent_for_mode("apps")),
                accent_windows: rgb(c.accent_for_mode("windows")),
                accent_clipboard: rgb(c.accent_for_mode("clipboard")),
                accent_settings: rgb(c.accent_for_mode("settings")),
                accent_toggle_on,
                accent_action: rgb(c.accent_color()),
            }
        }

        fn accent_for_entry(&self, entry: &Entry, mode: &Mode) -> Rgb {
            if let Some(hex) = &entry.accent_override
                && let Ok(c) = Color::from_hex(hex)
            {
                return (c.r, c.g, c.b);
            }
            match mode {
                Mode::Main => match entry.key.as_str() {
                    "files" => self.accent_files,
                    "apps" => self.accent_apps,
                    "windows" => self.accent_windows,
                    "clipboard" => self.accent_clipboard,
                    "settings" => self.accent_settings,
                    _ => self.accent_settings,
                },
                Mode::Files => self.accent_files,
                Mode::Apps => self.accent_apps,
                Mode::Windows => self.accent_windows,
                Mode::Clipboard => self.accent_clipboard,
                Mode::Settings => match entry.icon {
                    BuiltinIcon::ToggleOn => self.accent_toggle_on,
                    BuiltinIcon::ToggleOff | BuiltinIcon::Reset => self.accent_action,
                    BuiltinIcon::Window | BuiltinIcon::Disconnect | BuiltinIcon::Forget | BuiltinIcon::Remove => {
                        self.accent_windows
                    }
                    _ => self.accent_settings,
                },
                Mode::Custom(_) => self.accent_action,
            }
        }

        fn loading_accent(&self, mode: &Mode) -> Rgb {
            match mode {
                Mode::Apps => self.accent_apps,
                Mode::Files => self.accent_files,
                Mode::Windows => self.accent_windows,
                Mode::Clipboard => self.accent_clipboard,
                _ => self.accent_settings,
            }
        }
    }

    fn lerp_rgb(a: Rgb, b: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        (
            a.0 + (b.0 - a.0) * t,
            a.1 + (b.1 - a.1) * t,
            a.2 + (b.2 - a.2) * t,
        )
    }

    static CLIPBOARD_SVG_DOC: LazyLock<Option<SvgDoc>> =
        LazyLock::new(|| parse_svg(BUILTIN_CLIPBOARD_SVG));

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

    pub struct Renderer {
        canvas: D2dCanvas,
        store: ConfigStore,
        icon_loader: ShellIconLoader,
        phys_w: i32,
        phys_h: i32,
        logical_scale: f64,
        shell_icon_px: i32,
        active_font: String,
        last_backdrop: Option<(i32, i32, f64, u8)>,
        last_blur_pct: Option<u8>,
        resolved_paths: HashMap<String, Option<String>>,
        image_bitmaps: HashMap<String, Option<i32>>,
        shell_bitmaps: HashMap<String, Option<i32>>,
        more_ready_icons: bool,
        svg_icons: HashMap<String, Option<SvgDoc>>,
        measure_cache: HashMap<(u16, bool), HashMap<String, (f64, f64)>>,
        measure_cache_entries: usize,
    }

    fn desired_shell_icon_px(logical_scale: f64) -> i32 {
        if logical_scale > 1.55 { 96 } else { 64 }
    }

    impl Renderer {
        #[allow(dead_code)]
        pub fn new(hwnd: HWND, phys_w: i32, phys_h: i32, logical_scale: f64) -> Option<Self> {
            Self::new_with_store(
                hwnd,
                phys_w,
                phys_h,
                logical_scale,
                ConfigStore::default_store(),
            )
        }

        pub fn new_with_store(
            hwnd: HWND,
            phys_w: i32,
            phys_h: i32,
            logical_scale: f64,
            store: ConfigStore,
        ) -> Option<Self> {
            let canvas = D2dCanvas::new(hwnd, phys_w, phys_h, logical_scale)?;
            let icon_loader = ShellIconLoader::new(hwnd, store.dir());
            let clamped_scale = logical_scale.clamp(0.5, 4.0);
            Some(Self {
                canvas,
                store,
                icon_loader,
                phys_w,
                phys_h,
                logical_scale: clamped_scale,
                shell_icon_px: desired_shell_icon_px(clamped_scale),
                active_font: String::new(),
                last_backdrop: None,
                last_blur_pct: None,
                resolved_paths: HashMap::new(),
                image_bitmaps: HashMap::new(),
                shell_bitmaps: HashMap::new(),
                more_ready_icons: false,
                svg_icons: HashMap::new(),
                measure_cache: HashMap::new(),
                measure_cache_entries: 0,
            })
        }

        pub fn ack_icon_notify(&self) {
            self.icon_loader.ack_notify();
        }

        pub fn has_more_ready_icons(&self) -> bool {
            self.more_ready_icons
        }

        pub fn drain_ready_icons(&mut self, max_uploads: usize) -> bool {
            let mut count = 0usize;
            while count < max_uploads {
                let Some(decoded) = self.icon_loader.try_recv() else {
                    self.more_ready_icons = false;
                    return count > 0;
                };
                if decoded.desired_px != self.shell_icon_px {
                    continue;
                }
                let bmp = decoded.bitmap.and_then(|(w, h, bgra)| {
                    self.canvas.create_bitmap_from_bgra(w as i32, h as i32, &bgra)
                });
                if self.shell_bitmaps.len() >= 1024 {
                    for (_, old) in self.shell_bitmaps.drain() {
                        if let Some(id) = old {
                            self.canvas.destroy_bitmap(id);
                        }
                    }
                }
                if let Some(Some(old_id)) = self.shell_bitmaps.insert(decoded.path, bmp) {
                    self.canvas.destroy_bitmap(old_id);
                }
                count += 1;
            }
            self.more_ready_icons = true;
            count > 0
        }

        pub fn capture_backdrop(
            &mut self,
            screen_x: i32,
            screen_y: i32,
            dpi_scale: f64,
            blur_pct: u8,
        ) -> bool {
            let pct = blur_pct.min(100);
            self.last_backdrop = Some((screen_x, screen_y, dpi_scale, pct));
            self.last_blur_pct = Some(pct);
            let params = blur_params(pct, dpi_scale);
            let sigma = params.map_or(0.0, |(s, _)| s);
            let ok = self
                .canvas
                .capture_backdrop(screen_x, screen_y, self.phys_w, self.phys_h, sigma);
            if ok && let Some((s, sat)) = params {
                let _ = self.canvas.set_backdrop_blur(s, sat);
            }
            ok
        }

        pub fn update_backdrop_dpi(&mut self, dpi_scale: f64, blur_pct: u8) {
            let pct = blur_pct.min(100);
            if let Some((sx, sy, _, _)) = self.last_backdrop {
                self.last_backdrop = Some((sx, sy, dpi_scale, pct));
            }
            self.last_blur_pct = Some(pct);
            if let Some((sigma, sat)) = blur_params(pct, dpi_scale) {
                let _ = self.canvas.set_backdrop_blur(sigma, sat);
            } else {
                let _ = self.canvas.set_backdrop_blur(0.0, 1.0);
            }
        }

        fn sync_backdrop_blur(&mut self, blur_pct: u8) {
            let pct = blur_pct.min(100);
            if self.last_blur_pct == Some(pct) {
                return;
            }
            let dpi_scale = self.last_backdrop.map_or(1.0, |(_, _, d, _)| d);
            if let Some((sx, sy, d, _)) = self.last_backdrop {
                self.last_backdrop = Some((sx, sy, d, pct));
            }
            self.last_blur_pct = Some(pct);
            if let Some((sigma, sat)) = blur_params(pct, dpi_scale) {
                let _ = self.canvas.set_backdrop_blur(sigma, sat);
            } else {
                let _ = self.canvas.set_backdrop_blur(0.0, 1.0);
            }
        }

        fn recover_device(&mut self) {
            if self.canvas.recreate() {
                self.image_bitmaps.clear();
                self.shell_bitmaps.clear();
                self.icon_loader.clear_tracking();
                self.measure_cache.clear();
                self.measure_cache_entries = 0;
                if !self.active_font.is_empty() {
                    self.canvas.set_font_family(&self.active_font);
                }
                if let Some((sx, sy, dpi, blur_pct)) = self.last_backdrop {
                    let _ = self.capture_backdrop(sx, sy, dpi, blur_pct);
                }
            }
        }

        pub fn resize(&mut self, phys_w: i32, phys_h: i32, logical_scale: f64) {
            self.phys_w = phys_w.max(1);
            self.phys_h = phys_h.max(1);
            self.logical_scale = logical_scale.clamp(0.5, 4.0);
            self.sync_shell_icon_px();
            self.canvas.resize(self.phys_w, self.phys_h, self.logical_scale);
        }

        fn sync_shell_icon_px(&mut self) {
            let next_px = desired_shell_icon_px(self.logical_scale);
            if next_px != self.shell_icon_px {
                self.shell_icon_px = next_px;
                for (_, old) in self.shell_bitmaps.drain() {
                    if let Some(id) = old {
                        self.canvas.destroy_bitmap(id);
                    }
                }
                self.icon_loader.clear_tracking();
            }
        }

        fn prefetch_shell_icons(&mut self, app: &App) {
            if !matches!(app.mode, Mode::Apps | Mode::Windows) || app.menu.filtered.is_empty() {
                return;
            }
            let total = app.menu.filtered.len();
            let px = self.shell_icon_px;
            let (vis_start, vis_end, ahead_end, behind_start) = match app.surface {
                SurfaceMode::Grid => {
                    let first = app.menu.grid_first_visible().min(total);
                    let end = (first + GRID_SIZE).min(total);
                    let ahead = (end + GRID_COLUMNS * 2).min(total);
                    let behind = first.saturating_sub(GRID_COLUMNS);
                    (first, end, ahead, behind)
                }
                _ => {
                    let first = (app.menu.page * PAGE_SIZE).min(total);
                    let end = (first + PAGE_SIZE).min(total);
                    let ahead = (end + PAGE_SIZE).min(total);
                    let behind = first.saturating_sub(PAGE_SIZE);
                    (first, end, ahead, behind)
                }
            };

            // Queue lookahead rows first at low priority
            for &idx in &app.menu.filtered[vis_end..ahead_end] {
                if let Some(path) = app.menu.items.get(idx).and_then(|e| e.exe_icon_path.as_deref())
                    && !self.shell_bitmaps.contains_key(path)
                {
                    self.icon_loader.request(path, px, false);
                }
            }
            for &idx in &app.menu.filtered[behind_start..vis_start] {
                if let Some(path) = app.menu.items.get(idx).and_then(|e| e.exe_icon_path.as_deref())
                    && !self.shell_bitmaps.contains_key(path)
                {
                    self.icon_loader.request(path, px, false);
                }
            }
            // Push visible tiles in reverse so LIFO high queue resolves top-left first
            for &idx in app.menu.filtered[vis_start..vis_end].iter().rev() {
                if let Some(path) = app.menu.items.get(idx).and_then(|e| e.exe_icon_path.as_deref())
                    && !self.shell_bitmaps.contains_key(path)
                {
                    self.icon_loader.request(path, px, true);
                }
            }
        }

        pub fn render_and_present(&mut self, logical_scale: f64, app: &App) {
            let requested_font = app.cfg.appearance.font.trim();
            if requested_font != self.active_font {
                self.active_font = requested_font.to_string();
                self.canvas.set_font_family(requested_font);
                self.measure_cache.clear();
                self.measure_cache_entries = 0;
            }
            self.logical_scale = logical_scale.clamp(0.5, 4.0);
            self.sync_shell_icon_px();
            self.drain_ready_icons(16);
            self.prefetch_shell_icons(app);
            // Update GPU Gaussian blur BEFORE BeginDraw when the user adjusts Blur (0-latency live preview)
            if !app.cfg.appearance.high_contrast && app.cfg.appearance.translucency > 0 {
                self.sync_backdrop_blur(app.cfg.appearance.blur);
            }
            self.canvas.begin_frame(self.logical_scale);

            let alpha_master = app.ring_motion.master_alpha();
            if alpha_master > 0.004 {
                let palette = Palette::from_colors(&app.effective_colors);
                let material = SurfaceMaterial::resolve(
                    app.cfg.appearance.translucency,
                    app.cfg.appearance.blur,
                    self.canvas.has_backdrop(),
                    app.cfg.appearance.high_contrast,
                );
                let geom = app.ring_geometry();
                if let Some(bounds) = app.explorer_bounds() {
                    let draw_as_input = app.input_box.is_some()
                        || app.slider.is_some()
                        || app.color_picker.is_some()
                        || (app.surface != SurfaceMode::Grid
                            && app.surface != SurfaceMode::List
                            && app.ring_motion.input_mix() > 0.001)
                        || (app.ring_motion.input_mix()
                            > app.ring_motion.box_mix().max(app.ring_motion.list_mix()));
                    let draw_as_list = !draw_as_input
                        && (app.surface == SurfaceMode::List
                            || app.ring_motion.list_mix() > app.ring_motion.box_mix());

                    if draw_as_input {
                        self.draw_input_panel(bounds, app, &palette, material, alpha_master);
                    } else if draw_as_list {
                        self.draw_list_panel(bounds, app, &palette, material, alpha_master);
                    } else {
                        self.draw_grid_panel(bounds, app, &palette, material, alpha_master);
                    }
                } else {
                    self.draw_ring(geom, app, &palette, material, alpha_master);
                }

                if app.show_help {
                    self.draw_help_overlay(geom, app, &palette, material, alpha_master);
                }
            }

            // Use sync_interval = 0 because VsyncPacer already aligns frames to DwmFlush().
            if !self.canvas.end_frame(0) {
                self.recover_device();
            }
        }

        fn measure_cached(&mut self, text: &str, size: f64, bold: bool) -> (f64, f64) {
            if text.is_empty() {
                return (0.0, 0.0);
            }
            let q = (size.max(6.0) * 10.0).round().clamp(60.0, 2000.0) as u16;
            if let Some(&dims) = self
                .measure_cache
                .get(&(q, bold))
                .and_then(|bucket| bucket.get(text))
            {
                return dims;
            }
            let dims = self.canvas.measure_text(text, size, bold, 0.0, 1);
            if self.measure_cache_entries >= 512 {
                self.measure_cache.clear();
                self.measure_cache_entries = 0;
            }
            self.measure_cache
                .entry((q, bold))
                .or_default()
                .insert(text.to_string(), dims);
            self.measure_cache_entries += 1;
            dims
        }

        fn draw_ring(
            &mut self,
            geom: RingGeometry,
            app: &App,
            palette: &Palette,
            material: SurfaceMaterial,
            alpha_master: f64,
        ) {
            let scale = geom.scale * app.ring_motion.scale();
            if scale <= 0.001 {
                return;
            }

            self.canvas.save();
            self.canvas.translate(geom.center_x, geom.center_y);
            self.canvas.scale(scale, scale);

            if let Some(pose) = app.ring_motion.loading_pose() {
                self.draw_loading_ring_3d(app, palette, material, pose, alpha_master);
            } else if let Some((inner, outer)) = ring_radii(&app.ring_motion) {
                self.draw_ring_material_at(inner, outer, material, palette, alpha_master);
                if app.ring_motion.sectors().next().is_some() {
                    self.canvas.save();
                    let s = outer / OUTER;
                    self.canvas.scale(s, s);
                    self.draw_sectors(app, palette, alpha_master);
                    self.canvas.restore();
                }
                self.draw_ring_rim_at(
                    inner,
                    outer,
                    palette,
                    app.cfg.appearance.high_contrast,
                    alpha_master,
                );
            } else {
                self.draw_ring_material_at(INNER, OUTER, material, palette, alpha_master);
                self.draw_sectors(app, palette, alpha_master);
                self.draw_ring_rim_at(
                    INNER,
                    OUTER,
                    palette,
                    app.cfg.appearance.high_contrast,
                    alpha_master,
                );
            }

            self.draw_captions(app, palette, material, alpha_master);
            self.canvas.restore();
        }

        fn draw_ring_material_at(
            &self,
            inner: f64,
            outer: f64,
            material: SurfaceMaterial,
            palette: &Palette,
            alpha: f64,
        ) {
            // Soft graduated outer shadow strictly outside outer rim (center hole untouched)
            self.canvas.fill_annulus_shadow(
                1.0,
                outer,
                8.5,
                D2dColor::from_rgb(palette.shadow, 0.13 * alpha),
            );

            // When fading frosted glass (open/close), composite backdrop + tint inside one tight
            // opacity layer so the frosted-to-tint ratio stays constant throughout the fade.
            let uses_backdrop = material.uses_backdrop();
            let fade_layer = uses_backdrop && alpha < 0.995;
            let local_a = if fade_layer {
                self.canvas.push_opacity_layer(
                    -outer - 2.0,
                    -outer - 2.0,
                    outer * 2.0 + 4.0,
                    outer * 2.0 + 4.0,
                    alpha,
                );
                1.0
            } else {
                alpha
            };

            // True shape-clipped GPU frosted desktop backdrop strictly inside the donut
            if uses_backdrop {
                self.canvas
                    .fill_annulus_backdrop(inner, outer, material.backdrop_alpha * local_a);
            }

            // 3-stop vertical glass gradient scaled smoothly by translucency (0% = solid, 100% = clear)
            let a_top = material.ring_opacity(1.08) * local_a;
            let a_mid = material.ring_opacity(0.94) * local_a;
            let a_bot = material.ring_opacity(1.12) * local_a;
            if a_top.max(a_mid).max(a_bot) > 0.002 {
                let stops = [
                    D2dGradStop {
                        pos: 0.0,
                        color: D2dColor::from_rgb(palette.ring_top, a_top),
                    },
                    D2dGradStop {
                        pos: 0.42,
                        color: D2dColor::from_rgb(palette.ring_mid, a_mid),
                    },
                    D2dGradStop {
                        pos: 1.0,
                        color: D2dColor::from_rgb(palette.ring_bottom, a_bot),
                    },
                ];
                self.canvas
                    .fill_annulus_linear(inner, outer, 0.0, -outer, 0.0, outer, &stops);
            }
            // Subtle specular glass sheen catching light along the upper half of the translucent ring
            if material.translucency > 0.15 {
                let sheen_a = 0.065 * material.translucency * local_a;
                let sheen_stops = [
                    D2dGradStop {
                        pos: 0.0,
                        color: D2dColor::from_rgb(palette.rim_top, sheen_a),
                    },
                    D2dGradStop {
                        pos: 0.45,
                        color: D2dColor::from_rgb(palette.rim_top, 0.0),
                    },
                    D2dGradStop {
                        pos: 1.0,
                        color: D2dColor::from_rgb(palette.rim_top, 0.0),
                    },
                ];
                self.canvas
                    .fill_annulus_linear(inner, outer, 0.0, -outer, 0.0, outer, &sheen_stops);
            }
            if fade_layer {
                self.canvas.pop_layer();
            }
        }

        fn draw_ring_rim_at(
            &self,
            inner: f64,
            outer: f64,
            palette: &Palette,
            high_contrast: bool,
            alpha: f64,
        ) {
            self.canvas.stroke_annulus_rim(
                inner,
                outer,
                if high_contrast { 1.8 } else { 1.0 },
                0.0,
                -outer,
                0.0,
                outer,
                D2dColor::from_rgb(
                    palette.rim_top,
                    if high_contrast { 1.0 } else { 0.70 } * alpha,
                ),
                D2dColor::from_rgb(
                    palette.rim_bottom,
                    if high_contrast { 1.0 } else { 0.27 } * alpha,
                ),
            );
        }

        fn draw_sector_chrome(
            &self,
            sector: &RingSector<'_>,
            app: &App,
            palette: &Palette,
            alpha: f64,
        ) {
            let slot = sector.slot;
            let (start, end) = (sector.start, sector.end);
            let span = end - start;
            let pressed = slot.index.is_some() && slot.index == app.pressed_vis();
            let focus = slot.focus().clamp(0.0, 1.0);

            if (slot.focused() || focus > 0.004 || pressed)
                && let Some(face) = slot.faces.last()
            {
                let tint = palette.accent_for_entry(&face.entry, &app.mode);
                let fill_a = if pressed {
                    0.23
                } else if slot.focused() {
                    0.05 + 0.08 * focus
                } else {
                    0.13 * focus
                } * alpha;
                if fill_a > 0.002 {
                    self.canvas.fill_sector(
                        INNER,
                        OUTER,
                        start,
                        end,
                        D2dColor::from_rgb(tint, fill_a),
                    );
                }

                let rim_a = if pressed {
                    0.98
                } else if slot.focused() {
                    0.45 + 0.53 * focus
                } else {
                    0.98 * focus
                } * alpha;
                if rim_a > 0.004 {
                    if span >= TAU - 0.0005 {
                        self.canvas.stroke_arc(
                            0.0,
                            0.0,
                            OUTER - 2.6,
                            0.0,
                            TAU,
                            2.6,
                            D2dColor::from_rgb(tint, rim_a),
                            true,
                        );
                    } else {
                        let inset = 0.026_f64.min(span * 0.25).min((TAU - span) * 0.5);
                        self.canvas.stroke_arc(
                            0.0,
                            0.0,
                            OUTER - 2.6,
                            start + inset,
                            end - inset,
                            2.6,
                            D2dColor::from_rgb(tint, rim_a),
                            true,
                        );
                    }
                }
            }

            // Crisp 1px radial sector dividers (Linux parity)
            if span < TAU - 0.0005 {
                let opacity = slot
                    .faces
                    .iter()
                    .map(|f| f.opacity)
                    .sum::<f64>()
                    .min(1.0);
                let div_a = opacity * 0.19 * alpha;
                if div_a > 0.002 {
                    let (sa, ca) = start.sin_cos();
                    self.canvas.draw_line(
                        (INNER + 1.0) * ca,
                        (INNER + 1.0) * sa,
                        (OUTER - 1.0) * ca,
                        (OUTER - 1.0) * sa,
                        1.0,
                        D2dColor::from_rgb(palette.divider, div_a),
                        false,
                    );
                }
            }
        }

        fn draw_sectors(&mut self, app: &App, palette: &Palette, alpha: f64) {
            if alpha <= 0.004 {
                return;
            }
            let count = app.visible_count();
            let pressed_vis = app.pressed_vis();

            // Collect sector snapshots so we can call &mut self methods cleanly
            struct SectorDraw {
                start: f64,
                end: f64,
                index: Option<usize>,
                focus: f64,
                tilt: f64,
                faces: Vec<(Entry, f64)>,
            }
            let mut items_to_draw = Vec::new();
            for sec in app.ring_motion.sectors() {
                if sec.slot.born_in_box() {
                    continue;
                }
                self.draw_sector_chrome(&sec, app, palette, alpha);
                let faces = sec
                    .slot
                    .faces
                    .iter()
                    .filter_map(|f| {
                        let a = f.opacity * alpha;
                        (a > 0.008).then(|| (f.entry.clone(), a))
                    })
                    .collect();
                items_to_draw.push(SectorDraw {
                    start: sec.start,
                    end: sec.end,
                    index: sec.slot.index,
                    focus: sec.slot.focus(),
                    tilt: sec.slot.tilt(),
                    faces,
                });
            }

            for sec in items_to_draw {
                let span = sec.end - sec.start;
                let pressed = sec.index.is_some() && sec.index == pressed_vis;
                let needs_clip = span < 0.72 || sec.index.is_none();
                if needs_clip {
                    self.canvas.push_clip_sector(INNER, OUTER, sec.start, sec.end);
                }
                for (entry, face_alpha) in &sec.faces {
                    self.draw_ring_item(
                        entry,
                        &app.mode,
                        sec.start,
                        sec.end,
                        count,
                        pressed,
                        sec.focus,
                        sec.tilt,
                        palette,
                        *face_alpha,
                    );
                }
                if needs_clip {
                    self.canvas.pop_layer();
                }
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn draw_ring_item(
            &mut self,
            entry: &Entry,
            mode: &Mode,
            start: f64,
            end: f64,
            count: usize,
            pressed: bool,
            focus: f64,
            tilt: f64,
            palette: &Palette,
            alpha: f64,
        ) {
            self.canvas.save();
            let angle = (start + end) * 0.5;
            let style = item_style(mode, count);
            let origin_x = style.horizontal_radius * angle.cos();
            let origin_y = ITEM_RADIUS * angle.sin();
            self.canvas.translate(origin_x, origin_y);
            let zoom = if pressed { 0.96 } else { 1.0 + 0.04 * focus.clamp(0.0, 1.0) };
            if (zoom - 1.0).abs() >= 1e-4 {
                self.canvas.scale(zoom, zoom);
            }

            self.canvas.save();
            if tilt.abs() >= 0.003 {
                self.canvas.translate(0.0, style.icon_y);
                self.canvas.rotate(tilt);
                self.canvas.translate(0.0, -style.icon_y);
            }
            self.draw_icon_plate(
                entry,
                mode,
                style.icon_y,
                style.plate,
                style.icon_size,
                palette,
                alpha,
            );
            self.canvas.restore();

            let (font_size, label_x, width) = self.fit_item_label(
                &entry.title,
                (start, end),
                (origin_x, origin_y),
                style,
                zoom,
            );
            let box_h = (font_size * 1.45).max(18.0);
            self.canvas.draw_text(
                &entry.title,
                font_size,
                true,
                label_x - width * 0.5,
                style.label_y - box_h * 0.5,
                width,
                box_h,
                1, // Center
                3, // End ellipsis
                1,
                D2dColor::from_rgb(palette.text_primary, alpha),
                Some(D2dColor::from_rgb(palette.text_shadow, 0.80 * alpha)),
            );
            self.canvas.restore();
        }

        #[allow(clippy::too_many_arguments)]
        fn draw_icon_plate(
            &mut self,
            entry: &Entry,
            mode: &Mode,
            y: f64,
            plate: f64,
            icon_size: f64,
            palette: &Palette,
            alpha: f64,
        ) {
            if alpha <= 0.004 {
                return;
            }
            let tint = palette.accent_for_entry(entry, mode);
            let half = plate * 0.5;
            let rad = plate * 0.29;
            // Clean solid plate fill with crisp 1px border (no decorative multi-stop gradient)
            self.canvas.fill_rounded_rect_solid(
                -half,
                y - half,
                plate,
                plate,
                rad,
                D2dColor::from_rgb(tint, 0.14 * alpha),
            );
            self.canvas.stroke_rounded_rect(
                -half,
                y - half,
                plate,
                plate,
                rad,
                1.0,
                D2dColor::from_rgb(tint, 0.42 * alpha),
            );

            self.draw_icon_foreground(entry, y, plate, icon_size, tint, palette, alpha);

            if entry.active {
                let dot_x = half - 2.0;
                let dot_y = y - half + 2.0;
                self.canvas
                    .fill_circle(dot_x, dot_y, 3.8, D2dColor::from_rgb(tint, 0.98 * alpha));
                self.canvas.stroke_circle(
                    dot_x,
                    dot_y,
                    3.8,
                    1.2,
                    D2dColor::from_rgb(palette.ring_mid, 0.95 * alpha),
                );
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn draw_icon_foreground(
            &mut self,
            entry: &Entry,
            y: f64,
            plate: f64,
            icon_size: f64,
            tint: Rgb,
            palette: &Palette,
            alpha: f64,
        ) {
            // 1. Custom SVG or image file icon (preserve aspect ratio on image thumbnails)
            if let Some(path_str) = &entry.icon_path {
                if let Some(doc) = self.load_or_cache_svg(path_str) {
                    self.draw_svg_doc(&doc, 0.0, y, icon_size * 1.05, D2dColor::from_rgb(tint, alpha));
                    return;
                }
                if let Some(bmp_id) = self.load_or_cache_image_bitmap(path_str) {
                    let max_dim = (plate - 5.0).max(8.0);
                    let (dw, dh) = match self.canvas.get_bitmap_size(bmp_id) {
                        Some((bw, bh)) if bw > 0.0 && bh > 0.0 => {
                            let s = max_dim / bw.max(bh);
                            (bw * s, bh * s)
                        }
                        _ => (max_dim, max_dim),
                    };
                    self.canvas
                        .draw_bitmap(bmp_id, -dw * 0.5, y - dh * 0.5, dw, dh, alpha);
                    return;
                }
            }

            // 2. Native Windows app/window crisp HiDPI shell icon (shortcut-arrow free, zero clip layer)
            if let Some(exe_path) = &entry.exe_icon_path
                && let Some(bmp_id) = self.load_or_cache_shell_icon(exe_path)
            {
                let max_dim = (plate - 8.0).max(8.0);
                let (dw, dh) = match self.canvas.get_bitmap_size(bmp_id) {
                    Some((bw, bh)) if bw > 0.0 && bh > 0.0 => {
                        let s = max_dim / bw.max(bh);
                        (bw * s, bh * s)
                    }
                    _ => (max_dim, max_dim),
                };
                self.canvas
                    .draw_bitmap(bmp_id, -dw * 0.5, y - dh * 0.5, dw, dh, alpha);
                return;
            }

            // 3. Color swatch preview (in Settings -> Colors)
            if matches!(entry.icon, BuiltinIcon::Palette) && entry.accent_override.is_some() {
                self.canvas.save();
                self.canvas.translate(-icon_size * 0.5, y - icon_size * 0.5);
                self.canvas.scale(icon_size / 24.0, icon_size / 24.0);
                self.canvas
                    .fill_circle(12.0, 12.0, 7.8, D2dColor::from_rgb(tint, alpha));
                self.canvas.stroke_circle(
                    12.0,
                    12.0,
                    7.8,
                    1.6,
                    D2dColor::from_rgb(palette.text_primary, 0.55 * alpha),
                );
                self.canvas.restore();
                return;
            }

            // 4. Built-in Clipboard SVG (exact geometry)
            if entry.icon == BuiltinIcon::Clipboard
                && let Some(doc) = CLIPBOARD_SVG_DOC.as_ref()
            {
                self.draw_svg_doc(doc, 0.0, y, icon_size * 1.15, D2dColor::from_rgb(tint, alpha));
                return;
            }

            // 5. Crisp 24x24 Linux-exact vector glyph
            let pb = build_linux_glyph(entry.icon);
            self.canvas.save();
            self.canvas.translate(-icon_size * 0.5, y - icon_size * 0.5);
            self.canvas.scale(icon_size / 24.0, icon_size / 24.0);
            self.canvas.draw_path(
                &pb.cmds,
                None,
                Some((D2dColor::from_rgb(tint, alpha), 1.6, true)),
            );
            self.canvas.restore();
        }

        fn draw_svg_doc(&self, doc: &SvgDoc, cx: f64, cy: f64, size: f64, tint: D2dColor) {
            let scale = size / doc.vb_w.max(doc.vb_h).max(1.0);
            self.canvas.save();
            self.canvas.translate(
                cx - (doc.vb_x + doc.vb_w * 0.5) * scale,
                cy - (doc.vb_y + doc.vb_h * 0.5) * scale,
            );
            self.canvas.scale(scale, scale);
            for shape in &doc.shapes {
                let mut pb = PathBuilder::new();
                for seg in &shape.segments {
                    match *seg {
                        PathSeg::MoveTo(x, y) => pb.move_to(x, y),
                        PathSeg::LineTo(x, y) => pb.line_to(x, y),
                        PathSeg::CubicTo(x1, y1, x2, y2, x3, y3) => {
                            pb.curve_to(x1, y1, x2, y2, x3, y3)
                        }
                        PathSeg::Close => pb.close(),
                    }
                }
                let fill = shape.fill.map(|_| tint);
                let stroke = shape.stroke.map(|_| (tint, shape.stroke_width.max(1.0), shape.round_cap));
                self.canvas.draw_path(&pb.cmds, fill, stroke);
            }
            self.canvas.restore();
        }

        fn draw_captions(
            &mut self,
            app: &App,
            palette: &Palette,
            material: SurfaceMaterial,
            alpha: f64,
        ) {
            if alpha <= 0.004 {
                return;
            }
            let outer = caption_outer_radius(&app.ring_motion);
            let title = app.display_title();
            let query = &app.menu.query;
            let suggestion = app.inline_suggestion();
            let status = app.display_status();

            if !title.is_empty() {
                let full_title = match app.ring_page_indicator() {
                    Some((cur, tot)) if tot > 1 => format!("{title}   ·   {cur} / {tot}"),
                    _ => title,
                };
                self.draw_capsule(&full_title, -outer - 22.0, 280.0, None, material, palette, alpha);
            }
            if !query.is_empty() {
                self.draw_capsule(
                    query,
                    outer + 25.0,
                    332.0,
                    Some(suggestion.as_deref().unwrap_or("")),
                    material,
                    palette,
                    alpha,
                );
            }
            if !status.is_empty() {
                let y = outer + if query.is_empty() { 24.0 } else { 60.0 };
                self.draw_capsule(&status, y, 344.0, None, material, palette, alpha);
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn draw_capsule(
            &mut self,
            text: &str,
            y: f64,
            max_w: f64,
            suggestion: Option<&str>,
            material: SurfaceMaterial,
            palette: &Palette,
            alpha: f64,
        ) {
            let is_query = suggestion.is_some();
            let suffix = suggestion.unwrap_or_default();
            let font_sz = if is_query { FONT_BODY } else { FONT_CAPTION };
            let (text_w, _) = if suffix.is_empty() {
                self.measure_cached(text, font_sz, true)
            } else {
                let combined = format!("{text}{suffix}");
                self.measure_cached(&combined, font_sz, true)
            };
            let h = if is_query { 32.0 } else { 26.0 };
            let w = (text_w + 26.0).clamp(h, max_w);
            let left = -w * 0.5;
            let top = y - h * 0.5;
            let radius = h * 0.5;

            // Soft graduated outer shadow strictly outside capsule
            self.canvas.fill_rounded_rect_shadow(
                left,
                top,
                w,
                h,
                radius,
                D2dColor::from_rgb(palette.shadow, 1.0),
                alpha,
            );

            let top_col = lerp_rgb(palette.ring_top, palette.capsule_top, 0.5);
            let bot_col = lerp_rgb(palette.ring_bottom, palette.capsule_bottom, 0.5);
            let rim_top = lerp_rgb(palette.rim_top, palette.capsule_border, 0.5);
            let rim_bot = lerp_rgb(palette.capsule_border, palette.rim_bottom, 0.5);

            let uses_backdrop = material.uses_backdrop();
            let fade_layer = uses_backdrop && alpha < 0.995;
            let local_a = if fade_layer {
                self.canvas
                    .push_opacity_layer(left - 1.0, top - 1.0, w + 2.0, h + 2.0, alpha);
                1.0
            } else {
                alpha
            };

            // Frosted desktop blur strictly inside capsule pill
            if uses_backdrop {
                self.canvas.fill_rounded_rect_backdrop(
                    left,
                    top,
                    w,
                    h,
                    radius,
                    0.0,
                    material.backdrop_alpha * local_a,
                );
            }

            // Glass tint gradient matching translucency setting
            let a_top = material.ring_opacity(1.08) * local_a;
            let a_mid = material.ring_opacity(0.94) * local_a;
            let a_bot = material.ring_opacity(1.12) * local_a;
            if a_top.max(a_mid).max(a_bot) > 0.002 {
                let stops = [
                    D2dGradStop {
                        pos: 0.0,
                        color: D2dColor::from_rgb(top_col, a_top),
                    },
                    D2dGradStop {
                        pos: 0.42,
                        color: D2dColor::from_rgb(palette.ring_mid, a_mid),
                    },
                    D2dGradStop {
                        pos: 1.0,
                        color: D2dColor::from_rgb(bot_col, a_bot),
                    },
                ];
                self.canvas.fill_rounded_rect_linear(
                    left,
                    top,
                    w,
                    h,
                    radius,
                    0.0,
                    -w * 0.3,
                    top,
                    w * 0.25,
                    top + h,
                    &stops,
                );
            }
            if fade_layer {
                self.canvas.pop_layer();
            }

            // Diagonal glass rim
            self.canvas.stroke_rounded_rect_linear(
                left,
                top,
                w,
                h,
                radius,
                1.0,
                -w * 0.25,
                top,
                w * 0.25,
                top + h,
                D2dColor::from_rgb(rim_top, 0.70 * alpha),
                D2dColor::from_rgb(rim_bot, 0.27 * alpha),
            );

            let text_w_box = (w - 20.0).max(8.0);
            if is_query {
                self.canvas.draw_query_text(
                    text,
                    suffix,
                    font_sz,
                    true,
                    -text_w_box * 0.5,
                    top,
                    text_w_box,
                    h,
                    1,
                    D2dColor::from_rgb(palette.text_capsule, alpha),
                    D2dColor::from_rgb(palette.text_suggestion, alpha),
                    Some(D2dColor::from_rgb(palette.text_shadow, 0.80 * alpha)),
                );
            } else {
                self.canvas.draw_text(
                    text,
                    font_sz,
                    true,
                    -text_w_box * 0.5,
                    top,
                    text_w_box,
                    h,
                    1,
                    3,
                    1,
                    D2dColor::from_rgb(palette.text_capsule, alpha),
                    Some(D2dColor::from_rgb(palette.text_shadow, 0.80 * alpha)),
                );
            }
        }

        // ==================== 3D TUMBLING GLASS LOADING RING (EXACT LINUX) ====================
        fn draw_loading_ring_3d(
            &mut self,
            app: &App,
            palette: &Palette,
            material: SurfaceMaterial,
            pose: LoadingPose3D,
            alpha_master: f64,
        ) {
            let accent = palette.loading_accent(&app.mode);
            let tf = Transform3D { r: pose.rotation };
            let cam = tf.cam_local();
            let half_h = RING_HALF_THICKNESS * pose.depth;
            let radius_scale = 1.0 - 0.025 * pose.depth;
            let outer_r = OUTER * radius_scale;
            let inner_r = INNER * radius_scale;

            let mut outer_front = Vec::with_capacity(RING_SEGMENTS);
            let mut outer_back = Vec::with_capacity(RING_SEGMENTS);
            let mut inner_front = Vec::with_capacity(RING_SEGMENTS);
            let mut inner_back = Vec::with_capacity(RING_SEGMENTS);

            for &(ca, sa) in RING_COS_SIN.iter() {
                outer_front.push(tf.project(Vec3(outer_r * ca, outer_r * sa, half_h)));
                outer_back.push(tf.project(Vec3(outer_r * ca, outer_r * sa, -half_h)));
                inner_front.push(tf.project(Vec3(inner_r * ca, inner_r * sa, half_h)));
                inner_back.push(tf.project(Vec3(inner_r * ca, inner_r * sa, -half_h)));
            }

            let mut hull_in = Vec::with_capacity(RING_SEGMENTS * 2);
            for &(x, y, _) in &outer_front {
                hull_in.push((x, y));
            }
            for &(x, y, _) in &outer_back {
                hull_in.push((x, y));
            }
            let outer_hull = convex_hull(hull_in);
            let (mut hx0, mut hx1) = (f64::INFINITY, f64::NEG_INFINITY);
            let (mut hy0, mut hy1) = (f64::INFINITY, f64::NEG_INFINITY);
            for &(x, y) in &outer_hull {
                hx0 = hx0.min(x);
                hx1 = hx1.max(x);
                hy0 = hy0.min(y);
                hy1 = hy1.max(y);
            }

            // Soft graduated shadow around 3D silhouette
            for (expand, weight) in [(1.4, 0.85), (4.0, 0.50), (6.6, 0.18)] {
                let expanded: Vec<(f64, f64)> = outer_hull
                    .iter()
                    .map(|&(x, y)| {
                        let r = x.hypot(y);
                        let s = if r > 1e-3 { (r + expand) / r } else { 1.0 };
                        (x * s, y * s + 1.0)
                    })
                    .collect();
                self.canvas.stroke_polygon(
                    &expanded,
                    2.8,
                    D2dColor::from_rgb(palette.shadow, 0.13 * weight * alpha_master),
                );
            }

            let light = Vec3(-0.42, -0.72, 0.80).normalize();
            let half_vec = Vec3(light.0, light.1, light.2 + 1.0).normalize();
            let front_is_pos = cam.2 >= 0.0;

            let shade_outer = |ca: f64, sa: f64| -> Rgb {
                let n = tf.rotate(Vec3(ca, sa, 0.0));
                let diff = n.dot(light).max(0.0);
                let spec = n.dot(half_vec).max(0.0).powf(12.0);
                let base = lerp_rgb(palette.ring_bottom, palette.ring_top, 0.20 + 0.80 * diff);
                let lit = lerp_rgb(base, palette.rim_top, 0.28 * diff + 0.48 * spec);
                lerp_rgb(lit, accent, (0.22 * diff + 0.45 * spec) * pose.depth)
            };
            let shade_inner = |ca: f64, sa: f64| -> Rgb {
                let n = tf.rotate(Vec3(-ca, -sa, 0.0));
                let diff = n.dot(light).max(0.0);
                let spec = n.dot(half_vec).max(0.0).powf(10.0);
                let base = lerp_rgb(palette.ring_mid, palette.ring_top, 0.12 + 0.72 * diff);
                let lit = lerp_rgb(base, palette.rim_bottom, 0.20 * diff + 0.34 * spec);
                lerp_rgb(lit, accent, (0.16 * diff + 0.32 * spec) * pose.depth)
            };

            if half_h > 0.05 {
                struct WallQuad {
                    z_avg: f64,
                    pts: [(f64, f64); 4],
                    rgb: Rgb,
                }
                let mut quads = Vec::with_capacity(RING_SEGMENTS * 2);
                for (i, &(ca, sa)) in RING_MID_COS_SIN.iter().enumerate() {
                    let j = (i + 1) % RING_SEGMENTS;
                    if cam.0 * ca + cam.1 * sa - outer_r > -4.0 {
                        let (p0, p1, p2, p3) =
                            (outer_front[i], outer_front[j], outer_back[j], outer_back[i]);
                        quads.push(WallQuad {
                            z_avg: 0.25 * (p0.2 + p1.2 + p2.2 + p3.2),
                            pts: [(p0.0, p0.1), (p1.0, p1.1), (p2.0, p2.1), (p3.0, p3.1)],
                            rgb: shade_outer(ca, sa),
                        });
                    }
                    if -(cam.0 * ca + cam.1 * sa) + inner_r > -4.0 {
                        let (p0, p1, p2, p3) =
                            (inner_front[i], inner_front[j], inner_back[j], inner_back[i]);
                        quads.push(WallQuad {
                            z_avg: 0.25 * (p0.2 + p1.2 + p2.2 + p3.2),
                            pts: [(p0.0, p0.1), (p1.0, p1.1), (p2.0, p2.1), (p3.0, p3.1)],
                            rgb: shade_inner(ca, sa),
                        });
                    }
                }
                quads.sort_by(|a, b| a.z_avg.total_cmp(&b.z_avg));
                let wall_alpha = material.heavy_opacity(0.42) * alpha_master;
                if !quads.is_empty() && wall_alpha > 0.004 {
                    // Composite all overlapping cylindrical wall quads opaque inside one tight
                    // group opacity layer so segment overlaps never double-blend into seam stripes
                    // (1:1 with Linux `push_group` + `paint_with_alpha`).
                    let (lx, ly, lw, lh) = if hx0.is_finite() && hy0.is_finite() {
                        (hx0 - 4.0, hy0 - 4.0, (hx1 - hx0) + 8.0, (hy1 - hy0) + 8.0)
                    } else {
                        (-OUTER - 16.0, -OUTER - 16.0, OUTER * 2.0 + 32.0, OUTER * 2.0 + 32.0)
                    };
                    self.canvas.push_opacity_layer(lx, ly, lw, lh, wall_alpha);
                    for q in &quads {
                        let dt = (q.pts[1].0 - q.pts[0].0, q.pts[1].1 - q.pts[0].1);
                        let lt = dt.0.hypot(dt.1);
                        let ft = if lt > 1e-3 { (0.45 / lt).min(0.15) } else { 0.0 };
                        let db = (q.pts[2].0 - q.pts[3].0, q.pts[2].1 - q.pts[3].1);
                        let lb = db.0.hypot(db.1);
                        let fb = if lb > 1e-3 { (0.45 / lb).min(0.15) } else { 0.0 };
                        let poly = [
                            (q.pts[0].0 - dt.0 * ft, q.pts[0].1 - dt.1 * ft),
                            (q.pts[1].0 + dt.0 * ft, q.pts[1].1 + dt.1 * ft),
                            (q.pts[2].0 + db.0 * fb, q.pts[2].1 + db.1 * fb),
                            (q.pts[3].0 - db.0 * fb, q.pts[3].1 - db.1 * fb),
                        ];
                        self.canvas
                            .fill_polygon(&poly, D2dColor::from_rgb(q.rgb, 1.0));
                    }
                    self.canvas.pop_layer();
                }
            }

            if cam.2.abs() > half_h {
                let face_outer: Vec<(f64, f64)> = if front_is_pos {
                    outer_front.iter().map(|&(x, y, _)| (x, y)).collect()
                } else {
                    outer_back.iter().map(|&(x, y, _)| (x, y)).collect()
                };
                let face_inner: Vec<(f64, f64)> = if front_is_pos {
                    inner_front.iter().map(|&(x, y, _)| (x, y)).collect()
                } else {
                    inner_back.iter().map(|&(x, y, _)| (x, y)).collect()
                };

                let gy_top = if hy0.is_finite() { hy0 } else { -OUTER };
                let gy_bot = if hy1.is_finite() && hy1 > gy_top + 1.0 {
                    hy1
                } else {
                    gy_top + 2.0 * OUTER
                };

                let uses_backdrop = material.uses_backdrop();
                let face_fade = uses_backdrop && alpha_master < 0.995;
                let face_a = if face_fade {
                    let (lx, ly, lw, lh) = if hx0.is_finite() && hy0.is_finite() {
                        (hx0 - 4.0, hy0 - 4.0, (hx1 - hx0) + 8.0, (hy1 - hy0) + 8.0)
                    } else {
                        (-OUTER - 16.0, -OUTER - 16.0, OUTER * 2.0 + 32.0, OUTER * 2.0 + 32.0)
                    };
                    self.canvas.push_opacity_layer(lx, ly, lw, lh, alpha_master);
                    1.0
                } else {
                    alpha_master
                };

                if uses_backdrop {
                    self.canvas.fill_polygon_ring_backdrop(
                        &face_outer,
                        &face_inner,
                        material.backdrop_alpha * face_a,
                    );
                }

                let a_top = material.ring_opacity(1.08) * face_a;
                let a_mid = material.ring_opacity(0.94) * face_a;
                let a_bot = material.ring_opacity(1.12) * face_a;
                if a_top.max(a_mid).max(a_bot) > 0.002 {
                    let stops = [
                        D2dGradStop {
                            pos: 0.0,
                            color: D2dColor::from_rgb(palette.ring_top, a_top),
                        },
                        D2dGradStop {
                            pos: 0.42,
                            color: D2dColor::from_rgb(palette.ring_mid, a_mid),
                        },
                        D2dGradStop {
                            pos: 1.0,
                            color: D2dColor::from_rgb(palette.ring_bottom, a_bot),
                        },
                    ];
                    self.canvas.fill_polygon_ring_linear(
                        &face_outer,
                        &face_inner,
                        0.0,
                        gy_top,
                        0.0,
                        gy_bot,
                        &stops,
                    );
                }
                if face_fade {
                    self.canvas.pop_layer();
                }

                let face_n = tf.rotate(Vec3(0.0, 0.0, if front_is_pos { 1.0 } else { -1.0 }));
                let sheen = face_n.dot(half_vec).max(0.0).powf(4.0) * pose.depth;
                if sheen > 0.005 {
                    let highlight = lerp_rgb(palette.rim_top, accent, 0.35);
                    let peak = 0.22 * sheen * alpha_master;
                    let band = (0.5 - 0.22 * face_n.1).clamp(0.18, 0.82);
                    let sheen_stops = [
                        D2dGradStop {
                            pos: 0.0,
                            color: D2dColor::from_rgb(highlight, 0.0),
                        },
                        D2dGradStop {
                            pos: (band - 0.06).max(0.01) as f32,
                            color: D2dColor::from_rgb(highlight, peak),
                        },
                        D2dGradStop {
                            pos: (band + 0.06).min(0.99) as f32,
                            color: D2dColor::from_rgb(highlight, peak * 0.85),
                        },
                        D2dGradStop {
                            pos: 1.0,
                            color: D2dColor::from_rgb(highlight, 0.0),
                        },
                    ];
                    self.canvas.fill_polygon_ring_linear(
                        &face_outer,
                        &face_inner,
                        0.0,
                        gy_top,
                        0.0,
                        gy_bot,
                        &sheen_stops,
                    );
                }

                // Project emerging/fading sectors onto the near face during enter/settle
                let face_facing = (cam.2.abs() / PERSPECTIVE_D).clamp(0.0, 1.0);
                if face_facing > 0.35 && app.ring_motion.sectors().next().is_some() {
                    let face_u = Vec3(
                        1.0 - face_n.0 * face_n.0,
                        -face_n.0 * face_n.1,
                        -face_n.0 * face_n.2,
                    )
                    .normalize();
                    let face_v = face_n.cross(face_u);
                    let face_center = face_n.scale(half_h);
                    let proj = |fx: f64, fy: f64| {
                        Transform3D::project_world(
                            face_center.add(face_u.scale(fx)).add(face_v.scale(fy)),
                        )
                    };
                    let o = proj(0.0, 0.0);
                    let px = proj(100.0 * radius_scale, 0.0);
                    let py = proj(0.0, 100.0 * radius_scale);
                    let ex = ((px.0 - o.0) / 100.0, (px.1 - o.1) / 100.0);
                    let ey = ((py.0 - o.0) / 100.0, (py.1 - o.1) / 100.0);
                    let det = ex.0 * ey.1 - ex.1 * ey.0;
                    if det.abs() > 0.15 {
                        let sector_alpha = ((face_facing - 0.35) / 0.65).clamp(0.0, 1.0) * alpha_master;
                        self.canvas.save();
                        self.canvas.push_clip_polygon_ring(&face_outer, &face_inner);
                        self.canvas.concat_matrix(ex.0, ex.1, ey.0, ey.1, o.0, o.1);
                        self.draw_sectors(app, palette, sector_alpha);
                        self.canvas.pop_layer();
                        self.canvas.restore();
                    }
                }

                // Near face rims
                self.canvas.stroke_polygon_ring_linear(
                    &face_outer,
                    &face_inner,
                    1.0,
                    0.0,
                    gy_top,
                    0.0,
                    gy_bot,
                    D2dColor::from_rgb(palette.rim_top, 0.70 * alpha_master),
                    D2dColor::from_rgb(palette.rim_bottom, 0.27 * alpha_master),
                );
                if pose.depth > 0.01 {
                    self.canvas.stroke_polygon(
                        &face_outer,
                        1.4,
                        D2dColor::from_rgb(accent, (0.35 + 0.40 * sheen) * pose.depth * alpha_master),
                    );
                }
            }
        }

        // ==================== MORPHING EXPLORER PANELS (EXACT LINUX explorer.rs) ====================
        fn draw_panel_shell(
            &self,
            b: ExplorerBounds,
            material: SurfaceMaterial,
            palette: &Palette,
            alpha_master: f64,
        ) {
            let x = -b.width * 0.5;
            let y = -b.height * 0.5;

            self.canvas.fill_rounded_rect_shadow(
                x,
                y,
                b.width,
                b.height,
                b.radius,
                D2dColor::from_rgb(palette.shadow, 1.0),
                alpha_master,
            );

            let uses_backdrop = material.uses_backdrop();
            let fade_layer = uses_backdrop && alpha_master < 0.995;
            let local_a = if fade_layer {
                self.canvas
                    .push_opacity_layer(x - 1.0, y - 1.0, b.width + 2.0, b.height + 2.0, alpha_master);
                1.0
            } else {
                alpha_master
            };

            if uses_backdrop {
                self.canvas.fill_rounded_rect_backdrop(
                    x,
                    y,
                    b.width,
                    b.height,
                    b.radius,
                    b.hole,
                    material.backdrop_alpha * local_a,
                );
            }

            let a_top = material.panel_opacity(b.t, 1.08, 1.06) * local_a;
            let a_mid = material.panel_opacity(b.t, 0.94, 0.96) * local_a;
            let a_bot = material.panel_opacity(b.t, 1.12, 1.08) * local_a;
            if a_top.max(a_mid).max(a_bot) > 0.002 {
                let stops = [
                    D2dGradStop {
                        pos: 0.0,
                        color: D2dColor::from_rgb(palette.ring_top, a_top),
                    },
                    D2dGradStop {
                        pos: 0.45,
                        color: D2dColor::from_rgb(palette.ring_mid, a_mid),
                    },
                    D2dGradStop {
                        pos: 1.0,
                        color: D2dColor::from_rgb(palette.ring_bottom, a_bot),
                    },
                ];
                self.canvas.fill_rounded_rect_linear(
                    x,
                    y,
                    b.width,
                    b.height,
                    b.radius,
                    b.hole,
                    0.0,
                    y,
                    0.0,
                    y + b.height,
                    &stops,
                );
            }
            // Refined Apple-style specular glass top sheen catching light on the floating panel
            if material.translucency > 0.15 && b.t > 0.35 && b.hole <= 1.0 {
                let sheen_a = 0.08 * material.translucency * smoothstep(0.35, 0.85, b.t) * local_a;
                self.canvas.fill_top_sheen(
                    x,
                    y,
                    b.width,
                    b.height,
                    b.radius,
                    (b.height * 0.22).clamp(18.0, 38.0),
                    D2dColor::from_rgb(palette.rim_top, sheen_a),
                );
            }
            if fade_layer {
                self.canvas.pop_layer();
            }
        }

        fn draw_panel_rims(&self, b: ExplorerBounds, palette: &Palette, alpha_master: f64) {
            let x = -b.width * 0.5;
            let y = -b.height * 0.5;
            self.canvas.stroke_rounded_rect_linear(
                x,
                y,
                b.width,
                b.height,
                b.radius,
                1.0,
                0.0,
                y,
                0.0,
                y + b.height,
                D2dColor::from_rgb(palette.rim_top, 0.70 * alpha_master),
                D2dColor::from_rgb(palette.rim_bottom, 0.30 * alpha_master),
            );
            if b.hole > 2.0 {
                let hole_alpha = smoothstep(2.0, INNER * 0.65, b.hole) * alpha_master;
                self.canvas.stroke_circle_linear(
                    0.0,
                    0.0,
                    b.hole,
                    1.0,
                    0.0,
                    -b.hole,
                    0.0,
                    b.hole,
                    D2dColor::from_rgb(palette.rim_top, 0.70 * hole_alpha),
                    D2dColor::from_rgb(palette.rim_bottom, 0.27 * hole_alpha),
                );
            }
        }

        fn draw_soft_divider(&self, width: f64, y: f64, rgb: Rgb, alpha: f64) {
            if alpha <= 0.002 {
                return;
            }
            let half = width * 0.5 - 18.0;
            self.canvas.draw_line(
                -half,
                y,
                half,
                y,
                1.0,
                D2dColor::from_rgb(rgb, alpha),
                false,
            );
        }

        fn draw_morphing_ring_chrome(
            &self,
            b: ExplorerBounds,
            app: &App,
            palette: &Palette,
            alpha_master: f64,
            grid_mode: bool,
        ) {
            let ring_chrome = (1.0 - b.t / 0.28).clamp(0.0, 1.0) * alpha_master;
            if ring_chrome <= 0.004 {
                return;
            }
            self.canvas.push_clip_rounded_rect_hole(
                -b.width * 0.5,
                -b.height * 0.5,
                b.width,
                b.height,
                b.radius,
                b.hole,
            );
            for sec in app.ring_motion.sectors() {
                if sec.slot.born_in_box() || (grid_mode && sec.slot.merging_to_ring()) {
                    continue;
                }
                if grid_mode {
                    if let Some((start, end)) = sec.slot.ring_sector() {
                        let ring_sec = RingSector {
                            slot: sec.slot,
                            start,
                            end,
                        };
                        self.draw_sector_chrome(&ring_sec, app, palette, ring_chrome);
                    } else if sec.slot.ring_angle().is_none() {
                        self.draw_sector_chrome(&sec, app, palette, ring_chrome);
                    }
                } else {
                    self.draw_sector_chrome(&sec, app, palette, ring_chrome);
                }
            }
            self.canvas.pop_layer();
        }

        fn draw_panel_search_bar(
            &mut self,
            b: ExplorerBounds,
            query: &str,
            suggestion: &str,
            material: SurfaceMaterial,
            palette: &Palette,
            alpha: f64,
        ) {
            let top = -b.height * 0.5;
            let bar_x = -b.width * 0.5 + 18.0;
            let bar_y = top + 40.0;
            let bar_w = b.width - 36.0;
            let bar_h = 28.0;
            self.canvas.fill_rounded_rect_solid(
                bar_x,
                bar_y,
                bar_w,
                bar_h,
                8.0,
                D2dColor::from_rgb(
                    palette.capsule_bottom,
                    material.control_opacity(0.24, 0.80) * alpha,
                ),
            );
            self.canvas.stroke_rounded_rect(
                bar_x,
                bar_y,
                bar_w,
                bar_h,
                8.0,
                1.0,
                D2dColor::from_rgb(palette.capsule_border, 0.30 * alpha),
            );
            self.canvas.draw_query_text(
                query,
                suggestion,
                FONT_CONTROL,
                false,
                bar_x + 11.0,
                bar_y,
                (bar_w - 22.0).max(8.0),
                bar_h,
                0,
                D2dColor::from_rgb(palette.text_capsule, alpha),
                D2dColor::from_rgb(palette.text_suggestion, alpha),
                None,
            );
        }

        fn draw_grid_panel(
            &mut self,
            b: ExplorerBounds,
            app: &App,
            palette: &Palette,
            material: SurfaceMaterial,
            alpha_master: f64,
        ) {
            if b.scale <= 0.001 {
                return;
            }
            self.canvas.save();
            self.canvas.translate(b.cx, b.cy);
            self.canvas.scale(b.scale, b.scale);

            self.draw_panel_shell(b, material, palette, alpha_master);
            self.draw_morphing_ring_chrome(b, app, palette, alpha_master, true);

            let pressed_vis = app.pressed_vis();
            let selected_vis = app.selected_vis();
            let show_shortcuts = app.cfg.keybinds.number_shortcuts;
            let count = app.visible_count();

            // Snapshot tiles for rendering (siblings first, then primaries on top)
            struct TileDraw {
                tile: crate::layout::TileGeom,
                index: Option<usize>,
                grid_index: usize,
                merging: bool,
                focus: f64,
                tilt: f64,
                faces: Vec<(Entry, f64)>,
            }
            let mut tiles = Vec::new();
            for siblings in [true, false] {
                for sec in app.ring_motion.sectors() {
                    if sec.slot.ring_sibling() != siblings {
                        continue;
                    }
                    if !(sec.slot.index.is_some()
                        || sec.slot.merging_to_ring()
                        || b.box_t < 0.45
                        || (count == 0 && sec.slot.born_in_box()))
                    {
                        continue;
                    }
                    let tile = b.tile(&sec);
                    if tile.emerge_alpha <= 0.005 {
                        continue;
                    }
                    let faces = sec
                        .slot
                        .faces
                        .iter()
                        .filter_map(|f| {
                            let a = f.opacity * tile.emerge_alpha * alpha_master;
                            (a > 0.008).then(|| (f.entry.clone(), a))
                        })
                        .collect();
                    tiles.push(TileDraw {
                        tile,
                        index: sec.slot.index,
                        grid_index: sec.slot.grid_index(),
                        merging: sec.slot.merging_to_ring(),
                        focus: sec.slot.focus(),
                        tilt: sec.slot.tilt(),
                        faces,
                    });
                }
            }

            let card_mix = smoothstep(0.18, 0.58, b.box_t);
            let badge_alpha = smoothstep(0.74, 0.96, b.box_t);

            let clip_tiles = b.t < 0.999 || b.hole > 0.0;
            if clip_tiles {
                self.canvas.push_clip_rounded_rect_hole(
                    -b.width * 0.5,
                    -b.height * 0.5,
                    b.width,
                    b.height,
                    b.radius,
                    b.hole,
                );
            }
            for td in tiles {
                let pressed = td.index.is_some() && td.index == pressed_vis;
                let selected = td.index.is_some() && td.index == selected_vis;
                let tile_alpha = td.tile.emerge_alpha * alpha_master;

                self.canvas.save();
                self.canvas.translate(td.tile.x, td.tile.y);
                if td.tile.emerge_scale < 0.999 {
                    self.canvas.scale(td.tile.emerge_scale, td.tile.emerge_scale);
                }

                if (pressed || selected) && card_mix > 0.0 && let Some((entry, _)) = td.faces.last() {
                    let tint = palette.accent_for_entry(entry, &app.mode);
                    let focus = td.focus.clamp(0.0, 1.0);
                    let fill_a = if pressed {
                        0.27
                    } else {
                        0.11 + 0.08 * focus
                    } * card_mix
                        * tile_alpha;
                    let stroke_a = 0.50 * card_mix * tile_alpha;
                    self.canvas.fill_rounded_rect_solid(
                        0.5 - td.tile.width * 0.5,
                        0.5 - td.tile.height * 0.5,
                        td.tile.width - 1.0,
                        td.tile.height - 1.0,
                        12.0,
                        D2dColor::from_rgb(tint, fill_a),
                    );
                    self.canvas.stroke_rounded_rect(
                        0.5 - td.tile.width * 0.5,
                        0.5 - td.tile.height * 0.5,
                        td.tile.width - 1.0,
                        td.tile.height - 1.0,
                        12.0,
                        1.0,
                        D2dColor::from_rgb(tint, stroke_a),
                    );
                }

                let icon_y = lerp(-9.0, -15.0, b.box_t);
                let plate = lerp(32.0, 48.0, b.box_t);
                let icon_size = lerp(20.0, 26.0, b.box_t);
                let focus = td.focus.clamp(0.0, 1.0);
                let bold = b.box_t < 0.5;
                let wrap = b.box_t >= 0.45;
                let label_y = lerp(11.0, 14.5, b.box_t);
                let settled_avail_w = (CELL_W - 8.0 - 12.0).max(24.0);
                let current_avail_w = (td.tile.width - 12.0).max(24.0);
                let (layout_w, label_scale) = if wrap {
                    (
                        settled_avail_w,
                        if current_avail_w < settled_avail_w - 0.5 {
                            (current_avail_w / settled_avail_w).clamp(0.75, 1.0)
                        } else {
                            1.0
                        },
                    )
                } else {
                    let ring_avail_w = 76.0_f64;
                    (
                        ring_avail_w,
                        (current_avail_w / ring_avail_w).clamp(0.88, 1.25),
                    )
                };

                for (entry, face_alpha) in &td.faces {
                    self.canvas.save();
                    let zoom = if pressed { 0.96 } else { 1.0 + 0.04 * focus };
                    if (zoom - 1.0).abs() >= 1e-4 {
                        self.canvas.scale(zoom, zoom);
                    }
                    self.canvas.translate(0.0, icon_y);
                    if td.tilt.abs() >= 0.003 {
                        self.canvas.rotate(td.tilt);
                    }
                    self.draw_icon_plate(entry, &app.mode, 0.0, plate, icon_size, palette, *face_alpha);
                    self.canvas.restore();

                    let lbl_h = if wrap { 34.0 } else { 18.0 };
                    if (label_scale - 1.0).abs() >= 1e-3 {
                        self.canvas.save();
                        self.canvas.translate(0.0, label_y);
                        self.canvas.scale(label_scale, label_scale);
                        self.canvas.draw_text(
                            &entry.title,
                            FONT_CAPTION,
                            bold,
                            -layout_w * 0.5,
                            0.0,
                            layout_w,
                            lbl_h,
                            1,
                            3,
                            if wrap { 2 } else { 1 },
                            D2dColor::from_rgb(palette.text_primary, *face_alpha),
                            Some(D2dColor::from_rgb(palette.text_shadow, 0.72 * face_alpha)),
                        );
                        self.canvas.restore();
                    } else {
                        self.canvas.draw_text(
                            &entry.title,
                            FONT_CAPTION,
                            bold,
                            -layout_w * 0.5,
                            label_y,
                            layout_w,
                            lbl_h,
                            1,
                            3,
                            if wrap { 2 } else { 1 },
                            D2dColor::from_rgb(palette.text_primary, *face_alpha),
                            Some(D2dColor::from_rgb(palette.text_shadow, 0.72 * face_alpha)),
                        );
                    }
                }

                if show_shortcuts
                    && (td.index.is_some() || td.merging)
                    && td.grid_index < PAGE_SIZE
                    && badge_alpha > 0.01
                {
                    let badge = format!("{}", td.grid_index + 1);
                    let col = if selected || pressed {
                        palette.text_primary
                    } else {
                        palette.text_suggestion
                    };
                    let ba = (if selected || pressed { 0.92 } else { 0.62 }) * badge_alpha * tile_alpha;
                    self.canvas.draw_text(
                        &badge,
                        FONT_BADGE,
                        false,
                        td.tile.width * 0.5 - 18.0,
                        -td.tile.height * 0.5 + 5.0,
                        14.0,
                        16.0,
                        1,
                        3,
                        1,
                        D2dColor::from_rgb(col, ba),
                        None,
                    );
                }
                self.canvas.restore();
            }
            if clip_tiles {
                self.canvas.pop_layer();
            }

            // Grid chrome (header, search bar, dividers, scrollbar, footer)
            let box_chrome = smoothstep(0.65, 0.96, b.box_t) * alpha_master;
            if box_chrome > 0.004 {
                let top = -b.height * 0.5;
                let x = -b.width * 0.5 + 20.0;

                let total = app.menu.filtered.len();
                let first = app.grid_first_visible();
                let range_text = if total > 0 {
                    format!("{}–{} / {}", first + 1, (first + count).min(total), total)
                } else {
                    String::new()
                };
                let range_w = self.measure_cached(&range_text, FONT_CAPTION, false).0 + 8.0;
                let right = b.width * 0.5 - 20.0 - range_w;
                if !range_text.is_empty() {
                    self.canvas.draw_text(
                        &range_text,
                        FONT_CAPTION,
                        false,
                        right,
                        top + 11.0,
                        range_w,
                        24.0,
                        2,
                        3,
                        1,
                        D2dColor::from_rgb(palette.text_suggestion, 0.90 * box_chrome),
                        None,
                    );
                }

                let title_raw = app.display_title();
                let title = if title_raw.is_empty() {
                    "Main menu"
                } else {
                    title_raw.as_str()
                };
                self.canvas.draw_text(
                    title,
                    FONT_TITLE,
                    true,
                    x,
                    top + 10.0,
                    (right - x - 12.0).max(20.0),
                    26.0,
                    0,
                    3,
                    1,
                    D2dColor::from_rgb(palette.text_primary, box_chrome),
                    None,
                );

                if !app.menu.query.is_empty() {
                    let sugg = app.inline_suggestion().unwrap_or_default();
                    self.draw_panel_search_bar(b, &app.menu.query, &sugg, material, palette, box_chrome);
                }

                self.draw_soft_divider(b.width, b.grid_top() - 4.0, palette.divider, 0.14 * box_chrome);

                if let Some((thumb_top, height)) = b.thumb(first, total) {
                    let track_x = b.width * 0.5 - 10.0;
                    self.canvas.fill_rounded_rect_solid(
                        track_x,
                        b.grid_top() + 4.0,
                        4.0,
                        GRID_H - 8.0,
                        2.0,
                        D2dColor::from_rgb(palette.divider, 0.09 * box_chrome),
                    );
                    self.canvas.fill_rounded_rect_solid(
                        track_x,
                        thumb_top,
                        4.0,
                        height,
                        2.0,
                        D2dColor::from_rgb(
                            palette.rim_top,
                            (if app.scrollbar_dragging { 0.85 } else { 0.50 }) * box_chrome,
                        ),
                    );
                }

                self.draw_soft_divider(
                    b.width,
                    b.height * 0.5 - GRID_FOOTER + 2.0,
                    palette.divider,
                    0.11 * box_chrome,
                );

                let status = app.display_status();
                if !status.is_empty() {
                    self.canvas.draw_text(
                        &status,
                        FONT_CAPTION,
                        false,
                        x,
                        b.height * 0.5 - GRID_FOOTER + 5.0,
                        b.width - 40.0,
                        GRID_FOOTER - 8.0,
                        0,
                        2, // Middle ellipsize for paths
                        1,
                        D2dColor::from_rgb(palette.text_suggestion, 0.95 * box_chrome),
                        None,
                    );
                }
            }

            self.draw_panel_rims(b, palette, alpha_master);

            let ring_chrome = (1.0 - b.t / 0.28).clamp(0.0, 1.0) * alpha_master;
            if ring_chrome > 0.004 {
                self.draw_captions(app, palette, material, ring_chrome);
            }
            self.canvas.restore();
        }

        fn draw_list_panel(
            &mut self,
            b: ExplorerBounds,
            app: &App,
            palette: &Palette,
            material: SurfaceMaterial,
            alpha_master: f64,
        ) {
            if b.scale <= 0.001 {
                return;
            }
            self.canvas.save();
            self.canvas.translate(b.cx, b.cy);
            self.canvas.scale(b.scale, b.scale);

            self.draw_panel_shell(b, material, palette, alpha_master);
            self.draw_morphing_ring_chrome(b, app, palette, alpha_master, false);

            let total = app.menu.filtered.len();
            let has_scrollbar = total > LIST_ROWS;
            let pressed_vis = app.pressed_vis();
            let selected_vis = app.selected_vis();
            let count = app.visible_count();

            struct RowDraw {
                row: crate::layout::TileGeom,
                index: Option<usize>,
                born_in_box: bool,
                focus: f64,
                faces: Vec<(Entry, f64)>,
            }
            let mut rows = Vec::new();
            for sec in app.ring_motion.sectors() {
                if !(sec.slot.index.is_some() || b.list_t < 0.45) {
                    continue;
                }
                let row = b.list_row(&sec, has_scrollbar);
                if row.emerge_alpha <= 0.005 {
                    continue;
                }
                let faces = sec
                    .slot
                    .faces
                    .iter()
                    .filter_map(|f| {
                        let a = f.opacity * row.emerge_alpha * alpha_master;
                        (a > 0.008).then(|| (f.entry.clone(), a))
                    })
                    .collect();
                rows.push(RowDraw {
                    row,
                    index: sec.slot.index,
                    born_in_box: sec.slot.born_in_box(),
                    focus: sec.slot.focus(),
                    faces,
                });
            }

            let card_mix = smoothstep(0.18, 0.58, b.list_t);
            let clip_rows = b.t < 0.999 || b.hole > 0.0;
            if clip_rows {
                self.canvas.push_clip_rounded_rect_hole(
                    -b.width * 0.5,
                    -b.height * 0.5,
                    b.width,
                    b.height,
                    b.radius,
                    b.hole,
                );
            }
            for rd in rows {
                let pressed = rd.index.is_some() && rd.index == pressed_vis;
                let selected = rd.index.is_some() && rd.index == selected_vis;
                let row_alpha = rd.row.emerge_alpha * alpha_master;
                let focus = rd.focus.clamp(0.0, 1.0);
                let zoom = if pressed {
                    0.985
                } else {
                    1.0 + 0.012 * focus * card_mix
                };
                let total_scale = rd.row.emerge_scale * zoom;

                self.canvas.save();
                self.canvas.translate(rd.row.x, rd.row.y);
                if (total_scale - 1.0).abs() >= 1e-4 {
                    self.canvas.scale(total_scale, total_scale);
                }

                if (pressed || selected) && card_mix > 0.0 && let Some((entry, _)) = rd.faces.last() {
                    let tint = palette.accent_for_entry(entry, &app.mode);
                    let fill_a = if pressed { 0.27 } else { 0.13 + 0.08 * focus } * card_mix * row_alpha;
                    let stroke_a = 0.52 * card_mix * row_alpha;
                    self.canvas.fill_rounded_rect_solid(
                        0.5 - rd.row.width * 0.5,
                        0.5 - rd.row.height * 0.5,
                        rd.row.width - 1.0,
                        rd.row.height - 1.0,
                        10.0,
                        D2dColor::from_rgb(tint, fill_a),
                    );
                    self.canvas.stroke_rounded_rect(
                        0.5 - rd.row.width * 0.5,
                        0.5 - rd.row.height * 0.5,
                        rd.row.width - 1.0,
                        rd.row.height - 1.0,
                        10.0,
                        1.0,
                        D2dColor::from_rgb(tint, stroke_a),
                    );
                    // Left accent indicator bar (exact Linux draw_list_row)
                    self.canvas.fill_rounded_rect_solid(
                        -rd.row.width * 0.5 + 7.0,
                        -8.0,
                        3.0,
                        16.0,
                        1.5,
                        D2dColor::from_rgb(tint, (0.72 + 0.25 * focus) * card_mix * row_alpha),
                    );
                } else if b.t > 0.65 && rd.index.is_some_and(|idx| idx + 1 < count) {
                    let div_a = 0.07 * smoothstep(0.65, 0.95, b.t) * row_alpha;
                    self.canvas.draw_line(
                        -rd.row.width * 0.5 + 14.0,
                        rd.row.height * 0.5 + 1.5,
                        rd.row.width * 0.5 - 14.0,
                        rd.row.height * 0.5 + 1.5,
                        1.0,
                        D2dColor::from_rgb(palette.divider, div_a),
                        false,
                    );
                }

                let departing_icon_a = if rd.born_in_box {
                    0.0
                } else {
                    (1.0 - b.t / 0.28).clamp(0.0, 1.0)
                };
                for (entry, face_alpha) in &rd.faces {
                    if departing_icon_a > 0.01 {
                        self.draw_icon_plate(
                            entry,
                            &app.mode,
                            lerp(-9.0, 0.0, b.t),
                            32.0,
                            20.0,
                            palette,
                            face_alpha * departing_icon_a,
                        );
                    }
                    let bold = selected || pressed || entry.active || b.t < 0.45;
                    let left_pad = 16.0;
                    let right_reserve = if entry.active { 32.0 } else { left_pad };
                    let text_w = (rd.row.width - left_pad - right_reserve).max(24.0);
                    let align_t = smoothstep(0.15, 0.65, b.t);
                    let text_y = lerp(11.0, 0.0, smoothstep(0.0, 0.45, b.t));
                    let left_x = lerp(-text_w * 0.5, -rd.row.width * 0.5 + left_pad, align_t);
                    self.canvas.draw_text(
                        &entry.title,
                        FONT_CONTROL,
                        bold,
                        left_x,
                        text_y - 12.0,
                        text_w,
                        24.0,
                        if align_t < 0.4 { 1 } else { 0 },
                        3,
                        1,
                        D2dColor::from_rgb(palette.text_primary, *face_alpha),
                        Some(D2dColor::from_rgb(palette.text_shadow, 0.72 * face_alpha)),
                    );

                    if entry.active {
                        let dot_a = smoothstep(0.45, 0.85, b.list_t) * face_alpha;
                        if dot_a > 0.01 {
                            let tint = palette.accent_for_entry(entry, &app.mode);
                            self.canvas.fill_circle(
                                rd.row.width * 0.5 - 16.0,
                                0.0,
                                3.6,
                                D2dColor::from_rgb(tint, 0.92 * dot_a),
                            );
                        }
                    }
                }
                self.canvas.restore();
            }
            if clip_rows {
                self.canvas.pop_layer();
            }

            let box_chrome = smoothstep(0.60, 0.94, b.list_t) * alpha_master;
            if box_chrome > 0.004 {
                let top = -b.height * 0.5;
                let x = -b.width * 0.5 + 20.0;
                let title_raw = app.display_title();
                let title = if title_raw.is_empty() {
                    "Choose Option"
                } else {
                    title_raw.as_str()
                };
                self.canvas.draw_text(
                    title,
                    FONT_TITLE,
                    true,
                    x,
                    top + 10.0,
                    (b.width - 40.0).max(20.0),
                    26.0,
                    0,
                    3,
                    1,
                    D2dColor::from_rgb(palette.text_primary, box_chrome),
                    None,
                );
                if !app.menu.query.is_empty() {
                    let sugg = app.inline_suggestion().unwrap_or_default();
                    self.draw_panel_search_bar(b, &app.menu.query, &sugg, material, palette, box_chrome);
                }
                self.draw_soft_divider(b.width, b.grid_top() - 4.0, palette.divider, 0.14 * box_chrome);

                if let Some((thumb_top, height)) = b.list_thumb(app.menu.list_first_visible(), total) {
                    let track_x = b.width * 0.5 - 10.0;
                    let track_h = (b.list_body_height() - 8.0).max(24.0);
                    self.canvas.fill_rounded_rect_solid(
                        track_x,
                        b.grid_top() + 4.0,
                        4.0,
                        track_h,
                        2.0,
                        D2dColor::from_rgb(palette.divider, 0.09 * box_chrome),
                    );
                    self.canvas.fill_rounded_rect_solid(
                        track_x,
                        thumb_top,
                        4.0,
                        height,
                        2.0,
                        D2dColor::from_rgb(
                            palette.rim_top,
                            (if app.scrollbar_dragging { 0.85 } else { 0.50 }) * box_chrome,
                        ),
                    );
                }
            }

            self.draw_panel_rims(b, palette, alpha_master);
            let ring_chrome = (1.0 - b.t / 0.28).clamp(0.0, 1.0) * alpha_master;
            if ring_chrome > 0.004 {
                self.draw_captions(app, palette, material, ring_chrome);
            }
            self.canvas.restore();
        }

        fn draw_input_panel(
            &mut self,
            b: ExplorerBounds,
            app: &App,
            palette: &Palette,
            material: SurfaceMaterial,
            alpha_master: f64,
        ) {
            if b.scale <= 0.001 {
                return;
            }
            self.canvas.save();
            self.canvas.translate(b.cx, b.cy);
            self.canvas.scale(b.scale, b.scale);

            self.draw_panel_shell(b, material, palette, alpha_master);
            self.draw_morphing_ring_chrome(b, app, palette, alpha_master, false);

            // Fade departing ring items smoothly into the morphing input panel
            let item_fade = (1.0 - smoothstep(0.04, 0.44, b.input_t)).clamp(0.0, 1.0) * alpha_master;
            if item_fade > 0.008 {
                self.canvas.push_clip_rounded_rect_hole(
                    -b.width * 0.5,
                    -b.height * 0.5,
                    b.width,
                    b.height,
                    b.radius,
                    b.hole,
                );
                let pull = lerp(1.0, 0.72, b.input_t);
                let grid_total = b.box_t + b.list_t;
                let has_sb = app.visible_count() > LIST_ROWS;
                let mut fading = Vec::new();
                for sec in app.ring_motion.sectors() {
                    if sec.slot.born_in_box() && grid_total <= 0.01 {
                        continue;
                    }
                    let angle = sec
                        .slot
                        .ring_angle()
                        .unwrap_or((sec.start + sec.end) * 0.5);
                    let ring_x = ITEM_RADIUS * angle.cos();
                    let ring_y = ITEM_RADIUS * angle.sin();
                    let idx = sec.slot.grid_index();
                    let gx = GRID_LEFT + (idx % GRID_COLUMNS) as f64 * CELL_W + CELL_W * 0.5 - GRID_WIDTH * 0.5;
                    let gy = b.header + ((idx / GRID_COLUMNS) as f64 + 0.5) * CELL_H
                        - (b.header + GRID_H + GRID_FOOTER) * 0.5;
                    let right_pad = if has_sb { 20.0 } else { 14.0 };
                    let rx = (14.0 - right_pad) * 0.5;
                    let ry = b.header + (idx as f64 + 0.5) * LIST_ROW_H
                        - (b.header + b.list_body_height() + LIST_FOOTER) * 0.5;
                    let bx = if sec.slot.born_in_box() {
                        if b.list_t > b.box_t { rx } else { gx }
                    } else {
                        ring_x * (1.0 - grid_total).max(0.0) + gx * b.box_t + rx * b.list_t
                    };
                    let by = if sec.slot.born_in_box() {
                        if b.list_t > b.box_t { ry } else { gy }
                    } else {
                        ring_y * (1.0 - grid_total).max(0.0) + gy * b.box_t + ry * b.list_t
                    };
                    for f in &sec.slot.faces {
                        let a = f.opacity * item_fade;
                        if a > 0.008 {
                            fading.push((bx * pull, by * pull, f.entry.clone(), a));
                        }
                    }
                }
                for (x, y, entry, a) in fading {
                    self.canvas.save();
                    self.canvas.translate(x, y);
                    self.canvas.scale(pull, pull);
                    let plate = lerp(32.0, 48.0, b.box_t);
                    let icon_size = lerp(20.0, 26.0, b.box_t);
                    self.draw_icon_plate(&entry, &app.mode, lerp(-9.0, -15.0, b.box_t), plate, icon_size, palette, a);
                    self.canvas.restore();
                }
                self.canvas.pop_layer();
            }

            let chrome = smoothstep(0.28, 0.92, b.input_t) * alpha_master;
            if chrome > 0.005 {
                let top = -b.height * 0.5;
                let x = -b.width * 0.5 + 20.0;

                let accent = palette.accent_action;
                let is_color = app.color_picker.is_some();
                let is_slider = app.slider.is_some();
                let title = if let Some(cp) = &app.color_picker {
                    cp.field.label().to_string()
                } else if let Some(s) = &app.slider {
                    s.field.label().to_string()
                } else if let Some(inp) = &app.input_box {
                    inp.prompt.clone()
                } else {
                    app.display_title()
                };
                let title_w = if is_color || is_slider {
                    (b.width - 112.0).max(40.0)
                } else {
                    (b.width - 40.0).max(40.0)
                };
                self.canvas.draw_text(
                    &title,
                    FONT_TITLE,
                    true,
                    x,
                    top + 10.0,
                    title_w,
                    26.0,
                    0,
                    3,
                    1,
                    D2dColor::from_rgb(palette.text_primary, chrome),
                    Some(D2dColor::from_rgb(palette.text_shadow, 0.72 * chrome)),
                );

                if let Some(cp) = &app.color_picker {
                    let col = cp.color();
                    let sw = 54.0;
                    let sh = 22.0;
                    let sx = b.width * 0.5 - sw - 20.0;
                    let sy = top + 12.0;
                    self.canvas.fill_rounded_rect_solid(
                        sx,
                        sy,
                        sw,
                        sh,
                        7.0,
                        D2dColor::rgba(col.r, col.g, col.b, chrome),
                    );
                    self.canvas.stroke_rounded_rect(
                        sx,
                        sy,
                        sw,
                        sh,
                        7.0,
                        1.2,
                        D2dColor::from_rgb(palette.text_primary, 0.55 * chrome),
                    );
                } else if let Some(s) = &app.slider {
                    // Tabular-figure live percentage pill in header (zero horizontal jitter while scrubbing)
                    let pill_w = 58.0;
                    let pill_h = 24.0;
                    let px = b.width * 0.5 - pill_w - 20.0;
                    let py = top + 11.0;
                    self.canvas.fill_rounded_rect_solid(
                        px,
                        py,
                        pill_w,
                        pill_h,
                        pill_h * 0.5,
                        D2dColor::from_rgb(accent, (if s.dragging { 0.28 } else { 0.18 }) * chrome),
                    );
                    self.canvas.stroke_rounded_rect(
                        px,
                        py,
                        pill_w,
                        pill_h,
                        pill_h * 0.5,
                        1.0,
                        D2dColor::from_rgb(accent, (if s.dragging { 0.78 } else { 0.52 }) * chrome),
                    );
                    let pct_text = format!("{}%", s.value);
                    self.canvas.draw_text(
                        &pct_text,
                        FONT_CONTROL,
                        true,
                        px,
                        py,
                        pill_w,
                        pill_h,
                        1,
                        3,
                        1,
                        D2dColor::from_rgb(palette.text_primary, chrome),
                        Some(D2dColor::from_rgb(palette.text_shadow, 0.72 * chrome)),
                    );
                }

                self.draw_soft_divider(b.width, b.grid_top() - 4.0, palette.divider, 0.14 * chrome);

                if let Some(s) = &app.slider {
                    let (tx, ty, tw, th) = b.slider_track_rect();
                    let track_r = th * 0.5;
                    let track_shift = s.overshoot * 0.35;
                    let track_x = tx + track_shift;
                    let frac = (s.visual_value / 100.0).clamp(0.0, 1.0);
                    let thumb_x = tx + frac * tw + s.overshoot;
                    let thumb_y = ty + th * 0.5;

                    // Recessed glass track well (stays legible even at 100% translucency)
                    self.canvas.fill_rounded_rect_solid(
                        track_x,
                        ty,
                        tw,
                        th,
                        track_r,
                        D2dColor::from_rgb(
                            palette.capsule_bottom,
                            material.control_opacity(0.36, 0.80) * chrome,
                        ),
                    );
                    self.canvas.stroke_rounded_rect(
                        track_x,
                        ty,
                        tw,
                        th,
                        track_r,
                        1.0,
                        D2dColor::from_rgb(palette.capsule_border, 0.42 * chrome),
                    );

                    // Filled accent progress bar up to thumb center
                    let fill_w = (thumb_x - track_x).clamp(0.0, tw);
                    if fill_w > 1.0 {
                        let fill_stops = [
                            D2dGradStop {
                                pos: 0.0,
                                color: D2dColor::from_rgb(
                                    lerp_rgb(accent, palette.ring_top, 0.16),
                                    0.86 * chrome,
                                ),
                            },
                            D2dGradStop {
                                pos: 1.0,
                                color: D2dColor::from_rgb(
                                    lerp_rgb(accent, (1.0, 1.0, 1.0), 0.16),
                                    0.98 * chrome,
                                ),
                            },
                        ];
                        self.canvas.fill_rounded_rect_linear(
                            track_x,
                            ty,
                            fill_w.max(th),
                            th,
                            track_r,
                            0.0,
                            track_x,
                            ty,
                            track_x + tw,
                            ty,
                            &fill_stops,
                        );
                    }

                    // Subtle 0/25/50/75/100% tick detents underneath track
                    let tick_top = ty + th + 9.0;
                    for step in 0..=4 {
                        let t = f64::from(step) * 0.25;
                        let px = track_x + t * tw;
                        let h = if step == 0 || step == 4 { 5.5 } else { 4.0 };
                        let a = if step == 0 || step == 4 { 0.44 } else { 0.28 };
                        self.canvas.draw_line(
                            px,
                            tick_top,
                            px,
                            tick_top + h,
                            1.2,
                            D2dColor::from_rgb(palette.divider, a * chrome),
                            true,
                        );
                    }

                    // Wayfinding endpoint labels + optional override context hint
                    let lbl_y = tick_top + 9.0;
                    self.canvas.draw_text(
                        s.field.min_label(),
                        FONT_BADGE,
                        false,
                        track_x,
                        lbl_y,
                        118.0,
                        16.0,
                        0,
                        3,
                        1,
                        D2dColor::from_rgb(palette.text_suggestion, 0.90 * chrome),
                        Some(D2dColor::from_rgb(palette.text_shadow, 0.72 * chrome)),
                    );
                    self.canvas.draw_text(
                        s.field.max_label(),
                        FONT_BADGE,
                        false,
                        track_x + tw - 118.0,
                        lbl_y,
                        118.0,
                        16.0,
                        2,
                        3,
                        1,
                        D2dColor::from_rgb(palette.text_suggestion, 0.90 * chrome),
                        Some(D2dColor::from_rgb(palette.text_shadow, 0.72 * chrome)),
                    );
                    let hint = if app.cfg.appearance.high_contrast {
                        Some("High Contrast active")
                    } else if s.field == SliderField::Blur && app.cfg.appearance.translucency == 0 {
                        Some("Translucency is 0%")
                    } else {
                        None
                    };
                    if let Some(hint_str) = hint {
                        self.canvas.draw_text(
                            hint_str,
                            FONT_BADGE,
                            true,
                            track_x + 96.0,
                            lbl_y,
                            (tw - 192.0).max(40.0),
                            16.0,
                            1,
                            3,
                            1,
                            D2dColor::from_rgb(accent, 0.92 * chrome),
                            Some(D2dColor::from_rgb(palette.text_shadow, 0.75 * chrome)),
                        );
                    }

                    // Tactile physical Apple thumb knob with live spring hover/drag scale
                    let knob_r = 9.0 * s.thumb_scale;
                    if s.dragging || s.hovered {
                        let halo_a = if s.dragging { 0.26 } else { 0.14 };
                        self.canvas.fill_circle(
                            thumb_x,
                            thumb_y,
                            knob_r + 5.0,
                            D2dColor::from_rgb(accent, halo_a * chrome),
                        );
                    }
                    self.canvas.fill_circle(
                        thumb_x,
                        thumb_y + 1.5,
                        knob_r + 1.8,
                        D2dColor::from_rgb(palette.shadow, 0.44 * chrome),
                    );
                    self.canvas.fill_circle(
                        thumb_x,
                        thumb_y,
                        knob_r,
                        D2dColor::rgba(1.0, 1.0, 1.0, 0.98 * chrome),
                    );
                    self.canvas.stroke_circle(
                        thumb_x,
                        thumb_y,
                        knob_r,
                        2.1,
                        D2dColor::from_rgb(accent, (if s.dragging { 1.0 } else { 0.88 }) * chrome),
                    );
                    self.canvas.stroke_circle(
                        thumb_x,
                        thumb_y,
                        knob_r + 1.0,
                        0.8,
                        D2dColor::rgba(0.0, 0.0, 0.0, 0.28 * chrome),
                    );
                }

                if let Some(cp) = &app.color_picker {
                    let (sx, sy, sw, sh) = b.color_sv_rect();
                    let (hr, hg, hb) = cp.hue_rgb();
                    self.canvas.draw_color_sv(
                        sx,
                        sy,
                        sw,
                        sh,
                        10.0,
                        D2dColor::rgba(hr, hg, hb, 1.0),
                        chrome,
                    );
                    self.canvas.stroke_rounded_rect(
                        sx,
                        sy,
                        sw,
                        sh,
                        10.0,
                        0.9,
                        D2dColor::from_rgb(palette.capsule_border, 0.36 * chrome),
                    );
                    let sv_x = sx + cp.s.clamp(0.0, 1.0) * sw;
                    let sv_y = sy + (1.0 - cp.v.clamp(0.0, 1.0)) * sh;
                    let cur_col = cp.color();
                    self.canvas.fill_circle(
                        sv_x,
                        sv_y + 1.0,
                        9.5,
                        D2dColor::from_rgb(palette.shadow, 0.48 * chrome),
                    );
                    self.canvas.fill_circle(
                        sv_x,
                        sv_y,
                        8.0,
                        D2dColor::rgba(cur_col.r, cur_col.g, cur_col.b, chrome),
                    );
                    self.canvas
                        .stroke_circle(sv_x, sv_y, 8.0, 2.4, D2dColor::rgba(1.0, 1.0, 1.0, 0.96 * chrome));
                    self.canvas
                        .stroke_circle(sv_x, sv_y, 9.2, 0.8, D2dColor::rgba(0.0, 0.0, 0.0, 0.35 * chrome));

                    let (hx, hy, hw, hh) = b.color_hue_rect();
                    let hue_mid_y = hy + hh * 0.5;
                    self.canvas.draw_color_hue(hx, hy, hw, hh, chrome);
                    self.canvas.stroke_rounded_rect(
                        hx,
                        hy,
                        hw,
                        hh,
                        hh * 0.5,
                        0.9,
                        D2dColor::from_rgb(palette.capsule_border, 0.38 * chrome),
                    );
                    let hue_x = hx + (cp.h.rem_euclid(360.0) / 360.0) * hw;
                    let hrgb = cp.hue_rgb();
                    self.canvas.fill_circle(
                        hue_x,
                        hue_mid_y + 1.0,
                        10.0,
                        D2dColor::from_rgb(palette.shadow, 0.48 * chrome),
                    );
                    self.canvas
                        .fill_circle(hue_x, hue_mid_y, 8.4, D2dColor::from_rgb(hrgb, chrome));
                    self.canvas.stroke_circle(
                        hue_x,
                        hue_mid_y,
                        8.4,
                        2.4,
                        D2dColor::rgba(1.0, 1.0, 1.0, 0.96 * chrome),
                    );
                    self.canvas.stroke_circle(
                        hue_x,
                        hue_mid_y,
                        9.6,
                        0.8,
                        D2dColor::rgba(0.0, 0.0, 0.0, 0.35 * chrome),
                    );
                }

                // Recessed text field (only for ColorPicker hex input and Text InputBox)
                if !is_slider {
                    let (fx, fy, fw, fh) = if is_color {
                        b.color_hex_field_rect()
                    } else {
                        b.input_field_rect()
                    };
                    let inline_error = app
                        .color_picker
                        .as_ref()
                        .and_then(|cp| {
                            cp.error.as_deref().or_else(|| {
                                (!cp.hex_input.is_empty() && !cp.is_hex_valid())
                                    .then_some("Enter a valid #RRGGBB or #RRGGBBAA hex color")
                            })
                        })
                        .or_else(|| app.input_box.as_ref().and_then(|inp| inp.error.as_deref()));
                    let field_invalid = inline_error.is_some();
                    let err_rgb = (0.95, 0.36, 0.32);

                    self.canvas.stroke_rounded_rect(
                        fx - 1.2,
                        fy - 1.2,
                        fw + 2.4,
                        fh + 2.4,
                        11.2,
                        2.2,
                        D2dColor::from_rgb(if field_invalid { err_rgb } else { accent }, 0.20 * chrome),
                    );
                    self.canvas.fill_rounded_rect_solid(
                        fx,
                        fy,
                        fw,
                        fh,
                        10.0,
                        D2dColor::from_rgb(
                            palette.capsule_bottom,
                            material.control_opacity(0.28, 0.84) * chrome,
                        ),
                    );
                    let field_brd = if field_invalid {
                        err_rgb
                    } else {
                        lerp_rgb(palette.capsule_border, accent, 0.58)
                    };
                    self.canvas.stroke_rounded_rect(
                        fx,
                        fy,
                        fw,
                        fh,
                        10.0,
                        1.0,
                        D2dColor::from_rgb(field_brd, if field_invalid { 0.90 } else { 0.76 } * chrome),
                    );

                    if let Some(err_msg) = inline_error {
                        self.canvas.draw_text(
                            err_msg,
                            FONT_BADGE,
                            false,
                            fx + 4.0,
                            fy + fh + 2.0,
                            (fw - 8.0).max(20.0),
                            16.0,
                            0,
                            3,
                            1,
                            D2dColor::from_rgb(err_rgb, 0.96 * chrome),
                            None,
                        );
                    }

                    let (display_text, caret_prefix, is_placeholder, is_select_all, secret_btn) =
                        if let Some(cp) = &app.color_picker {
                            if cp.hex_input.is_empty() {
                                ("#rrggbb".to_string(), String::new(), true, false, None)
                            } else {
                                let bidx = cp.char_to_byte(cp.cursor.min(cp.char_len()));
                                (
                                    cp.hex_input.clone(),
                                    cp.hex_input[..bidx].to_string(),
                                    false,
                                    cp.select_all,
                                    None,
                                )
                            }
                        } else if let Some(inp) = &app.input_box {
                            let s_btn = inp.secret.then_some(inp.reveal_secret);
                            if inp.text.is_empty() {
                                (inp.placeholder.clone(), String::new(), true, false, s_btn)
                            } else if inp.secret && !inp.reveal_secret {
                                let n = inp.char_len();
                                let c = inp.cursor.min(n);
                                ("•".repeat(n), "•".repeat(c), false, inp.select_all, s_btn)
                            } else {
                                let bidx = inp.char_to_byte(inp.cursor.min(inp.char_len()));
                                (
                                    inp.text.clone(),
                                    inp.text[..bidx].to_string(),
                                    false,
                                    inp.select_all,
                                    s_btn,
                                )
                            }
                        } else {
                            (String::new(), String::new(), false, false, None)
                        };

                    let pad_x = 12.0;
                    let right_pad = if secret_btn.is_some() { 50.0 } else { 12.0 };
                    let text_avail_w = (fw - pad_x - right_pad).max(16.0);
                    let text_col = if is_placeholder {
                        palette.text_suggestion
                    } else {
                        palette.text_primary
                    };

                    // Exact DirectWrite glyph hit-testing + clip-and-pan scrolling (1:1 Linux explorer.rs)
                    let utf16_cursor = if is_placeholder {
                        0
                    } else {
                        caret_prefix.encode_utf16().count()
                    };
                    let utf16_end = if is_placeholder {
                        0
                    } else {
                        display_text.encode_utf16().count()
                    };
                    let cursor_x = if is_placeholder || utf16_cursor == 0 {
                        0.0
                    } else {
                        self.canvas
                            .hit_test_text_pos(&display_text, FONT_BODY, false, utf16_cursor)
                    };
                    let full_end_x = if is_placeholder || utf16_end == 0 {
                        0.0
                    } else if utf16_cursor == utf16_end {
                        cursor_x
                    } else {
                        self.canvas
                            .hit_test_text_pos(&display_text, FONT_BODY, false, utf16_end)
                    };
                    let scroll_x = if is_placeholder {
                        0.0
                    } else {
                        (cursor_x - (text_avail_w - 4.0)).max(0.0)
                    };

                    self.canvas
                        .push_clip_rect(fx + pad_x - 2.0, fy + 2.0, text_avail_w + 4.0, fh - 4.0);

                    if is_select_all && !is_placeholder && full_end_x > 1.0 {
                        self.canvas.fill_rounded_rect_solid(
                            fx + pad_x - scroll_x - 1.5,
                            fy + 6.0,
                            full_end_x + 3.0,
                            fh - 12.0,
                            4.0,
                            D2dColor::from_rgb(accent, 0.34 * chrome),
                        );
                    }

                    let draw_w = if is_placeholder {
                        text_avail_w
                    } else {
                        (full_end_x + 32.0).max(text_avail_w)
                    };
                    self.canvas.draw_text(
                        &display_text,
                        FONT_BODY,
                        false,
                        fx + pad_x - scroll_x,
                        fy,
                        draw_w,
                        fh,
                        0,
                        if is_placeholder { 3 } else { 0 },
                        1,
                        D2dColor::from_rgb(text_col, if is_placeholder { 0.65 * chrome } else { chrome }),
                        None,
                    );

                    // Crisp caret centered cleanly on exact DirectWrite inter-glyph boundary
                    let cx = if is_placeholder || utf16_cursor == 0 {
                        fx + pad_x - 0.5
                    } else if utf16_cursor >= utf16_end {
                        fx + pad_x - scroll_x + cursor_x + 0.5
                    } else {
                        fx + pad_x - scroll_x + cursor_x - 0.9
                    };
                    let caret_h = (fh - 14.0).max(14.0);
                    self.canvas.fill_rounded_rect_solid(
                        cx,
                        fy + (fh - caret_h) * 0.5,
                        1.8,
                        caret_h,
                        0.9,
                        D2dColor::from_rgb(accent, 0.95 * chrome),
                    );
                    self.canvas.pop_clip_rect();

                    if let Some(revealed) = secret_btn {
                        let (ex, ey, ew, eh) = b.input_eye_rect();
                        self.canvas.fill_rounded_rect_solid(
                            ex,
                            ey,
                            ew,
                            eh,
                            6.0,
                            D2dColor::from_rgb(palette.capsule_top, 0.72 * chrome),
                        );
                        self.canvas.draw_text(
                            if revealed { "Hide" } else { "Show" },
                            FONT_BADGE,
                            true,
                            ex,
                            ey,
                            ew,
                            eh,
                            1,
                            3,
                            1,
                            D2dColor::from_rgb(accent, chrome),
                            None,
                        );
                    }
                }

                self.draw_soft_divider(
                    b.width,
                    b.height * 0.5 - b.footer + 2.0,
                    palette.divider,
                    0.11 * chrome,
                );

                // Done / Save & Cancel buttons (exact Linux draw_input_button + 1/2 shortcut badges)
                let [btn0, btn1] = b.input_button_rects();
                let (sel_btn, prs_btn) = if let Some(cp) = &app.color_picker {
                    (cp.selected_button, cp.pressed_button)
                } else if let Some(s) = &app.slider {
                    (s.selected_button, s.pressed_button)
                } else if let Some(inp) = &app.input_box {
                    (inp.selected_button, inp.pressed_button)
                } else {
                    (None, None)
                };
                let show_badges = app.cfg.keybinds.number_shortcuts;
                self.draw_input_button(
                    btn0,
                    if is_color || is_slider { "Save" } else { "Done" },
                    show_badges.then_some("1"),
                    true,
                    app.ring_motion.input_button_focus(0),
                    sel_btn == Some(0),
                    prs_btn == Some(0),
                    accent,
                    palette,
                    chrome,
                );
                self.draw_input_button(
                    btn1,
                    "Cancel",
                    show_badges.then_some("2"),
                    false,
                    app.ring_motion.input_button_focus(1),
                    sel_btn == Some(1),
                    prs_btn == Some(1),
                    accent,
                    palette,
                    chrome,
                );
            }

            self.draw_panel_rims(b, palette, alpha_master);
            self.canvas.restore();
        }

        #[allow(clippy::too_many_arguments)]
        fn draw_input_button(
            &mut self,
            rect: (f64, f64, f64, f64),
            label: &str,
            badge: Option<&str>,
            primary: bool,
            focus: f64,
            selected: bool,
            pressed: bool,
            accent: Rgb,
            palette: &Palette,
            alpha: f64,
        ) {
            let (bx, by, bw, bh) = rect;
            let cx = bx + bw * 0.5;
            let cy = by + bh * 0.5;
            let zoom = if pressed { 0.97 } else { 1.0 + 0.018 * focus };

            self.canvas.save();
            self.canvas.translate(cx, cy);
            if (zoom - 1.0).abs() >= 1e-4 {
                self.canvas.scale(zoom, zoom);
            }

            if primary {
                let fill_a = if pressed {
                    0.32
                } else if selected {
                    0.23 + 0.07 * focus
                } else {
                    0.14 + 0.10 * focus
                } * alpha;
                self.canvas.fill_rounded_rect_solid(
                    -bw * 0.5,
                    -bh * 0.5,
                    bw,
                    bh,
                    9.0,
                    D2dColor::from_rgb(accent, fill_a),
                );
                let brd_a = if pressed || selected {
                    0.82
                } else {
                    0.48 + 0.28 * focus
                } * alpha;
                self.canvas.stroke_rounded_rect(
                    -bw * 0.5,
                    -bh * 0.5,
                    bw,
                    bh,
                    9.0,
                    1.0,
                    D2dColor::from_rgb(accent, brd_a),
                );
            } else {
                let boost = if pressed {
                    0.16
                } else if selected {
                    0.10
                } else {
                    0.06 * focus
                };
                self.canvas.fill_rounded_rect_solid(
                    -bw * 0.5,
                    -bh * 0.5,
                    bw,
                    bh,
                    9.0,
                    D2dColor::from_rgb(palette.capsule_bottom, (0.68 + boost) * alpha),
                );
                let brd_tint = lerp_rgb(palette.capsule_border, accent, 0.35 * focus);
                let brd_a = if pressed || selected {
                    0.68
                } else {
                    0.32 + 0.26 * focus
                } * alpha;
                self.canvas.stroke_rounded_rect(
                    -bw * 0.5,
                    -bh * 0.5,
                    bw,
                    bh,
                    9.0,
                    1.0,
                    D2dColor::from_rgb(brd_tint, brd_a),
                );
            }

            self.canvas.draw_text(
                label,
                FONT_CONTROL,
                true,
                -bw * 0.5 + 18.0,
                -bh * 0.5,
                (bw - 36.0).max(20.0),
                bh,
                1,
                3,
                1,
                D2dColor::from_rgb(palette.text_primary, alpha),
                Some(D2dColor::from_rgb(palette.text_shadow, 0.80 * alpha)),
            );

            if let Some(badge_str) = badge {
                let badge_w = 18.0;
                let badge_h = 16.0;
                let badge_x = bw * 0.5 - badge_w - 8.0;
                let badge_y = -badge_h * 0.5;
                let pill_tint = if primary { accent } else { palette.capsule_border };
                self.canvas.fill_rounded_rect_solid(
                    badge_x,
                    badge_y,
                    badge_w,
                    badge_h,
                    4.5,
                    D2dColor::from_rgb(
                        pill_tint,
                        (if selected || pressed { 0.28 } else { 0.16 }) * alpha,
                    ),
                );
                self.canvas.stroke_rounded_rect(
                    badge_x,
                    badge_y,
                    badge_w,
                    badge_h,
                    4.5,
                    1.0,
                    D2dColor::from_rgb(
                        pill_tint,
                        (if selected || pressed { 0.62 } else { 0.36 }) * alpha,
                    ),
                );
                let txt_col = if primary {
                    palette.rim_top
                } else {
                    palette.text_primary
                };
                self.canvas.draw_text(
                    badge_str,
                    FONT_BADGE,
                    true,
                    badge_x,
                    badge_y,
                    badge_w,
                    badge_h,
                    1,
                    3,
                    1,
                    D2dColor::from_rgb(
                        txt_col,
                        (if selected || pressed { 0.96 } else { 0.84 }) * alpha,
                    ),
                    Some(D2dColor::from_rgb(palette.text_shadow, 0.72 * alpha)),
                );
            }

            self.canvas.restore();
        }

        fn draw_help_overlay(
            &mut self,
            geom: RingGeometry,
            app: &App,
            palette: &Palette,
            material: SurfaceMaterial,
            alpha: f64,
        ) {
            let box_w = 420.0;
            let box_h = 338.0;
            let (vw, vh) = app.viewport_size;
            let half_w = box_w * 0.5 * geom.scale + 12.0;
            let half_h = box_h * 0.5 * geom.scale + 12.0;
            let hx = geom
                .center_x
                .clamp(half_w.min(vw * 0.5), (vw - half_w).max(vw * 0.5));
            let hy = geom
                .center_y
                .clamp(half_h.min(vh * 0.5), (vh - half_h).max(vh * 0.5));
            self.canvas.save();
            self.canvas.translate(hx, hy);
            self.canvas.scale(geom.scale, geom.scale);
            let x = -box_w * 0.5;
            let y = -box_h * 0.5;
            self.canvas.fill_rounded_rect_shadow(
                x,
                y,
                box_w,
                box_h,
                18.0,
                D2dColor::from_rgb(palette.shadow, 1.0),
                alpha,
            );
            if material.uses_backdrop() {
                self.canvas.fill_rounded_rect_backdrop(
                    x,
                    y,
                    box_w,
                    box_h,
                    18.0,
                    0.0,
                    material.backdrop_alpha * alpha,
                );
            }
            self.canvas.fill_rounded_rect_solid(
                x,
                y,
                box_w,
                box_h,
                18.0,
                D2dColor::from_rgb(palette.ring_mid, material.heavy_opacity(0.46) * alpha),
            );
            self.canvas.stroke_rounded_rect(
                x,
                y,
                box_w,
                box_h,
                18.0,
                1.0,
                D2dColor::from_rgb(palette.rim_top, 0.45 * alpha),
            );

            let kb = &app.cfg.keybinds;
            let next_prev = format!("{} / {}", kb.next.display(), kb.previous.display());
            let act_sec = format!("{} / {}", kb.activate.display(), kb.secondary_activate.display());
            let open_dir = kb.open_current_folder.display();
            let comps = format!(
                "{} / {}",
                kb.accept_completion.display(),
                kb.undo_completion.display()
            );
            let back_home = format!("{} / {}", kb.back.display(), kb.home.display());
            let pages = format!("{} / {}", kb.page_next.display(), kb.page_previous.display());
            let clear_q = kb.clear_query.display();
            let main_k = kb.main_menu.display();
            let lines = [
                ("ReCircles Shortcuts", ""),
                ("Main Ring Hotkey", main_k.as_str()),
                ("Next / Previous", next_prev.as_str()),
                ("Activate / Secondary", act_sec.as_str()),
                ("Open Current Folder", open_dir.as_str()),
                ("Complete / Parent Dir", comps.as_str()),
                ("Step Back / Home", back_home.as_str()),
                ("Next / Prev Page", pages.as_str()),
                ("Clear Search", clear_q.as_str()),
            ];
            for (i, (k, v)) in lines.iter().enumerate() {
                if i == 0 {
                    self.canvas.draw_text(
                        k,
                        FONT_BODY,
                        true,
                        x + 22.0,
                        y + 16.0,
                        box_w - 44.0,
                        26.0,
                        0,
                        3,
                        1,
                        D2dColor::from_rgb(palette.text_primary, alpha),
                        None,
                    );
                    self.draw_soft_divider(box_w, y + 48.0, palette.divider, 0.14 * alpha);
                } else {
                    let ry = y + 58.0 + (i - 1) as f64 * 33.0;
                    self.canvas.draw_text(
                        k,
                        FONT_CAPTION,
                        false,
                        x + 22.0,
                        ry,
                        178.0,
                        24.0,
                        0,
                        3,
                        1,
                        D2dColor::from_rgb(palette.text_primary, alpha),
                        None,
                    );
                    self.canvas.draw_text(
                        v,
                        FONT_CAPTION,
                        true,
                        x + 206.0,
                        ry,
                        192.0,
                        24.0,
                        2,
                        3,
                        1,
                        D2dColor::from_rgb(palette.accent_action, alpha),
                        None,
                    );
                }
            }
            self.canvas.restore();
        }

        // ==================== ICON & COLOR BITMAP CACHES ====================
        fn resolve_cached_icon_path(&mut self, path_str: &str) -> Option<String> {
            if let Some(hit) = self.resolved_paths.get(path_str) {
                return hit.clone();
            }
            let resolved = if std::path::Path::new(path_str).is_file() {
                Some(path_str.to_string())
            } else {
                self.store
                    .resolve_icon_path(path_str)
                    .map(|p| p.to_string_lossy().into_owned())
            };
            if self.resolved_paths.len() >= 256 {
                self.resolved_paths.clear();
            }
            self.resolved_paths
                .insert(path_str.to_string(), resolved.clone());
            resolved
        }

        fn load_or_cache_svg(&mut self, path_str: &str) -> Option<SvgDoc> {
            let lower = path_str.to_ascii_lowercase();
            if !lower.ends_with(".svg") {
                return None;
            }
            if let Some(hit) = self.svg_icons.get(path_str) {
                return hit.clone();
            }
            let doc = self
                .resolve_cached_icon_path(path_str)
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|xml| parse_svg(&xml));
            if self.svg_icons.len() >= 128 {
                self.svg_icons.clear();
            }
            self.svg_icons.insert(path_str.to_string(), doc.clone());
            doc
        }

        fn load_or_cache_image_bitmap(&mut self, path_str: &str) -> Option<i32> {
            if let Some(&hit) = self.image_bitmaps.get(path_str) {
                return hit;
            }
            let bmp = self
                .resolve_cached_icon_path(path_str)
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|bytes| self.canvas.create_bitmap_from_file_bytes(&bytes, 128));
            if self.image_bitmaps.len() >= 128 {
                for (_, old) in self.image_bitmaps.drain() {
                    if let Some(id) = old {
                        self.canvas.destroy_bitmap(id);
                    }
                }
            }
            self.image_bitmaps.insert(path_str.to_string(), bmp);
            bmp
        }

        fn load_or_cache_shell_icon(&mut self, exe_path: &str) -> Option<i32> {
            if let Some(&hit) = self.shell_bitmaps.get(exe_path) {
                return hit;
            }
            // Never block the UI/render thread on IShellLink/PrivateExtractIconsW/SHGetFileInfoW:
            // queue a high-priority background decode (backed by persistent disk cache) and fall
            // back to the crisp built-in vector glyph until WM_APP_ICON_READY uploads the bitmap.
            self.icon_loader.request(exe_path, self.shell_icon_px, true);
            None
        }

        fn fit_item_label(
            &mut self,
            label: &str,
            sector_bounds: (f64, f64),
            origin: (f64, f64),
            style: ItemStyle,
            zoom: f64,
        ) -> (f64, f64, f64) {
            let (start, end) = sector_bounds;
            let (origin_x, origin_y) = origin;
            let mut size = style.base_font;
            let (mut text_w, text_h) = self.measure_cached(label, size, true);
            let zoom_ref = zoom.max(1.04);
            let y_center = origin_y + style.label_y * zoom_ref;
            let half_h = (text_h * 0.5 + 1.0) * zoom_ref;

            let local_span = sector_label_span(y_center - half_h, y_center + half_h, start, end, origin_x)
                .map(|(lo, hi)| ((lo - origin_x) / zoom_ref, (hi - origin_x) / zoom_ref));

            let Some((local_lo, local_hi)) = local_span else {
                return (size, 0.0, style.max_label_width.min(104.0));
            };

            let avail_w = (local_hi - local_lo).max(0.0);
            let width = avail_w.min(style.max_label_width);
            if style.shrink_to_fit && width > 0.0 {
                // Keep a 2.5 DIP margin inside `width` so DirectWrite's subpixel layout rounding
                // and 0.5px width quantization never spuriously trigger character ellipsis trimming.
                let fit_target = (width - 2.5).max(16.0);
                if text_w > fit_target {
                    size = (style.base_font * (fit_target / text_w)).clamp(9.0, style.base_font);
                    text_w = self.measure_cached(label, size, true).0;
                    while text_w > fit_target && size > 9.0 {
                        size = (size - 0.5).max(9.0);
                        text_w = self.measure_cached(label, size, true).0;
                    }
                }
            }

            let draw_w = text_w.min(width);
            let label_x = if local_lo <= -draw_w * 0.5 && draw_w * 0.5 <= local_hi {
                0.0
            } else if draw_w < avail_w {
                0.0_f64.clamp(local_lo + draw_w * 0.5, local_hi - draw_w * 0.5)
            } else {
                (local_lo + local_hi) * 0.5
            };
            (size, label_x, width.max(20.0))
        }
    }

    // ==================== HELPERS (EXACT LINUX MATH & VECTOR GLYPHS) ====================

    fn ring_radii(motion: &RingMotion) -> Option<(f64, f64)> {
        let empty = motion.empty_ring()?;
        let target_outer = EMPTY_OUTER + EMPTY_OUTER_PULSE * empty.wave;
        let target_inner = EMPTY_INNER - EMPTY_INNER_PULSE * empty.wave;
        let outer = OUTER + (target_outer - OUTER) * empty.mix;
        let inner = INNER + (target_inner - INNER) * empty.mix;
        Some((inner, outer))
    }

    fn caption_outer_radius(motion: &RingMotion) -> f64 {
        if motion.loading_pose().is_some() || motion.panel_mix() > 0.001 {
            return OUTER;
        }
        match motion.empty_ring() {
            Some(empty) => {
                let envelope = EMPTY_OUTER + EMPTY_OUTER_PULSE * empty.pulse;
                OUTER + (envelope - OUTER) * empty.mix
            }
            None => OUTER,
        }
    }

    #[derive(Clone, Copy)]
    struct ItemStyle {
        horizontal_radius: f64,
        plate: f64,
        icon_size: f64,
        icon_y: f64,
        label_y: f64,
        base_font: f64,
        max_label_width: f64,
        shrink_to_fit: bool,
    }

    fn item_style(mode: &Mode, count: usize) -> ItemStyle {
        let main = matches!(mode, Mode::Main);
        if main && count <= 3 {
            ItemStyle {
                horizontal_radius: ITEM_RADIUS - 8.0,
                plate: 41.0,
                icon_size: 26.0,
                icon_y: -12.0,
                label_y: 22.0,
                base_font: 15.0,
                max_label_width: 148.0,
                shrink_to_fit: true,
            }
        } else if main && count <= 5 {
            ItemStyle {
                horizontal_radius: ITEM_RADIUS,
                plate: 36.0,
                icon_size: 23.0,
                icon_y: -10.5,
                label_y: 19.5,
                base_font: 13.5,
                max_label_width: 148.0,
                shrink_to_fit: true,
            }
        } else if main {
            ItemStyle {
                horizontal_radius: ITEM_RADIUS,
                plate: 33.0,
                icon_size: 21.0,
                icon_y: -9.5,
                label_y: 18.0,
                base_font: 12.5,
                max_label_width: 148.0,
                shrink_to_fit: true,
            }
        } else if matches!(mode, Mode::Settings) {
            ItemStyle {
                horizontal_radius: ITEM_RADIUS,
                plate: 32.0,
                icon_size: 20.0,
                icon_y: -9.0,
                label_y: 17.5,
                base_font: 12.5,
                max_label_width: 148.0,
                shrink_to_fit: true,
            }
        } else {
            ItemStyle {
                horizontal_radius: ITEM_RADIUS,
                plate: 32.0,
                icon_size: 20.0,
                icon_y: -9.0,
                label_y: 17.5,
                base_font: 12.5,
                max_label_width: if count <= 3 { 104.0 } else { 83.0 },
                shrink_to_fit: false,
            }
        }
    }

    fn sector_horizontal_span_at_y(
        y: f64,
        start: f64,
        end: f64,
        ref_x: f64,
    ) -> Option<(f64, f64)> {
        let r_in = INNER + 3.0;
        let r_out = OUTER - 4.5;
        if y.abs() >= r_out {
            return None;
        }
        let x_out = (r_out * r_out - y * y).sqrt();
        let mut intervals = [(0.0_f64, 0.0_f64); 2];
        let mut len = if y.abs() < r_in {
            let x_in = (r_in * r_in - y * y).sqrt();
            intervals[0] = (-x_out, -x_in);
            intervals[1] = (x_in, x_out);
            2
        } else {
            intervals[0] = (-x_out, x_out);
            1
        };

        if end - start <= PI + 1e-4 {
            let planes = [(-start.sin(), start.cos()), (end.sin(), -end.cos())];
            let mut out_len = 0;
            for i in 0..len {
                let (mut lo, mut hi) = intervals[i];
                for (a, b) in planes {
                    let rhs = 3.5 - b * y;
                    if a > 1e-9 {
                        lo = lo.max(rhs / a);
                    } else if a < -1e-9 {
                        hi = hi.min(rhs / a);
                    } else if rhs > 0.0 {
                        lo = hi + 1.0;
                    }
                }
                if lo < hi {
                    intervals[out_len] = (lo, hi);
                    out_len += 1;
                }
            }
            len = out_len;
        }

        intervals[..len].iter().copied().min_by(|&(l1, h1), &(l2, h2)| {
            let dist = |lo: f64, hi: f64| {
                if ref_x < lo {
                    lo - ref_x
                } else if ref_x > hi {
                    ref_x - hi
                } else {
                    0.0
                }
            };
            dist(l1, h1).total_cmp(&dist(l2, h2))
        })
    }

    fn sector_label_span(
        y_top: f64,
        y_bot: f64,
        start: f64,
        end: f64,
        ref_x: f64,
    ) -> Option<(f64, f64)> {
        let ys = [y_top, y_bot, 0.0];
        let n = if y_top < 0.0 && 0.0 < y_bot { 3 } else { 2 };
        let mut lo = f64::NEG_INFINITY;
        let mut hi = f64::INFINITY;
        for &y in &ys[..n] {
            let (l, h) = sector_horizontal_span_at_y(y, start, end, ref_x)?;
            lo = lo.max(l);
            hi = hi.min(h);
        }
        (lo < hi).then_some((lo, hi))
    }

    #[derive(Clone, Copy)]
    struct Vec3(f64, f64, f64);

    impl Vec3 {
        fn add(self, o: Self) -> Self {
            Self(self.0 + o.0, self.1 + o.1, self.2 + o.2)
        }
        fn scale(self, s: f64) -> Self {
            Self(self.0 * s, self.1 * s, self.2 * s)
        }
        fn dot(self, o: Self) -> f64 {
            self.0 * o.0 + self.1 * o.1 + self.2 * o.2
        }
        fn cross(self, o: Self) -> Self {
            Self(
                self.1 * o.2 - self.2 * o.1,
                self.2 * o.0 - self.0 * o.2,
                self.0 * o.1 - self.1 * o.0,
            )
        }
        fn normalize(self) -> Self {
            let len = self.dot(self).sqrt();
            if len > 0.0 {
                Self(self.0 / len, self.1 / len, self.2 / len)
            } else {
                Self(0.0, 0.0, 1.0)
            }
        }
    }

    struct Transform3D {
        r: [[f64; 3]; 3],
    }

    impl Transform3D {
        fn rotate(&self, v: Vec3) -> Vec3 {
            Vec3(
                self.r[0][0] * v.0 + self.r[0][1] * v.1 + self.r[0][2] * v.2,
                self.r[1][0] * v.0 + self.r[1][1] * v.1 + self.r[1][2] * v.2,
                self.r[2][0] * v.0 + self.r[2][1] * v.1 + self.r[2][2] * v.2,
            )
        }
        fn project_world(w: Vec3) -> (f64, f64, f64) {
            let persp = PERSPECTIVE_D / (PERSPECTIVE_D - w.2);
            (w.0 * persp, w.1 * persp, w.2)
        }
        fn project(&self, p: Vec3) -> (f64, f64, f64) {
            Self::project_world(self.rotate(p))
        }
        fn cam_local(&self) -> Vec3 {
            Vec3(
                self.r[2][0] * PERSPECTIVE_D,
                self.r[2][1] * PERSPECTIVE_D,
                self.r[2][2] * PERSPECTIVE_D,
            )
        }
    }

    fn convex_hull(mut pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
        pts.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
        let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
            (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
        };
        let mut lower: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
        for &p in &pts {
            while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
                lower.pop();
            }
            lower.push(p);
        }
        let mut upper: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
        for &p in pts.iter().rev() {
            while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
                upper.pop();
            }
            upper.push(p);
        }
        lower.pop();
        upper.pop();
        lower.extend(upper);
        lower
    }

    /// Exact 24×24 vector icon geometry matching Linux `ReCircles/src/ring.rs::glyph`.
    fn build_linux_glyph(icon: BuiltinIcon) -> PathBuilder {
        let mut pb = PathBuilder::new();
        match icon {
            BuiltinIcon::Folder => {
                pb.move_to(3.0, 8.5);
                pb.line_to(3.0, 6.3);
                pb.curve_to(3.0, 5.1, 3.9, 4.3, 5.2, 4.3);
                pb.line_to(9.1, 4.3);
                pb.line_to(11.8, 7.0);
                pb.line_to(18.6, 7.0);
                pb.curve_to(20.1, 7.0, 21.0, 7.9, 21.0, 9.4);
                pb.line_to(21.0, 17.3);
                pb.curve_to(21.0, 18.8, 20.0, 19.7, 18.7, 19.7);
                pb.line_to(5.3, 19.7);
                pb.curve_to(3.9, 19.7, 3.0, 18.8, 3.0, 17.4);
                pb.close();
                pb.move_to(3.5, 10.0);
                pb.line_to(20.5, 10.0);
            }
            BuiltinIcon::Home => {
                pb.move_to(3.2, 10.6);
                pb.line_to(12.0, 3.4);
                pb.line_to(20.8, 10.6);
                pb.move_to(5.4, 8.8);
                pb.line_to(5.4, 18.2);
                pb.curve_to(5.4, 19.4, 6.2, 20.1, 7.4, 20.1);
                pb.line_to(16.6, 20.1);
                pb.curve_to(17.8, 20.1, 18.6, 19.4, 18.6, 18.2);
                pb.line_to(18.6, 8.8);
                pb.move_to(9.8, 20.1);
                pb.line_to(9.8, 14.8);
                pb.curve_to(9.8, 13.7, 10.5, 13.1, 11.5, 13.1);
                pb.line_to(12.5, 13.1);
                pb.curve_to(13.5, 13.1, 14.2, 13.7, 14.2, 14.8);
                pb.line_to(14.2, 20.1);
            }
            BuiltinIcon::App => {
                for (x, y) in [(3.4, 3.4), (13.3, 3.4), (3.4, 13.3), (13.3, 13.3)] {
                    pb.rounded_rect(x, y, 7.2, 7.2, 1.8);
                }
            }
            BuiltinIcon::Window => {
                pb.move_to(6.4, 16.2);
                pb.line_to(4.3, 16.2);
                pb.curve_to(3.0, 16.2, 2.4, 15.4, 2.4, 14.2);
                pb.line_to(2.4, 5.5);
                pb.curve_to(2.4, 4.2, 3.3, 3.5, 4.5, 3.5);
                pb.line_to(15.0, 3.5);
                pb.curve_to(16.2, 3.5, 17.0, 4.3, 17.0, 5.4);
                pb.line_to(17.0, 6.4);
                pb.rounded_rect(7.1, 8.0, 14.5, 12.3, 1.9);
                pb.move_to(7.7, 11.6);
                pb.line_to(21.0, 11.6);
            }
            BuiltinIcon::File => {
                pb.move_to(6.0, 2.8);
                pb.line_to(13.5, 2.8);
                pb.line_to(19.4, 8.8);
                pb.line_to(19.4, 19.5);
                pb.curve_to(19.4, 20.5, 18.5, 21.2, 17.6, 21.2);
                pb.line_to(6.1, 21.2);
                pb.curve_to(5.0, 21.2, 4.5, 20.4, 4.5, 19.4);
                pb.line_to(4.5, 4.4);
                pb.curve_to(4.5, 3.4, 5.1, 2.8, 6.0, 2.8);
                pb.close();
                pb.move_to(13.4, 3.3);
                pb.line_to(13.4, 9.1);
                pb.line_to(18.8, 9.1);
                pb.move_to(8.0, 14.0);
                pb.line_to(15.5, 14.0);
            }
            BuiltinIcon::Settings => {
                // Exact Linux 3 horizontal sliders with circles!
                pb.move_to(4.0, 7.0);
                pb.line_to(8.2, 7.0);
                pb.move_to(13.0, 7.0);
                pb.line_to(20.0, 7.0);
                pb.arc(10.6, 7.0, 2.3, 0.0, TAU);

                pb.move_to(4.0, 12.0);
                pb.line_to(13.2, 12.0);
                pb.move_to(18.0, 12.0);
                pb.line_to(20.0, 12.0);
                pb.arc(15.6, 12.0, 2.3, 0.0, TAU);

                pb.move_to(4.0, 17.0);
                pb.line_to(6.8, 17.0);
                pb.move_to(11.6, 17.0);
                pb.line_to(20.0, 17.0);
                pb.arc(9.2, 17.0, 2.3, 0.0, TAU);
            }
            BuiltinIcon::Appearance => {
                pb.arc(12.0, 12.0, 5.2, 0.0, TAU);
                pb.move_to(12.0, 6.8);
                pb.line_to(12.0, 17.2);
                for i in 0..8 {
                    let a = f64::from(i) * FRAC_PI_4;
                    pb.move_to(12.0 + a.cos() * 7.6, 12.0 + a.sin() * 7.6);
                    pb.line_to(12.0 + a.cos() * 9.4, 12.0 + a.sin() * 9.4);
                }
            }
            BuiltinIcon::Material => {
                // Overlapping translucent glass panes
                pb.rounded_rect(3.6, 3.6, 11.2, 11.2, 2.6);
                pb.rounded_rect(9.2, 9.2, 11.2, 11.2, 2.6);
                pb.move_to(12.2, 17.2);
                pb.line_to(17.2, 12.2);
            }
            BuiltinIcon::Blur => {
                // Frosted droplet with soft interior frost lines
                pb.move_to(12.0, 3.2);
                pb.curve_to(12.0, 3.2, 5.2, 10.6, 5.2, 14.4);
                pb.curve_to(5.2, 18.1, 8.2, 20.6, 12.0, 20.6);
                pb.curve_to(15.8, 20.6, 18.8, 18.1, 18.8, 14.4);
                pb.curve_to(18.8, 10.6, 12.0, 3.2, 12.0, 3.2);
                pb.close();
                pb.move_to(8.6, 13.8);
                pb.line_to(15.4, 13.8);
                pb.move_to(9.6, 16.8);
                pb.line_to(14.4, 16.8);
            }
            BuiltinIcon::Theme | BuiltinIcon::Palette => {
                pb.arc(12.0, 12.0, 8.6, 0.35, TAU - 0.15);
                pb.curve_to(19.0, 13.8, 15.6, 13.8, 15.2, 16.2);
                pb.curve_to(14.8, 18.6, 17.8, 19.2, 16.4, 20.0);
                for (cx, cy) in [(8.0, 9.2), (12.0, 7.2), (16.0, 9.2), (7.6, 13.8)] {
                    pb.arc(cx, cy, 1.1, 0.0, TAU);
                }
            }
            BuiltinIcon::Keybind => {
                pb.rounded_rect(2.8, 6.2, 18.4, 11.6, 2.4);
                for x in [6.2, 9.6, 13.0, 16.4] {
                    pb.move_to(x, 9.8);
                    pb.line_to(x + 1.2, 9.8);
                }
                for x in [7.4, 10.8, 14.2] {
                    pb.move_to(x, 12.6);
                    pb.line_to(x + 1.2, 12.6);
                }
                pb.move_to(7.5, 15.2);
                pb.line_to(16.5, 15.2);
            }
            BuiltinIcon::Add => {
                pb.arc(12.0, 12.0, 8.6, 0.0, TAU);
                pb.move_to(12.0, 7.4);
                pb.line_to(12.0, 16.6);
                pb.move_to(7.4, 12.0);
                pb.line_to(16.6, 12.0);
            }
            BuiltinIcon::Disconnect | BuiltinIcon::Remove => {
                pb.arc(12.0, 12.0, 8.4, 0.0, TAU);
                pb.move_to(8.8, 8.8);
                pb.line_to(15.2, 15.2);
                pb.move_to(15.2, 8.8);
                pb.line_to(8.8, 15.2);
            }
            BuiltinIcon::Forget => {
                pb.move_to(4.5, 6.8);
                pb.line_to(19.5, 6.8);
                pb.move_to(9.2, 6.8);
                pb.line_to(9.6, 4.4);
                pb.line_to(14.4, 4.4);
                pb.line_to(14.8, 6.8);
                pb.move_to(6.6, 6.8);
                pb.line_to(7.4, 19.0);
                pb.curve_to(7.5, 20.0, 8.2, 20.5, 9.2, 20.5);
                pb.line_to(14.8, 20.5);
                pb.curve_to(15.8, 20.5, 16.5, 20.0, 16.6, 19.0);
                pb.line_to(17.4, 6.8);
                pb.move_to(10.2, 10.6);
                pb.line_to(10.2, 16.8);
                pb.move_to(13.8, 10.6);
                pb.line_to(13.8, 16.8);
            }
            BuiltinIcon::Refresh => {
                pb.arc(12.0, 12.0, 7.6, -2.55, 0.35);
                pb.move_to(16.6, 14.3);
                pb.line_to(19.5, 14.9);
                pb.line_to(19.6, 11.8);
                pb.arc(12.0, 12.0, 7.6, 0.6, 3.45);
                pb.move_to(7.4, 9.7);
                pb.line_to(4.5, 9.1);
                pb.line_to(4.4, 12.2);
            }
            BuiltinIcon::Reset => {
                pb.arc(12.0, 12.0, 7.6, -2.35, 2.55);
                pb.move_to(6.2, 4.4);
                pb.line_to(6.6, 8.8);
                pb.line_to(10.8, 7.6);
            }
            BuiltinIcon::MainMenu | BuiltinIcon::Circle => {
                pb.arc(12.0, 12.0, 8.6, 0.0, TAU);
                pb.arc(12.0, 12.0, 3.8, 0.0, TAU);
                for angle in [0.0, FRAC_PI_2, PI, PI + FRAC_PI_2] {
                    pb.move_to(12.0 + 3.8 * angle.cos(), 12.0 + 3.8 * angle.sin());
                    pb.line_to(12.0 + 8.6 * angle.cos(), 12.0 + 8.6 * angle.sin());
                }
            }
            BuiltinIcon::Check => {
                pb.rounded_rect(2.8, 6.8, 18.4, 10.4, 5.2);
                pb.arc(15.8, 12.0, 3.0, 0.0, TAU);
            }
            BuiltinIcon::ToggleOn | BuiltinIcon::ToggleOff => {
                pb.rounded_rect(2.8, 6.8, 18.4, 10.4, 5.2);
                let knob_x = if icon == BuiltinIcon::ToggleOn { 15.8 } else { 8.2 };
                pb.arc(knob_x, 12.0, 3.0, 0.0, TAU);
            }
            BuiltinIcon::Lock => {
                pb.rounded_rect(5.4, 10.6, 13.2, 9.8, 2.2);
                pb.arc(12.0, 10.6, 4.0, PI, TAU);
            }
            BuiltinIcon::Back => {
                pb.move_to(18.0, 12.0);
                pb.line_to(6.0, 12.0);
                pb.move_to(11.0, 7.0);
                pb.line_to(6.0, 12.0);
                pb.line_to(11.0, 17.0);
            }
            BuiltinIcon::Pin => {
                pb.move_to(12.0, 15.2);
                pb.line_to(12.0, 20.5);
                pb.move_to(7.5, 4.5);
                pb.line_to(16.5, 4.5);
                pb.line_to(15.0, 10.5);
                pb.line_to(17.8, 13.5);
                pb.line_to(6.2, 13.5);
                pb.line_to(9.0, 10.5);
                pb.close();
            }
            BuiltinIcon::Drive => {
                pb.rounded_rect(3.5, 6.2, 17.0, 11.6, 2.4);
                pb.move_to(3.8, 13.5);
                pb.line_to(20.2, 13.5);
                pb.arc(16.8, 15.6, 1.0, 0.0, TAU);
            }
            BuiltinIcon::Scale => {
                pb.move_to(14.5, 4.5);
                pb.line_to(19.5, 4.5);
                pb.line_to(19.5, 9.5);
                pb.move_to(19.5, 4.5);
                pb.line_to(13.2, 10.8);
                pb.move_to(9.5, 19.5);
                pb.line_to(4.5, 19.5);
                pb.line_to(4.5, 14.5);
                pb.move_to(4.5, 19.5);
                pb.line_to(10.8, 13.2);
            }
            BuiltinIcon::Layout => {
                pb.rounded_rect(3.4, 3.4, 17.2, 17.2, 2.4);
                pb.move_to(3.4, 9.6);
                pb.line_to(20.6, 9.6);
                pb.move_to(10.0, 9.6);
                pb.line_to(10.0, 20.6);
            }
            BuiltinIcon::Motion => {
                pb.arc(12.0, 12.0, 8.2, -0.4, PI * 1.15);
                pb.arc(12.0, 12.0, 4.2, 0.2, PI * 1.4);
                pb.arc(12.0, 12.0, 1.4, 0.0, TAU);
            }
            BuiltinIcon::Image => {
                pb.rounded_rect(3.4, 4.2, 17.2, 15.6, 2.4);
                pb.arc(8.6, 9.0, 1.6, 0.0, TAU);
                pb.move_to(4.2, 17.4);
                pb.line_to(9.4, 12.6);
                pb.line_to(13.0, 16.0);
                pb.line_to(16.0, 13.2);
                pb.line_to(19.8, 17.4);
            }
            BuiltinIcon::Clipboard => {
                pb.rounded_rect(8.0, 2.5, 8.0, 4.0, 1.0);
                pb.rounded_rect(4.5, 4.8, 15.0, 16.5, 2.0);
                pb.move_to(9.0, 14.0);
                pb.line_to(11.2, 16.2);
                pb.line_to(15.2, 12.0);
            }
        }
        pb
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::app::{AsyncJobResult, BackendBridge};
        use recircles_core::config::AppConfig;
        use recircles_core::ipc::win::to_wide_nul;
        use recircles_core::modules::{BuiltinSubmenu, InstalledModules};
        use recircles_core::settings::{self, ColorGroup, SettingsView};
        use recircles_core::submenu_abi::{AppDto, SubmenuRequest, SubmenuResponse};
        use std::path::PathBuf;
        use std::sync::Arc;
        use std::time::{Instant, SystemTime, UNIX_EPOCH};
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, WNDCLASSEXW,
            WS_EX_TOOLWINDOW, WS_POPUP,
        };

        struct DummyBackend;
        impl BackendBridge for DummyBackend {
            fn invoke(
                &self,
                _sub: BuiltinSubmenu,
                _req: &SubmenuRequest,
            ) -> Result<SubmenuResponse, String> {
                Ok(SubmenuResponse::ActionOutcome {
                    ok: true,
                    message: "ok".into(),
                })
            }
            fn is_installed(&self, _sub: BuiltinSubmenu) -> bool {
                true
            }
        }

        struct TestHwnd(HWND);
        impl TestHwnd {
            fn new(w: i32, h: i32) -> Self {
                const WS_EX_NOREDIRECTIONBITMAP: u32 = 0x0020_0000;
                let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
                let cls = to_wide_nul("ReCirclesRenderTestWnd");
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    lpfnWndProc: Some(DefWindowProcW),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinst,
                    hIcon: std::ptr::null_mut(),
                    hCursor: std::ptr::null_mut(),
                    hbrBackground: std::ptr::null_mut(),
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: cls.as_ptr(),
                    hIconSm: std::ptr::null_mut(),
                };
                unsafe { RegisterClassExW(&wc) };
                let hwnd = unsafe {
                    CreateWindowExW(
                        WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW,
                        cls.as_ptr(),
                        std::ptr::null(),
                        WS_POPUP,
                        0,
                        0,
                        w,
                        h,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        hinst,
                        std::ptr::null(),
                    )
                };
                assert!(!hwnd.is_null(), "failed to create hidden DirectComposition test HWND");
                Self(hwnd)
            }
        }
        impl Drop for TestHwnd {
            fn drop(&mut self) {
                unsafe { DestroyWindow(self.0) };
            }
        }

        #[test]
        fn settings_ring_labels_fit_without_truncation_in_real_directwrite() {
            let hwnd = TestHwnd::new(1280, 720);
            let mut renderer = Renderer::new(hwnd.0, 1280, 720, 1.0).expect("d2d renderer");
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("recircles_lbl_fit_{nanos}"));
            let store = ConfigStore::in_dir(dir.clone());
            let installed = InstalledModules::all_for_tests(PathBuf::from("."));
            let cfg = AppConfig::default_for_installed(&installed);

            let views = [
                SettingsView::Root,
                SettingsView::Appearance,
                SettingsView::MenuColors,
                SettingsView::ColorGroupEdit(ColorGroup::Surface),
                SettingsView::ColorGroupEdit(ColorGroup::Accents),
                SettingsView::MainMenuEntry(0),
                SettingsView::MainMenuAdd,
                SettingsView::ConfirmResetMemory,
                SettingsView::ConfirmResetAll,
            ];

            let check_ring_slice = |renderer: &mut Renderer, tag: &str, items: &[settings::SettingsItem]| {
                let count = items.len();
                assert!(count <= PAGE_SIZE, "ring slice {tag} has {count} items");
                let seam = -FRAC_PI_2 - PI / count as f64;
                let span = TAU / count as f64;
                let style = item_style(&Mode::Settings, count);
                for (i, item) in items.iter().enumerate() {
                    let start = seam + i as f64 * span;
                    let end = start + span;
                    let angle = (start + end) * 0.5;
                    let origin = (
                        style.horizontal_radius * angle.cos(),
                        ITEM_RADIUS * angle.sin(),
                    );
                    let (font_size, _lx, width) =
                        renderer.fit_item_label(&item.label, (start, end), origin, style, 1.04);
                    let (text_w, _) = renderer.measure_cached(&item.label, font_size, true);
                    assert!(
                        text_w <= width - 2.0,
                        "view={tag} item={:?} (font={font_size:.1}pt) measured {text_w:.1}px > safe avail {:.1}px (would clip with ellipsis)",
                        item.label,
                        width - 2.0
                    );
                }
            };

            for view in views {
                let items = settings::build_items(&cfg, &store, &installed, &view);
                check_ring_slice(&mut renderer, &format!("{view:?}"), &items);
            }

            // Also verify the default 5-item Main Ring labels fit cleanly without truncation
            let main_labels = [
                "Files",
                "Applications",
                "Windows",
                "Clipboard",
                "Settings",
            ];
            let count = main_labels.len();
            let seam = -FRAC_PI_2 - PI / count as f64;
            let span = TAU / count as f64;
            let main_style = item_style(&Mode::Main, count);
            for (i, label) in main_labels.iter().enumerate() {
                let start = seam + i as f64 * span;
                let end = start + span;
                let angle = (start + end) * 0.5;
                let origin = (
                    main_style.horizontal_radius * angle.cos(),
                    ITEM_RADIUS * angle.sin(),
                );
                let (font_size, _lx, width) =
                    renderer.fit_item_label(label, (start, end), origin, main_style, 1.04);
                let (text_w, _) = renderer.measure_cached(label, font_size, true);
                assert!(
                    text_w <= width - 2.0,
                    "Main Ring item={label:?} (font={font_size:.1}pt) measured {text_w:.1}px > safe avail {:.1}px",
                    width - 2.0
                );
            }

            // Keybinds (>8 items) uses 4x3 Grid in default OverflowLayout::Box; in paged Ring mode,
            // verify fit_item_label respects the readable 9.0pt floor without panicking.
            let kb_items = settings::build_items(&cfg, &store, &installed, &SettingsView::Keybinds);
            assert!(kb_items.len() > PAGE_SIZE);
            for chunk in kb_items.chunks(PAGE_SIZE) {
                let c = chunk.len();
                let s = -FRAC_PI_2 - PI / c as f64;
                let sp = TAU / c as f64;
                let st = item_style(&Mode::Settings, c);
                for (i, item) in chunk.iter().enumerate() {
                    let start = s + i as f64 * sp;
                    let end = start + sp;
                    let ang = (start + end) * 0.5;
                    let (font_size, _lx, width) = renderer.fit_item_label(
                        &item.label,
                        (start, end),
                        (st.horizontal_radius * ang.cos(), ITEM_RADIUS * ang.sin()),
                        st,
                        1.04,
                    );
                    assert!((9.0..=st.base_font).contains(&font_size));
                    assert!(width >= 20.0);
                }
            }
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn d2d_recreate_hit_test_and_cache_eviction_stress_test() {
            let hwnd = TestHwnd::new(1280, 720);
            let mut renderer = Renderer::new(hwnd.0, 1280, 720, 1.25).expect("d2d renderer");
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("recircles_stress_{nanos}"));
            let store = ConfigStore::in_dir(dir.clone());
            let installed = InstalledModules::all_for_tests(PathBuf::from("."));
            let mut app = App::new(store, installed, Arc::new(DummyBackend), None).unwrap();
            app.set_viewport(1024.0, 576.0, 512.0, 288.0);
            app.ring_motion.advance(5.0);

            // 1. DirectWrite HitTestTextPosition is strictly monotonic across "#26160de8"
            let sample = "#26160de8";
            let mut prev_x = -1.0_f64;
            for pos in 0..=sample.len() {
                let x = renderer.canvas.hit_test_text_pos(sample, 14.0, false, pos);
                assert!(
                    x > prev_x,
                    "hit_test_text_pos not monotonic at pos={pos}: {x} <= {prev_x}"
                );
                prev_x = x;
            }

            // 2. Stress layout LRU (>512), annulus (>64), shadow (>32), and surrogate-pair ellipsizing
            renderer.canvas.begin_frame(1.25);
            for i in 0..560 {
                let txt = if i % 4 == 0 {
                    format!("🎉 Long Multi Line Emoji App Title Number {i} 🚀")
                } else {
                    format!("Unique Label Entry #{i} — DirectWrite Cache Eviction Test")
                };
                renderer.canvas.draw_text(
                    &txt,
                    12.5,
                    i % 2 == 0,
                    20.0,
                    20.0,
                    120.0,
                    34.0,
                    i % 3,
                    (i % 3) + 1,
                    if i % 4 == 0 { 2 } else { 1 },
                    D2dColor::rgba(1.0, 1.0, 1.0, 1.0),
                    None,
                );
            }
            for i in 0..72 {
                let inner = 50.0 + i as f64 * 0.5;
                let outer = 140.0 + i as f64 * 0.5;
                renderer.canvas.fill_annulus_linear(
                    inner,
                    outer,
                    0.0,
                    -outer,
                    0.0,
                    outer,
                    &[
                        D2dGradStop { pos: 0.0, color: D2dColor::rgba(0.2, 0.1, 0.05, 0.8) },
                        D2dGradStop { pos: 1.0, color: D2dColor::rgba(0.1, 0.05, 0.02, 0.8) },
                    ],
                );
            }
            for i in 0..40 {
                renderer.canvas.fill_rounded_rect_shadow(
                    -100.0 - i as f64,
                    -50.0,
                    200.0 + i as f64 * 2.0,
                    100.0,
                    16.0,
                    D2dColor::rgba(0.0, 0.0, 0.0, 1.0),
                    1.0,
                );
            }
            assert!(renderer.canvas.end_frame(0));

            // 3. Custom font family switch & fallback
            renderer.canvas.set_font_family("Consolas");
            let mono_w = renderer.canvas.hit_test_text_pos("iii", 14.0, false, 3);
            let mono_m = renderer.canvas.hit_test_text_pos("mmm", 14.0, false, 3);
            assert!((mono_w - mono_m).abs() < 1.0, "Consolas should be monospace: {mono_w} vs {mono_m}");
            renderer.canvas.set_font_family("");

            // 4. Device-lost recreation (releases IDCompositionTarget before rebinding HWND!)
            assert!(
                renderer.canvas.recreate(),
                "d2d_recreate must succeed on existing DirectComposition HWND"
            );
            renderer.recover_device();
            renderer.resize(1366, 768, 1.5);
            renderer.render_and_present(1.5, &app);

            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn benchmark_d2d_frame_times_ring_grid_and_3d_loading() {
            let hwnd = TestHwnd::new(1536, 864);
            let mut renderer = Renderer::new(hwnd.0, 1536, 864, 1.0).expect("d2d renderer");
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("recircles_perf_{nanos}"));
            let store = ConfigStore::in_dir(dir.clone());
            let installed = InstalledModules::all_for_tests(PathBuf::from("."));
            let mut app = App::new(store, installed, Arc::new(DummyBackend), None).unwrap();
            app.set_viewport(1536.0, 864.0, 768.0, 432.0);
            app.ring_motion.advance(5.0);

            let measure = |renderer: &mut Renderer, app: &mut App, frames: usize, tick_dt: f64| -> (f64, f64) {
                for _ in 0..8 {
                    if tick_dt > 0.0 {
                        app.tick(tick_dt);
                    }
                    renderer.render_and_present(1.0, app);
                }
                let mut times_ms = Vec::with_capacity(frames);
                for _ in 0..frames {
                    if tick_dt > 0.0 {
                        app.tick(tick_dt);
                    }
                    let t0 = Instant::now();
                    renderer.render_and_present(1.0, app);
                    times_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                }
                times_ms.sort_by(f64::total_cmp);
                let mean = times_ms.iter().sum::<f64>() / frames as f64;
                let p95 = times_ms[(frames * 95 / 100).min(frames - 1)];
                (mean, p95)
            };

            let (ring_mean, ring_p95) = measure(&mut renderer, &mut app, 60, 0.0);

            // 4x3 Grid with 12 wrapped labels including 3+ line strings to verify DWrite layout cache
            app.open_menu_target("apps", false).unwrap();
            let job_gen = app.async_gen;
            let fake_apps: Vec<AppDto> = (0..32)
                .map(|i| AppDto {
                    id: format!("app_{i}"),
                    name: if i % 3 == 0 {
                        format!("Very Long Multi Line Application Title Example #{i}")
                    } else {
                        format!("Application Utility {i}")
                    },
                    launch_target: format!("app_{i}.exe"),
                    icon_path: String::new(),
                })
                .collect();
            let _ = app.async_tx.send(AsyncJobResult::AppsLoaded {
                job_gen,
                result: Ok(fake_apps),
            });
            app.tick(0.0);
            app.ring_motion.advance(5.0);
            let (grid_mean, grid_p95) = measure(&mut renderer, &mut app, 60, 0.0);

            // 3D tumbling glass loading ring
            app.enter_main_menu();
            app.ring_motion.set_loading(true, true);
            app.loading_active = true;
            app.loading_visible = true;
            app.ring_motion.advance(0.25);
            let (load3d_mean, load3d_p95) = measure(&mut renderer, &mut app, 60, 0.016);

            eprintln!(
                "PERF_MS ring=({ring_mean:.3} mean, {ring_p95:.3} p95) grid=({grid_mean:.3} mean, {grid_p95:.3} p95) loading3d=({load3d_mean:.3} mean, {load3d_p95:.3} p95)"
            );
            assert!(ring_mean < 50.0, "ring frame too slow: {ring_mean:.2}ms");
            assert!(grid_mean < 50.0, "grid frame too slow: {grid_mean:.2}ms");
            assert!(load3d_mean < 50.0, "3d loading frame too slow: {load3d_mean:.2}ms");

            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn apps_menu_morph_and_scroll_stay_non_blocking_with_real_shortcuts() {
            let hwnd = TestHwnd::new(1536, 864);
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("recircles_apps_perf_{nanos}"));
            let store = ConfigStore::in_dir(dir.clone());
            let mut renderer = Renderer::new_with_store(
                hwnd.0,
                1536,
                864,
                1.0,
                ConfigStore::in_dir(dir.clone()),
            )
            .expect("d2d renderer");
            let installed = InstalledModules::all_for_tests(PathBuf::from("."));
            let mut app = App::new(store, installed, Arc::new(DummyBackend), None).unwrap();
            app.set_viewport(1536.0, 864.0, 768.0, 432.0);
            app.ring_motion.advance(5.0);

            // Collect real Start Menu shortcuts (plus System32 notepad.exe first so at least one
            // guaranteed-extractable icon is always in the visible viewport).
            let mut real_paths: Vec<(String, String)> = Vec::new();
            let sys = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
            let notepad = PathBuf::from(&sys).join("System32").join("notepad.exe");
            if notepad.is_file() {
                real_paths.push(("Notepad".into(), notepad.to_string_lossy().into_owned()));
            }
            if let Ok(pd) = std::env::var("ProgramData") {
                let root = PathBuf::from(pd)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs");
                fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<(String, String)>) {
                    if depth > 4 || out.len() >= 48 {
                        return;
                    }
                    let Ok(rd) = std::fs::read_dir(dir) else {
                        return;
                    };
                    for e in rd.flatten() {
                        if out.len() >= 48 {
                            break;
                        }
                        let Ok(ft) = e.file_type() else { continue };
                        let p = e.path();
                        if ft.is_dir() {
                            walk(&p, depth + 1, out);
                        } else if p.extension().and_then(|s| s.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("lnk"))
                            && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                        {
                            out.push((stem.to_string(), p.to_string_lossy().into_owned()));
                        }
                    }
                }
                walk(&root, 0, &mut real_paths);
            }
            if real_paths.len() < 24 {
                let p = notepad.to_string_lossy().into_owned();
                for i in real_paths.len()..28 {
                    real_paths.push((format!("App Tool With Longer Name #{i}"), p.clone()));
                }
            }

            let apps: Vec<AppDto> = real_paths
                .into_iter()
                .enumerate()
                .map(|(i, (name, path))| AppDto {
                    id: format!("app:{i}:{name}"),
                    name,
                    launch_target: path.clone(),
                    icon_path: path,
                })
                .collect();

            app.open_menu_target("apps", false).unwrap();
            let job_gen = app.async_gen;
            let _ = app.async_tx.send(AsyncJobResult::AppsLoaded {
                job_gen,
                result: Ok(apps),
            });
            app.tick(0.0);

            // Warm up D2D pipeline (matching Main Ring open before entering Apps)
            for _ in 0..4 {
                renderer.render_and_present(1.0, &app);
            }

            // 1. Measure 40 frames of ring -> grid morph (zero UI-thread icon extraction stalls!)
            let mut morph_ms = Vec::with_capacity(40);
            for _ in 0..40 {
                app.tick(0.016);
                let t0 = Instant::now();
                renderer.render_and_present(1.0, &app);
                morph_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
            morph_ms.sort_by(f64::total_cmp);
            let morph_mean = morph_ms.iter().sum::<f64>() / morph_ms.len() as f64;
            let morph_max = *morph_ms.last().unwrap();

            // 2. Measure row-by-row scrolling through the apps grid (4 new icons per scroll step!)
            app.ring_motion.advance(5.0);
            let mut scroll_ms = Vec::new();
            for _ in 0..10 {
                app.handle_wheel(-120.0);
                app.tick(0.016);
                let t0 = Instant::now();
                renderer.render_and_present(1.0, &app);
                scroll_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
            scroll_ms.sort_by(f64::total_cmp);
            let scroll_mean = scroll_ms.iter().sum::<f64>() / scroll_ms.len() as f64;
            let scroll_max = *scroll_ms.last().unwrap();

            // Wait briefly for background icon workers to finish and verify GPU bitmaps upload cleanly
            let deadline = Instant::now() + std::time::Duration::from_secs(10);
            while Instant::now() < deadline
                && !renderer.shell_bitmaps.values().any(|v| v.is_some())
            {
                renderer.drain_ready_icons(32);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            renderer.render_and_present(1.0, &app);

            eprintln!(
                "APPS_REAL_PERF_MS morph=({morph_mean:.3} mean, {morph_max:.3} max) scroll=({scroll_mean:.3} mean, {scroll_max:.3} max) uploaded_icons={}",
                renderer.shell_bitmaps.len()
            );
            assert!(morph_mean < 50.0, "apps morph mean too slow: {morph_mean:.2}ms");
            assert!(scroll_mean < 50.0, "apps scroll mean too slow: {scroll_mean:.2}ms");
            assert!(
                renderer.shell_bitmaps.values().any(|v| v.is_some()),
                "background shell icon loader should populate bitmaps asynchronously"
            );

            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn backdrop_blur_scales_linearly_and_recaptures_cleanly() {
            let hwnd = TestHwnd::new(960, 640);
            let mut renderer = Renderer::new(hwnd.0, 960, 640, 1.0).expect("d2d renderer");
            assert!(
                renderer.capture_backdrop(0, 0, 1.0, 0),
                "capture_backdrop at 0% should retain raw_backdrop"
            );
            assert!(renderer.canvas.has_raw_backdrop(), "raw backdrop must be retained at 0%");
            assert!(!renderer.canvas.has_backdrop(), "0% blur must have no blurred backdrop");

            // Switching from 0% -> 1% -> 50% -> 100% must activate blur smoothly without recapture
            let (s1, sat1) = blur_params(1, 1.0).unwrap();
            assert!(renderer.canvas.set_backdrop_blur(s1, sat1));
            assert!(renderer.canvas.has_backdrop(), "1% blur must activate blurred backdrop");

            // Render a frame with annulus, rounded rect (with and without hole), and polygon ring
            renderer.canvas.begin_frame(1.0);
            renderer.canvas.save();
            renderer.canvas.translate(480.0, 320.0);
            renderer.canvas.fill_annulus_backdrop(56.0, 140.0, 1.0);
            renderer.canvas.restore();
            renderer.canvas.push_opacity_layer(20.0, 20.0, 300.0, 200.0, 0.65);
            renderer.canvas.fill_rounded_rect_backdrop(24.0, 24.0, 280.0, 180.0, 18.0, 0.0, 1.0);
            renderer.canvas.pop_layer();
            renderer.canvas.fill_rounded_rect_backdrop(360.0, 40.0, 220.0, 140.0, 20.0, 36.0, 0.85);
            let outer = [(680.0, 80.0), (820.0, 90.0), (810.0, 210.0), (670.0, 200.0)];
            let inner = [(720.0, 115.0), (775.0, 120.0), (770.0, 175.0), (715.0, 170.0)];
            renderer.canvas.fill_polygon_ring_backdrop(&outer, &inner, 0.9);
            assert!(renderer.canvas.end_frame(0), "backdrop frame must present");

            // Returning to 0% disables blur while retaining raw_backdrop for instant re-enable
            renderer.update_backdrop_dpi(1.5, 65);
            assert!(renderer.canvas.has_backdrop());
            renderer.update_backdrop_dpi(1.5, 0);
            assert!(!renderer.canvas.has_backdrop());
            assert!(renderer.canvas.has_raw_backdrop());

            // In-place recapture at same dimensions and after resize/recreate
            assert!(renderer.capture_backdrop(0, 0, 1.0, 25));
            assert!(renderer.canvas.has_backdrop());
            renderer.resize(1280, 720, 1.25);
            assert!(renderer.capture_backdrop(0, 0, 1.25, 40));
            assert!(renderer.canvas.recreate(), "recreate must succeed on DirectComposition HWND");
            assert!(renderer.capture_backdrop(0, 0, 1.25, 40));
            renderer.canvas.begin_frame(1.25);
            renderer.canvas.fill_rounded_rect_backdrop(40.0, 40.0, 400.0, 260.0, 24.0, 0.0, 1.0);
            assert!(renderer.canvas.end_frame(0), "post-recreate backdrop frame must present");
        }
    }
}
