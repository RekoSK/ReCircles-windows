//! 1:1 presentation state & fluid spring physics ported directly from Linux ReCircles.
//! Every transition starts from the current on-screen value, carries velocity across
//! mid-flight reversals, and uses closed-form springs independent of monitor refresh rate.
use recircles_core::menu::{Entry, GRID_COLUMNS, GRID_ROWS, LIST_ROWS};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

#[derive(Clone, Copy, Debug)]
struct Spring {
    value: f64,
    velocity: f64,
    goal: f64,
}

impl Default for Spring {
    fn default() -> Self {
        Self::at(0.0)
    }
}

impl Spring {
    fn at(value: f64) -> Self {
        Self {
            value,
            velocity: 0.0,
            goal: value,
        }
    }

    /// Closed-form under-damped spring (subtle physical pop), refresh-rate independent.
    fn advance(&mut self, dt: f64) -> bool {
        let offset = self.value - self.goal;
        let b = (self.velocity + 15.0 * offset) / 21.0;
        let (sin, cos) = (21.0 * dt).sin_cos();
        let decay = (-15.0 * dt).exp();
        let wave = offset * cos + b * sin;
        self.value = self.goal + decay * wave;
        self.velocity = decay * (21.0 * (b * cos - offset * sin) - 15.0 * wave);
        if (self.value - self.goal).abs() < 0.0005 && self.velocity.abs() < 0.01 {
            *self = Self::at(self.goal);
            return false;
        }
        true
    }

    /// Closed-form critically damped spring (damping = 1.0): zero overshoot,
    /// velocity-preserving on mid-flight reversals, independent of refresh rate.
    fn advance_critical(&mut self, dt: f64, omega: f64) -> bool {
        let offset = self.value - self.goal;
        let slope = self.velocity + omega * offset;
        let decay = (-omega * dt).exp();
        self.value = self.goal + decay * (offset + slope * dt);
        self.velocity = decay * (self.velocity - omega * slope * dt);
        if (self.value - self.goal).abs() < 0.0005 && self.velocity.abs() < 0.01 {
            *self = Self::at(self.goal);
            return false;
        }
        true
    }
}

fn ease(value: &mut f64, goal: f64, blend: f64) -> bool {
    *value += (goal - *value) * blend;
    if (*value - goal).abs() < 0.002 {
        *value = goal;
        return false;
    }
    true
}

#[derive(Clone)]
pub struct Face {
    pub entry: Entry,
    pub opacity: f64,
}

fn entry_matches(a: &Entry, b: &Entry) -> bool {
    a.key == b.key
        && a.title == b.title
        && a.icon == b.icon
        && a.icon_path == b.icon_path
        && a.exe_icon_path == b.exe_icon_path
        && a.accent_override == b.accent_override
        && a.active == b.active
}

#[derive(Default)]
pub struct RingSlot {
    pub faces: Vec<Face>,
    pub index: Option<usize>,
    last_index: usize,
    grid_index: usize,
    ring_angle: Option<f64>,
    ring_sector: Option<(f64, f64)>,
    ring_sibling: bool,
    merging_to_ring: bool,
    born_in_box: bool,
    width: f64,
    width_goal: f64,
    focus: Spring,
    pop: Spring,
}

impl RingSlot {
    fn matches(&self, entry: &Entry) -> bool {
        self.faces.last().is_some_and(|f| entry_matches(&f.entry, entry))
    }

    fn show(&mut self, index: usize, entry: Entry) {
        if !self.matches(&entry) {
            let face = self
                .faces
                .iter()
                .position(|f| entry_matches(&f.entry, &entry))
                .map(|i| self.faces.remove(i))
                .unwrap_or(Face {
                    entry,
                    opacity: 0.0,
                });
            self.faces.push(face);
            self.pop.velocity = -5.0;
        }
        self.index = Some(index);
        self.last_index = index;
    }

    pub fn focus(&self) -> f64 {
        self.focus.value
    }

    pub fn focused(&self) -> bool {
        self.focus.goal > 0.0
    }

    pub fn last_index(&self) -> usize {
        self.last_index
    }

    pub fn grid_index(&self) -> usize {
        self.grid_index
    }

    pub fn ring_angle(&self) -> Option<f64> {
        self.ring_angle
    }

    pub fn ring_sector(&self) -> Option<(f64, f64)> {
        self.ring_sector
    }

    pub fn ring_sibling(&self) -> bool {
        self.ring_sibling
    }

    pub fn merging_to_ring(&self) -> bool {
        self.merging_to_ring
    }

    pub fn born_in_box(&self) -> bool {
        self.born_in_box
    }

    pub fn tilt(&self) -> f64 {
        (self.pop.value + self.focus.velocity * 0.006).clamp(-0.16, 0.16)
    }

    fn advance(&mut self, layout_blend: f64, fade_blend: f64, dt: f64) -> bool {
        let active = self.index.is_some() || self.merging_to_ring;
        let mut moving = ease(&mut self.width, self.width_goal, layout_blend);
        let last = self.faces.len().saturating_sub(1);
        let mut i = 0;
        self.faces.retain_mut(|face| {
            let incoming = active && i == last;
            moving |= ease(&mut face.opacity, f64::from(incoming), fade_blend);
            i += 1;
            incoming || face.opacity > 0.0
        });
        moving | self.focus.advance_critical(dt, 30.0) | self.pop.advance_critical(dt, 30.0)
    }
}

pub struct RingSector<'a> {
    pub slot: &'a RingSlot,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct LoadingPose3D {
    pub rotation: [[f64; 3]; 3],
    pub depth: f64,
}

#[derive(Clone, Copy, Debug)]
struct Quat {
    w: f64,
    x: f64,
    y: f64,
    z: f64,
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Quat {
    const IDENTITY: Self = Self {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    fn from_axis_angle(axis: [f64; 3], angle: f64) -> Self {
        let (s, c) = (angle * 0.5).sin_cos();
        Self {
            w: c,
            x: axis[0] * s,
            y: axis[1] * s,
            z: axis[2] * s,
        }
    }

    fn from_omega(omega: [f64; 3], dt: f64) -> Self {
        let speed = (omega[0] * omega[0] + omega[1] * omega[1] + omega[2] * omega[2]).sqrt();
        let angle = speed * dt;
        if angle < 1e-9 {
            Self::IDENTITY
        } else {
            Self::from_axis_angle([omega[0] / speed, omega[1] / speed, omega[2] / speed], angle)
        }
    }

    fn mul(self, rhs: Self) -> Self {
        Self {
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
        }
    }

    fn normalize(self) -> Self {
        let norm = (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if norm > 0.0 {
            Self {
                w: self.w / norm,
                x: self.x / norm,
                y: self.y / norm,
                z: self.z / norm,
            }
        } else {
            Self::IDENTITY
        }
    }

    fn rotate_z(self) -> [f64; 3] {
        [
            2.0 * (self.x * self.z + self.w * self.y),
            2.0 * (self.y * self.z - self.w * self.x),
            1.0 - 2.0 * (self.x * self.x + self.y * self.y),
        ]
    }

    fn to_matrix(self) -> [[f64; 3]; 3] {
        let (w, x, y, z) = (self.w, self.x, self.y, self.z);
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - w * z),
                2.0 * (x * z + w * y),
            ],
            [
                2.0 * (x * y + w * z),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - w * x),
            ],
            [
                2.0 * (x * z - w * y),
                2.0 * (y * z + w * x),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ]
    }
}

struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        Self(0x9e37_79b9_7f4a_7c15)
    }
}

impl Rng {
    fn from_entropy() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        let mut x = nanos.wrapping_add(n.wrapping_mul(0x9e37_79b9_7f4a_7c15)) ^ 0xa076_1d64_78bd_642f;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^= x >> 31;
        Self(if x == 0 { 1 } else { x })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }
}

#[derive(Default)]
struct LoadingSpin {
    active: bool,
    pending: bool,
    orientation: Quat,
    omega: [f64; 3],
    omega_target: [f64; 3],
    azimuth: f64,
    drift: f64,
    xy_speed: f64,
    retarget_in: f64,
    settle_goal: Quat,
    settle_axis: [f64; 3],
    settle: Spring,
    depth: f64,
    rng: Rng,
}

impl LoadingSpin {
    fn is_idle(&self) -> bool {
        !self.active
            && self.depth == 0.0
            && (self.settle.value - self.settle.goal).abs() < 0.0005
            && self.settle.velocity.abs() < 0.01
    }

    fn is_requested(&self) -> bool {
        self.active || self.pending
    }

    fn engage(&mut self) {
        self.pending = false;
        if !self.active {
            self.active = true;
            self.rng = Rng::from_entropy();
            self.pick_target(true);
        }
    }

    fn pick_target(&mut self, initial: bool) {
        if initial {
            self.azimuth = self.rng.range(0.0, TAU);
        } else {
            let sign = if self.rng.next_u64() & 1 == 0 { 1.0 } else { -1.0 };
            self.azimuth = (self.azimuth + sign * self.rng.range(1.15, 2.75)).rem_euclid(TAU);
        }
        self.xy_speed = self.rng.range(2.6, 3.7);
        self.drift = self.rng.range(-1.3, 1.3);
        let wz = self.rng.range(-1.15, 1.15);
        self.omega_target = [
            self.xy_speed * self.azimuth.cos(),
            self.xy_speed * self.azimuth.sin(),
            wz,
        ];
        self.retarget_in = self.rng.range(0.50, 1.05);
    }

    fn set_active(&mut self, active: bool, animate: bool, defer: bool) -> bool {
        if !animate {
            *self = Self::default();
            return false;
        }
        if active {
            if defer && self.is_idle() {
                self.pending = true;
                return true;
            }
            self.engage();
            return true;
        }
        self.pending = false;
        if self.active {
            self.active = false;
            let n = self.orientation.rotate_z();
            let sign = if n[2] >= 0.0 { 1.0 } else { -1.0 };
            let ax = [n[1] * sign, -n[0] * sign, 0.0];
            let sin_a = ax[0].hypot(ax[1]);
            let cos_a = n[2] * sign;
            let angle = sin_a.atan2(cos_a);
            if sin_a > 1e-8 {
                let ux = ax[0] / sin_a;
                let uy = ax[1] / sin_a;
                let dq = Quat::from_axis_angle([ux, uy, 0.0], angle);
                self.settle_goal = dq.mul(self.orientation).normalize();
                self.settle_axis = [-ux, -uy, 0.0];
                let vel = (self.omega[0] * self.settle_axis[0]
                    + self.omega[1] * self.settle_axis[1])
                    .clamp(-4.0, 4.0);
                self.settle = Spring {
                    value: angle,
                    velocity: vel,
                    goal: 0.0,
                };
            } else {
                self.settle_goal = self.orientation;
                self.settle_axis = [1.0, 0.0, 0.0];
                self.settle = Spring::at(0.0);
            }
        }
        !self.is_idle()
    }

    fn advance(&mut self, dt: f64, panel_waiting: bool) -> bool {
        if self.active {
            if dt > 0.0 {
                let dt_spin = dt.min(0.05);
                self.retarget_in -= dt_spin;
                if self.retarget_in <= 0.0 {
                    self.pick_target(false);
                } else {
                    self.azimuth = (self.azimuth + self.drift * dt_spin).rem_euclid(TAU);
                    self.omega_target[0] = self.xy_speed * self.azimuth.cos();
                    self.omega_target[1] = self.xy_speed * self.azimuth.sin();
                }
                let steer = 1.0 - (-3.5 * dt_spin).exp();
                for i in 0..3 {
                    self.omega[i] += (self.omega_target[i] - self.omega[i]) * steer;
                }
                self.orientation = Quat::from_omega(self.omega, dt_spin)
                    .mul(self.orientation)
                    .normalize();
                let depth_blend = 1.0 - (-10.0 * dt_spin).exp();
                ease(&mut self.depth, 1.0, depth_blend);
            }
            return true;
        }
        if self.is_idle() {
            return self.pending;
        }
        let (moving, handoff) = if panel_waiting {
            let depth_blend = 1.0 - (-15.0 * dt).exp();
            let moving = self.settle.advance_critical(dt, 20.0)
                | ease(&mut self.depth, 0.0, depth_blend);
            let handoff = self.settle.value.abs() < 0.015 && self.depth < 0.015;
            (moving, handoff)
        } else {
            let depth_blend = 1.0 - (-14.0 * dt).exp();
            let moving = self.settle.advance(dt) | ease(&mut self.depth, 0.0, depth_blend);
            (moving, false)
        };
        if moving && !handoff {
            self.orientation = Quat::from_axis_angle(self.settle_axis, self.settle.value)
                .mul(self.settle_goal)
                .normalize();
            self.omega = [
                self.settle_axis[0] * self.settle.velocity,
                self.settle_axis[1] * self.settle.velocity,
                0.0,
            ];
            true
        } else {
            self.orientation = Quat::IDENTITY;
            self.omega = [0.0; 3];
            self.settle = Spring::at(0.0);
            self.depth = 0.0;
            false
        }
    }

    fn pose(&self) -> Option<LoadingPose3D> {
        if self.is_idle() {
            return None;
        }
        Some(LoadingPose3D {
            rotation: self.orientation.to_matrix(),
            depth: self.depth,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmptyRingState {
    pub mix: f64,
    pub pulse: f64,
    pub wave: f64,
}

#[derive(Default)]
struct EmptyPulse {
    active: bool,
    pulsing: bool,
    animated: bool,
    mix: Spring,
    pulse: Spring,
    phase: f64,
}

impl EmptyPulse {
    fn is_idle(&self) -> bool {
        !self.active
            && !self.pulsing
            && (self.mix.value - self.mix.goal).abs() < 0.0005
            && self.mix.velocity.abs() < 0.01
            && (self.pulse.value - self.pulse.goal).abs() < 0.0005
            && self.pulse.velocity.abs() < 0.01
    }

    fn set_active(&mut self, active: bool, pulsing: bool, animate: bool) -> bool {
        let pulsing = active && pulsing;
        self.active = active;
        self.pulsing = pulsing;
        self.animated = animate;
        self.mix.goal = f64::from(active);
        self.pulse.goal = f64::from(pulsing);
        if !animate {
            self.mix = Spring::at(self.mix.goal);
            self.pulse = Spring::at(0.0);
            self.phase = 0.0;
            return false;
        }
        if pulsing && self.pulse.value <= 0.001 && self.pulse.velocity.abs() < 0.01 {
            self.phase = 0.0;
        }
        !self.is_idle()
    }

    fn advance(&mut self, dt: f64) -> bool {
        if !self.animated {
            self.mix = Spring::at(self.mix.goal);
            self.pulse = Spring::at(0.0);
            self.phase = 0.0;
            return false;
        }
        if self.is_idle() {
            self.mix = Spring::at(self.mix.goal);
            self.pulse = Spring::at(self.pulse.goal);
            self.phase = 0.0;
            return false;
        }
        let mix_moving = self.mix.advance_critical(dt, 11.0);
        let pulse_moving = self.pulse.advance_critical(dt, 11.0);
        if dt > 0.0 && (self.pulsing || self.pulse.value > 0.0005) {
            self.phase = (self.phase + 3.8 * dt.min(0.05)).rem_euclid(TAU);
        } else if !self.pulsing && !pulse_moving {
            self.phase = 0.0;
        }
        self.pulsing || mix_moving || pulse_moving
    }

    fn state(&self) -> Option<EmptyRingState> {
        let mix = self.mix.value.clamp(0.0, 1.0);
        let pulse = self.pulse.value.clamp(0.0, 1.0);
        if mix <= 0.0005 && pulse <= 0.0005 && !self.active {
            return None;
        }
        let wave = if self.animated {
            -self.phase.sin() * pulse
        } else {
            0.0
        };
        Some(EmptyRingState { mix, pulse, wave })
    }
}

fn grid_cell_pos(index: usize) -> (f64, f64) {
    const CELL_W: f64 = 142.0;
    const CELL_H: f64 = 110.0;
    const LEFT: f64 = 14.0;
    const RIGHT_PAD: f64 = 20.0;
    const WIDTH: f64 = LEFT + GRID_COLUMNS as f64 * CELL_W + RIGHT_PAD;
    const GRID_H: f64 = CELL_H * GRID_ROWS as f64;
    const HEADER_PLAIN: f64 = 48.0;
    const FOOTER: f64 = 34.0;
    let gx = LEFT + (index % GRID_COLUMNS) as f64 * CELL_W + CELL_W * 0.5 - WIDTH * 0.5;
    let gy = HEADER_PLAIN + ((index / GRID_COLUMNS) as f64 + 0.5) * CELL_H
        - (HEADER_PLAIN + GRID_H + FOOTER) * 0.5;
    (gx, gy)
}

fn ring_item_pos(angle: f64) -> (f64, f64) {
    const R: f64 = 96.0;
    let (sin, cos) = angle.sin_cos();
    (R * cos, R * sin)
}

fn closest_cell_idx(grid_cells: &[usize], ring_angle: f64) -> usize {
    let (rx, ry) = ring_item_pos(ring_angle);
    grid_cells
        .iter()
        .enumerate()
        .min_by(|&(_, &a), &(_, &b)| {
            let (ax, ay) = grid_cell_pos(a);
            let (bx, by) = grid_cell_pos(b);
            (ax - rx)
                .hypot(ay - ry)
                .total_cmp(&(bx - rx).hypot(by - ry))
        })
        .map_or(0, |(idx, _)| idx)
}

fn pair_grid_cells_to_ring(
    grid_cells: &[usize],
    ring_angles: &[f64],
) -> (Vec<usize>, Vec<Option<usize>>) {
    let g_count = grid_cells.len();
    let r_count = ring_angles.len();
    if g_count == 0 || r_count == 0 {
        return (vec![0; g_count], vec![None; r_count]);
    }
    let dist: Vec<Vec<f64>> = grid_cells
        .iter()
        .map(|&cell| {
            let (gx, gy) = grid_cell_pos(cell);
            ring_angles
                .iter()
                .map(|&angle| {
                    let (rx, ry) = ring_item_pos(angle);
                    (gx - rx).hypot(gy - ry)
                })
                .collect()
        })
        .collect();

    let mut assigned: Vec<Option<usize>> = vec![None; g_count];
    let quota = g_count / r_count;
    if quota > 0 {
        for _ in 0..quota {
            for r_start in 0..r_count {
                let mut best_cost_r = vec![f64::INFINITY; r_count];
                let mut best_cost_g = vec![f64::INFINITY; g_count];
                let mut prev_r_of_g = vec![0usize; g_count];
                let mut prev_g_of_r = vec![0usize; r_count];
                best_cost_r[r_start] = 0.0;
                for _ in 0..=r_count {
                    let mut changed = false;
                    for r in 0..r_count {
                        let cr = best_cost_r[r];
                        if !cr.is_finite() {
                            continue;
                        }
                        for g in 0..g_count {
                            if assigned[g] == Some(r) {
                                continue;
                            }
                            let c = cr + dist[g][r];
                            if c + 1e-9 < best_cost_g[g] {
                                best_cost_g[g] = c;
                                prev_r_of_g[g] = r;
                                changed = true;
                                if let Some(r2) = assigned[g] {
                                    let c2 = c - dist[g][r2];
                                    if c2 + 1e-9 < best_cost_r[r2] {
                                        best_cost_r[r2] = c2;
                                        prev_g_of_r[r2] = g;
                                    }
                                }
                            }
                        }
                    }
                    if !changed {
                        break;
                    }
                }
                if let Some(mut g_cur) = (0..g_count)
                    .filter(|&g| assigned[g].is_none())
                    .min_by(|&a, &b| best_cost_g[a].total_cmp(&best_cost_g[b]))
                {
                    loop {
                        let r_cur = prev_r_of_g[g_cur];
                        let g_next = prev_g_of_r[r_cur];
                        assigned[g_cur] = Some(r_cur);
                        if r_cur == r_start {
                            break;
                        }
                        g_cur = g_next;
                    }
                }
            }
        }
    } else {
        let mut ring_owner: Vec<Option<usize>> = vec![None; r_count];
        for g_start in 0..g_count {
            let mut best_cost_g = vec![f64::INFINITY; g_count];
            let mut best_cost_r = vec![f64::INFINITY; r_count];
            let mut prev_g_of_r = vec![0usize; r_count];
            let mut prev_r_of_g = vec![0usize; g_count];
            best_cost_g[g_start] = 0.0;
            for _ in 0..=g_count {
                let mut changed = false;
                for g in 0..g_count {
                    let cg = best_cost_g[g];
                    if !cg.is_finite() {
                        continue;
                    }
                    for r in 0..r_count {
                        if ring_owner[r] == Some(g) {
                            continue;
                        }
                        let c = cg + dist[g][r];
                        if c + 1e-9 < best_cost_r[r] {
                            best_cost_r[r] = c;
                            prev_g_of_r[r] = g;
                            changed = true;
                            if let Some(g2) = ring_owner[r] {
                                let c2 = c - dist[g2][r];
                                if c2 + 1e-9 < best_cost_g[g2] {
                                    best_cost_g[g2] = c2;
                                    prev_r_of_g[g2] = r;
                                }
                            }
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            if let Some(mut r_cur) = (0..r_count)
                .filter(|&r| ring_owner[r].is_none())
                .min_by(|&a, &b| best_cost_r[a].total_cmp(&best_cost_r[b]))
            {
                loop {
                    let g_cur = prev_g_of_r[r_cur];
                    let r_next = prev_r_of_g[g_cur];
                    ring_owner[r_cur] = Some(g_cur);
                    assigned[g_cur] = Some(r_cur);
                    if g_cur == g_start {
                        break;
                    }
                    r_cur = r_next;
                }
            }
        }
    }

    let assignment: Vec<usize> = (0..g_count)
        .map(|g| {
            assigned[g].unwrap_or_else(|| {
                (0..r_count)
                    .min_by(|&a, &b| dist[g][a].total_cmp(&dist[g][b]))
                    .unwrap_or(0)
            })
        })
        .collect();

    let primary_idx: Vec<Option<usize>> = (0..r_count)
        .map(|r| {
            (0..g_count)
                .filter(|&g| assignment[g] == r)
                .min_by(|&a, &b| dist[a][r].total_cmp(&dist[b][r]))
        })
        .collect();

    (assignment, primary_idx)
}

pub struct RingMotion {
    slots: Vec<RingSlot>,
    seam: f64,
    seam_goal: f64,
    opening: Spring,
    alpha: Spring,
    box_mix: Spring,
    list_mix: Spring,
    input_mix: Spring,
    list_rows: Spring,
    input_focus: [Spring; 2],
    loading: LoadingSpin,
    empty: EmptyPulse,
    in_box_layout: bool,
    last_ring_sectors: Vec<(f64, f64)>,
}

impl Default for RingMotion {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            seam: 0.0,
            seam_goal: 0.0,
            opening: Spring::at(1.0),
            alpha: Spring::at(1.0),
            box_mix: Spring::at(0.0),
            list_mix: Spring::at(0.0),
            input_mix: Spring::at(0.0),
            list_rows: Spring::at(LIST_ROWS as f64),
            input_focus: [Spring::at(0.0); 2],
            loading: LoadingSpin::default(),
            empty: EmptyPulse::default(),
            in_box_layout: false,
            last_ring_sectors: Vec::new(),
        }
    }
}

impl RingMotion {
    pub fn open(&mut self, animate: bool) {
        if animate {
            self.opening = Spring {
                value: 0.91,
                velocity: 0.0,
                goal: 1.0,
            };
            self.alpha = Spring {
                value: 0.0,
                velocity: 0.0,
                goal: 1.0,
            };
        } else {
            self.opening = Spring::at(1.0);
            self.alpha = Spring::at(1.0);
        }
    }

    pub fn close(&mut self, animate: bool) {
        if animate {
            self.opening.goal = 0.92;
            self.alpha.goal = 0.0;
        } else {
            self.opening = Spring::at(0.92);
            self.alpha = Spring::at(0.0);
        }
    }

    pub fn scale(&self) -> f64 {
        self.opening.value
    }

    pub fn master_alpha(&self) -> f64 {
        self.alpha.value.clamp(0.0, 1.0)
    }

    pub fn set_box(&mut self, active: bool) {
        self.box_mix.goal = f64::from(active);
        if active {
            self.list_mix.goal = 0.0;
            self.input_mix.goal = 0.0;
        } else if self.list_mix.goal == 0.0 && self.input_mix.goal == 0.0 {
            for slot in &mut self.slots {
                slot.born_in_box = false;
            }
        }
    }

    pub fn box_mix(&self) -> f64 {
        self.box_mix.value
    }

    pub fn set_list(&mut self, active: bool, rows: usize) {
        self.list_mix.goal = f64::from(active);
        if active {
            self.box_mix.goal = 0.0;
            self.input_mix.goal = 0.0;
            let target_rows = rows.clamp(1, LIST_ROWS) as f64;
            if self.list_mix.value <= 0.001 {
                self.list_rows = Spring::at(target_rows);
            } else {
                self.list_rows.goal = target_rows;
            }
        } else if self.box_mix.goal == 0.0 && self.input_mix.goal == 0.0 {
            for slot in &mut self.slots {
                slot.born_in_box = false;
            }
        }
    }

    pub fn list_mix(&self) -> f64 {
        self.list_mix.value
    }

    pub fn list_rows(&self) -> f64 {
        self.list_rows.value.clamp(1.0, LIST_ROWS as f64)
    }

    pub fn list_rows_settled(&self) -> bool {
        (self.list_rows.value - self.list_rows.goal).abs() < 0.001
            && self.list_rows.velocity.abs() < 0.01
    }

    pub fn set_input(&mut self, active: bool) {
        self.input_mix.goal = f64::from(active);
        if active {
            self.box_mix.goal = 0.0;
            self.list_mix.goal = 0.0;
        } else {
            self.input_focus[0].goal = 0.0;
            self.input_focus[1].goal = 0.0;
            if self.box_mix.goal == 0.0 && self.list_mix.goal == 0.0 {
                for slot in &mut self.slots {
                    slot.born_in_box = false;
                }
            }
        }
    }

    pub fn input_mix(&self) -> f64 {
        self.input_mix.value
    }

    pub fn panel_mix(&self) -> f64 {
        (self.box_mix.value + self.list_mix.value + self.input_mix.value).clamp(0.0, 1.0)
    }

    pub fn select_input_button(&mut self, selected: Option<usize>) -> bool {
        let mut changed = false;
        for (i, spring) in self.input_focus.iter_mut().enumerate() {
            let goal = f64::from(selected == Some(i));
            changed |= spring.goal != goal;
            spring.goal = goal;
        }
        changed
    }

    pub fn input_button_focus(&self, index: usize) -> f64 {
        self.input_focus
            .get(index)
            .map_or(0.0, |s| s.value.clamp(0.0, 1.0))
    }

    pub fn set_loading(&mut self, loading: bool, animate: bool) -> bool {
        let defer = loading && self.panel_mix() > 0.003;
        self.loading.set_active(loading, animate, defer)
    }

    pub fn loading_pose(&self) -> Option<LoadingPose3D> {
        self.loading.pose()
    }

    pub fn set_empty(&mut self, empty: bool, pulsing: bool, animate: bool) -> bool {
        self.empty.set_active(empty, pulsing, animate)
    }

    pub fn empty_ring(&self) -> Option<EmptyRingState> {
        self.empty.state()
    }

    fn update_ring_to_box(&mut self, items: Vec<Entry>, selected: Option<usize>, animate: bool) -> bool {
        let count = items.len();
        let ring_spans = self.last_ring_sectors.clone();
        let ring_angles: Vec<f64> = ring_spans.iter().map(|&(s, e)| (s + e) * 0.5).collect();
        let grid_cells: Vec<usize> = (0..count).collect();
        let (assignment, primary_g) = pair_grid_cells_to_ring(&grid_cells, &ring_angles);
        let width = TAU / count as f64;
        let mut old_slots: Vec<Option<RingSlot>> = self.slots.drain(..).map(Some).collect();
        let was_box_or_merging = self.in_box_layout
            || old_slots.iter().flatten().any(|s| s.merging_to_ring);

        for (g, item) in items.into_iter().enumerate() {
            let r = assignment[g];
            let is_primary = primary_g[r] == Some(g);
            let existing = if was_box_or_merging {
                old_slots
                    .iter_mut()
                    .find(|s| s.as_ref().is_some_and(|slot| slot.grid_index == g))
                    .and_then(Option::take)
                    .or_else(|| {
                        is_primary
                            .then(|| {
                                old_slots
                                    .iter_mut()
                                    .find(|s| s.as_ref().is_some_and(|slot| slot.index == Some(r)))
                                    .and_then(Option::take)
                            })
                            .flatten()
                    })
            } else if is_primary {
                old_slots
                    .iter_mut()
                    .find(|s| s.as_ref().is_some_and(|slot| slot.index == Some(r)))
                    .and_then(Option::take)
            } else {
                None
            };

            let mut slot = if let Some(mut slot) = existing {
                slot.show(g, item);
                if !is_primary && slot.faces.len() == 1 {
                    slot.faces[0].opacity = 1.0;
                }
                slot
            } else if is_primary {
                let mut slot = RingSlot::default();
                slot.show(g, item);
                slot
            } else {
                RingSlot {
                    faces: vec![Face {
                        entry: item,
                        opacity: 1.0,
                    }],
                    index: Some(g),
                    last_index: g,
                    pop: Spring {
                        value: 0.0,
                        velocity: -5.0,
                        goal: 0.0,
                    },
                    ..RingSlot::default()
                }
            };
            slot.grid_index = g;
            slot.ring_angle = Some(ring_angles[r]);
            slot.ring_sector = is_primary.then_some(ring_spans[r]);
            slot.ring_sibling = !is_primary;
            slot.merging_to_ring = false;
            slot.born_in_box = false;
            slot.width = width;
            slot.width_goal = width;
            self.slots.push(slot);
        }

        self.seam_goal = -FRAC_PI_2 - PI / count as f64;
        self.seam = self.seam_goal;
        self.in_box_layout = true;
        self.select(selected);
        self.advance(if animate { 0.0 } else { 10.0 })
    }

    fn update_box_to_ring(&mut self, items: Vec<Entry>, selected: Option<usize>, animate: bool) -> bool {
        let count = items.len();
        let mut grid_slots: Vec<RingSlot> = self
            .slots
            .drain(..)
            .filter(|s| s.index.is_some() || s.merging_to_ring)
            .collect();
        grid_slots.sort_by_key(|s| s.grid_index);
        grid_slots.dedup_by_key(|s| s.grid_index);
        let grid_cells: Vec<usize> = grid_slots.iter().map(|s| s.grid_index).collect();

        self.seam_goal = -FRAC_PI_2 - PI / count as f64;
        self.seam = self.seam_goal;
        let width = TAU / count as f64;
        let ring_spans: Vec<(f64, f64)> = (0..count)
            .map(|r| {
                let start = self.seam_goal + r as f64 * width;
                (start, start + width)
            })
            .collect();
        let ring_angles: Vec<f64> = ring_spans.iter().map(|&(s, e)| (s + e) * 0.5).collect();
        self.last_ring_sectors = ring_spans.clone();

        let (assignment, primary_idx) = pair_grid_cells_to_ring(&grid_cells, &ring_angles);
        let mut old_grid_slots: Vec<Option<RingSlot>> = grid_slots.into_iter().map(Some).collect();

        for (r, item) in items.into_iter().enumerate() {
            let idx = primary_idx[r].unwrap_or_else(|| closest_cell_idx(&grid_cells, ring_angles[r]));
            let g_cell = grid_cells[idx];
            let mut slot = old_grid_slots[idx].take().unwrap_or_default();
            slot.show(r, item);
            slot.grid_index = g_cell;
            slot.ring_angle = Some(ring_angles[r]);
            slot.ring_sector = Some(ring_spans[r]);
            slot.ring_sibling = false;
            slot.merging_to_ring = false;
            slot.born_in_box = false;
            slot.width = width;
            slot.width_goal = width;
            self.slots.push(slot);
        }

        if animate && self.box_mix.value > 0.001 {
            for (idx, maybe_slot) in old_grid_slots.into_iter().enumerate() {
                if let Some(mut slot) = maybe_slot {
                    let r = assignment[idx];
                    slot.index = None;
                    slot.grid_index = grid_cells[idx];
                    slot.ring_angle = Some(ring_angles[r]);
                    slot.ring_sector = None;
                    slot.ring_sibling = true;
                    slot.merging_to_ring = true;
                    slot.born_in_box = false;
                    slot.width = 0.0;
                    slot.width_goal = 0.0;
                    slot.focus.goal = 0.0;
                    self.slots.push(slot);
                }
            }
        }

        self.in_box_layout = false;
        self.select(selected);
        self.advance(if animate { 0.0 } else { 10.0 })
    }

    pub fn update(&mut self, items: Vec<Entry>, selected: Option<usize>, animate: bool) -> bool {
        let count = items.len();
        let target_box = self.box_mix.goal > 0.5;
        let target_list = self.list_mix.goal > 0.5;
        let target_input = self.input_mix.goal > 0.5;
        let target_ring = !target_box && !target_list && !target_input;
        let from_other_panel = self.list_mix.value >= 0.5 || self.input_mix.value >= 0.5;

        if target_list || target_input {
            self.in_box_layout = false;
            self.last_ring_sectors.clear();
            self.slots.retain(|s| !s.merging_to_ring);
            for slot in &mut self.slots {
                slot.ring_angle = None;
                slot.ring_sector = None;
                slot.ring_sibling = false;
                slot.merging_to_ring = false;
            }
        }

        if target_box && !from_other_panel && count > 0 {
            let entering_from_ring = !self.in_box_layout
                || (self.box_mix.value < 0.85 && !self.last_ring_sectors.is_empty());
            if entering_from_ring {
                if !self.in_box_layout && !self.slots.iter().any(|s| s.merging_to_ring) {
                    let current_ring: Vec<(f64, f64)> = self
                        .sectors()
                        .filter(|s| s.slot.index.is_some())
                        .map(|s| (s.start, s.end))
                        .collect();
                    if !current_ring.is_empty() {
                        self.last_ring_sectors = current_ring;
                    }
                }
                if !self.last_ring_sectors.is_empty() {
                    return self.update_ring_to_box(items, selected, animate);
                }
            }
        }

        if target_ring && count > 0 && !self.slots.is_empty() {
            let leaving_box = self.in_box_layout
                || (self.box_mix.value > 0.001
                    && self
                        .slots
                        .iter()
                        .any(|s| s.merging_to_ring || s.ring_angle.is_some()));
            if leaving_box
                && self
                    .slots
                    .iter()
                    .any(|s| s.index.is_some() || s.merging_to_ring)
            {
                return self.update_box_to_ring(items, selected, animate);
            }
        }

        let leaving_box_to_empty =
            target_ring && count == 0 && (self.in_box_layout || self.box_mix.value > 0.001);
        self.in_box_layout = target_box && count > 0;
        self.slots.retain(|s| !s.merging_to_ring);
        if count > 0 {
            self.seam_goal = -FRAC_PI_2 - PI / count as f64;
        }
        if self.slots.is_empty() {
            self.seam = self.seam_goal;
        }

        let mut pos = 0;
        let expanding_box = target_box || target_list;
        for (i, item) in items.iter().enumerate() {
            if let Some(next) = self.slots[pos..].iter().position(|s| s.matches(item)) {
                for slot in &mut self.slots[pos..pos + next] {
                    slot.index = None;
                }
                pos += next;
            } else if self
                .slots
                .get(pos)
                .is_none_or(|s| items[i + 1..].iter().any(|v| s.matches(v)))
            {
                self.slots.insert(
                    pos,
                    RingSlot {
                        born_in_box: expanding_box,
                        ..RingSlot::default()
                    },
                );
            }
            self.slots[pos].show(i, item.clone());
            self.slots[pos].grid_index = i;
            if !target_box {
                self.slots[pos].ring_angle = None;
                self.slots[pos].ring_sector = None;
                self.slots[pos].ring_sibling = false;
            }
            pos += 1;
        }
        for slot in &mut self.slots[pos..] {
            if leaving_box_to_empty && slot.index.is_some() {
                slot.born_in_box = true;
            }
            slot.index = None;
        }
        let width = TAU / count.max(1) as f64;
        for slot in &mut self.slots {
            slot.width_goal = if slot.index.is_some() { width } else { 0.0 };
        }
        let mut displacement = 0.0;
        let mut weight = 0.0;
        for sector in self.sectors() {
            if let Some(i) = sector.slot.index {
                let opacity = sector.slot.faces.last().map_or(0.0, |f| f.opacity);
                let current = (sector.start + sector.end) * 0.5;
                let target = self.seam_goal + (i as f64 + 0.5) * width;
                displacement += opacity * (current - target);
                weight += opacity;
            }
        }
        if weight > 0.0 {
            self.seam_goal += TAU * (displacement / weight / TAU).round();
        } else if count > 0 {
            self.seam_goal += TAU * ((self.seam - self.seam_goal) / TAU).round();
        }
        if target_ring {
            if count > 0 {
                self.last_ring_sectors = (0..count)
                    .map(|r| {
                        let start = self.seam_goal + r as f64 * width;
                        (start, start + width)
                    })
                    .collect();
            } else if self.empty.active || self.loading.is_requested() || !animate {
                self.last_ring_sectors.clear();
            }
        }
        self.select(selected);
        self.advance(if animate { 0.0 } else { 10.0 })
    }

    pub fn select(&mut self, selected: Option<usize>) -> bool {
        let mut changed = false;
        for slot in &mut self.slots {
            let goal = f64::from(selected.is_some() && slot.index == selected);
            changed |= slot.focus.goal != goal;
            slot.focus.goal = goal;
        }
        changed
    }

    pub fn advance(&mut self, dt: f64) -> bool {
        let layout_blend = 1.0 - (-13.0 * dt).exp();
        let fade_blend = 1.0 - (-26.0 * dt).exp();
        let panel_wanted =
            self.box_mix.goal > 0.5 || self.list_mix.goal > 0.5 || self.input_mix.goal > 0.5;
        let mut loading_moving = self.loading.advance(dt, panel_wanted);
        let panel_moving = if self.loading.pose().is_some() {
            false
        } else {
            self.box_mix.advance_critical(dt, 18.0)
                | self.list_mix.advance_critical(dt, 18.0)
                | self.input_mix.advance_critical(dt, 18.0)
        };
        if self.loading.pending && self.panel_mix() <= 0.003 {
            self.box_mix = Spring::at(0.0);
            self.list_mix = Spring::at(0.0);
            self.input_mix = Spring::at(0.0);
            self.loading.engage();
            loading_moving |= self.loading.advance(dt, false);
        }
        let mut moving = self.opening.advance(dt)
            | self.alpha.advance_critical(dt, 28.0)
            | panel_moving
            | self.list_rows.advance_critical(dt, 22.0)
            | self.input_focus[0].advance_critical(dt, 30.0)
            | self.input_focus[1].advance_critical(dt, 30.0)
            | ease(&mut self.seam, self.seam_goal, layout_blend)
            | loading_moving
            | self.empty.advance(dt);
        for slot in &mut self.slots {
            moving |= slot.advance(layout_blend, fade_blend, dt);
        }
        let box_active = self.box_mix.value > 0.001;
        self.slots
            .retain(|s| s.index.is_some() || s.width > 0.0 || (box_active && s.merging_to_ring));
        if !box_active && self.box_mix.goal == 0.0 {
            for slot in &mut self.slots {
                slot.ring_angle = None;
                slot.ring_sector = None;
                slot.ring_sibling = false;
                slot.merging_to_ring = false;
                slot.grid_index = slot.last_index;
            }
        }
        moving
    }

    pub fn is_animating(&self) -> bool {
        let spring_active = |s: &Spring| {
            (s.value - s.goal).abs() >= 0.0005 || s.velocity.abs() >= 0.01
        };
        spring_active(&self.opening)
            || spring_active(&self.alpha)
            || spring_active(&self.box_mix)
            || spring_active(&self.list_mix)
            || spring_active(&self.input_mix)
            || spring_active(&self.list_rows)
            || spring_active(&self.input_focus[0])
            || spring_active(&self.input_focus[1])
            || (self.seam - self.seam_goal).abs() >= 0.002
            || !self.loading.is_idle()
            || self.loading.pending
            || !self.empty.is_idle()
            || self.slots.iter().any(|slot| {
                let active = slot.index.is_some() || slot.merging_to_ring;
                let last = slot.faces.len().saturating_sub(1);
                (slot.width - slot.width_goal).abs() >= 0.002
                    || spring_active(&slot.focus)
                    || spring_active(&slot.pop)
                    || slot.faces.iter().enumerate().any(|(i, f)| {
                        let goal = f64::from(active && i == last);
                        (f.opacity - goal).abs() >= 0.002
                    })
            })
    }

    pub fn sectors(&self) -> impl Iterator<Item = RingSector<'_>> {
        let total: f64 = self.slots.iter().map(|s| s.width).sum();
        let mut angle = self.seam;
        let box_active = self.box_mix.value > 0.001;
        self.slots
            .iter()
            .filter(move |s| s.width > 0.0 || (box_active && s.merging_to_ring))
            .map(move |slot| {
                let start = angle;
                if total > 0.0 {
                    angle += TAU * slot.width / total;
                }
                RingSector {
                    slot,
                    start,
                    end: angle,
                }
            })
    }

    pub fn hit(&self, angle: f64) -> Option<usize> {
        let angle = self.seam + (angle - self.seam).rem_euclid(TAU);
        let sector = self.sectors().find(|s| angle < s.end)?;
        sector
            .slot
            .index
            .filter(|_| sector.slot.faces.last().is_some_and(|f| f.opacity >= 0.5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recircles_core::menu::BuiltinIcon;

    fn entries(labels: &[&str]) -> Vec<Entry> {
        labels
            .iter()
            .map(|&l| Entry::simple(l, l, BuiltinIcon::Folder))
            .collect()
    }

    fn settle(m: &mut RingMotion) {
        for _ in 0..180 {
            if !m.advance(1.0 / 120.0) {
                return;
            }
        }
        panic!("RingMotion failed to settle within 1.5s");
    }

    #[test]
    fn ring_to_box_morph_reverses_mid_spring_without_jumping() {
        let mut m = RingMotion::default();
        m.update(entries(&["A", "B", "C", "D", "E", "F"]), None, false);
        m.set_box(true);
        m.update(
            entries(&["G0", "G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9", "G10", "G11"]),
            None,
            true,
        );
        m.advance(0.04);
        let mid = m.box_mix();
        assert!(mid > 0.05 && mid < 0.95);
        m.set_box(false);
        assert_eq!(m.box_mix(), mid);
        m.update(entries(&["A", "B", "C", "D", "E", "F"]), None, true);
        settle(&mut m);
        assert_eq!(m.box_mix(), 0.0);
        assert_eq!(m.sectors().count(), 6);
    }

    #[test]
    fn loading_3d_tumbles_on_multiple_axes_and_settles_flat() {
        let mut m = RingMotion::default();
        assert!(m.loading_pose().is_none());
        assert!(!m.set_loading(true, false));
        assert!(m.set_loading(true, true));
        let (mut max_nx, mut max_ny) = (0.0f64, 0.0f64);
        for _ in 0..150 {
            assert!(m.advance(1.0 / 60.0));
            let p = m.loading_pose().expect("loading pose");
            max_nx = max_nx.max(p.rotation[0][2].abs());
            max_ny = max_ny.max(p.rotation[1][2].abs());
        }
        assert!(max_nx > 0.2 && max_ny > 0.2);
        assert!(m.set_loading(false, true));
        settle(&mut m);
        assert!(m.loading_pose().is_none());
    }

    #[test]
    fn hit_aligns_with_animated_sectors_on_filter_narrowing() {
        let mut m = RingMotion::default();
        m.update(entries(&["A", "B", "C", "D", "E", "F", "G", "H"]), None, false);
        m.update(entries(&["H"]), None, true);
        for _ in 0..60 {
            m.advance(1.0 / 120.0);
            let mut span = 0.0;
            for sec in m.sectors() {
                span += sec.end - sec.start;
                let mid = (sec.start + sec.end) * 0.5;
                assert_eq!(m.hit(mid), sec.slot.index);
            }
            assert!((span - TAU).abs() < 1e-9);
        }
    }
}
