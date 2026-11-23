use crate::modules::{BuiltinSubmenu, InstalledModules};
use crate::paths;
use crate::themes::{
    ColorsConfig, builtin_theme, builtin_themes, normalize_theme_name, parse_theme_json,
    validate_theme_name,
};
use crate::win_keys::{
    Modifiers, ParsedKey, format_parsed_key, matches_vk_event, parse_global_hotkey, parse_key_spec,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const BUILTIN_CLIPBOARD_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="-4 -4 32 32" fill="none" stroke="#faad8a" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
  <rect x="8" y="2" width="8" height="4" rx="1" ry="1"/>
  <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>
  <path d="M9 14l2 2 4-4"/>
</svg>"##;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowLayout {
    #[default]
    #[serde(alias = "rings")]
    Ring,
    Box,
}

impl OverflowLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ring => "Ring",
            Self::Box => "Box",
        }
    }
    pub fn toggle(self) -> Self {
        match self {
            Self::Ring => Self::Box,
            Self::Box => Self::Ring,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    #[default]
    #[serde(alias = "frosted", alias = "blur")]
    Glass,
    Translucent,
    Solid,
}

impl Material {
    pub fn label(self) -> &'static str {
        match self {
            Self::Glass => "Frosted Glass",
            Self::Translucent => "Translucent",
            Self::Solid => "Solid",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Glass => Self::Translucent,
            Self::Translucent => Self::Solid,
            Self::Solid => Self::Glass,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadingStyle {
    #[serde(rename = "2d", alias = "two_d")]
    TwoD,
    #[default]
    #[serde(rename = "3d", alias = "three_d")]
    ThreeD,
}

impl LoadingStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::TwoD => "2D Pulse",
            Self::ThreeD => "3D Tumbling",
        }
    }
    pub fn toggle(self) -> Self {
        match self {
            Self::TwoD => Self::ThreeD,
            Self::ThreeD => Self::TwoD,
        }
    }
}

pub const DEFAULT_TRANSLUCENCY: u8 = 60;
pub const DEFAULT_BLUR: u8 = 20;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppearanceConfig {
    pub theme: String,
    pub layout: OverflowLayout,
    pub material: Material,
    pub translucency: u8,
    pub blur: u8,
    pub high_contrast: bool,
    pub scale: f64,
    pub reduced_motion: bool,
    pub loading_animation: LoadingStyle,
    pub font: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: "default".into(),
            layout: OverflowLayout::Box,
            material: Material::Glass,
            translucency: DEFAULT_TRANSLUCENCY,
            blur: DEFAULT_BLUR,
            high_contrast: false,
            scale: 1.0,
            reduced_motion: false,
            loading_animation: LoadingStyle::ThreeD,
            font: String::new(),
        }
    }
}

impl<'de> Deserialize<'de> for AppearanceConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PctOrBool {
            Bool(bool),
            Int(i64),
            Float(f64),
        }

        impl PctOrBool {
            fn into_blur_pct(self) -> u8 {
                match self {
                    Self::Bool(true) => DEFAULT_BLUR,
                    Self::Bool(false) => 0,
                    Self::Int(v) => v.clamp(0, 100) as u8,
                    Self::Float(f) if f.is_finite() => f.round().clamp(0.0, 100.0) as u8,
                    Self::Float(_) => DEFAULT_BLUR,
                }
            }

            fn into_translucency_pct(self) -> u8 {
                match self {
                    Self::Bool(true) => DEFAULT_TRANSLUCENCY,
                    Self::Bool(false) => 0,
                    Self::Int(v) => v.clamp(0, 100) as u8,
                    Self::Float(f) if f.is_finite() => f.round().clamp(0.0, 100.0) as u8,
                    Self::Float(_) => DEFAULT_TRANSLUCENCY,
                }
            }
        }

        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Raw {
            theme: Option<String>,
            #[serde(alias = "overflow_layout")]
            layout: Option<OverflowLayout>,
            material: Option<Material>,
            #[serde(alias = "transparency")]
            translucency: Option<PctOrBool>,
            blur: Option<PctOrBool>,
            high_contrast: Option<bool>,
            scale: Option<f64>,
            reduced_motion: Option<bool>,
            loading_animation: Option<LoadingStyle>,
            font: Option<String>,
        }

        let raw = Raw::deserialize(deserializer)?;
        let def = Self::default();
        let translucency = match raw.translucency {
            Some(v) => v.into_translucency_pct(),
            None if raw.material == Some(Material::Solid) => 0,
            None => def.translucency,
        };
        let blur = match raw.blur {
            Some(v) => v.into_blur_pct(),
            None if raw.material == Some(Material::Translucent) => 0,
            None => def.blur,
        };
        let mut cfg = Self {
            theme: raw.theme.unwrap_or(def.theme),
            layout: raw.layout.unwrap_or(def.layout),
            material: raw.material.unwrap_or(def.material),
            translucency,
            blur,
            high_contrast: raw.high_contrast.unwrap_or(def.high_contrast),
            scale: raw.scale.unwrap_or(def.scale),
            reduced_motion: raw.reduced_motion.unwrap_or(def.reduced_motion),
            loading_animation: raw.loading_animation.unwrap_or(def.loading_animation),
            font: raw.font.unwrap_or(def.font),
        };
        cfg.validate();
        Ok(cfg)
    }
}

impl AppearanceConfig {
    pub fn sync_material(&mut self) {
        self.translucency = self.translucency.min(100);
        self.blur = self.blur.min(100);
        self.material = if self.translucency == 0 {
            Material::Solid
        } else if self.blur > 0 {
            Material::Glass
        } else {
            Material::Translucent
        };
    }

    pub fn validate(&mut self) {
        if !self.scale.is_finite() {
            self.scale = 1.0;
        }
        self.scale = self.scale.clamp(0.6, 2.0);
        if self.theme.trim().is_empty() {
            self.theme = "default".into();
        }
        self.sync_material();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyList(pub Vec<String>);

impl KeyList {
    pub fn one(s: impl Into<String>) -> Self {
        let val = s.into();
        if val.trim().is_empty() {
            Self(Vec::new())
        } else {
            Self(vec![val])
        }
    }

    pub fn many(items: &[&str]) -> Self {
        Self(items.iter().map(|s| (*s).to_string()).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|s| s.trim().is_empty())
    }

    pub fn display(&self) -> String {
        let non_empty: Vec<&str> = self
            .0
            .iter()
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .collect();
        if non_empty.is_empty() {
            "Unbound".into()
        } else {
            non_empty.join(", ")
        }
    }

    pub fn primary(&self) -> Option<&str> {
        self.0
            .iter()
            .map(String::as_str)
            .find(|s| !s.trim().is_empty())
    }

    pub fn matches_vk(&self, mods: Modifiers, vk: u32, ch: Option<char>) -> bool {
        self.0.iter().any(|spec| {
            parse_key_spec(spec)
                .map(|p| matches_vk_event(&p, mods, vk, ch))
                .unwrap_or(false)
        })
    }

    pub fn validate_nav(&self, field: &str) -> Result<(), String> {
        for spec in &self.0 {
            if spec.trim().is_empty() {
                continue;
            }
            parse_key_spec(spec).map_err(|e| format!("keybinds.{field}: {e}"))?;
        }
        Ok(())
    }

    pub fn validate_global(&self, field: &str) -> Result<(), String> {
        for spec in &self.0 {
            if spec.trim().is_empty() {
                continue;
            }
            parse_global_hotkey(spec).map_err(|e| format!("keybinds.{field}: {e}"))?;
        }
        Ok(())
    }
}

impl Serialize for KeyList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.len() == 1 {
            serializer.serialize_str(&self.0[0])
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for KeyList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrMany {
            One(String),
            Many(Vec<String>),
        }
        match OneOrMany::deserialize(deserializer)? {
            OneOrMany::One(s) => {
                if s.trim().is_empty() {
                    Ok(KeyList(Vec::new()))
                } else {
                    Ok(KeyList(vec![s]))
                }
            }
            OneOrMany::Many(v) => Ok(KeyList(
                v.into_iter().filter(|s| !s.trim().is_empty()).collect(),
            )),
        }
    }
}

fn default_true_flag() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindsConfig {
    pub main_menu: KeyList,
    #[serde(default)]
    pub menu_keybinds: BTreeMap<String, KeyList>,
    #[serde(default = "default_true_flag")]
    pub number_shortcuts: bool,
    pub back: KeyList,
    pub home: KeyList,
    pub activate: KeyList,
    pub secondary_activate: KeyList,
    pub open_current_folder: KeyList,
    pub next: KeyList,
    pub previous: KeyList,
    pub page_next: KeyList,
    pub page_previous: KeyList,
    pub accept_completion: KeyList,
    pub undo_completion: KeyList,
    pub clear_query: KeyList,
    pub help: KeyList,
}

impl Default for KeybindsConfig {
    fn default() -> Self {
        Self {
            main_menu: KeyList::one("Win+e"),
            menu_keybinds: BTreeMap::new(),
            number_shortcuts: true,
            back: KeyList::one("Escape"),
            home: KeyList::one("Shift+Escape"),
            activate: KeyList::one("Enter"),
            secondary_activate: KeyList::one("Shift+Enter"),
            open_current_folder: KeyList::one("Alt+Enter"),
            next: KeyList::many(&["Down", "Tab"]),
            previous: KeyList::many(&["Up", "Shift+Tab"]),
            page_next: KeyList::one("PageDown"),
            page_previous: KeyList::one("PageUp"),
            accept_completion: KeyList::one("Right"),
            undo_completion: KeyList::one("Left"),
            clear_query: KeyList::one("Ctrl+u"),
            help: KeyList::many(&["F1", "?"]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub id: i32,
    pub target_menu: Option<String>, // None = Main Menu
    pub label: String,
    pub spec: String,
    pub parsed: ParsedKey,
}

impl KeybindsConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.main_menu.validate_global("main_menu")?;
        for (menu_id, keys) in &self.menu_keybinds {
            keys.validate_global(&format!("menu_keybinds.{menu_id}"))?;
        }
        let mut seen: std::collections::HashMap<(u32, u32), String> =
            std::collections::HashMap::new();
        for spec in &self.main_menu.0 {
            if let Ok(parsed) = parse_global_hotkey(spec) {
                seen.insert((parsed.mods.to_register_hotkey_flags(), parsed.vk), "Main Ring".into());
            }
        }
        for (menu_id, keys) in &self.menu_keybinds {
            for spec in &keys.0 {
                if let Ok(parsed) = parse_global_hotkey(spec)
                    && let Some(prev) = seen.insert((parsed.mods.to_register_hotkey_flags(), parsed.vk), menu_id.clone())
                {
                    return Err(format!(
                        "Conflicting global hotkey '{spec}': assigned to both '{prev}' and '{menu_id}'"
                    ));
                }
            }
        }
        for (name, keys) in [
            ("back", &self.back),
            ("home", &self.home),
            ("activate", &self.activate),
            ("secondary_activate", &self.secondary_activate),
            ("open_current_folder", &self.open_current_folder),
            ("next", &self.next),
            ("previous", &self.previous),
            ("page_next", &self.page_next),
            ("page_previous", &self.page_previous),
            ("accept_completion", &self.accept_completion),
            ("undo_completion", &self.undo_completion),
            ("clear_query", &self.clear_query),
            ("help", &self.help),
        ] {
            keys.validate_nav(name)?;
        }
        Ok(())
    }

    pub fn check_global_conflict(&self, target_menu: Option<&str>, spec: &str) -> Result<(), String> {
        let parsed = parse_global_hotkey(spec)?;
        if target_menu.is_some() {
            for existing in &self.main_menu.0 {
                if let Ok(ex) = parse_global_hotkey(existing)
                    && ex.mods.to_register_hotkey_flags() == parsed.mods.to_register_hotkey_flags()
                    && ex.vk == parsed.vk
                {
                    return Err(format!("'{spec}' is already bound to Main Ring"));
                }
            }
        }
        for (id, keys) in &self.menu_keybinds {
            if target_menu == Some(id.as_str()) {
                continue;
            }
            for existing in &keys.0 {
                if let Ok(ex) = parse_global_hotkey(existing)
                    && ex.mods.to_register_hotkey_flags() == parsed.mods.to_register_hotkey_flags()
                    && ex.vk == parsed.vk
                {
                    return Err(format!("'{spec}' is already bound to '{id}'"));
                }
            }
        }
        Ok(())
    }

    pub fn canonicalize(&mut self) {
        fn canon_list(list: &mut KeyList) {
            list.0 = list
                .0
                .iter()
                .filter_map(|s| parse_key_spec(s).ok().map(format_parsed_key))
                .collect();
        }
        canon_list(&mut self.main_menu);
        for keys in self.menu_keybinds.values_mut() {
            canon_list(keys);
        }
        self.menu_keybinds.retain(|_, v| !v.is_empty());
    }

    pub fn collect_active_global_hotkeys(
        &self,
        installed: &InstalledModules,
        menus: &BTreeMap<String, MenuConfig>,
    ) -> Vec<HotkeyBinding> {
        let mut out = Vec::new();
        let mut next_id = 1i32;

        for spec in &self.main_menu.0 {
            if let Ok(parsed) = parse_global_hotkey(spec) {
                out.push(HotkeyBinding {
                    id: next_id,
                    target_menu: None,
                    label: "Main Ring".into(),
                    spec: format_parsed_key(parsed),
                    parsed,
                });
                next_id += 1;
            }
        }

        for (menu_id, keys) in &self.menu_keybinds {
            if let Some(sub) = BuiltinSubmenu::from_id(menu_id) {
                if !installed.is_installed(sub) {
                    continue;
                }
            } else if !menus.contains_key(menu_id) {
                continue;
            }
            for spec in &keys.0 {
                if let Ok(parsed) = parse_global_hotkey(spec) {
                    let label = BuiltinSubmenu::from_id(menu_id)
                        .map(|s| s.title().to_string())
                        .or_else(|| menus.get(menu_id).map(|m| m.title.clone()))
                        .unwrap_or_else(|| menu_id.clone());
                    out.push(HotkeyBinding {
                        id: next_id,
                        target_menu: Some(menu_id.clone()),
                        label,
                        spec: format_parsed_key(parsed),
                        parsed,
                    });
                    next_id += 1;
                }
            }
        }

        out
    }

    pub fn menu_hotkey(&self, menu_id: &str) -> KeyList {
        self.menu_keybinds
            .get(menu_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_menu_hotkey(&mut self, menu_id: &str, keys: KeyList) {
        if keys.is_empty() {
            self.menu_keybinds.remove(menu_id);
        } else {
            self.menu_keybinds.insert(menu_id.to_string(), keys);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MainMenuEntryDe {
    Id(String),
    Obj(MainMenuEntry),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MainMenuEntry {
    #[serde(alias = "name")]
    pub menu: String,
    #[serde(default, alias = "label", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
}

impl MainMenuEntry {
    pub fn new(menu: impl Into<String>) -> Self {
        Self {
            menu: menu.into(),
            title: None,
            icon: None,
            accent: None,
        }
    }

    pub fn with_icon(menu: impl Into<String>, icon: impl Into<String>) -> Self {
        Self {
            menu: menu.into(),
            title: None,
            icon: Some(icon.into()),
            accent: None,
        }
    }

    pub fn resolved_title(&self, custom_menus: &BTreeMap<String, MenuConfig>) -> String {
        if let Some(t) = &self.title
            && !t.trim().is_empty()
        {
            return t.clone();
        }
        if let Some(sub) = BuiltinSubmenu::from_id(&self.menu) {
            return sub.title().to_string();
        }
        if let Some(m) = custom_menus.get(&self.menu)
            && !m.title.trim().is_empty()
        {
            return m.title.clone();
        }
        self.menu.clone()
    }
}

pub fn deserialize_main_menu<'de, D>(deserializer: D) -> Result<Vec<MainMenuEntry>, D::Error>
where
    D: Deserializer<'de>,
{
    let items: Vec<MainMenuEntryDe> = Vec::deserialize(deserializer)?;
    Ok(items
        .into_iter()
        .map(|item| match item {
            MainMenuEntryDe::Id(id) => MainMenuEntry::new(id),
            MainMenuEntryDe::Obj(entry) => entry,
        })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomMenuLayout {
    #[default]
    #[serde(alias = "default", alias = "auto")]
    Ring,
    #[serde(
        alias = "dropdown",
        alias = "pick",
        alias = "pick_and_choose",
        alias = "choose",
        alias = "select"
    )]
    List,
    #[serde(alias = "string", alias = "text", alias = "prompt")]
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct MenuActionSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submenu: Option<SubmenuTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_menu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub secret: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reload: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub keep_open: bool,
}

impl<'de> Deserialize<'de> for MenuActionSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Obj {
            command: Option<String>,
            submenu: Option<SubmenuTarget>,
            open_menu: Option<String>,
            input: Option<String>,
            input_placeholder: Option<String>,
            #[serde(alias = "password")]
            secret: bool,
            #[serde(alias = "update")]
            reload: bool,
            keep_open: bool,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Cmd(String),
            Obj(Obj),
        }
        match Repr::deserialize(deserializer)? {
            Repr::Cmd(cmd) => Ok(Self {
                command: Some(cmd),
                ..Default::default()
            }),
            Repr::Obj(o) => Ok(Self {
                command: o.command,
                submenu: o.submenu,
                open_menu: o.open_menu,
                input: o.input,
                input_placeholder: o.input_placeholder,
                secret: o.secret,
                reload: o.reload,
                keep_open: o.keep_open,
            }),
        }
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SubmenuTarget {
    Inline(Box<MenuConfig>),
    Ref(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MenuItemConfig {
    #[serde(alias = "name")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(default, alias = "left", skip_serializing_if = "Option::is_none")]
    pub left_click: Option<MenuActionSpec>,
    #[serde(default, alias = "right", skip_serializing_if = "Option::is_none")]
    pub right_click: Option<MenuActionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submenu: Option<SubmenuTarget>,
}

impl MenuItemConfig {
    pub fn primary_action(&self) -> Option<MenuActionSpec> {
        if let Some(a) = &self.left_click {
            return Some(a.clone());
        }
        if self.command.is_some() || self.submenu.is_some() {
            return Some(MenuActionSpec {
                command: self.command.clone(),
                submenu: self.submenu.clone(),
                ..Default::default()
            });
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MenuConfig {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    pub layout: CustomMenuLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading_animation: Option<LoadingStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_open: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_submit: Option<MenuActionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub secret: bool,
    #[serde(default)]
    pub items: Vec<MenuItemConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub appearance: AppearanceConfig,
    pub colors: ColorsConfig,
    pub keybinds: KeybindsConfig,
    #[serde(deserialize_with = "deserialize_main_menu")]
    pub main_menu: Vec<MainMenuEntry>,
    pub menus: BTreeMap<String, MenuConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::default_for_installed(&InstalledModules::all_for_tests(PathBuf::from(".")))
    }
}

impl AppConfig {
    pub fn default_for_installed(installed: &InstalledModules) -> Self {
        let main_menu = BuiltinSubmenu::ALL
            .iter()
            .copied()
            .filter(|&sub| installed.is_installed(sub))
            .map(|sub| MainMenuEntry::new(sub.id()))
            .collect();

        Self {
            appearance: AppearanceConfig::default(),
            colors: ColorsConfig::default(),
            keybinds: KeybindsConfig::default(),
            main_menu,
            menus: BTreeMap::new(),
        }
    }

    pub fn effective_colors(&self, store: &ConfigStore) -> ColorsConfig {
        let mut base = store
            .resolve_theme_colors(&self.appearance.theme)
            .unwrap_or_default();
        if self.appearance.theme == "default" {
            return self.colors.clone();
        }
        let def = ColorsConfig::default();
        if self.colors.ring != def.ring {
            base.ring = self.colors.ring.clone();
            base.ring_top = None;
            base.ring_bottom = None;
        }
        if self.colors.ring_top != def.ring_top {
            base.ring_top = self.colors.ring_top.clone();
        }
        if self.colors.ring_bottom != def.ring_bottom {
            base.ring_bottom = self.colors.ring_bottom.clone();
        }
        if self.colors.ring_border != def.ring_border {
            base.ring_border = self.colors.ring_border.clone();
            base.ring_rim_bottom = None;
            base.ring_divider = None;
            base.capsule_border = None;
        }
        if self.colors.ring_rim_bottom != def.ring_rim_bottom {
            base.ring_rim_bottom = self.colors.ring_rim_bottom.clone();
        }
        if self.colors.ring_divider != def.ring_divider {
            base.ring_divider = self.colors.ring_divider.clone();
        }
        if self.colors.ring_shadow != def.ring_shadow {
            base.ring_shadow = self.colors.ring_shadow.clone();
        }
        if self.colors.center != def.center {
            base.center = self.colors.center.clone();
        }
        if self.colors.capsule != def.capsule {
            base.capsule = self.colors.capsule.clone();
            base.capsule_bottom = None;
        }
        if self.colors.capsule_bottom != def.capsule_bottom {
            base.capsule_bottom = self.colors.capsule_bottom.clone();
        }
        if self.colors.capsule_border != def.capsule_border {
            base.capsule_border = self.colors.capsule_border.clone();
        }
        if self.colors.text != def.text {
            base.text = self.colors.text.clone();
            base.text_shadow = None;
        }
        if self.colors.text_shadow != def.text_shadow {
            base.text_shadow = self.colors.text_shadow.clone();
        }
        if self.colors.muted != def.muted {
            base.muted = self.colors.muted.clone();
        }
        if self.colors.accent != def.accent {
            base.accent = self.colors.accent.clone();
        }
        if self.colors.accents.files != def.accents.files {
            base.accents.files = self.colors.accents.files.clone();
        }
        if self.colors.accents.apps != def.accents.apps {
            base.accents.apps = self.colors.accents.apps.clone();
        }
        if self.colors.accents.windows != def.accents.windows {
            base.accents.windows = self.colors.accents.windows.clone();
        }
        if self.colors.accents.clipboard != def.accents.clipboard {
            base.accents.clipboard = self.colors.accents.clipboard.clone();
        }
        if self.colors.accents.settings != def.accents.settings {
            base.accents.settings = self.colors.accents.settings.clone();
        }
        base
    }

    pub fn prune_uninstalled(&mut self, installed: &InstalledModules) {
        self.main_menu.retain(|e| {
            if let Some(sub) = BuiltinSubmenu::from_id(&e.menu) {
                installed.is_installed(sub)
            } else {
                true
            }
        });
        if !self.main_menu.iter().any(|e| e.menu == "settings") {
            self.main_menu.push(MainMenuEntry::new("settings"));
        }
        self.keybinds.menu_keybinds.retain(|k, _| {
            if let Some(sub) = BuiltinSubmenu::from_id(k) {
                installed.is_installed(sub)
            } else {
                true
            }
        });
    }

    pub fn validate(&mut self) -> Result<(), String> {
        self.appearance.validate();
        self.colors.validate()?;
        self.keybinds.validate()?;
        self.keybinds.canonicalize();

        let reserved: BTreeSet<&str> = BuiltinSubmenu::ALL.iter().map(|s| s.id()).collect();
        for key in self.menus.keys() {
            if reserved.contains(key.as_str()) {
                return Err(format!(
                    "Custom menu name '{key}' conflicts with a reserved built-in submenu name"
                ));
            }
            if key.trim().is_empty() {
                return Err("Custom menu key cannot be empty".into());
            }
        }

        for entry in &self.main_menu {
            if BuiltinSubmenu::from_id(&entry.menu).is_none()
                && !self.menus.contains_key(&entry.menu)
            {
                return Err(format!(
                    "main_menu references unknown menu '{}'",
                    entry.menu
                ));
            }
        }

        // Ensure menu_keybinds referenced custom menus or built-ins exist
        for menu_id in self.keybinds.menu_keybinds.keys() {
            if BuiltinSubmenu::from_id(menu_id).is_none() && !self.menus.contains_key(menu_id) {
                return Err(format!(
                    "keybinds.menu_keybinds references unknown menu '{menu_id}'"
                ));
            }
        }

        Ok(())
    }
}

fn strip_utf8_bom(raw: &str) -> &str {
    raw.strip_prefix('\u{feff}').unwrap_or(raw)
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    pub fn default_store() -> Self {
        Self {
            dir: paths::config_dir(),
        }
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn config_path(&self) -> PathBuf {
        paths::config_file_in(&self.dir)
    }

    pub fn themes_dir(&self) -> PathBuf {
        paths::themes_dir_in(&self.dir)
    }

    pub fn menus_dir(&self) -> PathBuf {
        paths::menus_dir_in(&self.dir)
    }

    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        fs::create_dir_all(self.themes_dir())?;
        fs::create_dir_all(self.menus_dir())?;
        Ok(())
    }

    pub fn load_or_create(&self, installed: &InstalledModules) -> Result<AppConfig, String> {
        let _ = self.ensure_dirs();
        let cfg_path = self.config_path();
        let existed = cfg_path.is_file();
        let mut cfg = if existed {
            let raw = fs::read_to_string(&cfg_path)
                .map_err(|e| format!("Failed reading {}: {e}", cfg_path.display()))?;
            serde_json::from_str::<AppConfig>(strip_utf8_bom(&raw))
                .map_err(|e| format!("Failed parsing {}: {e}", cfg_path.display()))?
        } else {
            AppConfig::default_for_installed(installed)
        };

        self.load_menus_dir_into(&mut cfg);
        cfg.prune_uninstalled(installed);
        cfg.validate()?;
        if !existed {
            let _ = self.save(&cfg);
        }
        Ok(cfg)
    }

    pub fn load_menus_dir_into(&self, cfg: &mut AppConfig) {
        let load_from = |dir: &Path, skip_reserved_files: bool, cfg: &mut AppConfig| {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file()
                    || path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_none_or(|e| !e.eq_ignore_ascii_case("json"))
                {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if skip_reserved_files
                    && matches!(
                        stem.to_ascii_lowercase().as_str(),
                        "recircles" | "clipboard" | "history" | "frequent" | "pins"
                    )
                {
                    continue;
                }
                if BuiltinSubmenu::from_id(stem).is_some() || cfg.menus.contains_key(stem) {
                    continue;
                }
                if let Ok(raw) = fs::read_to_string(&path)
                    && let Ok(menu_cfg) = serde_json::from_str::<MenuConfig>(strip_utf8_bom(&raw))
                    && (!menu_cfg.title.trim().is_empty()
                        || !menu_cfg.items.is_empty()
                        || menu_cfg.on_open.is_some()
                        || menu_cfg.on_submit.is_some()
                        || menu_cfg.layout == CustomMenuLayout::Input)
                {
                    cfg.menus.insert(stem.to_string(), menu_cfg);
                }
            }
        };
        load_from(&self.menus_dir(), false, cfg);
        load_from(&self.dir, true, cfg);
    }

    pub fn save(&self, cfg: &AppConfig) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let target = self.dir.join("recircles.json");
        let tmp = self
            .dir
            .join(format!("recircles.json.tmp.{}", std::process::id()));
        let json = serde_json::to_string_pretty(cfg)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        {
            let mut f = File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all().ok();
        }
        let _ = fs::remove_file(&target);
        fs::rename(&tmp, &target)?;
        Ok(())
    }

    pub fn generate_default_config(
        &self,
        force: bool,
        installed: &InstalledModules,
    ) -> Result<PathBuf, String> {
        self.ensure_dirs().map_err(|e| e.to_string())?;
        let path = self.dir.join("recircles.json");
        if path.exists() && !force {
            return Err(format!(
                "{} already exists (pass --force to overwrite)",
                path.display()
            ));
        }
        let cfg = AppConfig::default_for_installed(installed);
        self.save(&cfg).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn list_local_themes(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = fs::read_dir(self.themes_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    let norm = normalize_theme_name(stem);
                    if builtin_theme(&norm).is_none() {
                        names.push(norm);
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }

    pub fn resolve_theme_colors(&self, theme_name: &str) -> Option<ColorsConfig> {
        if let Some(builtin) = builtin_theme(theme_name) {
            return Some(builtin.colors);
        }
        let norm = normalize_theme_name(theme_name);
        let path = self.themes_dir().join(format!("{norm}.json"));
        let raw = fs::read_to_string(path).ok()?;
        parse_theme_json(&raw, &ColorsConfig::default())
            .ok()
            .map(|p| p.colors)
    }

    pub fn all_theme_choices(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = builtin_themes()
            .into_iter()
            .map(|t| (t.id.to_string(), t.label.to_string()))
            .collect();
        for local in self.list_local_themes() {
            out.push((local.clone(), local));
        }
        out
    }

    pub fn import_theme_file(&self, src: &Path) -> Result<String, String> {
        self.ensure_dirs().map_err(|e| e.to_string())?;
        if src
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            return self.import_zip_bundle(src, None);
        }
        let raw = fs::read_to_string(src).map_err(|e| format!("Failed reading {}: {e}", src.display()))?;
        let parsed = parse_theme_json(&raw, &ColorsConfig::default())?;
        let raw_name = parsed
            .name
            .or_else(|| src.file_stem().and_then(|s| s.to_str()).map(ToOwned::to_owned))
            .ok_or_else(|| "Could not determine theme name".to_string())?;
        let id = validate_theme_name(&raw_name)?;
        let out_path = self.themes_dir().join(format!("{id}.json"));
        let content = serde_json::to_string_pretty(&parsed.colors).map_err(|e| e.to_string())?;
        fs::write(&out_path, content).map_err(|e| e.to_string())?;
        Ok(id)
    }

    pub fn import_menu_file(
        &self,
        src: &Path,
        installed: &InstalledModules,
    ) -> Result<String, String> {
        self.ensure_dirs().map_err(|e| e.to_string())?;
        if src
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            return self.import_zip_bundle(src, Some(installed));
        }
        let raw = fs::read_to_string(src).map_err(|e| format!("Failed reading {}: {e}", src.display()))?;
        let clean = strip_utf8_bom(&raw);
        let menu_cfg: MenuConfig =
            serde_json::from_str(clean).map_err(|e| format!("Invalid menu JSON: {e}"))?;
        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "Invalid menu filename".to_string())?;
        let id = stem.trim().to_ascii_lowercase().replace(' ', "_");
        if BuiltinSubmenu::from_id(&id).is_some() {
            return Err(format!("Menu name '{id}' conflicts with a built-in submenu"));
        }

        let mut cfg = self.load_or_create(installed)?;
        fs::write(self.menus_dir().join(format!("{id}.json")), &raw)
            .map_err(|e| e.to_string())?;
        cfg.menus.insert(id.clone(), menu_cfg);
        if !cfg.main_menu.iter().any(|e| e.menu == id) {
            cfg.main_menu.push(MainMenuEntry::new(id.clone()));
        }
        cfg.validate()?;
        self.save(&cfg).map_err(|e| e.to_string())?;
        Ok(id)
    }

    pub fn remove_menu(
        &self,
        id: &str,
        installed: &InstalledModules,
    ) -> Result<(), String> {
        let mut cfg = self.load_or_create(installed)?;
        let before_main = cfg.main_menu.len();
        cfg.main_menu.retain(|e| e.menu != id);
        let removed_custom = cfg.menus.remove(id).is_some();
        cfg.keybinds.menu_keybinds.remove(id);
        let file = self.menus_dir().join(format!("{id}.json"));
        let removed_file = if file.exists() {
            fs::remove_file(file).is_ok()
        } else {
            false
        };
        if cfg.main_menu.len() == before_main && !removed_custom && !removed_file {
            return Err(format!("Menu '{id}' was not found"));
        }
        self.save(&cfg).map_err(|e| e.to_string())
    }

    /// Stored-entry ZIP importer with OS-tool fallback for Deflate-compressed ZIP bundles.
    fn import_zip_bundle(
        &self,
        zip_path: &Path,
        installed: Option<&InstalledModules>,
    ) -> Result<String, String> {
        let bytes = fs::read(zip_path).map_err(|e| e.to_string())?;
        let mut raw_entries = extract_stored_zip_entries(&bytes);
        if raw_entries.is_empty() {
            raw_entries = extract_zip_via_system_tool(zip_path)?;
        }

        let mut entries = Vec::with_capacity(raw_entries.len());
        for (name, data) in raw_entries {
            if let Some(rel) = sanitize_zip_rel_path(&name) {
                entries.push((rel, data));
            }
        }
        let entries = unwrap_zip_wrapper_dir(entries);

        let mut last_theme: Option<String> = None;
        let mut imported_menus: Vec<(String, MenuConfig)> = Vec::new();
        let mut imported_icons: Vec<String> = Vec::new();

        for (rel, data) in entries {
            let fname_lower = rel
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                fname_lower.as_str(),
                "recircles.json" | "history.json" | "clipboard.json" | "frequent.json"
            ) {
                continue;
            }

            let is_json = rel
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("json"));
            if is_json && let Ok(raw_text) = std::str::from_utf8(&data) {
                let text = strip_utf8_bom(raw_text);
                if let Ok(parsed) = parse_theme_json(text, &ColorsConfig::default()) {
                    let stem = rel
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("imported_theme");
                    let id = validate_theme_name(parsed.name.as_deref().unwrap_or(stem))?;
                    let content = serde_json::to_string_pretty(&parsed.colors)
                        .map_err(|e| e.to_string())?;
                    fs::write(self.themes_dir().join(format!("{id}.json")), content)
                        .map_err(|e| e.to_string())?;
                    last_theme = Some(id);
                    continue;
                }
                if let Ok(menu) = serde_json::from_str::<MenuConfig>(text)
                    && (!menu.title.trim().is_empty()
                        || !menu.items.is_empty()
                        || menu.on_open.is_some()
                        || menu.on_submit.is_some()
                        || menu.layout == CustomMenuLayout::Input)
                {
                    let stem = rel
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("imported_menu")
                        .trim()
                        .to_ascii_lowercase()
                        .replace(' ', "_");
                    if BuiltinSubmenu::from_id(&stem).is_some() {
                        return Err(format!(
                            "Menu name '{stem}' conflicts with a built-in submenu"
                        ));
                    }
                    fs::write(self.menus_dir().join(format!("{stem}.json")), text.as_bytes())
                        .map_err(|e| e.to_string())?;
                    imported_menus.push((stem, menu));
                    continue;
                }
            }

            let dest = self.dir.join(&rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&dest, &data).map_err(|e| e.to_string())?;
            let is_img = rel
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| {
                    matches!(
                        e.to_ascii_lowercase().as_str(),
                        "png" | "svg" | "ico" | "bmp" | "jpg" | "jpeg" | "webp"
                    )
                });
            if is_img {
                imported_icons.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }

        if !imported_menus.is_empty() {
            let detected = if installed.is_none() {
                Some(InstalledModules::detect_default())
            } else {
                None
            };
            let inst_ref = installed.unwrap_or_else(|| detected.as_ref().unwrap());
            let mut cfg = self.load_or_create(inst_ref)?;
            let single = imported_menus.len() == 1;
            let mut last_menu = String::new();
            for (id, menu) in imported_menus {
                cfg.menus.insert(id.clone(), menu);
                if !cfg.main_menu.iter().any(|e| e.menu == id) {
                    let matched_icon = imported_icons
                        .iter()
                        .find(|p| {
                            Path::new(p)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .is_some_and(|s| s.to_ascii_lowercase().contains(&id))
                        })
                        .or_else(|| (single && imported_icons.len() == 1).then(|| &imported_icons[0]))
                        .cloned();
                    let mut entry = MainMenuEntry::new(id.clone());
                    entry.icon = matched_icon;
                    cfg.main_menu.push(entry);
                }
                last_menu = id;
            }
            cfg.validate()?;
            self.save(&cfg).map_err(|e| e.to_string())?;
            return Ok(last_menu);
        }

        if let Some(theme_id) = last_theme {
            return Ok(theme_id);
        }
        Err("ZIP archive contained no supported .json theme or menu entries".into())
    }

    pub fn load_pins(&self) -> Vec<String> {
        let p = paths::pins_file_in(&self.dir);
        let Ok(raw) = fs::read_to_string(p) else {
            return Vec::new();
        };
        raw.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    pub fn resolve_icon_path(&self, icon_spec: &str) -> Option<PathBuf> {
        let trimmed = icon_spec.trim();
        if trimmed.is_empty() {
            return None;
        }
        let expanded = expand_env_vars(trimmed);
        let p = PathBuf::from(&expanded);
        if p.is_absolute() && p.is_file() {
            return Some(p);
        }
        let in_cfg = self.dir.join(&expanded);
        if in_cfg.is_file() {
            return Some(in_cfg);
        }
        let in_icons = self.dir.join("icons").join(&expanded);
        if in_icons.is_file() {
            return Some(in_icons);
        }
        None
    }
}

/// Pure-Rust reader for stored (compression method 0) ZIP local entries.
fn extract_stored_zip_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 30 <= bytes.len() {
        if &bytes[pos..pos + 4] != b"PK\x03\x04" {
            break;
        }
        let method = u16::from_le_bytes([bytes[pos + 8], bytes[pos + 9]]);
        let comp_size = u32::from_le_bytes([
            bytes[pos + 18],
            bytes[pos + 19],
            bytes[pos + 20],
            bytes[pos + 21],
        ]) as usize;
        let name_len = u16::from_le_bytes([bytes[pos + 26], bytes[pos + 27]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[pos + 28], bytes[pos + 29]]) as usize;
        let data_start = pos + 30 + name_len + extra_len;
        let data_end = data_start.saturating_add(comp_size);
        if data_end > bytes.len() {
            break;
        }
        let name = String::from_utf8_lossy(&bytes[pos + 30..pos + 30 + name_len]).into_owned();
        if method != 0 {
            return Vec::new();
        }
        if !name.ends_with('/') {
            out.push((name, bytes[data_start..data_end].to_vec()));
        }
        pos = data_end;
    }
    out
}

fn sanitize_zip_rel_path(raw: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for seg in raw.split(['/', '\\']) {
        let s = seg.trim();
        if s.is_empty() || s == "." {
            continue;
        }
        if s == ".." || s.contains(':') {
            return None;
        }
        out.push(s);
    }
    if out.as_os_str().is_empty()
        || out
            .components()
            .next()
            .is_some_and(|c| c.as_os_str() == "__MACOSX")
    {
        None
    } else {
        Some(out)
    }
}

fn unwrap_zip_wrapper_dir(entries: Vec<(PathBuf, Vec<u8>)>) -> Vec<(PathBuf, Vec<u8>)> {
    if entries.is_empty() {
        return entries;
    }
    let Some(first_top) = entries[0].0.components().next() else {
        return entries;
    };
    let top_str = first_top.as_os_str().to_string_lossy().to_ascii_lowercase();
    if matches!(top_str.as_str(), "menus" | "themes" | "icons" | "submenus") {
        return entries;
    }
    let all_nested_under_same = entries.iter().all(|(p, _)| {
        let mut comps = p.components();
        comps.next() == Some(first_top) && comps.next().is_some()
    });
    if all_nested_under_same {
        entries
            .into_iter()
            .filter_map(|(p, data)| {
                let mut comps = p.components();
                comps.next()?;
                let rest: PathBuf = comps.collect();
                (!rest.as_os_str().is_empty()).then_some((rest, data))
            })
            .collect()
    } else {
        entries
    }
}

fn extract_zip_via_system_tool(zip_path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    static ZIP_TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = ZIP_TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir().join(format!(
        "recircles-zip-{}-{seq}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp_dir);
    fs::create_dir_all(&tmp_dir).map_err(|e| format!("Failed creating temp dir: {e}"))?;

    let res = (|| -> Result<Vec<(String, Vec<u8>)>, String> {
        let ok = run_zip_extractor(zip_path, &tmp_dir)?;
        if !ok {
            return Err("Could not extract compressed ZIP archive".into());
        }
        let mut out = Vec::new();
        collect_dir_bytes(&tmp_dir, &tmp_dir, &mut out)?;
        Ok(out)
    })();

    let _ = fs::remove_dir_all(&tmp_dir);
    res
}

fn run_zip_extractor(zip_path: &Path, dest: &Path) -> Result<bool, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let tar_ok = Command::new("tar.exe")
            .args(["-xf"])
            .arg(zip_path)
            .arg("-C")
            .arg(dest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|s| s.success());
        if tar_ok {
            return Ok(true);
        }
        let z = zip_path.to_string_lossy().replace('\'', "''");
        let d = dest.to_string_lossy().replace('\'', "''");
        let ps = format!("Expand-Archive -LiteralPath '{z}' -DestinationPath '{d}' -Force");
        let ps_ok = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|s| s.success());
        Ok(ps_ok)
    }
    #[cfg(not(windows))]
    {
        for (bin, args) in [
            ("unzip", vec!["-oq".to_string()]),
            ("bsdtar", vec!["-xf".to_string()]),
            ("tar", vec!["-xf".to_string()]),
        ] {
            let mut cmd = Command::new(bin);
            for a in &args {
                cmd.arg(a);
            }
            cmd.arg(zip_path);
            if bin == "unzip" {
                cmd.arg("-d").arg(dest);
            } else {
                cmd.arg("-C").arg(dest);
            }
            let ok = cmd
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn collect_dir_bytes(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_dir_bytes(root, &p, out)?;
        } else if p.is_file() {
            let rel = p
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let data = fs::read(&p).map_err(|e| e.to_string())?;
            out.push((rel, data));
        }
    }
    Ok(())
}

pub fn expand_command_template(
    template: &str,
    input: Option<&str>,
    label: Option<&str>,
    value: Option<&str>,
) -> String {
    let mut s = template.to_string();
    let val = value.or(label).unwrap_or("");
    let lbl = label.unwrap_or(val);
    let inp = input.unwrap_or("");
    s = s.replace("{{input}}", inp);
    s = s.replace("{input}", inp);
    s = s.replace("{{value}}", val);
    s = s.replace("{value}", val);
    s = s.replace("{{label}}", lbl);
    s = s.replace("{label}", lbl);
    let fallback = if input.is_some() { inp } else { val };
    s = s.replace("{}", fallback);
    expand_env_vars(&s)
}

pub fn expand_env_vars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '~' && (i == 0 || chars[i - 1] == ' ') && (i + 1 == chars.len() || chars[i + 1] == '/' || chars[i + 1] == '\\') {
            out.push_str(&paths::home_dir().to_string_lossy());
            i += 1;
            continue;
        }
        if chars[i] == '%'
            && let Some(end) = chars[i + 1..].iter().position(|&c| c == '%')
        {
            let var: String = chars[i + 1..i + 1 + end].iter().collect();
            if let Ok(val) = std::env::var(&var) {
                out.push_str(&val);
                i += end + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(windows)]
fn format_cmd_c_raw_arg(cmd: &str) -> String {
    let trimmed = cmd.trim();
    if trimmed.starts_with('"') {
        format!("\"{trimmed}\"")
    } else {
        trimmed.to_string()
    }
}

pub fn spawn_command_detached(cmd: &str) -> Result<(), String> {
    if cmd.trim().is_empty() {
        return Err("Empty command".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let raw = format_cmd_c_raw_arg(cmd);
        Command::new("cmd.exe")
            .raw_arg("/C")
            .raw_arg(&raw)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed launching '{cmd}': {e}"))
    }
    #[cfg(not(windows))]
    {
        Command::new("sh")
            .args(["-c", cmd])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed launching '{cmd}': {e}"))
    }
}

pub fn run_command_capture(cmd: &str, timeout: std::time::Duration) -> Result<String, String> {
    if cmd.trim().is_empty() {
        return Err("Empty command".into());
    }
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let raw = format_cmd_c_raw_arg(cmd);
        let mut c = Command::new("cmd.exe");
        c.raw_arg("/C").raw_arg(&raw).creation_flags(CREATE_NO_WINDOW);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed spawning '{cmd}': {e}"))?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                if status.success() {
                    return Ok(out);
                }
                let mut err = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut err);
                }
                return Err(if err.trim().is_empty() {
                    format!("Command exited with {status}")
                } else {
                    err.trim().to_string()
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Command timed out".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(12));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store(tag: &str) -> (PathBuf, ConfigStore) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("recircles_cfg_{tag}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let store = ConfigStore::in_dir(dir.clone());
        (dir, store)
    }

    #[test]
    fn default_config_prunes_uninstalled_optional_submenus_and_validates_keybinds() {
        let (dir, store) = temp_store("prune");
        let sub_dir = dir.join("submenus");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(sub_dir.join("recircles_files.dll"), b"dll").unwrap();
        fs::write(sub_dir.join("recircles_clipboard.dll"), b"dll").unwrap();
        let installed = InstalledModules::detect_in(sub_dir);

        let mut cfg = AppConfig::default_for_installed(&installed);
        assert_eq!(
            cfg.main_menu
                .iter()
                .map(|e| e.menu.as_str())
                .collect::<Vec<_>>(),
            vec!["files", "clipboard", "settings"]
        );

        cfg.keybinds.main_menu = KeyList::one("Ctrl+Alt+Space");
        cfg.keybinds
            .set_menu_hotkey("clipboard", KeyList::one("Ctrl+Alt+v"));
        cfg.keybinds
            .set_menu_hotkey("apps", KeyList::one("Ctrl+Alt+a")); // uninstalled -> pruned!
        cfg.prune_uninstalled(&installed);
        cfg.validate().unwrap();

        let active = cfg.keybinds.collect_active_global_hotkeys(&installed, &cfg.menus);
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].target_menu, None);
        assert_eq!(active[1].target_menu.as_deref(), Some("clipboard"));

        store.save(&cfg).unwrap();
        let reloaded = store.load_or_create(&installed).unwrap();
        assert_eq!(reloaded.keybinds.main_menu.primary(), Some("Ctrl+Alt+Space"));
        assert_eq!(
            reloaded.keybinds.menu_hotkey("clipboard").primary(),
            Some("Ctrl+Alt+v")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn validates_and_rejects_reserved_menu_names_and_broken_refs() {
        let installed = InstalledModules::all_for_tests(PathBuf::from("."));
        let mut cfg = AppConfig::default_for_installed(&installed);
        cfg.menus.insert(
            "files".into(),
            MenuConfig {
                title: "Bad".into(),
                ..Default::default()
            },
        );
        assert!(cfg.validate().is_err());

        cfg.menus.clear();
        cfg.main_menu.push(MainMenuEntry::new("missing_menu"));
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn imports_standalone_theme_and_menu_json_files() {
        let (dir, store) = temp_store("import");
        let sub_dir = dir.join("submenus");
        fs::create_dir_all(&sub_dir).unwrap();
        let installed = InstalledModules::all_for_tests(sub_dir);

        let theme_src = dir.join("nordic.json");
        fs::write(&theme_src, r##"{"name":"Nordic","ring":"#2e3440d8","accent":"#88c0d0"}"##).unwrap();
        let imported_theme = store.import_theme_file(&theme_src).unwrap();
        assert_eq!(imported_theme, "nordic");
        assert!(store.resolve_theme_colors("nordic").is_some());

        let menu_src = dir.join("power.json");
        fs::write(
            &menu_src,
            r#"{"title":"Power","layout":"ring","items":[{"label":"Lock","command":"rundll32.exe user32.dll,LockWorkStation"}]}"#,
        )
        .unwrap();
        let imported_menu = store.import_menu_file(&menu_src, &installed).unwrap();
        assert_eq!(imported_menu, "power");

        let cfg = store.load_or_create(&installed).unwrap();
        assert!(cfg.menus.contains_key("power"));
        assert!(cfg.main_menu.iter().any(|m| m.menu == "power"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn defaults_to_box_layout_rejects_duplicate_hotkeys_and_parses_linux_actions() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.appearance.layout, OverflowLayout::Box);
        assert!(cfg.keybinds.number_shortcuts);

        cfg.keybinds.main_menu = KeyList::one("Ctrl+Space");
        cfg.menus.insert(
            "notes".into(),
            MenuConfig {
                title: "Notes".into(),
                ..Default::default()
            },
        );
        cfg.keybinds.set_menu_hotkey("notes", KeyList::one("Ctrl+Space"));
        assert!(cfg.validate().is_err());

        let linux_menu = r#"{
          "title": "Clip",
          "items": [
            {
              "label": "Entry",
              "left_click": "cmd /c echo {value}",
              "right_click": { "command": "cmd /c del {value}", "update": true },
              "value": "42"
            },
            {
              "label": "Ask",
              "left_click": { "input": "Prompt:", "command": "cmd /c echo {input}" }
            }
          ]
        }"#;
        let parsed: MenuConfig = serde_json::from_str(linux_menu).unwrap();
        assert_eq!(parsed.items[0].left_click.as_ref().unwrap().command.as_deref(), Some("cmd /c echo {value}"));
        assert!(parsed.items[0].right_click.as_ref().unwrap().reload);
        assert_eq!(parsed.items[1].left_click.as_ref().unwrap().input.as_deref(), Some("Prompt:"));
    }

    #[test]
    fn load_or_create_loads_menus_dir_before_validate_and_supports_linux_aliases() {
        let (dir, store) = temp_store("menus_pre_val");
        store.ensure_dirs().unwrap();
        let sub_dir = dir.join("submenus");
        fs::create_dir_all(&sub_dir).unwrap();
        let installed = InstalledModules::detect_in(sub_dir);

        fs::write(
            store.menus_dir().join("quick.json"),
            r#"{"title":"Quick","layout":"dropdown","items":[{"name":"Echo","left":"echo {}"}]}"#,
        )
        .unwrap();
        fs::write(
            store.config_path(),
            r#"{"appearance":{"overflow_layout":"rings"},"main_menu":[{"name":"quick","label":"Quick Custom"},"settings"]}"#,
        )
        .unwrap();

        let cfg = store.load_or_create(&installed).unwrap();
        assert_eq!(cfg.appearance.layout, OverflowLayout::Ring);
        assert!(cfg.menus.contains_key("quick"));
        assert_eq!(cfg.menus["quick"].layout, CustomMenuLayout::List);
        assert_eq!(cfg.menus["quick"].items[0].label, "Echo");
        assert_eq!(cfg.main_menu[0].menu, "quick");
        assert_eq!(cfg.main_menu[0].title.as_deref(), Some("Quick Custom"));

        let expanded_input = expand_command_template("cmd /c echo {input} {}", Some("abc"), Some("Lbl"), Some("Val"));
        assert_eq!(expanded_input, "cmd /c echo abc abc");
        let expanded_val = expand_command_template("cmd /c echo {label}:{value}:{}", None, Some("Lbl"), Some("Val"));
        assert_eq!(expanded_val, "cmd /c echo Lbl:Val:Val");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn effective_colors_propagates_accents_and_clears_derived_on_override() {
        let (dir, store) = temp_store("eff_colors");
        let mut cfg = AppConfig::default();
        cfg.appearance.theme = "midnight".into();
        cfg.colors.ring = "#112233ff".into();
        cfg.colors.ring_top = None;
        cfg.colors.ring_bottom = None;
        cfg.colors.accents.files = "#abcdef".into();

        let eff = cfg.effective_colors(&store);
        assert_eq!(eff.ring, "#112233ff");
        assert_eq!(eff.ring_top, None);
        assert_eq!(eff.ring_bottom, None);
        assert_eq!(eff.accents.files, "#abcdef");
        // Untouched midnight theme properties remain midnight
        let mid = builtin_theme("midnight").unwrap().colors;
        assert_eq!(eff.center, mid.center);
        assert_eq!(eff.capsule, mid.capsule);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn loads_utf8_bom_json_files_and_runs_quoted_commands_cleanly() {
        let (dir, store) = temp_store("bom_and_cmd");
        store.ensure_dirs().unwrap();
        let sub_dir = dir.join("submenus");
        fs::create_dir_all(&sub_dir).unwrap();
        let installed = InstalledModules::detect_in(sub_dir);

        // UTF-8 BOM in recircles.json, menus/*.json, and themes/*.json must all parse cleanly
        fs::write(
            store.config_path(),
            "\u{feff}{\"appearance\":{\"theme\":\"bomtheme\"},\"main_menu\":[\"bommenu\",\"settings\"]}",
        )
        .unwrap();
        fs::write(
            store.menus_dir().join("bommenu.json"),
            "\u{feff}{\"title\":\"BOM Menu\",\"items\":[{\"label\":\"One\"}]}",
        )
        .unwrap();
        fs::write(
            store.themes_dir().join("bomtheme.json"),
            "\u{feff}{\"ring\":\"#102030e8\",\"accent\":\"#44aaff\"}",
        )
        .unwrap();

        let cfg = store.load_or_create(&installed).unwrap();
        assert!(cfg.menus.contains_key("bommenu"));
        assert_eq!(cfg.menus["bommenu"].title, "BOM Menu");
        let resolved = store.resolve_theme_colors("bomtheme").unwrap();
        assert_eq!(resolved.accent, "#44aaff");

        // Quoted command via cmd.exe /C must preserve inner quotes without backslash mangling
        #[cfg(windows)]
        {
            let out = run_command_capture(
                "powershell -NoProfile -NonInteractive -Command \"Write-Output 'quoted_ok'\"",
                std::time::Duration::from_secs(5),
            )
            .expect("quoted powershell command succeeds");
            assert_eq!(out.trim(), "quoted_ok");
        }

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn appearance_translucency_and_blur_parse_percent_and_legacy_bools() {
        let def = AppearanceConfig::default();
        assert_eq!(def.translucency, DEFAULT_TRANSLUCENCY);
        assert_eq!(def.blur, DEFAULT_BLUR);
        assert_eq!(def.material, Material::Glass);

        // Numeric percentages clamp cleanly to 0..=100 and sync material
        let num: AppearanceConfig =
            serde_json::from_str(r#"{"translucency":85,"blur":140}"#).unwrap();
        assert_eq!(num.translucency, 85);
        assert_eq!(num.blur, 100);
        assert_eq!(num.material, Material::Glass);

        let clear_no_blur: AppearanceConfig =
            serde_json::from_str(r#"{"translucency":100,"blur":0}"#).unwrap();
        assert_eq!(clear_no_blur.translucency, 100);
        assert_eq!(clear_no_blur.blur, 0);
        assert_eq!(clear_no_blur.material, Material::Translucent);

        let opaque: AppearanceConfig =
            serde_json::from_str(r#"{"translucency":0,"blur":50}"#).unwrap();
        assert_eq!(opaque.translucency, 0);
        assert_eq!(opaque.blur, 50);
        assert_eq!(opaque.material, Material::Solid);

        // Legacy boolean blur + legacy material migrate seamlessly
        let leg_true: AppearanceConfig =
            serde_json::from_str(r#"{"material":"glass","blur":true}"#).unwrap();
        assert_eq!(leg_true.translucency, DEFAULT_TRANSLUCENCY);
        assert_eq!(leg_true.blur, DEFAULT_BLUR);
        assert_eq!(leg_true.material, Material::Glass);

        let leg_false: AppearanceConfig =
            serde_json::from_str(r#"{"material":"glass","blur":false}"#).unwrap();
        assert_eq!(leg_false.translucency, DEFAULT_TRANSLUCENCY);
        assert_eq!(leg_false.blur, 0);
        assert_eq!(leg_false.material, Material::Translucent);

        let leg_solid: AppearanceConfig =
            serde_json::from_str(r#"{"material":"solid"}"#).unwrap();
        assert_eq!(leg_solid.translucency, 0);
        assert_eq!(leg_solid.material, Material::Solid);
    }
}
