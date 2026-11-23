#[cfg(windows)]
use recircles_core::colors::Color;
#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use windows_sys::Win32::Foundation::HWND;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct D2dColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl D2dColor {
    pub const fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self {
            r: r as f32,
            g: g as f32,
            b: b as f32,
            a: a as f32,
        }
    }

    pub const fn from_rgb(rgb: (f64, f64, f64), a: f64) -> Self {
        Self {
            r: rgb.0 as f32,
            g: rgb.1 as f32,
            b: rgb.2 as f32,
            a: a as f32,
        }
    }
}

#[cfg(windows)]
impl From<Color> for D2dColor {
    fn from(c: Color) -> Self {
        Self {
            r: c.r as f32,
            g: c.g as f32,
            b: c.b as f32,
            a: c.a as f32,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct D2dGradStop {
    pub pos: f32,
    pub color: D2dColor,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct D2dPathCmd {
    pub op: u32,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub x3: f32,
    pub y3: f32,
}

pub struct PathBuilder {
    pub cmds: Vec<D2dPathCmd>,
}

impl PathBuilder {
    pub fn new() -> Self {
        Self {
            cmds: Vec::with_capacity(24),
        }
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.cmds.push(D2dPathCmd {
            op: 0,
            x1: x as f32,
            y1: y as f32,
            x2: 0.0,
            y2: 0.0,
            x3: 0.0,
            y3: 0.0,
        });
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        self.cmds.push(D2dPathCmd {
            op: 1,
            x1: x as f32,
            y1: y as f32,
            x2: 0.0,
            y2: 0.0,
            x3: 0.0,
            y3: 0.0,
        });
    }

    pub fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        self.cmds.push(D2dPathCmd {
            op: 2,
            x1: x1 as f32,
            y1: y1 as f32,
            x2: x2 as f32,
            y2: y2 as f32,
            x3: x3 as f32,
            y3: y3 as f32,
        });
    }

    pub fn arc(&mut self, cx: f64, cy: f64, r: f64, start_rad: f64, end_rad: f64) {
        self.cmds.push(D2dPathCmd {
            op: 3,
            x1: cx as f32,
            y1: cy as f32,
            x2: r as f32,
            y2: start_rad as f32,
            x3: end_rad as f32,
            y3: 0.0,
        });
    }

    pub fn close(&mut self) {
        self.cmds.push(D2dPathCmd {
            op: 4,
            x1: 0.0,
            y1: 0.0,
            x2: 0.0,
            y2: 0.0,
            x3: 0.0,
            y3: 0.0,
        });
    }

    pub fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.cmds.push(D2dPathCmd {
            op: 5,
            x1: x as f32,
            y1: y as f32,
            x2: w as f32,
            y2: h as f32,
            x3: r as f32,
            y3: 0.0,
        });
    }
}

#[cfg(windows)]
#[allow(dead_code)]
unsafe extern "C" {
    fn d2d_create(hwnd: HWND, phys_w: i32, phys_h: i32, logical_scale: f32) -> *mut c_void;
    fn d2d_recreate(ctx: *mut c_void) -> i32;
    fn d2d_destroy(ctx: *mut c_void);
    fn d2d_capture_backdrop(
        ctx: *mut c_void,
        screen_x: i32,
        screen_y: i32,
        phys_w: i32,
        phys_h: i32,
        blur_sigma_px: f32,
    ) -> i32;
    fn d2d_set_backdrop_blur(ctx: *mut c_void, blur_sigma_px: f32, saturation: f32) -> i32;
    fn d2d_set_backdrop_origin(ctx: *mut c_void, origin_x_px: f32, origin_y_px: f32);
    fn d2d_has_backdrop(ctx: *mut c_void) -> i32;
    fn d2d_fill_rounded_rect_backdrop(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        hole_r: f32,
        alpha: f32,
    );
    fn d2d_resize(ctx: *mut c_void, phys_w: i32, phys_h: i32, logical_scale: f32);
    fn d2d_begin_frame(ctx: *mut c_void, logical_scale: f32);
    fn d2d_end_frame(ctx: *mut c_void, sync_interval: i32) -> i32;

    fn d2d_save(ctx: *mut c_void);
    fn d2d_restore(ctx: *mut c_void);
    fn d2d_translate(ctx: *mut c_void, tx: f32, ty: f32);
    fn d2d_scale(ctx: *mut c_void, sx: f32, sy: f32);
    fn d2d_rotate(ctx: *mut c_void, radians: f32);

    fn d2d_push_clip_rect(ctx: *mut c_void, x: f32, y: f32, w: f32, h: f32);
    fn d2d_pop_clip_rect(ctx: *mut c_void);
    fn d2d_push_opacity_layer(ctx: *mut c_void, x: f32, y: f32, w: f32, h: f32, opacity: f32);
    fn d2d_pop_layer(ctx: *mut c_void);

    fn d2d_fill_top_sheen(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        sheen_h: f32,
        top_col: D2dColor,
    );

    fn d2d_fill_annulus_shadow(
        ctx: *mut c_void,
        cy_off: f32,
        outer_r: f32,
        spread: f32,
        shadow_col: D2dColor,
    );
    fn d2d_fill_annulus_linear(
        ctx: *mut c_void,
        inner_r: f32,
        outer_r: f32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        stops: *const D2dGradStop,
        count: i32,
    );
    fn d2d_stroke_annulus_rim(
        ctx: *mut c_void,
        inner_r: f32,
        outer_r: f32,
        line_w: f32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        top_col: D2dColor,
        bot_col: D2dColor,
    );
    fn d2d_fill_sector(
        ctx: *mut c_void,
        inner_r: f32,
        outer_r: f32,
        start_rad: f32,
        end_rad: f32,
        fill_col: D2dColor,
    );
    fn d2d_stroke_arc(
        ctx: *mut c_void,
        cx: f32,
        cy: f32,
        r: f32,
        start_rad: f32,
        end_rad: f32,
        line_w: f32,
        col: D2dColor,
        round_cap: i32,
    );
    fn d2d_draw_line(
        ctx: *mut c_void,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        line_w: f32,
        col: D2dColor,
        round_cap: i32,
    );
    fn d2d_draw_line_gradient(
        ctx: *mut c_void,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        line_w: f32,
        stops: *const D2dGradStop,
        count: i32,
    );

    fn d2d_fill_rounded_rect_solid(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        col: D2dColor,
    );
    fn d2d_fill_rounded_rect_linear(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        hole_r: f32,
        gx0: f32,
        gy0: f32,
        gx1: f32,
        gy1: f32,
        stops: *const D2dGradStop,
        count: i32,
    );
    fn d2d_stroke_rounded_rect(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        line_w: f32,
        col: D2dColor,
    );
    fn d2d_stroke_rounded_rect_linear(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        line_w: f32,
        gx0: f32,
        gy0: f32,
        gx1: f32,
        gy1: f32,
        c0: D2dColor,
        c1: D2dColor,
    );
    fn d2d_fill_rounded_rect_shadow(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        shadow_col: D2dColor,
        alpha_scale: f32,
    );
    fn d2d_fill_circle(ctx: *mut c_void, cx: f32, cy: f32, r: f32, col: D2dColor);
    fn d2d_stroke_circle(ctx: *mut c_void, cx: f32, cy: f32, r: f32, line_w: f32, col: D2dColor);
    fn d2d_fill_polygon(ctx: *mut c_void, xy: *const f32, count: i32, col: D2dColor);
    fn d2d_stroke_polygon(
        ctx: *mut c_void,
        xy: *const f32,
        count: i32,
        line_w: f32,
        col: D2dColor,
    );
    fn d2d_fill_polygon_ring_linear(
        ctx: *mut c_void,
        outer_xy: *const f32,
        outer_n: i32,
        inner_xy: *const f32,
        inner_n: i32,
        gx0: f32,
        gy0: f32,
        gx1: f32,
        gy1: f32,
        stops: *const D2dGradStop,
        count: i32,
    );
    fn d2d_stroke_polygon_ring_linear(
        ctx: *mut c_void,
        outer_xy: *const f32,
        outer_n: i32,
        inner_xy: *const f32,
        inner_n: i32,
        line_w: f32,
        gx0: f32,
        gy0: f32,
        gx1: f32,
        gy1: f32,
        c0: D2dColor,
        c1: D2dColor,
    );

    fn d2d_draw_path(
        ctx: *mut c_void,
        cmds: *const D2dPathCmd,
        count: i32,
        do_fill: i32,
        fill_col: D2dColor,
        do_stroke: i32,
        stroke_col: D2dColor,
        stroke_w: f32,
        round_cap: i32,
    );

    fn d2d_measure_text(
        ctx: *mut c_void,
        text: *const u16,
        len: i32,
        size: f32,
        bold: i32,
        max_w: f32,
        max_lines: i32,
        out_w: *mut f32,
        out_h: *mut f32,
    );
    fn d2d_hit_test_text_pos(
        ctx: *mut c_void,
        text: *const u16,
        len: i32,
        size: f32,
        bold: i32,
        utf16_pos: i32,
    ) -> f32;
    fn d2d_draw_text(
        ctx: *mut c_void,
        text: *const u16,
        len: i32,
        size: f32,
        bold: i32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        align: i32,
        ellipsize: i32,
        wrap_lines: i32,
        col: D2dColor,
        has_shadow: i32,
        shadow_col: D2dColor,
    );
}

#[cfg(windows)]
pub struct D2dCanvas {
    ptr: *mut c_void,
    utf16: Vec<u16>,
}

#[cfg(windows)]
#[allow(dead_code)]
impl D2dCanvas {
    pub fn new(hwnd: HWND, phys_w: i32, phys_h: i32, logical_scale: f64) -> Option<Self> {
        let ptr = unsafe { d2d_create(hwnd, phys_w, phys_h, logical_scale as f32) };
        if ptr.is_null() {
            None
        } else {
            Some(Self {
                ptr,
                utf16: Vec::with_capacity(256),
            })
        }
    }

    pub fn capture_backdrop(
        &mut self,
        screen_x: i32,
        screen_y: i32,
        phys_w: i32,
        phys_h: i32,
        blur_sigma_px: f64,
    ) -> bool {
        unsafe {
            d2d_capture_backdrop(
                self.ptr,
                screen_x,
                screen_y,
                phys_w,
                phys_h,
                blur_sigma_px as f32,
            ) != 0
        }
    }

    pub fn set_backdrop_blur(&mut self, blur_sigma_px: f64, saturation: f64) -> bool {
        unsafe { d2d_set_backdrop_blur(self.ptr, blur_sigma_px as f32, saturation as f32) != 0 }
    }

    pub fn set_backdrop_origin(&self, origin_x_px: f64, origin_y_px: f64) {
        unsafe { d2d_set_backdrop_origin(self.ptr, origin_x_px as f32, origin_y_px as f32) };
    }

    pub fn has_backdrop(&self) -> bool {
        unsafe { d2d_has_backdrop(self.ptr) != 0 }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_rounded_rect_backdrop(
        &self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        hole_r: f64,
        alpha: f64,
    ) {
        unsafe {
            d2d_fill_rounded_rect_backdrop(
                self.ptr,
                x as f32,
                y as f32,
                w as f32,
                h as f32,
                r as f32,
                hole_r as f32,
                alpha as f32,
            )
        };
    }

    pub fn resize(&mut self, phys_w: i32, phys_h: i32, logical_scale: f64) {
        unsafe { d2d_resize(self.ptr, phys_w, phys_h, logical_scale as f32) };
    }

    pub fn recreate(&mut self) -> bool {
        unsafe { d2d_recreate(self.ptr) != 0 }
    }

    pub fn begin_frame(&mut self, logical_scale: f64) {
        unsafe { d2d_begin_frame(self.ptr, logical_scale as f32) };
    }

    pub fn end_frame(&mut self, sync_interval: i32) -> bool {
        unsafe { d2d_end_frame(self.ptr, sync_interval) != 0 }
    }

    pub fn save(&self) {
        unsafe { d2d_save(self.ptr) };
    }

    pub fn restore(&self) {
        unsafe { d2d_restore(self.ptr) };
    }

    pub fn translate(&self, tx: f64, ty: f64) {
        unsafe { d2d_translate(self.ptr, tx as f32, ty as f32) };
    }

    pub fn scale(&self, sx: f64, sy: f64) {
        unsafe { d2d_scale(self.ptr, sx as f32, sy as f32) };
    }

    pub fn rotate(&self, radians: f64) {
        unsafe { d2d_rotate(self.ptr, radians as f32) };
    }

    pub fn push_clip_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        unsafe { d2d_push_clip_rect(self.ptr, x as f32, y as f32, w as f32, h as f32) };
    }

    pub fn pop_clip_rect(&self) {
        unsafe { d2d_pop_clip_rect(self.ptr) };
    }

    pub fn push_opacity_layer(&self, x: f64, y: f64, w: f64, h: f64, opacity: f64) {
        unsafe {
            d2d_push_opacity_layer(
                self.ptr,
                x as f32,
                y as f32,
                w as f32,
                h as f32,
                opacity as f32,
            )
        };
    }

    pub fn pop_layer(&self) {
        unsafe { d2d_pop_layer(self.ptr) };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_top_sheen(
        &self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        sheen_h: f64,
        top_col: D2dColor,
    ) {
        unsafe {
            d2d_fill_top_sheen(
                self.ptr,
                x as f32,
                y as f32,
                w as f32,
                h as f32,
                r as f32,
                sheen_h as f32,
                top_col,
            )
        };
    }

    pub fn fill_annulus_shadow(&self, cy_off: f64, outer_r: f64, spread: f64, col: D2dColor) {
        unsafe {
            d2d_fill_annulus_shadow(self.ptr, cy_off as f32, outer_r as f32, spread as f32, col)
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_annulus_linear(
        &self,
        inner_r: f64,
        outer_r: f64,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        stops: &[D2dGradStop],
    ) {
        unsafe {
            d2d_fill_annulus_linear(
                self.ptr,
                inner_r as f32,
                outer_r as f32,
                x0 as f32,
                y0 as f32,
                x1 as f32,
                y1 as f32,
                stops.as_ptr(),
                stops.len() as i32,
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_annulus_rim(
        &self,
        inner_r: f64,
        outer_r: f64,
        line_w: f64,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        top_col: D2dColor,
        bot_col: D2dColor,
    ) {
        unsafe {
            d2d_stroke_annulus_rim(
                self.ptr,
                inner_r as f32,
                outer_r as f32,
                line_w as f32,
                x0 as f32,
                y0 as f32,
                x1 as f32,
                y1 as f32,
                top_col,
                bot_col,
            )
        };
    }

    pub fn fill_sector(
        &self,
        inner_r: f64,
        outer_r: f64,
        start_rad: f64,
        end_rad: f64,
        col: D2dColor,
    ) {
        unsafe {
            d2d_fill_sector(
                self.ptr,
                inner_r as f32,
                outer_r as f32,
                start_rad as f32,
                end_rad as f32,
                col,
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_arc(
        &self,
        cx: f64,
        cy: f64,
        r: f64,
        start_rad: f64,
        end_rad: f64,
        line_w: f64,
        col: D2dColor,
        round_cap: bool,
    ) {
        unsafe {
            d2d_stroke_arc(
                self.ptr,
                cx as f32,
                cy as f32,
                r as f32,
                start_rad as f32,
                end_rad as f32,
                line_w as f32,
                col,
                if round_cap { 1 } else { 0 },
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_line(
        &self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        line_w: f64,
        col: D2dColor,
        round_cap: bool,
    ) {
        unsafe {
            d2d_draw_line(
                self.ptr,
                x0 as f32,
                y0 as f32,
                x1 as f32,
                y1 as f32,
                line_w as f32,
                col,
                if round_cap { 1 } else { 0 },
            )
        };
    }

    pub fn draw_line_gradient(
        &self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        line_w: f64,
        stops: &[D2dGradStop],
    ) {
        unsafe {
            d2d_draw_line_gradient(
                self.ptr,
                x0 as f32,
                y0 as f32,
                x1 as f32,
                y1 as f32,
                line_w as f32,
                stops.as_ptr(),
                stops.len() as i32,
            )
        };
    }

    pub fn fill_rounded_rect_solid(
        &self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        col: D2dColor,
    ) {
        unsafe {
            d2d_fill_rounded_rect_solid(
                self.ptr, x as f32, y as f32, w as f32, h as f32, r as f32, col,
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_rounded_rect_linear(
        &self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        gx0: f64,
        gy0: f64,
        gx1: f64,
        gy1: f64,
        stops: &[D2dGradStop],
    ) {
        unsafe {
            d2d_fill_rounded_rect_linear(
                self.ptr,
                x as f32,
                y as f32,
                w as f32,
                h as f32,
                r as f32,
                0.0,
                gx0 as f32,
                gy0 as f32,
                gx1 as f32,
                gy1 as f32,
                stops.as_ptr(),
                stops.len() as i32,
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rounded_rect(
        &self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        line_w: f64,
        col: D2dColor,
    ) {
        unsafe {
            d2d_stroke_rounded_rect(
                self.ptr,
                x as f32,
                y as f32,
                w as f32,
                h as f32,
                r as f32,
                line_w as f32,
                col,
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rounded_rect_linear(
        &self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        line_w: f64,
        gx0: f64,
        gy0: f64,
        gx1: f64,
        gy1: f64,
        c0: D2dColor,
        c1: D2dColor,
    ) {
        unsafe {
            d2d_stroke_rounded_rect_linear(
                self.ptr,
                x as f32,
                y as f32,
                w as f32,
                h as f32,
                r as f32,
                line_w as f32,
                gx0 as f32,
                gy0 as f32,
                gx1 as f32,
                gy1 as f32,
                c0,
                c1,
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_rounded_rect_shadow(
        &self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        shadow_col: D2dColor,
        alpha_scale: f64,
    ) {
        unsafe {
            d2d_fill_rounded_rect_shadow(
                self.ptr,
                x as f32,
                y as f32,
                w as f32,
                h as f32,
                r as f32,
                shadow_col,
                alpha_scale as f32,
            )
        };
    }

    pub fn fill_circle(&self, cx: f64, cy: f64, r: f64, col: D2dColor) {
        unsafe { d2d_fill_circle(self.ptr, cx as f32, cy as f32, r as f32, col) };
    }

    pub fn stroke_circle(&self, cx: f64, cy: f64, r: f64, line_w: f64, col: D2dColor) {
        unsafe { d2d_stroke_circle(self.ptr, cx as f32, cy as f32, r as f32, line_w as f32, col) };
    }

    pub fn fill_polygon(&self, pts: &[(f64, f64)], col: D2dColor) {
        if pts.len() <= 16 {
            let mut stack = [0.0f32; 32];
            for (i, &(x, y)) in pts.iter().enumerate() {
                stack[i * 2] = x as f32;
                stack[i * 2 + 1] = y as f32;
            }
            unsafe { d2d_fill_polygon(self.ptr, stack.as_ptr(), pts.len() as i32, col) };
            return;
        }
        let mut flat = Vec::with_capacity(pts.len() * 2);
        for &(x, y) in pts {
            flat.push(x as f32);
            flat.push(y as f32);
        }
        unsafe { d2d_fill_polygon(self.ptr, flat.as_ptr(), pts.len() as i32, col) };
    }

    pub fn stroke_polygon(&self, pts: &[(f64, f64)], line_w: f64, col: D2dColor) {
        if pts.len() <= 64 {
            let mut stack = [0.0f32; 128];
            for (i, &(x, y)) in pts.iter().enumerate() {
                stack[i * 2] = x as f32;
                stack[i * 2 + 1] = y as f32;
            }
            unsafe {
                d2d_stroke_polygon(
                    self.ptr,
                    stack.as_ptr(),
                    pts.len() as i32,
                    line_w as f32,
                    col,
                )
            };
            return;
        }
        let mut flat = Vec::with_capacity(pts.len() * 2);
        for &(x, y) in pts {
            flat.push(x as f32);
            flat.push(y as f32);
        }
        unsafe {
            d2d_stroke_polygon(
                self.ptr,
                flat.as_ptr(),
                pts.len() as i32,
                line_w as f32,
                col,
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_polygon_ring_linear(
        &self,
        outer: &[(f64, f64)],
        inner: &[(f64, f64)],
        gx0: f64,
        gy0: f64,
        gx1: f64,
        gy1: f64,
        stops: &[D2dGradStop],
    ) {
        if outer.len() <= 64 && inner.len() <= 64 {
            let mut o_stack = [0.0f32; 128];
            for (i, &(x, y)) in outer.iter().enumerate() {
                o_stack[i * 2] = x as f32;
                o_stack[i * 2 + 1] = y as f32;
            }
            let mut i_stack = [0.0f32; 128];
            for (i, &(x, y)) in inner.iter().enumerate() {
                i_stack[i * 2] = x as f32;
                i_stack[i * 2 + 1] = y as f32;
            }
            unsafe {
                d2d_fill_polygon_ring_linear(
                    self.ptr,
                    o_stack.as_ptr(),
                    outer.len() as i32,
                    i_stack.as_ptr(),
                    inner.len() as i32,
                    gx0 as f32,
                    gy0 as f32,
                    gx1 as f32,
                    gy1 as f32,
                    stops.as_ptr(),
                    stops.len() as i32,
                )
            };
            return;
        }
        let mut o = Vec::with_capacity(outer.len() * 2);
        for &(x, y) in outer {
            o.push(x as f32);
            o.push(y as f32);
        }
        let mut inn = Vec::with_capacity(inner.len() * 2);
        for &(x, y) in inner {
            inn.push(x as f32);
            inn.push(y as f32);
        }
        unsafe {
            d2d_fill_polygon_ring_linear(
                self.ptr,
                o.as_ptr(),
                outer.len() as i32,
                inn.as_ptr(),
                inner.len() as i32,
                gx0 as f32,
                gy0 as f32,
                gx1 as f32,
                gy1 as f32,
                stops.as_ptr(),
                stops.len() as i32,
            )
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_polygon_ring_linear(
        &self,
        outer: &[(f64, f64)],
        inner: &[(f64, f64)],
        line_w: f64,
        gx0: f64,
        gy0: f64,
        gx1: f64,
        gy1: f64,
        c0: D2dColor,
        c1: D2dColor,
    ) {
        if outer.len() <= 64 && inner.len() <= 64 {
            let mut o_stack = [0.0f32; 128];
            for (i, &(x, y)) in outer.iter().enumerate() {
                o_stack[i * 2] = x as f32;
                o_stack[i * 2 + 1] = y as f32;
            }
            let mut i_stack = [0.0f32; 128];
            for (i, &(x, y)) in inner.iter().enumerate() {
                i_stack[i * 2] = x as f32;
                i_stack[i * 2 + 1] = y as f32;
            }
            unsafe {
                d2d_stroke_polygon_ring_linear(
                    self.ptr,
                    o_stack.as_ptr(),
                    outer.len() as i32,
                    i_stack.as_ptr(),
                    inner.len() as i32,
                    line_w as f32,
                    gx0 as f32,
                    gy0 as f32,
                    gx1 as f32,
                    gy1 as f32,
                    c0,
                    c1,
                )
            };
            return;
        }
        let mut o = Vec::with_capacity(outer.len() * 2);
        for &(x, y) in outer {
            o.push(x as f32);
            o.push(y as f32);
        }
        let mut inn = Vec::with_capacity(inner.len() * 2);
        for &(x, y) in inner {
            inn.push(x as f32);
            inn.push(y as f32);
        }
        unsafe {
            d2d_stroke_polygon_ring_linear(
                self.ptr,
                o.as_ptr(),
                outer.len() as i32,
                inn.as_ptr(),
                inner.len() as i32,
                line_w as f32,
                gx0 as f32,
                gy0 as f32,
                gx1 as f32,
                gy1 as f32,
                c0,
                c1,
            )
        };
    }

    pub fn draw_path(
        &self,
        cmds: &[D2dPathCmd],
        fill: Option<D2dColor>,
        stroke: Option<(D2dColor, f64, bool)>,
    ) {
        if cmds.is_empty() {
            return;
        }
        let (df, fc) = match fill {
            Some(c) => (1, c),
            None => (0, D2dColor::rgba(0.0, 0.0, 0.0, 0.0)),
        };
        let (ds, sc, sw, rc) = match stroke {
            Some((c, w, r)) => (1, c, w as f32, if r { 1 } else { 0 }),
            None => (0, D2dColor::rgba(0.0, 0.0, 0.0, 0.0), 0.0, 0),
        };
        unsafe {
            d2d_draw_path(
                self.ptr,
                cmds.as_ptr(),
                cmds.len() as i32,
                df,
                fc,
                ds,
                sc,
                sw,
                rc,
            )
        };
    }

    pub fn measure_text(&mut self, text: &str, size: f64, bold: bool, max_w: f64, max_lines: i32) -> (f64, f64) {
        self.utf16.clear();
        self.utf16.extend(text.encode_utf16());
        let mut ow = 0.0f32;
        let mut oh = 0.0f32;
        unsafe {
            d2d_measure_text(
                self.ptr,
                self.utf16.as_ptr(),
                self.utf16.len() as i32,
                size as f32,
                if bold { 1 } else { 0 },
                max_w as f32,
                max_lines,
                &mut ow,
                &mut oh,
            );
        }
        (ow as f64, oh as f64)
    }

    pub fn hit_test_text_end(&mut self, text: &str, size: f64, bold: bool) -> f64 {
        self.utf16.clear();
        self.utf16.extend(text.encode_utf16());
        let len = self.utf16.len() as i32;
        if len == 0 {
            return 0.0;
        }
        unsafe {
            d2d_hit_test_text_pos(
                self.ptr,
                self.utf16.as_ptr(),
                len,
                size as f32,
                if bold { 1 } else { 0 },
                len,
            ) as f64
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(
        &mut self,
        text: &str,
        size: f64,
        bold: bool,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        align: i32,
        ellipsize: i32,
        wrap_lines: i32,
        col: D2dColor,
        shadow: Option<D2dColor>,
    ) {
        if text.is_empty() || col.a <= 0.002 {
            return;
        }
        self.utf16.clear();
        self.utf16.extend(text.encode_utf16());
        let (has_sh, sh_col) = match shadow {
            Some(c) => (1, c),
            None => (0, D2dColor::rgba(0.0, 0.0, 0.0, 0.0)),
        };
        unsafe {
            d2d_draw_text(
                self.ptr,
                self.utf16.as_ptr(),
                self.utf16.len() as i32,
                size as f32,
                if bold { 1 } else { 0 },
                x as f32,
                y as f32,
                w as f32,
                h as f32,
                align,
                ellipsize,
                wrap_lines,
                col,
                has_sh,
                sh_col,
            );
        }
    }
}

#[cfg(windows)]
impl Drop for D2dCanvas {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { d2d_destroy(self.ptr) };
            self.ptr = std::ptr::null_mut();
        }
    }
}
