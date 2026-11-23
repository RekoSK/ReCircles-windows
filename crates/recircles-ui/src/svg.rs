use recircles_core::colors::Color;

#[derive(Debug, Clone)]
pub enum PathSeg {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    CubicTo(f64, f64, f64, f64, f64, f64),
    Close,
}

#[derive(Debug, Clone)]
pub struct SvgShape {
    pub segments: Vec<PathSeg>,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f64,
    pub round_cap: bool,
}

#[derive(Debug, Clone)]
pub struct SvgDoc {
    pub vb_x: f64,
    pub vb_y: f64,
    pub vb_w: f64,
    pub vb_h: f64,
    pub shapes: Vec<SvgShape>,
}

pub fn parse_svg(xml: &str) -> Option<SvgDoc> {
    let mut doc = SvgDoc {
        vb_x: 0.0,
        vb_y: 0.0,
        vb_w: 24.0,
        vb_h: 24.0,
        shapes: Vec::new(),
    };

    let mut root_fill: Option<Color> = None;
    let mut root_stroke: Option<Color> = None;
    let mut root_sw: f64 = 1.6;
    let mut root_round = true;

    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        if rest.starts_with("!--") {
            if let Some(end) = rest.find("-->") {
                rest = &rest[end + 3..];
                continue;
            }
            break;
        }
        let Some(gt) = rest.find('>') else {
            break;
        };
        let tag_raw = &rest[..gt];
        rest = &rest[gt + 1..];
        let tag_trimmed = tag_raw.trim();
        if tag_trimmed.starts_with('/') || tag_trimmed.starts_with('?') || tag_trimmed.starts_with('!') {
            continue;
        }

        let mut parts = tag_trimmed.splitn(2, char::is_whitespace);
        let tag_name = parts.next().unwrap_or("").trim_end_matches('/').to_ascii_lowercase();
        let attrs_str = parts.next().unwrap_or("");

        if tag_name == "svg" {
            if let Some(vb) = attr(attrs_str, "viewBox") {
                let nums = parse_num_list(&vb);
                if nums.len() >= 4 && nums[2] > 0.0 && nums[3] > 0.0 {
                    doc.vb_x = nums[0];
                    doc.vb_y = nums[1];
                    doc.vb_w = nums[2];
                    doc.vb_h = nums[3];
                }
            } else if let (Some(w), Some(h)) = (
                attr(attrs_str, "width").and_then(|s| parse_len(&s)),
                attr(attrs_str, "height").and_then(|s| parse_len(&s)),
            )
                && w > 0.0
                && h > 0.0
            {
                doc.vb_w = w;
                doc.vb_h = h;
            }
            if let Some(f) = attr(attrs_str, "fill") {
                root_fill = parse_paint(&f);
            }
            if let Some(s) = attr(attrs_str, "stroke") {
                root_stroke = parse_paint(&s);
            }
            if let Some(sw) = attr(attrs_str, "stroke-width").and_then(|s| parse_len(&s)) {
                root_sw = sw;
            }
            if let Some(cap) = attr(attrs_str, "stroke-linecap") {
                root_round = cap.trim().eq_ignore_ascii_case("round");
            }
            continue;
        }

        let shape_fill = match attr(attrs_str, "fill") {
            Some(f) => parse_paint(&f),
            None => root_fill,
        };
        let shape_stroke = match attr(attrs_str, "stroke") {
            Some(s) => parse_paint(&s),
            None => root_stroke,
        };
        let shape_sw = attr(attrs_str, "stroke-width")
            .and_then(|s| parse_len(&s))
            .unwrap_or(root_sw);
        let shape_round = attr(attrs_str, "stroke-linecap")
            .map(|c| c.trim().eq_ignore_ascii_case("round"))
            .unwrap_or(root_round);

        let segments = match tag_name.as_str() {
            "path" => attr(attrs_str, "d")
                .map(|d| parse_path_d(&d))
                .unwrap_or_default(),
            "rect" => {
                let x = attr(attrs_str, "x").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let y = attr(attrs_str, "y").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let w = attr(attrs_str, "width").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let h = attr(attrs_str, "height").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let rx = attr(attrs_str, "rx")
                    .or_else(|| attr(attrs_str, "ry"))
                    .and_then(|s| parse_len(&s))
                    .unwrap_or(0.0);
                rect_segments(x, y, w, h, rx)
            }
            "circle" => {
                let cx = attr(attrs_str, "cx").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let cy = attr(attrs_str, "cy").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let r = attr(attrs_str, "r").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                ellipse_segments(cx, cy, r, r)
            }
            "ellipse" => {
                let cx = attr(attrs_str, "cx").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let cy = attr(attrs_str, "cy").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let rx = attr(attrs_str, "rx").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let ry = attr(attrs_str, "ry").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                ellipse_segments(cx, cy, rx, ry)
            }
            "line" => {
                let x1 = attr(attrs_str, "x1").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let y1 = attr(attrs_str, "y1").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let x2 = attr(attrs_str, "x2").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                let y2 = attr(attrs_str, "y2").and_then(|s| parse_len(&s)).unwrap_or(0.0);
                vec![PathSeg::MoveTo(x1, y1), PathSeg::LineTo(x2, y2)]
            }
            "polyline" | "polygon" => {
                let pts = attr(attrs_str, "points")
                    .map(|p| parse_num_list(&p))
                    .unwrap_or_default();
                let mut segs = Vec::new();
                for pair in pts.as_chunks::<2>().0 {
                    if segs.is_empty() {
                        segs.push(PathSeg::MoveTo(pair[0], pair[1]));
                    } else {
                        segs.push(PathSeg::LineTo(pair[0], pair[1]));
                    }
                }
                if tag_name == "polygon" && !segs.is_empty() {
                    segs.push(PathSeg::Close);
                }
                segs
            }
            _ => Vec::new(),
        };

        if !segments.is_empty() {
            doc.shapes.push(SvgShape {
                segments,
                fill: shape_fill,
                stroke: shape_stroke,
                stroke_width: shape_sw,
                round_cap: shape_round,
            });
        }
    }

    if doc.shapes.is_empty() {
        None
    } else {
        Some(doc)
    }
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let key_start = pos;
        while pos < bytes.len() && bytes[pos] != b'=' && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let key = &attrs[key_start..pos];
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || bytes[pos] != b'=' {
            pos += 1;
            continue;
        }
        pos += 1;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        let quote = bytes[pos];
        if quote == b'"' || quote == b'\'' {
            pos += 1;
            let val_start = pos;
            while pos < bytes.len() && bytes[pos] != quote {
                pos += 1;
            }
            let val = &attrs[val_start..pos];
            pos += 1;
            if key.eq_ignore_ascii_case(name) {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn parse_paint(s: &str) -> Option<Color> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("transparent") {
        return None;
    }
    if t.eq_ignore_ascii_case("currentcolor") || t.eq_ignore_ascii_case("white") {
        return Some(Color::rgb(1.0, 1.0, 1.0));
    }
    if t.eq_ignore_ascii_case("black") {
        return Some(Color::rgb(0.0, 0.0, 0.0));
    }
    Color::from_hex(t).ok()
}

fn parse_len(s: &str) -> Option<f64> {
    let cleaned = s.trim().trim_end_matches("px");
    cleaned.parse::<f64>().ok()
}

fn parse_num_list(s: &str) -> Vec<f64> {
    s.split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

fn rect_segments(x: f64, y: f64, w: f64, h: f64, rx: f64) -> Vec<PathSeg> {
    if w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    let r = rx.clamp(0.0, (w * 0.5).min(h * 0.5));
    if r <= 0.01 {
        return vec![
            PathSeg::MoveTo(x, y),
            PathSeg::LineTo(x + w, y),
            PathSeg::LineTo(x + w, y + h),
            PathSeg::LineTo(x, y + h),
            PathSeg::Close,
        ];
    }
    let k = 0.552284749831 * r;
    vec![
        PathSeg::MoveTo(x + r, y),
        PathSeg::LineTo(x + w - r, y),
        PathSeg::CubicTo(x + w - r + k, y, x + w, y + r - k, x + w, y + r),
        PathSeg::LineTo(x + w, y + h - r),
        PathSeg::CubicTo(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h),
        PathSeg::LineTo(x + r, y + h),
        PathSeg::CubicTo(x + r - k, y + h, x, y + h - r + k, x, y + h - r),
        PathSeg::LineTo(x, y + r),
        PathSeg::CubicTo(x, y + r - k, x + r - k, y, x + r, y),
        PathSeg::Close,
    ]
}

fn ellipse_segments(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<PathSeg> {
    if rx <= 0.0 || ry <= 0.0 {
        return Vec::new();
    }
    let kx = 0.552284749831 * rx;
    let ky = 0.552284749831 * ry;
    vec![
        PathSeg::MoveTo(cx + rx, cy),
        PathSeg::CubicTo(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry),
        PathSeg::CubicTo(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy),
        PathSeg::CubicTo(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry),
        PathSeg::CubicTo(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy),
        PathSeg::Close,
    ]
}

pub fn parse_path_d(d: &str) -> Vec<PathSeg> {
    let mut segs = Vec::new();
    let tokens = tokenize_path_d(d);
    let mut idx = 0;
    let mut cmd = '\0';
    let mut cur_x = 0.0;
    let mut cur_y = 0.0;
    let mut start_x = 0.0;
    let mut start_y = 0.0;
    let mut last_ctrl: Option<(f64, f64)> = None;

    let read_num = |i: &mut usize| -> Option<f64> {
        if *i >= tokens.len() {
            return None;
        }
        if let Token::Num(v) = tokens[*i] {
            *i += 1;
            Some(v)
        } else {
            None
        }
    };

    while idx < tokens.len() {
        if let Token::Cmd(c) = tokens[idx] {
            cmd = c;
            idx += 1;
            if c == 'Z' || c == 'z' {
                segs.push(PathSeg::Close);
                cur_x = start_x;
                cur_y = start_y;
                last_ctrl = None;
                continue;
            }
        }
        if cmd == '\0' {
            break;
        }
        match cmd {
            'M' | 'm' => {
                let (Some(x), Some(y)) = (read_num(&mut idx), read_num(&mut idx)) else {
                    break;
                };
                if cmd == 'm' {
                    cur_x += x;
                    cur_y += y;
                } else {
                    cur_x = x;
                    cur_y = y;
                }
                start_x = cur_x;
                start_y = cur_y;
                segs.push(PathSeg::MoveTo(cur_x, cur_y));
                cmd = if cmd == 'm' { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' | 'l' => {
                let (Some(x), Some(y)) = (read_num(&mut idx), read_num(&mut idx)) else {
                    break;
                };
                if cmd == 'l' {
                    cur_x += x;
                    cur_y += y;
                } else {
                    cur_x = x;
                    cur_y = y;
                }
                segs.push(PathSeg::LineTo(cur_x, cur_y));
                last_ctrl = None;
            }
            'H' | 'h' => {
                let Some(x) = read_num(&mut idx) else { break };
                cur_x = if cmd == 'h' { cur_x + x } else { x };
                segs.push(PathSeg::LineTo(cur_x, cur_y));
                last_ctrl = None;
            }
            'V' | 'v' => {
                let Some(y) = read_num(&mut idx) else { break };
                cur_y = if cmd == 'v' { cur_y + y } else { y };
                segs.push(PathSeg::LineTo(cur_x, cur_y));
                last_ctrl = None;
            }
            'C' | 'c' => {
                let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x), Some(y)) = (
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                ) else {
                    break;
                };
                let (ax1, ay1, ax2, ay2, ax, ay) = if cmd == 'c' {
                    (cur_x + x1, cur_y + y1, cur_x + x2, cur_y + y2, cur_x + x, cur_y + y)
                } else {
                    (x1, y1, x2, y2, x, y)
                };
                segs.push(PathSeg::CubicTo(ax1, ay1, ax2, ay2, ax, ay));
                last_ctrl = Some((ax2, ay2));
                cur_x = ax;
                cur_y = ay;
            }
            'S' | 's' => {
                let (Some(x2), Some(y2), Some(x), Some(y)) = (
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                ) else {
                    break;
                };
                let (ax1, ay1) = match last_ctrl {
                    Some((lx, ly)) => (2.0 * cur_x - lx, 2.0 * cur_y - ly),
                    None => (cur_x, cur_y),
                };
                let (ax2, ay2, ax, ay) = if cmd == 's' {
                    (cur_x + x2, cur_y + y2, cur_x + x, cur_y + y)
                } else {
                    (x2, y2, x, y)
                };
                segs.push(PathSeg::CubicTo(ax1, ay1, ax2, ay2, ax, ay));
                last_ctrl = Some((ax2, ay2));
                cur_x = ax;
                cur_y = ay;
            }
            'Q' | 'q' => {
                let (Some(qx), Some(qy), Some(x), Some(y)) = (
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                ) else {
                    break;
                };
                let (aqx, aqy, ax, ay) = if cmd == 'q' {
                    (cur_x + qx, cur_y + qy, cur_x + x, cur_y + y)
                } else {
                    (qx, qy, x, y)
                };
                let c1x = cur_x + (2.0 / 3.0) * (aqx - cur_x);
                let c1y = cur_y + (2.0 / 3.0) * (aqy - cur_y);
                let c2x = ax + (2.0 / 3.0) * (aqx - ax);
                let c2y = ay + (2.0 / 3.0) * (aqy - ay);
                segs.push(PathSeg::CubicTo(c1x, c1y, c2x, c2y, ax, ay));
                last_ctrl = Some((aqx, aqy));
                cur_x = ax;
                cur_y = ay;
            }
            'A' | 'a' => {
                let (Some(rx), Some(ry), Some(rot), Some(large), Some(sweep), Some(x), Some(y)) = (
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                    read_num(&mut idx),
                ) else {
                    break;
                };
                let (ax, ay) = if cmd == 'a' {
                    (cur_x + x, cur_y + y)
                } else {
                    (x, y)
                };
                arc_to_cubics(
                    cur_x,
                    cur_y,
                    rx,
                    ry,
                    rot,
                    large != 0.0,
                    sweep != 0.0,
                    ax,
                    ay,
                    &mut segs,
                );
                cur_x = ax;
                cur_y = ay;
                last_ctrl = None;
            }
            _ => {
                idx += 1;
            }
        }
    }
    segs
}

#[derive(Debug, Clone, Copy)]
enum Token {
    Cmd(char),
    Num(f64),
}

fn tokenize_path_d(d: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let bytes = d.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() || b == b',' {
            i += 1;
            continue;
        }
        if b.is_ascii_alphabetic() {
            out.push(Token::Cmd(b as char));
            i += 1;
            continue;
        }
        // Parse float number including leading +/- and optional single decimal point / e-exponent
        let start = i;
        if b == b'+' || b == b'-' {
            i += 1;
        }
        let mut seen_dot = false;
        let mut seen_exp = false;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_digit() {
                i += 1;
            } else if c == b'.' && !seen_dot && !seen_exp {
                seen_dot = true;
                i += 1;
            } else if (c == b'e' || c == b'E') && !seen_exp {
                seen_exp = true;
                i += 1;
                if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                    i += 1;
                }
            } else {
                break;
            }
        }
        if let Ok(v) = d[start..i].parse::<f64>() {
            out.push(Token::Num(v));
        } else {
            i += 1;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    x0: f64,
    y0: f64,
    rx_in: f64,
    ry_in: f64,
    x_axis_rotation_deg: f64,
    large_arc: bool,
    sweep: bool,
    x: f64,
    y: f64,
    out: &mut Vec<PathSeg>,
) {
    if (x0 - x).abs() < 1e-6 && (y0 - y).abs() < 1e-6 {
        return;
    }
    let mut rx = rx_in.abs();
    let mut ry = ry_in.abs();
    if rx < 1e-6 || ry < 1e-6 {
        out.push(PathSeg::LineTo(x, y));
        return;
    }
    let phi = x_axis_rotation_deg.to_radians();
    let (sin_p, cos_p) = phi.sin_cos();
    let dx2 = (x0 - x) * 0.5;
    let dy2 = (y0 - y) * 0.5;
    let x1p = cos_p * dx2 + sin_p * dy2;
    let y1p = -sin_p * dx2 + cos_p * dy2;

    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }

    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;
    let denom = rx2 * y1p2 + ry2 * x1p2;
    let num = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
    let factor = if denom > 1e-9 { (num / denom).sqrt() } else { 0.0 };
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let cxp = sign * factor * (rx * y1p / ry);
    let cyp = sign * factor * (-ry * x1p / rx);

    let cx = cos_p * cxp - sin_p * cyp + (x0 + x) * 0.5;
    let cy = sin_p * cxp + cos_p * cyp + (y0 + y) * 0.5;

    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| -> f64 {
        let dot = ux * vx + uy * vy;
        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
        let mut a = (dot / len.max(1e-9)).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };

    let t1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dt = ang(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && dt > 0.0 {
        dt -= std::f64::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f64::consts::TAU;
    }

    let segs_count = ((dt.abs() / (std::f64::consts::FRAC_PI_2)).ceil() as usize).max(1);
    let delta = dt / segs_count as f64;
    for i in 0..segs_count {
        let a0 = t1 + delta * i as f64;
        let a1 = a0 + delta;
        let alpha = (4.0 / 3.0) * (delta * 0.25).tan();
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        let p1 = (c0 - alpha * s0, s0 + alpha * c0);
        let p2 = (c1 + alpha * s1, s1 - alpha * c1);
        let p3 = (c1, s1);

        let xform = |(px, py): (f64, f64)| -> (f64, f64) {
            let sx = px * rx;
            let sy = py * ry;
            (cx + cos_p * sx - sin_p * sy, cy + sin_p * sx + cos_p * sy)
        };
        let (c1x, c1y) = xform(p1);
        let (c2x, c2y) = xform(p2);
        let (ex, ey) = xform(p3);
        out.push(PathSeg::CubicTo(c1x, c1y, c2x, c2y, ex, ey));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recircles_core::config::BUILTIN_CLIPBOARD_SVG;

    #[test]
    fn parses_clipboard_svg_viewbox_and_shapes() {
        let doc = parse_svg(BUILTIN_CLIPBOARD_SVG).unwrap();
        assert_eq!(doc.vb_x, -4.0);
        assert_eq!(doc.vb_w, 32.0);
        assert_eq!(doc.shapes.len(), 3);
        assert!(doc.shapes[0].stroke.is_some());
    }
}
