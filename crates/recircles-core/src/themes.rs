use crate::colors::Color;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccentPalette {
    #[serde(default = "default_accent_files")]
    pub files: String,
    #[serde(default = "default_accent_apps")]
    pub apps: String,
    #[serde(default = "default_accent_windows")]
    pub windows: String,
    #[serde(default = "default_accent_clipboard")]
    pub clipboard: String,
    #[serde(default = "default_accent_settings")]
    pub settings: String,
}

fn default_accent_files() -> String {
    "#ff9e47".into()
}
fn default_accent_apps() -> String {
    "#ffc75c".into()
}
fn default_accent_windows() -> String {
    "#ff7a57".into()
}
fn default_accent_clipboard() -> String {
    "#faad8a".into()
}
fn default_accent_settings() -> String {
    "#ffb86b".into()
}

impl Default for AccentPalette {
    fn default() -> Self {
        Self {
            files: default_accent_files(),
            apps: default_accent_apps(),
            windows: default_accent_windows(),
            clipboard: default_accent_clipboard(),
            settings: default_accent_settings(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorsConfig {
    pub ring: String,
    pub ring_border: String,
    pub center: String,
    pub capsule: String,
    pub text: String,
    pub muted: String,
    pub accent: String,
    pub accents: AccentPalette,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_top: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_bottom: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_rim_bottom: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_divider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_shadow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capsule_bottom: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capsule_border: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_shadow: Option<String>,
}

impl Default for ColorsConfig {
    fn default() -> Self {
        Self {
            ring: "#26160de8".into(),
            ring_border: "#ffd19e".into(),
            center: "#1a0e08eb".into(),
            capsule: "#362114f0".into(),
            text: "#fff7f0".into(),
            muted: "#d7b9a4".into(),
            accent: "#ff9e47".into(),
            accents: AccentPalette::default(),
            ring_top: Some("#422614e8".into()),
            ring_bottom: Some("#2e1a0fe8".into()),
            ring_rim_bottom: Some("#f59447".into()),
            ring_divider: Some("#ffd6ad".into()),
            ring_shadow: Some("#030100".into()),
            capsule_bottom: Some("#21130df0".into()),
            capsule_border: Some("#ffd1a8".into()),
            text_shadow: Some("#090402".into()),
        }
    }
}

impl ColorsConfig {
    pub fn validate(&self) -> Result<(), String> {
        for (name, hex) in [
            ("ring", &self.ring),
            ("ring_border", &self.ring_border),
            ("center", &self.center),
            ("capsule", &self.capsule),
            ("text", &self.text),
            ("muted", &self.muted),
            ("accent", &self.accent),
            ("accents.files", &self.accents.files),
            ("accents.apps", &self.accents.apps),
            ("accents.windows", &self.accents.windows),
            ("accents.clipboard", &self.accents.clipboard),
            ("accents.settings", &self.accents.settings),
        ] {
            Color::from_hex(hex).map_err(|e| format!("colors.{name}: {e}"))?;
        }
        for (name, opt) in [
            ("ring_top", &self.ring_top),
            ("ring_bottom", &self.ring_bottom),
            ("ring_rim_bottom", &self.ring_rim_bottom),
            ("ring_divider", &self.ring_divider),
            ("ring_shadow", &self.ring_shadow),
            ("capsule_bottom", &self.capsule_bottom),
            ("capsule_border", &self.capsule_border),
            ("text_shadow", &self.text_shadow),
        ] {
            if let Some(hex) = opt {
                Color::from_hex(hex).map_err(|e| format!("colors.{name}: {e}"))?;
            }
        }
        Ok(())
    }

    pub fn ring_color(&self) -> Color {
        Color::from_hex(&self.ring).unwrap_or_else(|_| Color::from_hex("#26160de8").unwrap())
    }

    pub fn ring_border_color(&self) -> Color {
        Color::from_hex(&self.ring_border).unwrap_or_else(|_| Color::from_hex("#ffd19e").unwrap())
    }

    pub fn center_color(&self) -> Color {
        Color::from_hex(&self.center).unwrap_or_else(|_| Color::from_hex("#1a0e08eb").unwrap())
    }

    pub fn capsule_color(&self) -> Color {
        Color::from_hex(&self.capsule).unwrap_or_else(|_| Color::from_hex("#362114f0").unwrap())
    }

    pub fn text_color(&self) -> Color {
        Color::from_hex(&self.text).unwrap_or_else(|_| Color::from_hex("#fff7f0").unwrap())
    }

    pub fn muted_color(&self) -> Color {
        Color::from_hex(&self.muted).unwrap_or_else(|_| Color::from_hex("#d7b9a4").unwrap())
    }

    pub fn accent_color(&self) -> Color {
        Color::from_hex(&self.accent).unwrap_or_else(|_| Color::from_hex("#ff9e47").unwrap())
    }

    pub fn ring_top_color(&self) -> Color {
        self.ring_top
            .as_deref()
            .and_then(|s| Color::from_hex(s).ok())
            .unwrap_or_else(|| {
                let c = self.ring_color();
                c.mix_rgb(Color::rgba(1.0, 1.0, 1.0, c.a), 0.08)
            })
    }

    pub fn ring_bottom_color(&self) -> Color {
        self.ring_bottom
            .as_deref()
            .and_then(|s| Color::from_hex(s).ok())
            .unwrap_or_else(|| {
                let c = self.ring_color();
                c.mix_rgb(Color::rgba(0.0, 0.0, 0.0, c.a), 0.06)
            })
    }

    pub fn rim_bottom_color(&self) -> Color {
        self.ring_rim_bottom
            .as_deref()
            .and_then(|s| Color::from_hex(s).ok())
            .unwrap_or_else(|| self.ring_border_color().mix_rgb(self.accent_color(), 0.55))
    }

    pub fn divider_color(&self) -> Color {
        self.ring_divider
            .as_deref()
            .and_then(|s| Color::from_hex(s).ok())
            .unwrap_or_else(|| self.ring_border_color())
    }

    pub fn shadow_color(&self) -> Color {
        self.ring_shadow
            .as_deref()
            .and_then(|s| Color::from_hex(s).ok())
            .unwrap_or_else(|| Color::rgb(0.012, 0.004, 0.0))
    }

    pub fn capsule_bottom_color(&self) -> Color {
        self.capsule_bottom
            .as_deref()
            .and_then(|s| Color::from_hex(s).ok())
            .unwrap_or_else(|| {
                let c = self.capsule_color();
                c.mix_rgb(Color::rgba(0.0, 0.0, 0.0, c.a), 0.16)
            })
    }

    pub fn capsule_border_color(&self) -> Color {
        self.capsule_border
            .as_deref()
            .and_then(|s| Color::from_hex(s).ok())
            .unwrap_or_else(|| self.ring_border_color())
    }

    pub fn text_shadow_color(&self) -> Color {
        self.text_shadow
            .as_deref()
            .and_then(|s| Color::from_hex(s).ok())
            .unwrap_or_else(|| Color::rgb(0.035, 0.015, 0.008))
    }

    pub fn accent_for_mode(&self, mode_id: &str) -> Color {
        let hex = match mode_id {
            "files" => &self.accents.files,
            "apps" => &self.accents.apps,
            "windows" => &self.accents.windows,
            "clipboard" => &self.accents.clipboard,
            "settings" => &self.accents.settings,
            _ => &self.accent,
        };
        Color::from_hex(hex).unwrap_or_else(|_| self.accent_color())
    }
}

#[derive(Debug, Clone)]
pub struct BuiltinTheme {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub colors: ColorsConfig,
}

pub fn builtin_themes() -> Vec<BuiltinTheme> {
    vec![
        BuiltinTheme {
            id: "default",
            label: "Amber (Default)",
            description: "Warm amber glass with molten gold highlights and burnt-sugar depth",
            colors: ColorsConfig::default(),
        },
        BuiltinTheme {
            id: "light_green",
            label: "Light Green",
            description: "Crisp frosted daylight sage and emerald glass",
            colors: ColorsConfig {
                ring: "#c7ebd0e8".into(),
                ring_border: "#ffffff".into(),
                center: "#eaf8edeb".into(),
                capsule: "#ecfbf0f0".into(),
                text: "#102819".into(),
                muted: "#386649".into(),
                accent: "#1b8047".into(),
                accents: AccentPalette {
                    files: "#1b8047".into(),
                    apps: "#298f66".into(),
                    windows: "#cc5635".into(),
                    clipboard: "#c85d32".into(),
                    settings: "#1f8a52".into(),
                },
                ring_top: Some("#dff5e4eb".into()),
                ring_bottom: Some("#b6e2c2e8".into()),
                ring_rim_bottom: Some("#5eb878".into()),
                ring_divider: Some("#347349".into()),
                ring_shadow: Some("#0d2415".into()),
                capsule_bottom: Some("#d4f0dcf0".into()),
                capsule_border: Some("#4fa368".into()),
                text_shadow: Some("#e9faed".into()),
            },
        },
        BuiltinTheme {
            id: "forest",
            label: "Forest",
            description: "Deep pine glass with mint, moss, and fern highlights",
            colors: ColorsConfig {
                ring: "#0e2219e8".into(),
                ring_border: "#a8f0c6".into(),
                center: "#0c1b14eb".into(),
                capsule: "#173326f0".into(),
                text: "#f0fff5".into(),
                muted: "#9bc9ae".into(),
                accent: "#5ee092".into(),
                accents: AccentPalette {
                    files: "#5ee092".into(),
                    apps: "#9be870".into(),
                    windows: "#ff8c6b".into(),
                    clipboard: "#faad8a".into(),
                    settings: "#7ae6a8".into(),
                },
                ring_top: Some("#163326e8".into()),
                ring_bottom: Some("#122a1fe8".into()),
                ring_rim_bottom: Some("#45b878".into()),
                ring_divider: Some("#98e8ba".into()),
                ring_shadow: Some("#010804".into()),
                capsule_bottom: Some("#0d1f17f0".into()),
                capsule_border: Some("#9cebc0".into()),
                text_shadow: Some("#020a05".into()),
            },
        },
        BuiltinTheme {
            id: "midnight",
            label: "Midnight Blue",
            description: "Inky cobalt glass with crisp electric sky highlights",
            colors: ColorsConfig {
                ring: "#101a2ee8".into(),
                ring_border: "#a8d4ff".into(),
                center: "#0b1322eb".into(),
                capsule: "#192942f0".into(),
                text: "#f0f6ff".into(),
                muted: "#9db6d6".into(),
                accent: "#5cb8ff".into(),
                accents: AccentPalette {
                    files: "#5cb8ff".into(),
                    apps: "#7ae0c8".into(),
                    windows: "#ff7888".into(),
                    clipboard: "#faad8a".into(),
                    settings: "#8fa8ff".into(),
                },
                ring_top: Some("#182842e8".into()),
                ring_bottom: Some("#132038e8".into()),
                ring_rim_bottom: Some("#4a8ef5".into()),
                ring_divider: Some("#9cc8ff".into()),
                ring_shadow: Some("#010308".into()),
                capsule_bottom: Some("#0f192bf0".into()),
                capsule_border: Some("#9ecbff".into()),
                text_shadow: Some("#02060d".into()),
            },
        },
        BuiltinTheme {
            id: "rose",
            label: "Rose Quartz",
            description: "Warm plum charcoal with soft coral and rose-gold tones",
            colors: ColorsConfig {
                ring: "#26111de8".into(),
                ring_border: "#ffc7dc".into(),
                center: "#1c0c15eb".into(),
                capsule: "#381a2bf0".into(),
                text: "#fff2f7".into(),
                muted: "#d4a5ba".into(),
                accent: "#ff8cb4".into(),
                accents: AccentPalette {
                    files: "#ff8cb4".into(),
                    apps: "#ffbf75".into(),
                    windows: "#ff6b78".into(),
                    clipboard: "#faad8a".into(),
                    settings: "#f598d8".into(),
                },
                ring_top: Some("#3d1c2ee8".into()),
                ring_bottom: Some("#301625e8".into()),
                ring_rim_bottom: Some("#e8649b".into()),
                ring_divider: Some("#ffbfd8".into()),
                ring_shadow: Some("#080105".into()),
                capsule_bottom: Some("#210f19f0".into()),
                capsule_border: Some("#ffc4dc".into()),
                text_shadow: Some("#0a0206".into()),
            },
        },
        BuiltinTheme {
            id: "violet",
            label: "Amethyst",
            description: "Dusk amethyst glass with lavender and mint highlights",
            colors: ColorsConfig {
                ring: "#1a112ee8".into(),
                ring_border: "#d8c4ff".into(),
                center: "#120b21eb".into(),
                capsule: "#261a40f0".into(),
                text: "#f6f2ff".into(),
                muted: "#b6a5d9".into(),
                accent: "#b38cff".into(),
                accents: AccentPalette {
                    files: "#b38cff".into(),
                    apps: "#ff9be0".into(),
                    windows: "#ff7a8a".into(),
                    clipboard: "#faad8a".into(),
                    settings: "#c799ff".into(),
                },
                ring_top: Some("#291c45e8".into()),
                ring_bottom: Some("#211638e8".into()),
                ring_rim_bottom: Some("#8c61ff".into()),
                ring_divider: Some("#cfbaff".into()),
                ring_shadow: Some("#04010a".into()),
                capsule_bottom: Some("#160f26f0".into()),
                capsule_border: Some("#d4bfff".into()),
                text_shadow: Some("#06020f".into()),
            },
        },
        BuiltinTheme {
            id: "monochrome",
            label: "Monochrome",
            description: "High-contrast obsidian and silver grayscale",
            colors: ColorsConfig {
                ring: "#1c1e22e8".into(),
                ring_border: "#e6e9ef".into(),
                center: "#14161aeb".into(),
                capsule: "#292b30f0".into(),
                text: "#ffffff".into(),
                muted: "#a8b0bd".into(),
                accent: "#d8dde6".into(),
                accents: AccentPalette {
                    files: "#d8dde6".into(),
                    apps: "#eceff4".into(),
                    windows: "#b8c0cc".into(),
                    clipboard: "#e0e4ec".into(),
                    settings: "#cfd5e0".into(),
                },
                ring_top: Some("#2e3035e8".into()),
                ring_bottom: Some("#23252ae8".into()),
                ring_rim_bottom: Some("#88909e".into()),
                ring_divider: Some("#d4d8e0".into()),
                ring_shadow: Some("#000000".into()),
                capsule_bottom: Some("#191b1ff0".into()),
                capsule_border: Some("#d8dce3".into()),
                text_shadow: Some("#050608".into()),
            },
        },
    ]
}

pub fn builtin_theme(name: &str) -> Option<BuiltinTheme> {
    let key = normalize_theme_name(name);
    let canonical = match key.as_str() {
        "amber" | "amber_(default)" | "amber_default" | "default_dark" => "default",
        "midnight_blue" => "midnight",
        "rose_quartz" => "rose",
        "amethyst" => "violet",
        other => other,
    };
    builtin_themes()
        .into_iter()
        .find(|t| t.id == canonical || normalize_theme_name(t.label) == key)
}

pub fn normalize_theme_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

pub fn validate_theme_name(name: &str) -> Result<String, String> {
    let clean = name.trim();
    if clean.is_empty() {
        return Err("Theme name cannot be empty".into());
    }
    if clean.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || clean.contains("..") {
        return Err("Theme name contains invalid path characters".into());
    }
    Ok(normalize_theme_name(clean))
}

#[derive(Debug, Clone)]
pub struct ParsedTheme {
    pub name: Option<String>,
    pub colors: ColorsConfig,
}

fn with_alpha_if_6(hex: &str, alpha_suffix: &str) -> String {
    let h = hex.trim();
    if h.starts_with('#')
        && (h.len() == 7 || h.len() == 4)
        && let Ok(c) = Color::from_hex(h)
    {
        let a = u8::from_str_radix(alpha_suffix, 16).unwrap_or(0xe8);
        return format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            (c.r * 255.0).round() as u8,
            (c.g * 255.0).round() as u8,
            (c.b * 255.0).round() as u8,
            a
        );
    }
    h.to_string()
}

fn normalize_linux_colors_json(colors_val: &Value) -> Value {
    let Some(obj) = colors_val.as_object() else {
        return colors_val.clone();
    };
    let is_nested = obj.get("ring").is_some_and(Value::is_object)
        || obj.get("capsule").is_some_and(Value::is_object)
        || obj.get("text").is_some_and(Value::is_object);
    if !is_nested {
        return colors_val.clone();
    }

    let mut out = serde_json::Map::new();
    if let Some(ring) = obj.get("ring").and_then(Value::as_object) {
        if let Some(mid) = ring.get("mid").or_else(|| ring.get("top")).and_then(Value::as_str) {
            out.insert("ring".into(), Value::String(with_alpha_if_6(mid, "e8")));
        }
        if let Some(top) = ring.get("top").and_then(Value::as_str) {
            out.insert("ring_top".into(), Value::String(with_alpha_if_6(top, "e8")));
        }
        if let Some(bot) = ring.get("bottom").and_then(Value::as_str) {
            out.insert("ring_bottom".into(), Value::String(with_alpha_if_6(bot, "e8")));
        }
        if let Some(rim_top) = ring.get("rim_top").and_then(Value::as_str) {
            out.insert("ring_border".into(), Value::String(rim_top.to_string()));
        }
        if let Some(rim_bot) = ring.get("rim_bottom").and_then(Value::as_str) {
            out.insert("ring_rim_bottom".into(), Value::String(rim_bot.to_string()));
        }
        if let Some(div) = ring.get("divider").and_then(Value::as_str) {
            out.insert("ring_divider".into(), Value::String(div.to_string()));
        }
        if let Some(sh) = ring.get("shadow").and_then(Value::as_str) {
            out.insert("ring_shadow".into(), Value::String(sh.to_string()));
        }
        if let Some(mid) = ring.get("mid").or_else(|| ring.get("bottom")).and_then(Value::as_str) {
            out.insert("center".into(), Value::String(with_alpha_if_6(mid, "eb")));
        }
    } else if let Some(s) = obj.get("ring").and_then(Value::as_str) {
        out.insert("ring".into(), Value::String(s.to_string()));
    }

    if let Some(cap) = obj.get("capsule").and_then(Value::as_object) {
        if let Some(top) = cap.get("top").and_then(Value::as_str) {
            out.insert("capsule".into(), Value::String(with_alpha_if_6(top, "f0")));
        }
        if let Some(bot) = cap.get("bottom").and_then(Value::as_str) {
            out.insert("capsule_bottom".into(), Value::String(with_alpha_if_6(bot, "f0")));
        }
        if let Some(brd) = cap.get("border").and_then(Value::as_str) {
            out.insert("capsule_border".into(), Value::String(brd.to_string()));
        }
    } else if let Some(s) = obj.get("capsule").and_then(Value::as_str) {
        out.insert("capsule".into(), Value::String(s.to_string()));
    }

    if let Some(txt) = obj.get("text").and_then(Value::as_object) {
        if let Some(p) = txt.get("primary").and_then(Value::as_str) {
            out.insert("text".into(), Value::String(p.to_string()));
        }
        if let Some(m) = txt.get("suggestion").and_then(Value::as_str) {
            out.insert("muted".into(), Value::String(m.to_string()));
        }
        if let Some(s) = txt.get("shadow").and_then(Value::as_str) {
            out.insert("text_shadow".into(), Value::String(s.to_string()));
        }
    } else if let Some(s) = obj.get("text").and_then(Value::as_str) {
        out.insert("text".into(), Value::String(s.to_string()));
    }

    if let Some(acc) = obj.get("accents").and_then(Value::as_object) {
        let mut a = acc.clone();
        if let Some(files) = acc.get("files").and_then(Value::as_str) {
            out.entry("accent".to_string()).or_insert_with(|| Value::String(files.to_string()));
        }
        if !a.contains_key("settings")
            && let Some(custom) = acc.get("custom").and_then(Value::as_str)
        {
            a.insert("settings".into(), Value::String(custom.to_string()));
        }
        out.insert("accents".into(), Value::Object(a));
    }

    Value::Object(out)
}

/// Parses standalone theme JSON:
/// - `{ "colors": { ... }, "name": "..." }` (wrapped, either Windows flat or Linux nested)
/// - `{ "ring": "...", "text": "...", ... }` (flat or partial merged onto base defaults)
pub fn parse_theme_json(raw: &str, base: &ColorsConfig) -> Result<ParsedTheme, String> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let v: Value = serde_json::from_str(raw).map_err(|e| format!("Invalid theme JSON: {e}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| "Theme JSON must be a JSON object".to_string())?;

    let name = obj
        .get("name")
        .or_else(|| obj.get("theme"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let colors_val_raw = obj.get("colors").unwrap_or(&v);
    let colors_val = normalize_linux_colors_json(colors_val_raw);
    let colors_obj = colors_val
        .as_object()
        .ok_or_else(|| "Theme colors section must be an object".to_string())?;

    let known_color_keys = [
        "ring",
        "ring_border",
        "ring_top",
        "ring_bottom",
        "ring_rim_bottom",
        "ring_divider",
        "ring_shadow",
        "center",
        "capsule",
        "capsule_bottom",
        "capsule_border",
        "text",
        "text_shadow",
        "muted",
        "accent",
        "accents",
    ];
    let has_any_color = colors_obj
        .keys()
        .any(|k| known_color_keys.contains(&k.as_str()));
    if !has_any_color {
        return Err("JSON file does not contain any theme color keys".into());
    }

    let mut base_val = serde_json::to_value(base).map_err(|e| e.to_string())?;
    merge_json_objects(&mut base_val, &colors_val);
    let parsed: ColorsConfig =
        serde_json::from_value(base_val).map_err(|e| format!("Invalid theme color values: {e}"))?;
    parsed.validate()?;
    Ok(ParsedTheme {
        name,
        colors: parsed,
    })
}

fn merge_json_objects(dst: &mut Value, src: &Value) {
    if let (Some(dst_map), Some(src_map)) = (dst.as_object_mut(), src.as_object()) {
        for (k, v) in src_map {
            if v.is_object()
                && let Some(existing) = dst_map.get_mut(k)
                && existing.is_object()
            {
                merge_json_objects(existing, v);
                continue;
            }
            dst_map.insert(k.clone(), v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtin_themes_are_valid_unique_and_include_light_green() {
        let themes = builtin_themes();
        assert_eq!(themes.len(), 7);
        let mut ids = HashSet::new();
        for theme in &themes {
            assert!(ids.insert(theme.id), "duplicate id {}", theme.id);
            theme.colors.validate().expect("builtin theme valid");
        }
        assert!(builtin_theme("light_green").is_some());
        assert!(builtin_theme("Light Green").is_some());
        assert_eq!(builtin_theme("default").unwrap().colors.accents.clipboard, "#faad8a");
    }

    #[test]
    fn parses_flat_wrapped_and_partial_theme_json_and_rejects_non_themes() {
        let base = ColorsConfig::default();
        let flat = r##"{"ring":"#112233aa","accent":"#44cc88"}"##;
        let parsed = parse_theme_json(flat, &base).unwrap();
        assert_eq!(parsed.colors.ring, "#112233aa");
        assert_eq!(parsed.colors.accent, "#44cc88");
        assert_eq!(parsed.colors.text, base.text);

        let wrapped = r##"{"name":"Ocean","colors":{"center":"#001122ee","accents":{"clipboard":"#ff8844"}}}"##;
        let parsed2 = parse_theme_json(wrapped, &base).unwrap();
        assert_eq!(parsed2.name.as_deref(), Some("Ocean"));
        assert_eq!(parsed2.colors.center, "#001122ee");
        assert_eq!(parsed2.colors.accents.clipboard, "#ff8844");

        let menu_not_theme = r#"{"title":"Power","items":[]}"#;
        assert!(parse_theme_json(menu_not_theme, &base).is_err());
    }

    #[test]
    fn matches_amber_alias_and_parses_linux_nested_theme_json() {
        let amber = builtin_theme("Amber").expect("Amber alias resolves to default");
        assert_eq!(amber.id, "default");
        assert_eq!(amber.colors.accent, "#ff9e47");
        assert_eq!(amber.colors.ring_top.as_deref(), Some("#422614e8"));

        let linux_json = r##"{
          "name": "Amber Linux",
          "colors": {
            "ring": {
              "top": "#422614",
              "mid": "#26160d",
              "bottom": "#2e1a0f",
              "rim_top": "#ffd19e",
              "rim_bottom": "#f59447",
              "divider": "#ffd6ad",
              "shadow": "#030100"
            },
            "capsule": { "top": "#362114", "bottom": "#21130d", "border": "#ffd1a8" },
            "text": { "primary": "#fff7f0", "suggestion": "#d7b9a4", "shadow": "#090402" },
            "accents": { "files": "#ff9e47", "custom": "#ffb86b" }
          }
        }"##;
        let parsed = parse_theme_json(linux_json, &ColorsConfig::default()).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("Amber Linux"));
        assert_eq!(parsed.colors.ring_border, "#ffd19e");
        assert_eq!(parsed.colors.ring_rim_bottom.as_deref(), Some("#f59447"));
        assert_eq!(parsed.colors.capsule_border.as_deref(), Some("#ffd1a8"));
        assert_eq!(parsed.colors.text, "#fff7f0");
        assert_eq!(parsed.colors.muted, "#d7b9a4");
        assert_eq!(parsed.colors.accents.settings, "#ffb86b");
    }
}
