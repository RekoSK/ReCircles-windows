use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub fn from_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: f64::from(r) / 255.0,
            g: f64::from(g) / 255.0,
            b: f64::from(b) / 255.0,
            a: f64::from(a) / 255.0,
        }
    }

    pub fn from_hex(s: &str) -> Result<Self, String> {
        let hex = s.trim();
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| format!("invalid hex color '{s}'"))?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| format!("invalid hex color '{s}'"))?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| format!("invalid hex color '{s}'"))?;
                Ok(Self::from_u8(r, g, b, 255))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| format!("invalid hex color '{s}'"))?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| format!("invalid hex color '{s}'"))?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| format!("invalid hex color '{s}'"))?;
                let a = u8::from_str_radix(&hex[6..8], 16).map_err(|_| format!("invalid hex color '{s}'"))?;
                Ok(Self::from_u8(r, g, b, a))
            }
            _ => Err(format!("expected #RRGGBB or #RRGGBBAA color, got '{s}'")),
        }
    }

    pub fn to_hex(self) -> String {
        let r = (self.r.clamp(0.0, 1.0) * 255.0).round() as u8;
        let g = (self.g.clamp(0.0, 1.0) * 255.0).round() as u8;
        let b = (self.b.clamp(0.0, 1.0) * 255.0).round() as u8;
        let a = (self.a.clamp(0.0, 1.0) * 255.0).round() as u8;
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    pub fn to_hex_rgba(self) -> String {
        let r = (self.r.clamp(0.0, 1.0) * 255.0).round() as u8;
        let g = (self.g.clamp(0.0, 1.0) * 255.0).round() as u8;
        let b = (self.b.clamp(0.0, 1.0) * 255.0).round() as u8;
        let a = (self.a.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }

    pub fn with_alpha(self, alpha: f64) -> Self {
        Self {
            a: alpha.clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn scale_alpha(self, factor: f64) -> Self {
        Self {
            a: (self.a * factor).clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn lerp(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    pub fn mix_rgb(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a,
        }
    }

    pub fn relative_luminance(self) -> f64 {
        fn linearize(c: f64) -> f64 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linearize(self.r.clamp(0.0, 1.0))
            + 0.7152 * linearize(self.g.clamp(0.0, 1.0))
            + 0.0722 * linearize(self.b.clamp(0.0, 1.0))
    }

    pub fn contrast_fg(self) -> Self {
        if self.relative_luminance() > 0.36 {
            Self::from_u8(20, 24, 30, 255)
        } else {
            Self::from_u8(245, 247, 250, 255)
        }
    }

    /// GDI+ / Win32 ARGB packed 32-bit integer (`0xAARRGGBB`).
    pub fn to_argb_u32(self) -> u32 {
        let a = (self.a.clamp(0.0, 1.0) * 255.0).round() as u32;
        let r = (self.r.clamp(0.0, 1.0) * 255.0).round() as u32;
        let g = (self.g.clamp(0.0, 1.0) * 255.0).round() as u32;
        let b = (self.b.clamp(0.0, 1.0) * 255.0).round() as u32;
        (a << 24) | (r << 16) | (g << 8) | b
    }

    /// GDI `COLORREF` (`0x00BBGGRR`).
    pub fn to_colorref(self) -> u32 {
        let r = (self.r.clamp(0.0, 1.0) * 255.0).round() as u32;
        let g = (self.g.clamp(0.0, 1.0) * 255.0).round() as u32;
        let b = (self.b.clamp(0.0, 1.0) * 255.0).round() as u32;
        (b << 16) | (g << 8) | r
    }

    pub fn to_u8_rgba(self) -> (u8, u8, u8, u8) {
        (
            (self.r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.b.clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        )
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Color::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::Color;

    #[test]
    fn hex_roundtrip_and_argb_conversion() {
        let c = Color::from_hex("#faad8a").unwrap();
        assert_eq!(c.to_hex(), "#faad8a");
        assert_eq!(c.to_argb_u32(), 0xfffaad8a);

        let ca = Color::from_hex("#1a1e24c0").unwrap();
        assert_eq!(ca.to_hex(), "#1a1e24c0");
        assert_eq!(ca.to_argb_u32(), 0xc01a1e24);
        assert!(Color::from_hex("#12345").is_err());
    }
}
