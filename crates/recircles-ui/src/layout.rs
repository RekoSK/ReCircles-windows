use crate::motion_ring::{RingMotion, RingSector};
use recircles_core::colors::Color;
use recircles_core::menu::{GRID_COLUMNS, GRID_ROWS, LIST_ROWS};
use recircles_core::settings::{ColorField, SliderField};
use std::f64::consts::TAU;

pub const OUTER: f64 = 172.0;
pub const INNER: f64 = 75.0;
pub const EMPTY_OUTER: f64 = 98.0;
pub const EMPTY_INNER: f64 = 56.0;
pub const EMPTY_OUTER_PULSE: f64 = 11.0;
pub const EMPTY_INNER_PULSE: f64 = 6.0;
pub const ITEM_RADIUS: f64 = (OUTER + INNER) / 2.0;

pub const SIDE: f64 = OUTER + 12.0;
pub const TOP: f64 = OUTER + 44.0;
pub const BOTTOM: f64 = OUTER + 82.0;

pub const PERSPECTIVE_D: f64 = 1050.0;
pub const RING_HALF_THICKNESS: f64 = 11.0;
pub const RING_SEGMENTS: usize = 64;

pub const CELL_W: f64 = 142.0;
pub const CELL_H: f64 = 110.0;
pub const GRID_LEFT: f64 = 14.0;
pub const GRID_RIGHT_PAD: f64 = 20.0;
pub const GRID_WIDTH: f64 = GRID_LEFT + GRID_COLUMNS as f64 * CELL_W + GRID_RIGHT_PAD;
pub const GRID_H: f64 = CELL_H * GRID_ROWS as f64;
pub const HEADER_PLAIN: f64 = 48.0;
pub const HEADER_QUERY: f64 = 80.0;
pub const GRID_FOOTER: f64 = 34.0;
pub const BOX_RADIUS: f64 = 20.0;

pub const LIST_W: f64 = 368.0;
pub const LIST_ROW_H: f64 = 42.0;
pub const LIST_FOOTER: f64 = 10.0;

pub const INPUT_W: f64 = 388.0;
pub const INPUT_H: f64 = 160.0;
pub const SLIDER_INPUT_H: f64 = 192.0;
pub const COLOR_INPUT_H: f64 = 380.0;
pub const INPUT_HEADER: f64 = 46.0;
pub const INPUT_FOOTER: f64 = 54.0;

// Legacy fallback constants kept for unit tests
#[allow(dead_code)]
pub const CANVAS_W: f64 = 640.0;
#[allow(dead_code)]
pub const CANVAS_H: f64 = 492.0;
#[allow(dead_code)]
pub const RING_CX: f64 = CANVAS_W * 0.5;
#[allow(dead_code)]
pub const RING_CY: f64 = TOP + 12.0;

#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[inline]
pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn rubberband(overshoot: f64, dimension: f64, constant: f64) -> f64 {
    if dimension <= 1e-6 {
        return 0.0;
    }
    (overshoot * dimension * constant) / (dimension + constant * overshoot.abs())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPanelKind {
    Text,
    Slider,
    ColorPicker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceMode {
    Ring,
    Grid,
    List,
    Input,
    Slider,
    ColorPicker,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[allow(dead_code)]
impl RectF {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

/// Exact 1:1 Linux `ring::Geometry`: clamps ring center to monitor margins around cursor anchor.
#[derive(Clone, Copy, Debug)]
pub struct RingGeometry {
    pub center_x: f64,
    pub center_y: f64,
    pub scale: f64,
}

impl RingGeometry {
    pub fn new(width: f64, height: f64, anchor_x: f64, anchor_y: f64) -> Self {
        let scale = (width / (2.0 * SIDE))
            .min(height / (TOP + BOTTOM))
            .clamp(0.0, 1.0);
        Self {
            center_x: anchor_x.max(SIDE * scale).min(width - SIDE * scale),
            center_y: anchor_y.max(TOP * scale).min(height - BOTTOM * scale),
            scale,
        }
    }

    #[allow(dead_code)]
    pub fn angle(&self, x: f64, y: f64, motion_scale: f64) -> Option<f64> {
        let scale = self.scale * motion_scale;
        if scale <= 0.0 {
            return None;
        }
        let dx = (x - self.center_x) / scale;
        let dy = (y - self.center_y) / scale;
        if !(INNER..=OUTER).contains(&dx.hypot(dy)) {
            return None;
        }
        Some(dy.atan2(dx))
    }

    pub fn angle_with_radii(
        &self,
        x: f64,
        y: f64,
        motion_scale: f64,
        inner: f64,
        outer: f64,
    ) -> Option<f64> {
        let scale = self.scale * motion_scale;
        if scale <= 0.0 {
            return None;
        }
        let dx = (x - self.center_x) / scale;
        let dy = (y - self.center_y) / scale;
        if !(inner..=outer).contains(&dx.hypot(dy)) {
            return None;
        }
        Some(dy.atan2(dx))
    }
}

/// Exact 1:1 Linux `ring::explorer::Bounds`: smoothly morphs circular ring glass into Grid, List,
/// Input, or ColorPicker using one critically damped spring.
#[derive(Clone, Copy, Debug)]
pub struct ExplorerBounds {
    pub cx: f64,
    pub cy: f64,
    pub scale: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
    pub hole: f64,
    pub header: f64,
    pub footer: f64,
    pub list_rows: f64,
    #[allow(dead_code)]
    pub list_rows_settled: bool,
    pub t: f64,
    pub box_t: f64,
    pub list_t: f64,
    pub input_t: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct TileGeom {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub emerge_alpha: f64,
    pub emerge_scale: f64,
}

impl TileGeom {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        (x - self.x).abs() <= self.width * 0.5 && (y - self.y).abs() <= self.height * 0.5
    }
}

impl ExplorerBounds {
    pub fn new(
        size: (f64, f64),
        anchor: (f64, f64),
        has_query: bool,
        motion: &RingMotion,
        input_kind: InputPanelKind,
    ) -> Self {
        let input_h = match input_kind {
            InputPanelKind::ColorPicker => COLOR_INPUT_H,
            InputPanelKind::Slider => SLIDER_INPUT_H,
            InputPanelKind::Text => INPUT_H,
        };
        let grid_header = if has_query {
            HEADER_QUERY
        } else {
            HEADER_PLAIN
        };
        let grid_h = grid_header + GRID_H + GRID_FOOTER;
        let list_rows = motion.list_rows();
        let list_h = grid_header + list_rows * LIST_ROW_H + LIST_FOOTER;

        let w_box = motion.box_mix().clamp(0.0, 1.0);
        let w_list = motion.list_mix().clamp(0.0, 1.0);
        let w_input = motion.input_mix().clamp(0.0, 1.0);
        let w_sum = w_box + w_list + w_input;
        let t = w_sum.clamp(0.0, 1.0);

        let (target_w, target_h, header, footer) = if w_sum > 1e-6 {
            (
                (w_box * GRID_WIDTH + w_list * LIST_W + w_input * INPUT_W) / w_sum,
                (w_box * grid_h + w_list * list_h + w_input * input_h) / w_sum,
                (w_box * grid_header + w_list * grid_header + w_input * INPUT_HEADER) / w_sum,
                (w_box * GRID_FOOTER + w_list * LIST_FOOTER + w_input * INPUT_FOOTER) / w_sum,
            )
        } else {
            (GRID_WIDTH, grid_h, grid_header, GRID_FOOTER)
        };

        let fit = (size.0 / (target_w + 20.0))
            .min(size.1 / (target_h + 20.0))
            .min(1.0);
        let center = |a: f64, extent: f64, screen: f64| {
            let margin = (extent * 0.5 + 8.0) * fit;
            a.max(margin).min(screen - margin)
        };
        let ring = RingGeometry::new(size.0, size.1, anchor.0, anchor.1);
        let scale = lerp(ring.scale, fit, t) * motion.scale();
        let extent = |from: f64, to: f64| {
            lerp(from * ring.scale, to * fit, t) / lerp(ring.scale, fit, t).max(f64::EPSILON)
        };
        let hole_t = (1.0 - (t / 0.52).min(1.0)).powi(2);
        let hole = INNER * hole_t;
        Self {
            cx: lerp(ring.center_x, center(anchor.0, target_w, size.0), t),
            cy: lerp(ring.center_y, center(anchor.1, target_h, size.1), t),
            width: extent(2.0 * OUTER, target_w),
            height: extent(2.0 * OUTER, target_h),
            radius: extent(OUTER, BOX_RADIUS),
            hole: if hole < 1.0 { 0.0 } else { hole },
            header,
            footer,
            list_rows,
            list_rows_settled: motion.list_rows_settled(),
            scale,
            t,
            box_t: w_box,
            list_t: w_list,
            input_t: w_input,
        }
    }

    pub fn local(&self, (x, y): (f64, f64)) -> (f64, f64) {
        if self.scale <= 1e-6 {
            return (1e9, 1e9);
        }
        ((x - self.cx) / self.scale, (y - self.cy) / self.scale)
    }

    pub fn inside(&self, x: f64, y: f64) -> bool {
        let dx = (x.abs() - (self.width * 0.5 - self.radius)).max(0.0);
        let dy = (y.abs() - (self.height * 0.5 - self.radius)).max(0.0);
        dx.hypot(dy) <= self.radius && x.hypot(y) >= self.hole
    }

    pub fn contains(&self, pos: (f64, f64)) -> bool {
        let (x, y) = self.local(pos);
        self.inside(x, y)
    }

    pub fn grid_top(&self) -> f64 {
        -self.height * 0.5 + self.header
    }

    pub fn tile(&self, sector: &RingSector<'_>) -> TileGeom {
        let index = sector.slot.grid_index();
        let gx = GRID_LEFT + (index % GRID_COLUMNS) as f64 * CELL_W + CELL_W * 0.5 - GRID_WIDTH * 0.5;
        let gy = self.header + ((index / GRID_COLUMNS) as f64 + 0.5) * CELL_H
            - (self.header + GRID_H + GRID_FOOTER) * 0.5;
        let (x, y, emerge_alpha, emerge_scale) = if sector.slot.born_in_box() {
            let expand = lerp(0.76, 1.0, self.box_t);
            (
                gx * expand,
                gy * expand,
                smoothstep(0.18, 0.62, self.box_t),
                lerp(0.86, 1.0, self.box_t),
            )
        } else {
            let angle = sector
                .slot
                .ring_angle()
                .unwrap_or((sector.start + sector.end) * 0.5);
            let (emerge_alpha, emerge_scale) = if sector.slot.ring_sibling() {
                let a = smoothstep(0.0, 0.22, self.box_t);
                (a, lerp(0.88, 1.0, a))
            } else {
                (1.0, 1.0)
            };
            (
                lerp(ITEM_RADIUS * angle.cos(), gx, self.box_t),
                lerp(ITEM_RADIUS * angle.sin(), gy, self.box_t),
                emerge_alpha,
                emerge_scale,
            )
        };
        TileGeom {
            x,
            y,
            width: lerp(84.0, CELL_W - 8.0, self.box_t),
            height: lerp(72.0, CELL_H - 8.0, self.box_t),
            emerge_alpha,
            emerge_scale,
        }
    }

    pub fn hit(&self, pos: (f64, f64), motion: &RingMotion) -> Option<usize> {
        let (x, y) = self.local(pos);
        if !self.inside(x, y) {
            return None;
        }
        if self.box_t < 0.12 {
            return motion.hit(y.atan2(x));
        }
        [true, false]
            .into_iter()
            .flat_map(|siblings| {
                motion
                    .sectors()
                    .filter(move |s| s.slot.ring_sibling() == siblings)
            })
            .filter_map(|sector| {
                let index = sector.slot.index?;
                let tile = self.tile(&sector);
                (tile.emerge_alpha >= 0.5
                    && tile.contains(x, y)
                    && sector.slot.faces.last()?.opacity >= 0.5)
                    .then_some(index)
            })
            .last()
    }

    pub fn thumb(&self, first: usize, total: usize) -> Option<(f64, f64)> {
        let rows = total.div_ceil(GRID_COLUMNS);
        if rows <= GRID_ROWS || self.box_t < 0.96 {
            return None;
        }
        let track_h = GRID_H - 8.0;
        let height = (track_h * GRID_ROWS as f64 / rows as f64)
            .max(26.0)
            .min(track_h);
        let ratio = ((first / GRID_COLUMNS) as f64 / (rows - GRID_ROWS) as f64).clamp(0.0, 1.0);
        let top = self.grid_top() + 4.0 + (track_h - height) * ratio;
        Some((top, height))
    }

    pub fn grab_thumb(&self, pos: (f64, f64), first: usize, total: usize) -> Option<f64> {
        let (top, height) = self.thumb(first, total)?;
        let (x, y) = self.local(pos);
        if !(self.width * 0.5 - 16.0..self.width * 0.5).contains(&x)
            || !(self.grid_top()..self.grid_top() + GRID_H).contains(&y)
        {
            return None;
        }
        let offset = y - top;
        Some(if (0.0..height).contains(&offset) {
            offset
        } else {
            height * 0.5
        })
    }

    pub fn dragged_first(&self, pos: (f64, f64), grab: f64, total: usize) -> usize {
        let Some((_, height)) = self.thumb(0, total) else {
            return 0;
        };
        let track_h = GRID_H - 8.0;
        let travel = track_h - height;
        if travel <= 1e-6 {
            return 0;
        }
        let fraction = ((self.local(pos).1 - (self.grid_top() + 4.0) - grab) / travel)
            .clamp(0.0, 1.0);
        (fraction * (total.div_ceil(GRID_COLUMNS) - GRID_ROWS) as f64).round() as usize
            * GRID_COLUMNS
    }

    pub fn list_body_height(&self) -> f64 {
        self.list_rows * LIST_ROW_H
    }

    pub fn list_row(&self, sector: &RingSector<'_>, has_scrollbar: bool) -> TileGeom {
        let index = sector.slot.last_index();
        let right_pad = if has_scrollbar { 20.0 } else { 14.0 };
        let row_w = LIST_W - 14.0 - right_pad;
        let rx = (14.0 - right_pad) * 0.5;
        let body_h = self.list_body_height();
        let ry = self.header + (index as f64 + 0.5) * LIST_ROW_H
            - (self.header + body_h + LIST_FOOTER) * 0.5;
        let (x, y, emerge_alpha, emerge_scale) = if sector.slot.born_in_box() {
            let expand = lerp(0.84, 1.0, self.list_t);
            (
                rx * expand,
                ry * expand,
                smoothstep(0.18, 0.62, self.list_t),
                lerp(0.90, 1.0, self.list_t),
            )
        } else {
            let angle = (sector.start + sector.end) * 0.5;
            (
                lerp(ITEM_RADIUS * angle.cos(), rx, self.list_t),
                lerp(ITEM_RADIUS * angle.sin(), ry, self.list_t),
                1.0,
                1.0,
            )
        };
        TileGeom {
            x,
            y,
            width: lerp(84.0, row_w, self.list_t),
            height: lerp(64.0, LIST_ROW_H - 4.0, self.list_t),
            emerge_alpha,
            emerge_scale,
        }
    }

    pub fn hit_list(&self, pos: (f64, f64), total: usize, motion: &RingMotion) -> Option<usize> {
        let (x, y) = self.local(pos);
        if !self.inside(x, y) {
            return None;
        }
        if self.list_t < 0.12 {
            return motion.hit(y.atan2(x));
        }
        let has_scrollbar = total > LIST_ROWS;
        motion
            .sectors()
            .filter_map(|sector| {
                let index = sector.slot.index?;
                let row = self.list_row(&sector, has_scrollbar);
                (row.emerge_alpha >= 0.5
                    && row.contains(x, y)
                    && sector.slot.faces.last()?.opacity >= 0.5)
                    .then_some(index)
            })
            .last()
    }

    pub fn list_thumb(&self, first: usize, total: usize) -> Option<(f64, f64)> {
        if total <= LIST_ROWS || self.list_t < 0.96 {
            return None;
        }
        let body_h = self.list_body_height();
        let track_h = (body_h - 8.0).max(24.0);
        let height = (track_h * LIST_ROWS as f64 / total as f64)
            .max(22.0)
            .min(track_h);
        let ratio = (first.min(total - LIST_ROWS) as f64 / (total - LIST_ROWS) as f64).clamp(0.0, 1.0);
        let top = self.grid_top() + 4.0 + (track_h - height) * ratio;
        Some((top, height))
    }

    pub fn grab_list_thumb(&self, pos: (f64, f64), first: usize, total: usize) -> Option<f64> {
        let (top, height) = self.list_thumb(first, total)?;
        let (x, y) = self.local(pos);
        let body_h = self.list_body_height();
        if !(self.width * 0.5 - 16.0..self.width * 0.5).contains(&x)
            || !(self.grid_top()..self.grid_top() + body_h).contains(&y)
        {
            return None;
        }
        let offset = y - top;
        Some(if (0.0..height).contains(&offset) {
            offset
        } else {
            height * 0.5
        })
    }

    pub fn dragged_list_first(&self, pos: (f64, f64), grab: f64, total: usize) -> usize {
        let Some((_, height)) = self.list_thumb(0, total) else {
            return 0;
        };
        let track_h = (self.list_body_height() - 8.0).max(24.0);
        let travel = track_h - height;
        if travel <= 1e-6 {
            return 0;
        }
        let fraction = ((self.local(pos).1 - (self.grid_top() + 4.0) - grab) / travel)
            .clamp(0.0, 1.0);
        (fraction * (total - LIST_ROWS) as f64).round() as usize
    }

    pub fn input_field_rect(&self) -> (f64, f64, f64, f64) {
        let top = -self.height * 0.5;
        let x = -self.width * 0.5 + 18.0;
        let w = (self.width - 36.0).max(40.0);
        let body_h = (self.height - self.header - self.footer).max(40.0);
        let h = 40.0;
        let y = top + self.header + (body_h - h) * 0.5;
        (x, y, w, h)
    }

    pub fn input_eye_rect(&self) -> (f64, f64, f64, f64) {
        let (fx, fy, fw, fh) = self.input_field_rect();
        (fx + fw - 44.0, fy + 4.0, 38.0, fh - 8.0)
    }

    pub fn color_sv_rect(&self) -> (f64, f64, f64, f64) {
        let top = -self.height * 0.5;
        let x = -self.width * 0.5 + 20.0;
        let w = (self.width - 40.0).max(80.0);
        let y = top + self.header + 10.0;
        (x, y, w, 172.0)
    }

    pub fn color_hue_rect(&self) -> (f64, f64, f64, f64) {
        let top = -self.height * 0.5;
        let x = -self.width * 0.5 + 20.0;
        let w = (self.width - 40.0).max(80.0);
        let y = top + self.header + 192.0;
        (x, y, w, 14.0)
    }

    pub fn color_hex_field_rect(&self) -> (f64, f64, f64, f64) {
        let top = -self.height * 0.5;
        let x = -self.width * 0.5 + 20.0;
        let w = (self.width - 40.0).max(40.0);
        let y = top + self.header + 216.0;
        (x, y, w, 34.0)
    }

    pub fn input_button_rects(&self) -> [(f64, f64, f64, f64); 2] {
        let bottom = self.height * 0.5;
        let y = bottom - 43.0;
        let h = 32.0;
        let inner_w = (self.width - 36.0).max(60.0);
        let gap = 12.0;
        let btn_w = (inner_w - gap) * 0.5;
        let x0 = -self.width * 0.5 + 18.0;
        let x1 = x0 + btn_w + gap;
        // 0 = primary (Done/Save), 1 = secondary (Cancel) — exact Linux order!
        [(x0, y, btn_w, h), (x1, y, btn_w, h)]
    }

    pub fn hit_input_button(&self, pos: (f64, f64)) -> Option<usize> {
        let (x, y) = self.local(pos);
        if !self.inside(x, y) || self.input_t < 0.45 {
            return None;
        }
        for (i, (bx, by, bw, bh)) in self.input_button_rects().into_iter().enumerate() {
            if x >= bx && x <= bx + bw && y >= by && y <= by + bh {
                return Some(i);
            }
        }
        None
    }

    pub fn hit_input_eye(&self, pos: (f64, f64)) -> bool {
        let (x, y) = self.local(pos);
        if !self.inside(x, y) || self.input_t < 0.45 {
            return false;
        }
        let (ex, ey, ew, eh) = self.input_eye_rect();
        x >= ex && x <= ex + ew && y >= ey && y <= ey + eh
    }

    pub fn hit_input_field(&self, pos: (f64, f64), is_color: bool) -> bool {
        let (x, y) = self.local(pos);
        if !self.inside(x, y) || self.input_t < 0.45 {
            return false;
        }
        let (fx, fy, fw, fh) = if is_color {
            self.color_hex_field_rect()
        } else {
            self.input_field_rect()
        };
        x >= fx && x <= fx + fw && y >= fy && y <= fy + fh
    }

    pub fn hit_color_sv(&self, pos: (f64, f64)) -> Option<(f64, f64)> {
        let (x, y) = self.local(pos);
        if !self.inside(x, y) || self.input_t < 0.45 {
            return None;
        }
        let (sx, sy, sw, sh) = self.color_sv_rect();
        if x >= sx - 8.0 && x <= sx + sw + 8.0 && y >= sy - 8.0 && y <= sy + sh + 8.0 {
            return Some((((x - sx) / sw).clamp(0.0, 1.0), (1.0 - (y - sy) / sh).clamp(0.0, 1.0)));
        }
        None
    }

    pub fn color_sv_at(&self, pos: (f64, f64)) -> (f64, f64) {
        let (x, y) = self.local(pos);
        let (sx, sy, sw, sh) = self.color_sv_rect();
        (
            ((x - sx) / sw).clamp(0.0, 1.0),
            (1.0 - (y - sy) / sh).clamp(0.0, 1.0),
        )
    }

    pub fn hit_color_hue(&self, pos: (f64, f64)) -> Option<f64> {
        let (x, y) = self.local(pos);
        if !self.inside(x, y) || self.input_t < 0.45 {
            return None;
        }
        let (hx, hy, hw, hh) = self.color_hue_rect();
        if x >= hx - 8.0 && x <= hx + hw + 8.0 && y >= hy - 8.0 && y <= hy + hh + 8.0 {
            return Some((((x - hx) / hw).clamp(0.0, 1.0) * 360.0).min(359.999));
        }
        None
    }

    pub fn color_hue_at(&self, pos: (f64, f64)) -> f64 {
        let (x, _) = self.local(pos);
        let (hx, _, hw, _) = self.color_hue_rect();
        (((x - hx) / hw).clamp(0.0, 1.0) * 360.0).min(359.999)
    }

    pub fn slider_track_rect(&self) -> (f64, f64, f64, f64) {
        let top = -self.height * 0.5;
        let x = -self.width * 0.5 + 24.0;
        let w = (self.width - 48.0).max(80.0);
        let body_top = top + self.header;
        let body_h = (self.height - self.header - self.footer).max(40.0);
        let h = 10.0;
        let y = body_top + body_h * 0.40 - h * 0.5;
        (x, y, w, h)
    }

    pub fn slider_thumb_center(&self, visual_value: f64, overshoot: f64) -> (f64, f64) {
        let (tx, ty, tw, th) = self.slider_track_rect();
        let frac = (visual_value / 100.0).clamp(0.0, 1.0);
        (tx + frac * tw + overshoot, ty + th * 0.5)
    }

    pub fn hit_slider_track(&self, pos: (f64, f64)) -> bool {
        let (x, y) = self.local(pos);
        if !self.inside(x, y) || self.input_t < 0.45 {
            return false;
        }
        let (tx, ty, tw, th) = self.slider_track_rect();
        x >= tx - 16.0 && x <= tx + tw + 16.0 && y >= ty - 18.0 && y <= ty + th + 18.0
    }

    pub fn slider_grab_offset(&self, pos: (f64, f64), visual_value: f64) -> f64 {
        let (x, _) = self.local(pos);
        let (thumb_x, _) = self.slider_thumb_center(visual_value, 0.0);
        let dx = x - thumb_x;
        if dx.abs() <= 16.0 { dx } else { 0.0 }
    }

    pub fn slider_drag_value(&self, pos: (f64, f64), grab_offset_x: f64) -> (u8, f64, f64) {
        let (x, _) = self.local(pos);
        let (tx, _, tw, _) = self.slider_track_rect();
        let raw_px = (x - grab_offset_x) - tx;
        let overshoot = if raw_px < 0.0 {
            rubberband(raw_px, 40.0, 0.55).clamp(-6.0, 0.0)
        } else if raw_px > tw {
            rubberband(raw_px - tw, 40.0, 0.55).clamp(0.0, 6.0)
        } else {
            0.0
        };
        let exact = (raw_px / tw * 100.0).clamp(0.0, 100.0);
        (exact.round() as u8, exact, overshoot)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingHit {
    Sector(usize),
    CenterHole,
    Outside,
}

#[allow(dead_code)]
pub fn hit_test_ring(px: f64, py: f64, visible_count: usize, outer_r: f64, inner_r: f64) -> RingHit {
    let dx = px - RING_CX;
    let dy = py - RING_CY;
    let r = (dx * dx + dy * dy).sqrt();
    if r < inner_r {
        return RingHit::CenterHole;
    }
    if r > outer_r {
        return RingHit::Outside;
    }
    if visible_count == 0 {
        return RingHit::Outside;
    }
    if visible_count == 1 {
        return RingHit::Sector(0);
    }
    let step = TAU / visible_count as f64;
    let start0 = -std::f64::consts::FRAC_PI_2 - step * 0.5;
    let angle = dy.atan2(dx);
    let rel = (angle - start0).rem_euclid(TAU);
    let idx = ((rel / step).floor() as usize).min(visible_count - 1);
    RingHit::Sector(idx)
}

#[derive(Debug, Clone)]
pub struct InputBoxState {
    pub prompt: String,
    pub placeholder: String,
    pub text: String,
    pub cursor: usize,
    pub select_all: bool,
    pub secret: bool,
    pub reveal_secret: bool,
    pub selected_button: Option<usize>,
    pub pressed_button: Option<usize>,
    pub error: Option<String>,
}

impl InputBoxState {
    pub fn new(prompt: String, placeholder: String, secret: bool) -> Self {
        Self {
            prompt,
            placeholder,
            text: String::new(),
            cursor: 0,
            select_all: false,
            secret,
            reveal_secret: !secret,
            selected_button: None,
            pressed_button: None,
            error: None,
        }
    }

    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map_or(self.text.len(), |(b, _)| b)
    }

    pub fn insert_char(&mut self, c: char) {
        self.error = None;
        if self.select_all {
            self.text.clear();
            self.cursor = 0;
            self.select_all = false;
        }
        let clamped = self.cursor.min(self.char_len());
        let byte_idx = self.char_to_byte(clamped);
        self.text.insert(byte_idx, c);
        self.cursor = clamped + 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        self.error = None;
        if self.select_all {
            self.text.clear();
            self.cursor = 0;
            self.select_all = false;
        }
        let clamped = self.cursor.min(self.char_len());
        let byte_idx = self.char_to_byte(clamped);
        self.text.insert_str(byte_idx, s);
        self.cursor = clamped + s.chars().count();
    }

    pub fn backspace(&mut self, whole_word: bool) {
        self.error = None;
        if self.select_all {
            self.text.clear();
            self.cursor = 0;
            self.select_all = false;
            return;
        }
        let clamped = self.cursor.min(self.char_len());
        if clamped == 0 {
            return;
        }
        let end_b = self.char_to_byte(clamped);
        let new_cursor = if whole_word {
            let prefix = &self.text[..end_b];
            let trimmed = prefix.trim_end();
            let cut_b = trimmed
                .rfind(|c: char| !c.is_alphanumeric())
                .map(|idx| idx + trimmed[idx..].chars().next().map_or(1, char::len_utf8))
                .unwrap_or(0);
            prefix[..cut_b].chars().count()
        } else {
            clamped - 1
        };
        let start_b = self.char_to_byte(new_cursor);
        self.text.replace_range(start_b..end_b, "");
        self.cursor = new_cursor;
    }

    pub fn delete_forward(&mut self) {
        self.error = None;
        if self.select_all {
            self.text.clear();
            self.cursor = 0;
            self.select_all = false;
            return;
        }
        let len = self.char_len();
        let clamped = self.cursor.min(len);
        if clamped >= len {
            return;
        }
        let start_b = self.char_to_byte(clamped);
        let end_b = self.char_to_byte(clamped + 1);
        self.text.replace_range(start_b..end_b, "");
        self.cursor = clamped;
    }

    pub fn move_left(&mut self) {
        if self.select_all {
            self.cursor = 0;
            self.select_all = false;
        } else {
            self.cursor = self.cursor.min(self.char_len()).saturating_sub(1);
        }
    }

    pub fn move_right(&mut self) {
        let len = self.char_len();
        if self.select_all {
            self.cursor = len;
            self.select_all = false;
        } else {
            self.cursor = (self.cursor + 1).min(len);
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
        self.select_all = false;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.char_len();
        self.select_all = false;
    }

    pub fn select_all_text(&mut self) {
        if !self.text.is_empty() {
            self.select_all = true;
            self.cursor = self.char_len();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDrag {
    None,
    Sv,
    Hue,
}

#[derive(Debug, Clone)]
pub struct ColorPickerState {
    pub field: ColorField,
    pub h: f64,
    pub s: f64,
    pub v: f64,
    pub a: f64,
    pub hex_input: String,
    pub cursor: usize,
    pub select_all: bool,
    pub dragging: ColorDrag,
    pub selected_button: Option<usize>,
    pub pressed_button: Option<usize>,
    pub error: Option<String>,
}

impl ColorPickerState {
    pub fn new(field: ColorField, initial_hex: &str) -> Self {
        let color = Color::from_hex(initial_hex).unwrap_or_else(|_| Color::rgb(0.5, 0.7, 1.0));
        let (h, s, v) = rgb_to_hsv(color.r, color.g, color.b);
        let hex_input = color.to_hex();
        let cursor = hex_input.chars().count();
        Self {
            field,
            h,
            s,
            v,
            a: color.a,
            hex_input,
            cursor,
            select_all: false,
            dragging: ColorDrag::None,
            selected_button: None,
            pressed_button: None,
            error: None,
        }
    }

    pub fn color(&self) -> Color {
        let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
        Color::rgba(r, g, b, self.a)
    }

    pub fn hue_rgb(&self) -> (f64, f64, f64) {
        hsv_to_rgb(self.h, 1.0, 1.0)
    }

    pub fn char_len(&self) -> usize {
        self.hex_input.chars().count()
    }

    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        self.hex_input
            .char_indices()
            .nth(char_idx)
            .map_or(self.hex_input.len(), |(b, _)| b)
    }

    pub fn sync_hex(&mut self) {
        self.hex_input = self.color().to_hex();
        self.cursor = self.char_len();
        self.select_all = false;
        self.error = None;
    }

    pub fn is_hex_valid(&self) -> bool {
        Color::from_hex(&self.hex_input).is_ok()
    }

    pub fn try_apply_hex_input(&mut self) -> bool {
        if let Ok(c) = Color::from_hex(&self.hex_input) {
            let (h, s, v) = rgb_to_hsv(c.r, c.g, c.b);
            if s > 1e-4 {
                self.h = h;
            }
            self.s = s;
            self.v = v;
            self.a = c.a;
            self.error = None;
            true
        } else {
            false
        }
    }

    pub fn insert_hex_char(&mut self, c: char) -> bool {
        if !(c.is_ascii_hexdigit() || c == '#') {
            return false;
        }
        if self.select_all {
            self.hex_input.clear();
            self.cursor = 0;
            self.select_all = false;
        }
        let clamped = self.cursor.min(self.char_len());
        if c == '#' && (clamped != 0 || self.hex_input.contains('#')) {
            return false;
        }
        if self.char_len() >= 9 {
            return false;
        }
        let byte_idx = self.char_to_byte(clamped);
        self.hex_input.insert(byte_idx, c);
        self.cursor = clamped + 1;
        self.try_apply_hex_input();
        true
    }

    pub fn paste_hex(&mut self, raw: &str) -> bool {
        let trimmed = raw
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim();
        if trimmed.is_empty() {
            return false;
        }
        let (has_prefix, digits) = if let Some(rest) = trimmed.strip_prefix('#') {
            (true, rest)
        } else if let Some(rest) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
            (true, rest)
        } else {
            (false, trimmed)
        };
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
        if digits.is_empty() {
            if has_prefix && trimmed == "#" {
                if self.select_all {
                    self.hex_input = "#".into();
                    self.cursor = 1;
                    self.select_all = false;
                    return true;
                }
                return self.insert_hex_char('#');
            }
            return false;
        }
        if digits.len() > 8 {
            return false;
        }
        let full_replace = self.select_all || has_prefix || digits.len() == 6 || digits.len() == 8;
        if full_replace {
            self.hex_input = format!("#{}", digits.to_ascii_lowercase());
            self.cursor = self.char_len();
            self.select_all = false;
            self.try_apply_hex_input();
            return true;
        }
        let mut changed = false;
        for c in digits.chars() {
            if self.insert_hex_char(c) {
                changed = true;
            }
        }
        changed
    }

    pub fn backspace(&mut self, whole_line: bool) -> bool {
        if self.select_all || whole_line {
            let had = !self.hex_input.is_empty() || self.select_all;
            self.hex_input.clear();
            self.cursor = 0;
            self.select_all = false;
            return had;
        }
        let clamped = self.cursor.min(self.char_len());
        if clamped == 0 {
            return false;
        }
        let start_b = self.char_to_byte(clamped - 1);
        let end_b = self.char_to_byte(clamped);
        self.hex_input.replace_range(start_b..end_b, "");
        self.cursor = clamped - 1;
        self.try_apply_hex_input();
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        if self.select_all {
            self.hex_input.clear();
            self.cursor = 0;
            self.select_all = false;
            return true;
        }
        let len = self.char_len();
        let clamped = self.cursor.min(len);
        if clamped >= len {
            return false;
        }
        let start_b = self.char_to_byte(clamped);
        let end_b = self.char_to_byte(clamped + 1);
        self.hex_input.replace_range(start_b..end_b, "");
        self.cursor = clamped;
        self.try_apply_hex_input();
        true
    }

    pub fn move_left(&mut self) {
        if self.select_all {
            self.cursor = 0;
            self.select_all = false;
        } else {
            self.cursor = self.cursor.min(self.char_len()).saturating_sub(1);
        }
    }

    pub fn move_right(&mut self) {
        let len = self.char_len();
        if self.select_all {
            self.cursor = len;
            self.select_all = false;
        } else {
            self.cursor = (self.cursor + 1).min(len);
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
        self.select_all = false;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.char_len();
        self.select_all = false;
    }

    pub fn select_all_text(&mut self) {
        if !self.hex_input.is_empty() {
            self.select_all = true;
            self.cursor = self.char_len();
        }
    }
}

#[derive(Debug, Clone)]
pub struct SliderState {
    pub field: SliderField,
    pub value: u8,
    pub visual_value: f64,
    pub visual_vel: f64,
    pub overshoot: f64,
    pub overshoot_vel: f64,
    pub thumb_scale: f64,
    pub thumb_scale_vel: f64,
    pub dragging: bool,
    pub hovered: bool,
    pub grab_offset_x: f64,
    pub selected_button: Option<usize>,
    pub pressed_button: Option<usize>,
}

fn step_critical_spring(value: &mut f64, vel: &mut f64, goal: f64, dt: f64, omega: f64) -> bool {
    let offset = *value - goal;
    let slope = *vel + omega * offset;
    let decay = (-omega * dt).exp();
    *value = goal + decay * (offset + slope * dt);
    *vel = decay * (*vel - omega * slope * dt);
    if (*value - goal).abs() < 0.001 && vel.abs() < 0.01 {
        *value = goal;
        *vel = 0.0;
        return false;
    }
    true
}

impl SliderState {
    pub fn new(field: SliderField, initial_value: u8) -> Self {
        let v = initial_value.min(100);
        Self {
            field,
            value: v,
            visual_value: f64::from(v),
            visual_vel: 0.0,
            overshoot: 0.0,
            overshoot_vel: 0.0,
            thumb_scale: 1.0,
            thumb_scale_vel: 0.0,
            dragging: false,
            hovered: false,
            grab_offset_x: 0.0,
            selected_button: None,
            pressed_button: None,
        }
    }

    /// 1:1 direct pointer tracking during drag (zero latency).
    pub fn set_dragged(&mut self, value: u8, exact: f64, overshoot: f64, reduced_motion: bool) {
        self.value = value.min(100);
        self.visual_value = exact.clamp(0.0, 100.0);
        self.visual_vel = 0.0;
        self.overshoot = if reduced_motion { 0.0 } else { overshoot };
        self.overshoot_vel = 0.0;
    }

    /// Discrete keyboard or wheel step — animates smoothly with critically damped spring.
    pub fn set_stepped(&mut self, value: u8, reduced_motion: bool) {
        let v = value.min(100);
        self.value = v;
        if reduced_motion {
            self.visual_value = f64::from(v);
            self.visual_vel = 0.0;
            self.overshoot = 0.0;
            self.overshoot_vel = 0.0;
        }
    }

    pub fn step_by(&mut self, delta: i16, reduced_motion: bool) -> bool {
        let next = (i16::from(self.value) + delta).clamp(0, 100) as u8;
        if next == self.value {
            return false;
        }
        self.set_stepped(next, reduced_motion);
        true
    }

    pub fn end_drag(&mut self, reduced_motion: bool) {
        self.dragging = false;
        self.grab_offset_x = 0.0;
        if reduced_motion {
            self.visual_value = f64::from(self.value);
            self.visual_vel = 0.0;
            self.overshoot = 0.0;
            self.overshoot_vel = 0.0;
            self.thumb_scale = if self.hovered { 1.08 } else { 1.0 };
            self.thumb_scale_vel = 0.0;
        }
    }

    fn thumb_scale_goal(&self) -> f64 {
        if self.dragging {
            1.18
        } else if self.hovered {
            1.08
        } else {
            1.0
        }
    }

    pub fn advance(&mut self, dt: f64, reduced_motion: bool) -> bool {
        let scale_goal = self.thumb_scale_goal();
        if reduced_motion {
            if !self.dragging {
                self.visual_value = f64::from(self.value);
                self.visual_vel = 0.0;
                self.overshoot = 0.0;
                self.overshoot_vel = 0.0;
            }
            self.thumb_scale = scale_goal;
            self.thumb_scale_vel = 0.0;
            return false;
        }
        let mut active = false;
        if !self.dragging {
            active |= step_critical_spring(
                &mut self.visual_value,
                &mut self.visual_vel,
                f64::from(self.value),
                dt,
                30.0,
            );
            active |= step_critical_spring(&mut self.overshoot, &mut self.overshoot_vel, 0.0, dt, 26.0);
        }
        active |= step_critical_spring(
            &mut self.thumb_scale,
            &mut self.thumb_scale_vel,
            scale_goal,
            dt,
            34.0,
        );
        active
    }

    pub fn is_animating(&self) -> bool {
        let scale_goal = self.thumb_scale_goal();
        let val_moving = !self.dragging
            && ((self.visual_value - f64::from(self.value)).abs() > 0.001
                || self.visual_vel.abs() > 0.01);
        let over_moving = !self.dragging && (self.overshoot.abs() > 0.001 || self.overshoot_vel.abs() > 0.01);
        let scale_moving =
            (self.thumb_scale - scale_goal).abs() > 0.001 || self.thumb_scale_vel.abs() > 0.01;
        val_moving || over_moving || scale_moving
    }
}

pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (r1 + m, g1 + m, b1 + m)
}

pub fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if (max - g).abs() < 1e-6 {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    let s = if max < 1e-6 { 0.0 } else { d / max };
    (h, s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_ring_identifies_center_hole_sectors_and_outside() {
        assert_eq!(hit_test_ring(RING_CX, RING_CY, 8, OUTER, INNER), RingHit::CenterHole);
        assert_eq!(hit_test_ring(RING_CX, RING_CY - ITEM_RADIUS, 8, OUTER, INNER), RingHit::Sector(0));
        assert_eq!(hit_test_ring(RING_CX + ITEM_RADIUS, RING_CY, 8, OUTER, INNER), RingHit::Sector(2));
        assert_eq!(hit_test_ring(RING_CX, RING_CY - OUTER - 10.0, 8, OUTER, INNER), RingHit::Outside);
    }

    #[test]
    fn hsv_rgb_roundtrip_is_accurate() {
        for (r, g, b) in [(0.9, 0.2, 0.3), (0.1, 0.8, 0.4), (0.4, 0.5, 0.95)] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(h, s, v);
            assert!((r - r2).abs() < 1e-5);
            assert!((g - g2).abs() < 1e-5);
            assert!((b - b2).abs() < 1e-5);
        }
    }

    #[test]
    fn ring_geometry_clamps_to_screen_margins() {
        let g = RingGeometry::new(1536.0, 864.0, 10.0, 860.0);
        assert_eq!(g.scale, 1.0);
        assert_eq!(g.center_x, SIDE);
        assert_eq!(g.center_y, 864.0 - BOTTOM);
        assert_eq!(g.angle(g.center_x, g.center_y, 1.0), None);
        assert!(g.angle(g.center_x, g.center_y - ITEM_RADIUS, 1.0).is_some());
    }

    #[test]
    fn grid_scrollbar_thumb_stays_strictly_inside_track_for_13_to_23_and_last_page() {
        let mut motion = RingMotion::default();
        motion.open(false);
        motion.set_box(true);
        motion.advance(5.0);
        let b = ExplorerBounds::new((1536.0, 864.0), (768.0, 432.0), false, &motion, InputPanelKind::Text);
        assert!(b.box_t >= 0.99);

        let track_top = b.grid_top() + 4.0;
        let track_bot = track_top + (GRID_H - 8.0);

        // <= 12 items: no scrollbar
        assert_eq!(b.thumb(0, 9), None);
        assert_eq!(b.thumb(0, 12), None);

        for total in (13usize..=23).chain([24, 37, 111, 400]) {
            let max_first = (total.div_ceil(GRID_COLUMNS) - GRID_ROWS) * GRID_COLUMNS;
            for first in [0, GRID_COLUMNS, max_first, max_first + 8, total + 12] {
                let (top, h) = b.thumb(first, total).expect("thumb expected for >12 items");
                assert!(
                    top >= track_top - 1e-6 && top + h <= track_bot + 1e-6,
                    "total={total} first={first}: thumb [{top:.2}, {:.2}] overflowed track [{track_top:.2}, {track_bot:.2}]",
                    top + h
                );
            }
            // Exact top and bottom endpoints at 0 and max_first
            let (top0, _) = b.thumb(0, total).unwrap();
            let (top_end, h_end) = b.thumb(max_first, total).unwrap();
            assert!((top0 - track_top).abs() < 1e-6);
            assert!((top_end + h_end - track_bot).abs() < 1e-6);

            // Dragging to top/bottom lands cleanly at 0 and max_first
            let top_pos = (b.cx + (b.width * 0.5 - 8.0) * b.scale, b.cy + track_top * b.scale);
            let bot_pos = (b.cx + (b.width * 0.5 - 8.0) * b.scale, b.cy + track_bot * b.scale);
            assert_eq!(b.dragged_first(top_pos, 0.0, total), 0);
            assert_eq!(b.dragged_first(bot_pos, 0.0, total), max_first);
        }
    }

    #[test]
    fn color_picker_state_supports_select_all_cursor_and_hex_editing() {
        let mut cp = ColorPickerState::new(ColorField::Ring, "#26160de8");
        assert_eq!(cp.hex_input, "#26160de8");
        assert_eq!(cp.cursor, 9);
        assert!(!cp.select_all);

        // At 9 chars without select_all, extra hex digits are ignored
        assert!(!cp.insert_hex_char('f'));
        assert_eq!(cp.hex_input, "#26160de8");

        // Ctrl+A then typing a brand-new color replaces the full 9-char value
        cp.select_all_text();
        assert!(cp.select_all);
        for c in "#ff3366".chars() {
            assert!(cp.insert_hex_char(c));
        }
        assert_eq!(cp.hex_input, "#ff3366");
        assert!(!cp.select_all);
        assert_eq!(cp.cursor, 7);
        assert_eq!(cp.color().to_hex(), "#ff3366");

        // Home, Right, Delete, insert mid-string
        cp.move_home();
        assert_eq!(cp.cursor, 0);
        cp.move_right(); // after '#'
        assert_eq!(cp.cursor, 1);
        assert!(cp.delete_forward()); // "#f3366"
        assert!(cp.delete_forward()); // "#3366"
        assert!(cp.insert_hex_char('0'));
        assert!(cp.insert_hex_char('0'));
        assert_eq!(cp.hex_input, "#003366");
        assert_eq!(cp.color().to_hex(), "#003366");

        // Left + BackSpace
        cp.move_end();
        assert_eq!(cp.cursor, 7);
        cp.move_left();
        assert!(cp.backspace(false)); // deletes first '6' -> "#00336"
        assert!(cp.insert_hex_char('a')); // "#0033a6"
        assert_eq!(cp.hex_input, "#0033a6");
        assert_eq!(cp.color().to_hex(), "#0033a6");

        // Arbitrary non-hex prose paste must be rejected without corrupting hex_input
        assert!(!cp.paste_hex("hello world"));
        assert!(!cp.paste_hex("Background color"));
        assert!(!cp.paste_hex("rgb(10, 20, 30)"));
        assert!(!cp.paste_hex("#1122334455"));
        assert_eq!(cp.hex_input, "#0033a6");

        // Clean whitespace / 0x / 8-digit RGBA hex paste works
        assert!(cp.paste_hex("  0x336699  "));
        assert_eq!(cp.hex_input, "#336699");
        assert!(cp.paste_hex(" \"#112233cc\" "));
        assert_eq!(cp.hex_input, "#112233cc");
        assert_eq!(cp.color().to_hex(), "#112233cc");

        // Grayscale hex preserves hue instead of snapping hue slider back to red (0 deg)
        let prev_hue = cp.h;
        assert!(prev_hue > 150.0);
        assert!(cp.paste_hex("#ffffff"));
        assert!((cp.h - prev_hue).abs() < 1e-6);
        assert_eq!(cp.s, 0.0);
        assert_eq!(cp.v, 1.0);
    }

    #[test]
    fn slider_state_tracks_1_to_1_rubberbands_and_springs_cleanly() {
        let mut s = SliderState::new(SliderField::Translucency, 65);
        assert_eq!(s.value, 65);
        assert!((s.visual_value - 65.0).abs() < 1e-6);

        assert!(s.step_by(10, false));
        assert_eq!(s.value, 75);
        assert!(s.is_animating());
        for _ in 0..80 {
            s.advance(1.0 / 60.0, false);
        }
        assert!((s.visual_value - 75.0).abs() < 0.05);

        // Rubber-band resists overshoot progressively at boundaries
        let r1 = rubberband(20.0, 40.0, 0.55);
        let r2 = rubberband(200.0, 40.0, 0.55);
        assert!(r1 > 0.0 && r2 > r1 && r2 < 40.0);

        s.dragging = true;
        s.set_dragged(100, 100.0, 4.5, false);
        assert_eq!(s.value, 100);
        assert!(s.overshoot > 4.0);
        s.end_drag(false);
        for _ in 0..80 {
            s.advance(1.0 / 60.0, false);
        }
        assert!(s.overshoot.abs() < 0.01);
    }
}
