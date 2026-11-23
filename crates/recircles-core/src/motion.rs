use std::collections::HashMap;
use std::f64::consts::TAU;

pub const SLOT_COUNT: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f64,
    pub target: f64,
    pub velocity: f64,
    pub stiffness: f64,
    pub damping: f64,
}

impl Spring {
    pub fn new(initial: f64, stiffness: f64, damping: f64) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            stiffness,
            damping,
        }
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
        // Sub-step integration for unconditionally crisp stability at 60Hz–240Hz
        let substeps = ((dt / 0.004).ceil() as usize).clamp(1, 8);
        let h = dt / substeps as f64;
        for _ in 0..substeps {
            let disp = self.value - self.target;
            let accel = -self.stiffness * disp - self.damping * self.velocity;
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }
        if (self.value - self.target).abs() < 0.0008 && self.velocity.abs() < 0.004 {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        true
    }

    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 0.0008 && self.velocity.abs() < 0.004
    }
}

#[derive(Debug, Clone)]
pub struct Slot {
    pub x: Spring,
    pub y: Spring,
    pub alpha: Spring,
    pub scale: Spring,
    pub key: String,
    pub active: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            x: Spring::new(0.0, 340.0, 32.0),
            y: Spring::new(0.0, 340.0, 32.0),
            alpha: Spring::new(0.0, 320.0, 30.0),
            scale: Spring::new(0.84, 340.0, 32.0),
            key: String::new(),
            active: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LoadingPose {
    pub active: bool,
    pub elapsed: f64,
    pub rx: f64,
    pub ry: f64,
    pub rz: f64,
    pub alpha: f64,
    pub pulse: f64,
    pub is_3d: bool,
    settling: bool,
}

impl Default for LoadingPose {
    fn default() -> Self {
        Self {
            active: false,
            elapsed: 0.0,
            rx: 0.0,
            ry: 0.0,
            rz: 0.0,
            alpha: 0.0,
            pulse: 0.0,
            is_3d: true,
            settling: false,
        }
    }
}

impl LoadingPose {
    pub fn start(&mut self, is_3d: bool) {
        self.active = true;
        self.settling = false;
        self.is_3d = is_3d;
    }

    pub fn stop(&mut self) {
        if self.active {
            self.active = false;
            self.settling = true;
        }
    }

    pub fn is_animating(&self) -> bool {
        self.active || self.settling
    }

    pub fn step(&mut self, dt: f64, reduced_motion: bool) -> bool {
        if reduced_motion {
            let changed = self.is_animating();
            self.elapsed = 0.0;
            self.rx = 0.0;
            self.ry = 0.0;
            self.rz = 0.0;
            self.pulse = 0.0;
            self.alpha = if self.active { 1.0 } else { 0.0 };
            self.settling = false;
            return changed;
        }

        if self.active {
            self.elapsed += dt;
            self.alpha = (self.alpha + dt * 5.0).min(1.0);
            if self.is_3d {
                let t = self.elapsed;
                self.rx = (t * 2.2).sin() * 0.48;
                self.ry = (t * 1.7 + 0.6).cos() * 0.52;
                self.rz += dt * 1.35;
                if self.rz >= TAU {
                    self.rz -= TAU;
                }
                self.pulse = 0.5 + 0.5 * (t * 3.0).sin();
            } else {
                self.rx = 0.0;
                self.ry = 0.0;
                self.rz = 0.0;
                self.pulse = 0.5 + 0.5 * (self.elapsed * 3.8).sin();
            }
            return true;
        }

        if self.settling {
            let k = (dt * 12.0).min(1.0);
            self.rx += (0.0 - self.rx) * k;
            self.ry += (0.0 - self.ry) * k;
            let dz = shortest_angle_diff(self.rz, 0.0);
            self.rz += dz * k;
            self.alpha = (self.alpha - dt * 6.0).max(0.0);
            self.pulse += (0.0 - self.pulse) * k;
            if self.rx.abs() < 0.008 && self.ry.abs() < 0.008 && self.alpha < 0.01 {
                self.rx = 0.0;
                self.ry = 0.0;
                self.rz = 0.0;
                self.alpha = 0.0;
                self.pulse = 0.0;
                self.settling = false;
                return false;
            }
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EmptyRing {
    pub active: bool,
    pub shrink: Spring,
    pub text_alpha: Spring,
    pub pulse_phase: f64,
}

impl Default for EmptyRing {
    fn default() -> Self {
        Self {
            active: false,
            shrink: Spring::new(0.0, 260.0, 26.0),
            text_alpha: Spring::new(0.0, 240.0, 28.0),
            pulse_phase: 0.0,
        }
    }
}

impl EmptyRing {
    pub fn set_active(&mut self, empty: bool) {
        self.active = empty;
        self.shrink.target = if empty { 1.0 } else { 0.0 };
        self.text_alpha.target = if empty { 1.0 } else { 0.0 };
    }

    pub fn step(&mut self, dt: f64, reduced_motion: bool) -> bool {
        let a = self.shrink.step(dt, reduced_motion);
        let b = self.text_alpha.step(dt, reduced_motion);
        if self.active && !reduced_motion {
            self.pulse_phase = (self.pulse_phase + dt * 2.8) % TAU;
            return true;
        }
        a || b
    }

    pub fn pulse_offset(&self) -> f64 {
        if self.active {
            self.pulse_phase.sin() * 2.2 * self.shrink.value
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct Motion {
    pub ring_scale: Spring,
    pub ring_alpha: Spring,
    pub focus_angle: f64,
    pub focus_target: f64,
    pub focus_velocity: f64,
    pub focus_mix: Spring,
    pub center_blend: Spring,
    pub prev_center_title: String,
    pub center_title: String,
    pub prev_center_subtitle: String,
    pub center_subtitle: String,
    pub slots: [Slot; SLOT_COUNT],
    pub loading: LoadingPose,
    pub empty: EmptyRing,
    pub box_progress: Spring,
    pub color_hue: Spring,
    pub reduced_motion: bool,
}

impl Default for Motion {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Motion {
    pub fn new(reduced_motion: bool) -> Self {
        Self {
            ring_scale: Spring::new(if reduced_motion { 1.0 } else { 0.86 }, 420.0, 32.0),
            ring_alpha: Spring::new(if reduced_motion { 1.0 } else { 0.0 }, 380.0, 30.0),
            focus_angle: -std::f64::consts::FRAC_PI_2,
            focus_target: -std::f64::consts::FRAC_PI_2,
            focus_velocity: 0.0,
            focus_mix: Spring::new(1.0, 360.0, 32.0),
            center_blend: Spring::new(1.0, 320.0, 30.0),
            prev_center_title: String::new(),
            center_title: String::new(),
            prev_center_subtitle: String::new(),
            center_subtitle: String::new(),
            slots: std::array::from_fn(|_| Slot::default()),
            loading: LoadingPose::default(),
            empty: EmptyRing::default(),
            box_progress: Spring::new(0.0, 360.0, 34.0),
            color_hue: Spring::new(0.0, 340.0, 32.0),
            reduced_motion,
        }
    }

    pub fn open(&mut self) {
        self.ring_scale.target = 1.0;
        self.ring_alpha.target = 1.0;
        if self.reduced_motion {
            self.ring_scale.snap(1.0);
            self.ring_alpha.snap(1.0);
        }
    }

    pub fn close(&mut self) {
        self.ring_scale.target = 0.88;
        self.ring_alpha.target = 0.0;
    }

    pub fn set_center_text(&mut self, title: &str, subtitle: &str) {
        if self.center_title == title && self.center_subtitle == subtitle {
            return;
        }
        self.prev_center_title = std::mem::take(&mut self.center_title);
        self.prev_center_subtitle = std::mem::take(&mut self.center_subtitle);
        self.center_title = title.to_string();
        self.center_subtitle = subtitle.to_string();
        if self.reduced_motion {
            self.center_blend.snap(1.0);
        } else {
            self.center_blend.value = 0.0;
            self.center_blend.velocity = 0.0;
            self.center_blend.target = 1.0;
        }
    }

    pub fn set_focus_sector(&mut self, idx: Option<usize>, count: usize) {
        if let Some(i) = idx
            && count > 0
        {
            let target = sector_center_angle(i, count);
            // Snap direction cleanly across wrap-around using shortest arc
            let diff = shortest_angle_diff(self.focus_angle, target);
            self.focus_target = self.focus_angle + diff;
            self.focus_mix.target = 1.0;
            if self.reduced_motion {
                self.focus_angle = target;
                self.focus_target = target;
                self.focus_velocity = 0.0;
                self.focus_mix.snap(1.0);
            }
        } else {
            self.focus_mix.target = 0.0;
        }
    }

    pub fn retarget_ring_slots(&mut self, keys: &[String], item_radius: f64) {
        let count = keys.len().min(SLOT_COUNT);
        let mut prev_pos: HashMap<&str, (f64, f64, f64, f64)> = HashMap::new();
        for s in &self.slots {
            if s.active && !s.key.is_empty() {
                prev_pos.insert(
                    s.key.as_str(),
                    (s.x.value, s.y.value, s.alpha.value, s.scale.value),
                );
            }
        }

        let mut next_slots: [Slot; SLOT_COUNT] = std::array::from_fn(|_| Slot::default());
        for i in 0..count {
            let angle = sector_center_angle(i, count);
            let tx = angle.cos() * item_radius;
            let ty = angle.sin() * item_radius;
            let key = &keys[i];
            let mut slot = Slot {
                active: true,
                key: key.clone(),
                ..Default::default()
            };
            if let Some(&(px, py, pa, ps)) = prev_pos.get(key.as_str()) {
                slot.x.value = px;
                slot.y.value = py;
                slot.alpha.value = pa;
                slot.scale.value = ps;
            } else {
                slot.x.value = tx * 0.72;
                slot.y.value = ty * 0.72;
                slot.alpha.value = 0.0;
                slot.scale.value = 0.78;
            }
            slot.x.target = tx;
            slot.y.target = ty;
            slot.alpha.target = 1.0;
            slot.scale.target = 1.0;
            if self.reduced_motion {
                slot.x.snap(tx);
                slot.y.snap(ty);
                slot.alpha.snap(1.0);
                slot.scale.snap(1.0);
            }
            next_slots[i] = slot;
        }
        self.slots = next_slots;
    }

    pub fn step(&mut self, dt: f64) -> bool {
        let rm = self.reduced_motion;
        let mut active = false;
        active |= self.ring_scale.step(dt, rm);
        active |= self.ring_alpha.step(dt, rm);
        active |= self.focus_mix.step(dt, rm);
        active |= self.center_blend.step(dt, rm);
        active |= self.loading.step(dt, rm);
        active |= self.empty.step(dt, rm);
        active |= self.box_progress.step(dt, rm);
        active |= self.color_hue.step(dt, rm);

        // Focus angle spring (shortest angular path)
        if rm {
            self.focus_angle = self.focus_target;
            self.focus_velocity = 0.0;
        } else {
            let dt_c = dt.clamp(0.0, 0.05);
            let disp = self.focus_angle - self.focus_target;
            let accel = -460.0 * disp - 38.0 * self.focus_velocity;
            self.focus_velocity += accel * dt_c;
            self.focus_angle += self.focus_velocity * dt_c;
            if (self.focus_angle - self.focus_target).abs() < 0.001 && self.focus_velocity.abs() < 0.005 {
                self.focus_angle = self.focus_target;
                self.focus_velocity = 0.0;
            } else {
                active = true;
            }
        }

        for slot in &mut self.slots {
            if slot.active {
                active |= slot.x.step(dt, rm);
                active |= slot.y.step(dt, rm);
                active |= slot.alpha.step(dt, rm);
                active |= slot.scale.step(dt, rm);
            }
        }
        active
    }
}

pub fn sector_center_angle(index: usize, count: usize) -> f64 {
    if count <= 1 {
        return -std::f64::consts::FRAC_PI_2;
    }
    let step = TAU / count as f64;
    -std::f64::consts::FRAC_PI_2 + step * index as f64
}

pub fn shortest_angle_diff(from: f64, to: f64) -> f64 {
    let mut diff = (to - from) % TAU;
    if diff > std::f64::consts::PI {
        diff -= TAU;
    } else if diff < -std::f64::consts::PI {
        diff += TAU;
    }
    diff
}

/// Maps N overflow box tiles to their nearest visible ring sector index (0..ring_count)
/// evenly and with remainder balance, matching ReCircles' box<->ring morph transitions.
pub fn pair_grid_cells_to_ring(cell_count: usize, ring_count: usize) -> Vec<usize> {
    if cell_count == 0 || ring_count == 0 {
        return Vec::new();
    }
    (0..cell_count)
        .map(|i| (i * ring_count) / cell_count)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_reversal_and_reduced_motion_settle() {
        let mut s = Spring::new(0.0, 360.0, 32.0);
        s.target = 1.0;
        for _ in 0..8 {
            s.step(1.0 / 120.0, false);
        }
        let mid = s.value;
        assert!(mid > 0.05 && mid < 1.0);
        s.target = 0.0; // mid-flight reversal
        for _ in 0..180 {
            s.step(1.0 / 120.0, false);
        }
        assert!(s.is_settled());
        assert!((s.value - 0.0).abs() < 1e-3);

        s.target = 1.0;
        s.step(0.016, true);
        assert!(s.is_settled());
        assert_eq!(s.value, 1.0);
    }

    #[test]
    fn slot_retargeting_preserves_key_positions_and_short_arc() {
        let mut m = Motion::new(false);
        let keys = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        m.retarget_ring_slots(&keys, 123.5);
        for _ in 0..120 {
            m.step(1.0 / 120.0);
        }
        let prev_c = (m.slots[2].x.value, m.slots[2].y.value);
        m.retarget_ring_slots(&["c".into(), "d".into()], 123.5);
        // Existing item 'c' starts smoothly from its prior coordinates instead of popping from center!
        assert!((m.slots[0].x.value - prev_c.0).abs() < 0.01);
        assert!((m.slots[0].y.value - prev_c.1).abs() < 0.01);

        let diff = shortest_angle_diff(3.0, -3.0);
        assert!(diff.abs() < 0.4);
    }

    #[test]
    fn grid_pairing_distributes_evenly_with_remainder() {
        assert_eq!(pair_grid_cells_to_ring(12, 8), vec![0, 0, 1, 2, 2, 3, 4, 4, 5, 6, 6, 7]);
        assert_eq!(pair_grid_cells_to_ring(6, 6), vec![0, 1, 2, 3, 4, 5]);
    }
}
