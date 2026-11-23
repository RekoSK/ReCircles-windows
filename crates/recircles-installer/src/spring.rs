use recircles_core::colors::Color;
use recircles_core::modules::BuiltinSubmenu;
use recircles_core::themes::ColorsConfig;
use std::collections::BTreeSet;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

pub type Rgb = (f64, f64, f64);

/// Apple-style spring parameterized by (damping_ratio, response_seconds).
/// - `damping_ratio = 1.0`: critically damped, smooth settle with zero overshoot.
/// - `damping_ratio ~ 0.8`: slight physical bounce for momentum / release moments.
/// - `response`: natural period in seconds (smaller = snappier).
#[derive(Debug, Clone, Copy)]
pub struct AppleSpring {
    pub value: f64,
    pub target: f64,
    pub velocity: f64,
    stiffness: f64,
    damping: f64,
}

impl AppleSpring {
    pub fn new(initial: f64, damping_ratio: f64, response: f64) -> Self {
        let r = response.max(0.05);
        let dr = damping_ratio.max(0.15);
        let omega = TAU / r;
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            stiffness: omega * omega,
            damping: 2.0 * dr * omega,
        }
    }

    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    pub fn snap(&mut self, value: f64) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    pub fn step(&mut self, dt: f64, reduced_motion: bool) -> bool {
        if reduced_motion {
            let moved = (self.value - self.target).abs() > 1e-4;
            self.value = self.target;
            self.velocity = 0.0;
            return moved;
        }
        let dt = dt.clamp(0.0, 0.05);
        if dt <= 0.0 {
            return !self.is_settled();
        }
        // Unconditionally crisp sub-step integration across 60Hz-240Hz
        let substeps = ((dt / 0.003).ceil() as usize).clamp(1, 12);
        let h = dt / substeps as f64;
        for _ in 0..substeps {
            let disp = self.value - self.target;
            let accel = -self.stiffness * disp - self.damping * self.velocity;
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }
        if (self.value - self.target).abs() < 0.0005 && self.velocity.abs() < 0.0025 {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        true
    }

    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 0.0005 && self.velocity.abs() < 0.0025
    }
}

/// Apple WWDC 2018 rubber-band boundary resistance.
#[allow(dead_code)]
pub fn rubberband(overshoot: f64, dimension: f64, constant: f64) -> f64 {
    let dim = dimension.max(1.0);
    (overshoot * dim * constant) / (dim + constant * overshoot.abs())
}

/// Apple WWDC 2018 exponential-decay momentum projection.
#[allow(dead_code)]
pub fn project_momentum(velocity_px_per_sec: f64, deceleration_rate: f64) -> f64 {
    let d = deceleration_rate.clamp(0.9, 0.9995);
    (velocity_px_per_sec / 1000.0) * d / (1.0 - d)
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn lerp_rgb(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

pub fn shortest_angle_diff(from: f64, to: f64) -> f64 {
    let mut d = (to - from) % TAU;
    if d > PI {
        d -= TAU;
    } else if d < -PI {
        d += TAU;
    }
    d
}

/// Complete ReCircles visual palette with smooth theme-to-theme interpolation.
#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub ring_top: Rgb,
    pub ring_mid: Rgb,
    pub ring_bottom: Rgb,
    pub rim_top: Rgb,
    pub rim_bottom: Rgb,
    pub divider: Rgb,
    pub shadow: Rgb,
    pub capsule_top: Rgb,
    pub capsule_bottom: Rgb,
    pub capsule_border: Rgb,
    pub text_primary: Rgb,
    pub text_muted: Rgb,
    pub text_shadow: Rgb,
    pub accent: Rgb,
    pub accent_fg: Rgb,
    pub accents: [Rgb; 5],
    pub is_light: f64, // 0.0 = dark glass theme, 1.0 = frosted daylight theme (e.g. Light Green)
}

impl ThemePalette {
    pub fn from_colors(c: &ColorsConfig) -> Self {
        let rgb = |col: Color| (col.r, col.g, col.b);
        let ring_mid_col = c.ring_color();
        let accent_col = c.accent_color();
        let is_light = if ring_mid_col.relative_luminance() > 0.36 {
            1.0
        } else {
            0.0
        };
        Self {
            ring_top: rgb(c.ring_top_color()),
            ring_mid: rgb(ring_mid_col),
            ring_bottom: rgb(c.ring_bottom_color()),
            rim_top: rgb(c.ring_border_color()),
            rim_bottom: rgb(c.rim_bottom_color()),
            divider: rgb(c.divider_color()),
            shadow: rgb(c.shadow_color()),
            capsule_top: rgb(c.capsule_color()),
            capsule_bottom: rgb(c.capsule_bottom_color()),
            capsule_border: rgb(c.capsule_border_color()),
            text_primary: rgb(c.text_color()),
            text_muted: rgb(c.muted_color()),
            text_shadow: rgb(c.text_shadow_color()),
            accent: rgb(accent_col),
            accent_fg: rgb(accent_col.contrast_fg()),
            accents: [
                rgb(c.accent_for_mode("files")),
                rgb(c.accent_for_mode("apps")),
                rgb(c.accent_for_mode("windows")),
                rgb(c.accent_for_mode("clipboard")),
                rgb(c.accent_for_mode("settings")),
            ],
            is_light,
        }
    }

    pub fn accent_for_submenu(&self, sub: BuiltinSubmenu) -> Rgb {
        let idx = match sub {
            BuiltinSubmenu::Files => 0,
            BuiltinSubmenu::Apps => 1,
            BuiltinSubmenu::Windows => 2,
            BuiltinSubmenu::Clipboard => 3,
            BuiltinSubmenu::Settings => 4,
        };
        self.accents[idx]
    }

    pub fn lerp(&self, to: &Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            ring_top: lerp_rgb(self.ring_top, to.ring_top, t),
            ring_mid: lerp_rgb(self.ring_mid, to.ring_mid, t),
            ring_bottom: lerp_rgb(self.ring_bottom, to.ring_bottom, t),
            rim_top: lerp_rgb(self.rim_top, to.rim_top, t),
            rim_bottom: lerp_rgb(self.rim_bottom, to.rim_bottom, t),
            divider: lerp_rgb(self.divider, to.divider, t),
            shadow: lerp_rgb(self.shadow, to.shadow, t),
            capsule_top: lerp_rgb(self.capsule_top, to.capsule_top, t),
            capsule_bottom: lerp_rgb(self.capsule_bottom, to.capsule_bottom, t),
            capsule_border: lerp_rgb(self.capsule_border, to.capsule_border, t),
            text_primary: lerp_rgb(self.text_primary, to.text_primary, t),
            text_muted: lerp_rgb(self.text_muted, to.text_muted, t),
            text_shadow: lerp_rgb(self.text_shadow, to.text_shadow, t),
            accent: lerp_rgb(self.accent, to.accent, t),
            accent_fg: lerp_rgb(self.accent_fg, to.accent_fg, t),
            accents: std::array::from_fn(|i| lerp_rgb(self.accents[i], to.accents[i], t)),
            is_light: lerp(self.is_light, to.is_light, t),
        }
    }
}

/// Smoothly animated theme blender so dark <-> light theme transitions never flash abruptly.
#[derive(Debug, Clone)]
pub struct PaletteTransition {
    from: ThemePalette,
    to: ThemePalette,
    mix: AppleSpring,
}

impl PaletteTransition {
    pub fn new(initial: ThemePalette) -> Self {
        Self {
            from: initial,
            to: initial,
            mix: AppleSpring::new(1.0, 1.0, 0.36),
        }
    }

    pub fn transition_to(&mut self, next: ThemePalette, reduced_motion: bool) {
        let live = self.current();
        self.from = live;
        self.to = next;
        if reduced_motion {
            self.mix.snap(1.0);
        } else {
            self.mix.value = 0.0;
            self.mix.velocity = 0.0;
            self.mix.target = 1.0;
        }
    }

    pub fn current(&self) -> ThemePalette {
        self.from.lerp(&self.to, self.mix.value)
    }

    pub fn step(&mut self, dt: f64, reduced_motion: bool) -> bool {
        self.mix.step(dt, reduced_motion)
    }
}

/// Per-submenu animated sector slot on the live ReCircles radial ring.
#[derive(Debug, Clone)]
pub struct RingSectorSlot {
    pub submenu: BuiltinSubmenu,
    pub weight: AppleSpring,
    pub focus: AppleSpring,
    pub press: AppleSpring,
    pub tilt: AppleSpring,
}

impl RingSectorSlot {
    pub fn new(submenu: BuiltinSubmenu, enabled: bool) -> Self {
        let w = if enabled { 1.0 } else { 0.0 };
        Self {
            submenu,
            weight: AppleSpring::new(w, 0.96, 0.28),
            focus: AppleSpring::new(0.0, 1.0, 0.16),
            press: AppleSpring::new(0.0, 1.0, 0.14),
            tilt: AppleSpring::new(0.0, 0.92, 0.18),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LoadingPose3D {
    pub rotation: [[f64; 3]; 3],
    pub depth: f64,
}

/// Live ReCircles radial ring motion state (including authentic 3D tumbling glass install ring!).
#[derive(Debug, Clone)]
pub struct LiveRingMotion {
    pub open_scale: AppleSpring,
    pub open_alpha: AppleSpring,
    pub slots: [RingSectorSlot; 5],
    pub hovered: Option<BuiltinSubmenu>,
    pub pressed: Option<BuiltinSubmenu>,
    pub highlighted_hint: Option<BuiltinSubmenu>,
    pub installing: bool,
    pub loading_depth: AppleSpring,
    pub tumble_time: f64,
    pub rot_x: f64,
    pub rot_y: f64,
    pub rot_z: f64,
    pub complete_bloom: AppleSpring,
}

impl LiveRingMotion {
    pub fn new(selected: &BTreeSet<BuiltinSubmenu>, reduced_motion: bool) -> Self {
        let mut open_scale = AppleSpring::new(if reduced_motion { 1.0 } else { 0.84 }, 0.84, 0.38);
        open_scale.set_target(1.0);
        let mut open_alpha = AppleSpring::new(if reduced_motion { 1.0 } else { 0.0 }, 1.0, 0.28);
        open_alpha.set_target(1.0);
        let slots = std::array::from_fn(|i| {
            let sub = BuiltinSubmenu::ALL[i];
            RingSectorSlot::new(sub, selected.contains(&sub) || sub.is_mandatory())
        });
        Self {
            open_scale,
            open_alpha,
            slots,
            hovered: None,
            pressed: None,
            highlighted_hint: None,
            installing: false,
            loading_depth: AppleSpring::new(0.0, 0.95, 0.36),
            tumble_time: 0.0,
            rot_x: 0.0,
            rot_y: 0.0,
            rot_z: 0.0,
            complete_bloom: AppleSpring::new(0.0, 0.75, 0.50),
        }
    }

    pub fn sync_selected(&mut self, selected: &BTreeSet<BuiltinSubmenu>) {
        for slot in &mut self.slots {
            let on = selected.contains(&slot.submenu) || slot.submenu.is_mandatory();
            slot.weight.set_target(if on { 1.0 } else { 0.0 });
        }
    }

    pub fn set_hover(&mut self, next: Option<BuiltinSubmenu>) {
        if self.hovered != next {
            if let Some(sub) = next
                && let Some(slot) = self.slots.iter_mut().find(|s| s.submenu == sub)
            {
                // Subtle physical nudge on hover enter
                slot.tilt.velocity += 0.95;
            }
            self.hovered = next;
        }
    }

    pub fn set_installing(&mut self, active: bool) {
        self.installing = active;
        self.loading_depth.set_target(if active { 1.0 } else { 0.0 });
    }

    pub fn trigger_complete_bloom(&mut self) {
        self.complete_bloom.value = 1.0;
        self.complete_bloom.velocity = 0.0;
        self.complete_bloom.target = 0.0;
    }

    pub fn step(&mut self, dt: f64, reduced_motion: bool) -> bool {
        let mut active = false;
        active |= self.open_scale.step(dt, reduced_motion);
        active |= self.open_alpha.step(dt, reduced_motion);
        active |= self.loading_depth.step(dt, reduced_motion);
        active |= self.complete_bloom.step(dt, reduced_motion);

        let focus_sub = self.hovered.or(self.highlighted_hint);
        for slot in &mut self.slots {
            slot.focus
                .set_target(if focus_sub == Some(slot.submenu) { 1.0 } else { 0.0 });
            slot.press
                .set_target(if self.pressed == Some(slot.submenu) { 1.0 } else { 0.0 });
            slot.tilt.set_target(0.0);
            active |= slot.weight.step(dt, reduced_motion);
            active |= slot.focus.step(dt, reduced_motion);
            active |= slot.press.step(dt, reduced_motion);
            active |= slot.tilt.step(dt, reduced_motion);
        }

        if self.installing && !reduced_motion {
            let dt_c = dt.clamp(0.0, 0.05);
            self.tumble_time += dt_c;
            let t = self.tumble_time;
            self.rot_x = (t * 2.1).sin() * 0.50;
            self.rot_y = (t * 1.65 + 0.5).cos() * 0.54;
            self.rot_z = (self.rot_z + dt_c * 1.45) % TAU;
            active = true;
        } else if self.loading_depth.value > 0.005 || self.rot_x.abs() > 0.004 || self.rot_y.abs() > 0.004 {
            let k = (dt.clamp(0.0, 0.05) * 10.0).min(1.0);
            self.rot_x += (0.0 - self.rot_x) * k;
            self.rot_y += (0.0 - self.rot_y) * k;
            self.rot_z += shortest_angle_diff(self.rot_z, 0.0) * k;
            if reduced_motion || (self.rot_x.abs() < 0.005 && self.rot_y.abs() < 0.005 && self.loading_depth.value < 0.01) {
                self.rot_x = 0.0;
                self.rot_y = 0.0;
                self.rot_z = 0.0;
            } else {
                active = true;
            }
        }
        active
    }

    pub fn loading_pose(&self, reduced_motion: bool) -> Option<LoadingPose3D> {
        if reduced_motion || self.loading_depth.value < 0.01 {
            return None;
        }
        let d = self.loading_depth.value.clamp(0.0, 1.0);
        let (sx, cx) = (self.rot_x * d).sin_cos();
        let (sy, cy) = (self.rot_y * d).sin_cos();
        let (sz, cz) = (self.rot_z * d).sin_cos();

        // R = Rz * Ry * Rx
        let r = [
            [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
            [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
            [-sy, cy * sx, cy * cx],
        ];
        Some(LoadingPose3D {
            rotation: r,
            depth: d,
        })
    }

    /// Computes live continuous sector angular spans into a stack-allocated array, returning count.
    pub fn sector_spans_into(
        &self,
        out: &mut [(BuiltinSubmenu, f64, f64, f64, f64, f64, f64); 5],
    ) -> usize {
        let total_w: f64 = self.slots.iter().map(|s| s.weight.value.max(0.0)).sum();
        if total_w <= 1e-4 {
            return 0;
        }
        // Keep first active sector centered at 12 o'clock (-PI/2) like ReCircles!
        let first_w = self
            .slots
            .iter()
            .find(|s| s.weight.value > 0.005)
            .map(|s| s.weight.value)
            .unwrap_or(1.0);
        let mut cursor = -FRAC_PI_2 - (first_w / total_w) * PI;
        let mut count = 0usize;
        for slot in &self.slots {
            let w = slot.weight.value.max(0.0);
            if w <= 0.005 {
                continue;
            }
            let span = (w / total_w) * TAU;
            out[count] = (
                slot.submenu,
                cursor,
                cursor + span,
                w.clamp(0.0, 1.0),
                slot.focus.value.clamp(0.0, 1.0),
                slot.press.value.clamp(0.0, 1.0),
                slot.tilt.value,
            );
            count += 1;
            cursor += span;
        }
        count
    }

    /// Computes live continuous sector angular spans `(submenu, start_rad, end_rad, weight, focus, press, tilt)`.
    #[allow(dead_code)]
    pub fn sector_spans(&self) -> Vec<(BuiltinSubmenu, f64, f64, f64, f64, f64, f64)> {
        let mut buf = [(BuiltinSubmenu::Files, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0); 5];
        let n = self.sector_spans_into(&mut buf);
        buf[..n].to_vec()
    }

    /// Hit-tests a point `(dx, dy)` relative to ring center against `(inner_r, outer_r)`.
    pub fn hit_test_sector(&self, dx: f64, dy: f64, inner_r: f64, outer_r: f64) -> Option<BuiltinSubmenu> {
        let dist = dx.hypot(dy);
        if dist < inner_r || dist > outer_r {
            return None;
        }
        let mut ang = dy.atan2(dx);
        let mut spans = [(BuiltinSubmenu::Files, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0); 5];
        let count = self.sector_spans_into(&mut spans);
        if count == 0 {
            return None;
        }
        let start0 = spans[0].1;
        while ang < start0 {
            ang += TAU;
        }
        while ang >= start0 + TAU {
            ang -= TAU;
        }
        for &(sub, s, e, w, _, _, _) in &spans[..count] {
            if w > 0.25 && ang >= s && ang <= e {
                return Some(sub);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recircles_core::themes::builtin_theme;

    #[test]
    fn apple_spring_interruptible_reversal_and_reduced_motion() {
        let mut s = AppleSpring::new(0.0, 1.0, 0.34);
        s.set_target(1.0);
        for _ in 0..10 {
            s.step(1.0 / 120.0, false);
        }
        let mid = s.value;
        assert!(mid > 0.1 && mid < 0.99, "mid={mid}");
        // Mid-flight reversal preserves continuous presentation value
        s.set_target(0.0);
        let next_step = s.step(1.0 / 120.0, false);
        assert!(next_step);
        for _ in 0..240 {
            s.step(1.0 / 120.0, false);
        }
        assert!(s.is_settled());
        assert!(s.value.abs() < 0.001);

        // Reduced motion snaps immediately
        s.set_target(1.0);
        s.step(0.016, true);
        assert!(s.is_settled());
        assert_eq!(s.value, 1.0);
    }

    #[test]
    fn rubberband_resists_progressively_and_never_exceeds_dim() {
        let r1 = rubberband(20.0, 200.0, 0.55);
        let r2 = rubberband(200.0, 200.0, 0.55);
        assert!(r1 > 0.0 && r1 < 20.0);
        assert!(r2 > r1 && r2 < 200.0);
    }

    #[test]
    fn palette_transition_eases_dark_amber_to_light_green() {
        let amber = ThemePalette::from_colors(&builtin_theme("default").unwrap().colors);
        let light = ThemePalette::from_colors(&builtin_theme("light_green").unwrap().colors);
        assert_eq!(amber.is_light, 0.0);
        assert_eq!(light.is_light, 1.0);

        let mut pt = PaletteTransition::new(amber);
        pt.transition_to(light, false);
        pt.step(0.12, false);
        let mid = pt.current();
        assert!(mid.is_light > 0.1 && mid.is_light < 0.95);
        for _ in 0..180 {
            pt.step(1.0 / 120.0, false);
        }
        assert!((pt.current().is_light - 1.0).abs() < 0.001);
    }

    #[test]
    fn live_ring_sectors_cover_full_circle_and_hit_test_cleanly() {
        let mut selected = BTreeSet::new();
        selected.insert(BuiltinSubmenu::Files);
        selected.insert(BuiltinSubmenu::Apps);
        selected.insert(BuiltinSubmenu::Settings);
        let ring = LiveRingMotion::new(&selected, true);
        let spans = ring.sector_spans();
        assert_eq!(spans.len(), 3);
        let total_arc: f64 = spans.iter().map(|(_, s, e, _, _, _, _)| e - s).sum();
        assert!((total_arc - TAU).abs() < 1e-6);
        // 12 o'clock should hit first sector (Files)
        assert_eq!(ring.hit_test_sector(0.0, -70.0, 40.0, 100.0), Some(BuiltinSubmenu::Files));
        // Inside center hole hits nothing
        assert_eq!(ring.hit_test_sector(0.0, 0.0, 40.0, 100.0), None);

        let mut stack_buf = [(BuiltinSubmenu::Files, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0); 5];
        let count = ring.sector_spans_into(&mut stack_buf);
        assert_eq!(count, 3);
        for i in 0..count {
            assert_eq!(stack_buf[i].0, spans[i].0);
            assert!((stack_buf[i].1 - spans[i].1).abs() < 1e-9);
            assert!((stack_buf[i].2 - spans[i].2).abs() < 1e-9);
        }
    }
}
