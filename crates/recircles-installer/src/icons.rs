use crate::d2d::{D2dPathCmd, PathBuilder};
use recircles_core::modules::BuiltinSubmenu;
use std::f64::consts::{PI, TAU};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiGlyph {
    Submenu(BuiltinSubmenu),
    Check,
    Lock,
    Close,
    Minimize,
    Alert,
    FolderOpen,
    RotateCcw,
    Keyboard,
    Palette,
    Sliders,
    Download,
    Sparkles,
    ChevronLeft,
    ChevronRight,
    Trash,
}

impl UiGlyph {
    const ALL: [UiGlyph; 20] = [
        UiGlyph::Submenu(BuiltinSubmenu::Files),
        UiGlyph::Submenu(BuiltinSubmenu::Apps),
        UiGlyph::Submenu(BuiltinSubmenu::Windows),
        UiGlyph::Submenu(BuiltinSubmenu::Clipboard),
        UiGlyph::Submenu(BuiltinSubmenu::Settings),
        UiGlyph::Check,
        UiGlyph::Lock,
        UiGlyph::Close,
        UiGlyph::Minimize,
        UiGlyph::Alert,
        UiGlyph::FolderOpen,
        UiGlyph::RotateCcw,
        UiGlyph::Keyboard,
        UiGlyph::Palette,
        UiGlyph::Sliders,
        UiGlyph::Download,
        UiGlyph::Sparkles,
        UiGlyph::ChevronLeft,
        UiGlyph::ChevronRight,
        UiGlyph::Trash,
    ];

    const fn index(self) -> usize {
        match self {
            UiGlyph::Submenu(BuiltinSubmenu::Files) => 0,
            UiGlyph::Submenu(BuiltinSubmenu::Apps) => 1,
            UiGlyph::Submenu(BuiltinSubmenu::Windows) => 2,
            UiGlyph::Submenu(BuiltinSubmenu::Clipboard) => 3,
            UiGlyph::Submenu(BuiltinSubmenu::Settings) => 4,
            UiGlyph::Check => 5,
            UiGlyph::Lock => 6,
            UiGlyph::Close => 7,
            UiGlyph::Minimize => 8,
            UiGlyph::Alert => 9,
            UiGlyph::FolderOpen => 10,
            UiGlyph::RotateCcw => 11,
            UiGlyph::Keyboard => 12,
            UiGlyph::Palette => 13,
            UiGlyph::Sliders => 14,
            UiGlyph::Download => 15,
            UiGlyph::Sparkles => 16,
            UiGlyph::ChevronLeft => 17,
            UiGlyph::ChevronRight => 18,
            UiGlyph::Trash => 19,
        }
    }
}

static CACHED_GLYPHS: LazyLock<[Vec<D2dPathCmd>; 20]> =
    LazyLock::new(|| std::array::from_fn(|i| build_glyph(UiGlyph::ALL[i]).cmds));

#[cfg_attr(not(windows), allow(dead_code))]
pub fn glyph_cmds(glyph: UiGlyph) -> &'static [D2dPathCmd] {
    &CACHED_GLYPHS[glyph.index()]
}

/// 24x24 vector icons matching ReCircles' authentic visual language.
pub fn build_glyph(glyph: UiGlyph) -> PathBuilder {
    let mut pb = PathBuilder::new();
    match glyph {
        UiGlyph::Submenu(BuiltinSubmenu::Files) => {
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
        UiGlyph::Submenu(BuiltinSubmenu::Apps) => {
            for (x, y) in [(3.4, 3.4), (13.3, 3.4), (3.4, 13.3), (13.3, 13.3)] {
                pb.rounded_rect(x, y, 7.2, 7.2, 1.8);
            }
        }
        UiGlyph::Submenu(BuiltinSubmenu::Windows) => {
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
        UiGlyph::Submenu(BuiltinSubmenu::Clipboard) => {
            // Clipboard board + top clip + clean lines
            pb.rounded_rect(8.5, 2.6, 7.0, 3.2, 1.4);
            pb.move_to(8.5, 4.4);
            pb.line_to(6.2, 4.4);
            pb.curve_to(5.0, 4.4, 4.2, 5.2, 4.2, 6.4);
            pb.line_to(4.2, 19.2);
            pb.curve_to(4.2, 20.4, 5.0, 21.2, 6.2, 21.2);
            pb.line_to(17.8, 21.2);
            pb.curve_to(19.0, 21.2, 19.8, 20.4, 19.8, 19.2);
            pb.line_to(19.8, 6.4);
            pb.curve_to(19.8, 5.2, 19.0, 4.4, 17.8, 4.4);
            pb.line_to(15.5, 4.4);
            pb.move_to(8.0, 11.2);
            pb.line_to(16.0, 11.2);
            pb.move_to(8.0, 15.6);
            pb.line_to(13.6, 15.6);
        }
        UiGlyph::Submenu(BuiltinSubmenu::Settings) | UiGlyph::Sliders => {
            // Exact ReCircles 3 horizontal sliders with circles
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
        UiGlyph::Check => {
            pb.move_to(5.0, 12.5);
            pb.line_to(10.0, 17.5);
            pb.line_to(19.2, 7.2);
        }
        UiGlyph::Lock => {
            pb.rounded_rect(5.2, 10.6, 13.6, 9.6, 2.2);
            pb.move_to(8.2, 10.6);
            pb.line_to(8.2, 7.6);
            pb.arc(12.0, 7.6, 3.8, PI, TAU);
            pb.line_to(15.8, 10.6);
        }
        UiGlyph::Close => {
            pb.move_to(6.8, 6.8);
            pb.line_to(17.2, 17.2);
            pb.move_to(17.2, 6.8);
            pb.line_to(6.8, 17.2);
        }
        UiGlyph::Minimize => {
            pb.move_to(6.5, 12.5);
            pb.line_to(17.5, 12.5);
        }
        UiGlyph::Alert => {
            pb.move_to(12.0, 3.6);
            pb.line_to(20.8, 19.4);
            pb.line_to(3.2, 19.4);
            pb.close();
            pb.move_to(12.0, 9.4);
            pb.line_to(12.0, 14.0);
            pb.arc(12.0, 16.8, 0.7, 0.0, TAU);
        }
        UiGlyph::FolderOpen => {
            pb.move_to(3.0, 8.8);
            pb.line_to(3.0, 6.4);
            pb.curve_to(3.0, 5.2, 3.9, 4.4, 5.2, 4.4);
            pb.line_to(9.2, 4.4);
            pb.line_to(11.4, 6.8);
            pb.line_to(17.8, 6.8);
            pb.curve_to(19.0, 6.8, 19.8, 7.6, 19.8, 8.8);
            pb.move_to(3.0, 18.8);
            pb.line_to(5.4, 10.6);
            pb.line_to(21.2, 10.6);
            pb.line_to(18.8, 18.8);
            pb.close();
        }
        UiGlyph::RotateCcw => {
            pb.arc(12.0, 12.0, 7.4, -PI * 0.75, PI * 0.85);
            pb.move_to(6.2, 5.2);
            pb.line_to(6.2, 9.6);
            pb.line_to(10.6, 9.6);
        }
        UiGlyph::Keyboard => {
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
        UiGlyph::Palette => {
            pb.arc(12.0, 12.0, 8.6, 0.35, TAU - 0.15);
            pb.curve_to(19.0, 13.8, 15.6, 13.8, 15.2, 16.2);
            pb.curve_to(14.8, 18.6, 17.8, 19.2, 16.4, 20.0);
            for (cx, cy) in [(8.0, 9.2), (12.0, 7.2), (16.0, 9.2), (7.6, 13.8)] {
                pb.arc(cx, cy, 1.1, 0.0, TAU);
            }
        }
        UiGlyph::Download => {
            pb.move_to(12.0, 4.2);
            pb.line_to(12.0, 14.8);
            pb.move_to(7.6, 10.8);
            pb.line_to(12.0, 15.2);
            pb.line_to(16.4, 10.8);
            pb.move_to(4.6, 17.2);
            pb.line_to(4.6, 19.2);
            pb.line_to(19.4, 19.2);
            pb.line_to(19.4, 17.2);
        }
        UiGlyph::Sparkles => {
            pb.move_to(12.0, 3.2);
            pb.curve_to(12.6, 8.8, 15.2, 11.4, 20.8, 12.0);
            pb.curve_to(15.2, 12.6, 12.6, 15.2, 12.0, 20.8);
            pb.curve_to(11.4, 15.2, 8.8, 12.6, 3.2, 12.0);
            pb.curve_to(8.8, 11.4, 11.4, 8.8, 12.0, 3.2);
            pb.close();
        }
        UiGlyph::ChevronLeft => {
            pb.move_to(14.8, 6.2);
            pb.line_to(9.0, 12.0);
            pb.line_to(14.8, 17.8);
        }
        UiGlyph::ChevronRight => {
            pb.move_to(9.2, 6.2);
            pb.line_to(15.0, 12.0);
            pb.line_to(9.2, 17.8);
        }
        UiGlyph::Trash => {
            pb.move_to(4.5, 6.8);
            pb.line_to(19.5, 6.8);
            pb.move_to(9.2, 6.8);
            pb.line_to(9.6, 4.2);
            pb.line_to(14.4, 4.2);
            pb.line_to(14.8, 6.8);
            pb.move_to(6.6, 6.8);
            pb.line_to(7.5, 19.4);
            pb.curve_to(7.6, 20.2, 8.2, 20.8, 9.0, 20.8);
            pb.line_to(15.0, 20.8);
            pb.curve_to(15.8, 20.8, 16.4, 20.2, 16.5, 19.4);
            pb.line_to(17.4, 6.8);
        }
    }
    pb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_glyph_cmds_match_build_glyph_for_all_variants() {
        for glyph in UiGlyph::ALL {
            let cached = glyph_cmds(glyph);
            let built = build_glyph(glyph).cmds;
            assert_eq!(cached.len(), built.len(), "len mismatch for {glyph:?}");
            for (a, b) in cached.iter().zip(built.iter()) {
                assert_eq!(a.op, b.op);
                assert_eq!(a.x1, b.x1);
                assert_eq!(a.y1, b.y1);
            }
        }
    }
}
