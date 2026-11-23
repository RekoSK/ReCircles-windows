use crate::config::{
    AppConfig, AppearanceConfig, ConfigStore, KeyList, KeybindsConfig, LoadingStyle, MainMenuEntry,
    OverflowLayout,
};
use crate::menu::BuiltinIcon;
use crate::modules::InstalledModules;
use crate::themes::ColorsConfig;
use crate::updater::{CURRENT_VERSION, UpdateState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorGroup {
    Surface,
    Accents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorField {
    Ring,
    RingBorder,
    Center,
    Capsule,
    Text,
    Muted,
    Accent,
    AccentFiles,
    AccentApps,
    AccentWindows,
    AccentClipboard,
    AccentSettings,
}

impl ColorField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ring => "Ring Fill",
            Self::RingBorder => "Ring Border",
            Self::Center => "Center Fill",
            Self::Capsule => "Selection Capsule",
            Self::Text => "Primary Text",
            Self::Muted => "Muted Text",
            Self::Accent => "General Accent",
            Self::AccentFiles => "Files Accent",
            Self::AccentApps => "Apps Accent",
            Self::AccentWindows => "Windows Accent",
            Self::AccentClipboard => "Clipboard Accent",
            Self::AccentSettings => "Settings Accent",
        }
    }

    pub fn get(self, c: &ColorsConfig) -> &str {
        match self {
            Self::Ring => &c.ring,
            Self::RingBorder => &c.ring_border,
            Self::Center => &c.center,
            Self::Capsule => &c.capsule,
            Self::Text => &c.text,
            Self::Muted => &c.muted,
            Self::Accent => &c.accent,
            Self::AccentFiles => &c.accents.files,
            Self::AccentApps => &c.accents.apps,
            Self::AccentWindows => &c.accents.windows,
            Self::AccentClipboard => &c.accents.clipboard,
            Self::AccentSettings => &c.accents.settings,
        }
    }

    pub fn set(self, c: &mut ColorsConfig, hex: String) {
        match self {
            Self::Ring => {
                c.ring = hex;
                c.ring_top = None;
                c.ring_bottom = None;
            }
            Self::RingBorder => {
                c.ring_border = hex;
                c.ring_rim_bottom = None;
                c.ring_divider = None;
                c.capsule_border = None;
            }
            Self::Center => c.center = hex,
            Self::Capsule => {
                c.capsule = hex;
                c.capsule_bottom = None;
            }
            Self::Text => {
                c.text = hex;
                c.text_shadow = None;
            }
            Self::Muted => c.muted = hex,
            Self::Accent => c.accent = hex,
            Self::AccentFiles => c.accents.files = hex,
            Self::AccentApps => c.accents.apps = hex,
            Self::AccentWindows => c.accents.windows = hex,
            Self::AccentClipboard => c.accents.clipboard = hex,
            Self::AccentSettings => c.accents.settings = hex,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKeyField {
    Back,
    Home,
    Activate,
    SecondaryActivate,
    OpenCurrentFolder,
    Next,
    Previous,
    PageNext,
    PagePrevious,
    AcceptCompletion,
    UndoCompletion,
    ClearQuery,
    Help,
}

impl NavKeyField {
    pub const ALL: [NavKeyField; 13] = [
        Self::Back,
        Self::Home,
        Self::Activate,
        Self::SecondaryActivate,
        Self::OpenCurrentFolder,
        Self::Next,
        Self::Previous,
        Self::PageNext,
        Self::PagePrevious,
        Self::AcceptCompletion,
        Self::UndoCompletion,
        Self::ClearQuery,
        Self::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Back => "Back",
            Self::Home => "Home",
            Self::Activate => "Activate",
            Self::SecondaryActivate => "Secondary Activate",
            Self::OpenCurrentFolder => "Open Current Folder",
            Self::Next => "Next Item",
            Self::Previous => "Previous Item",
            Self::PageNext => "Next Page",
            Self::PagePrevious => "Previous Page",
            Self::AcceptCompletion => "Accept Completion",
            Self::UndoCompletion => "Parent / Undo",
            Self::ClearQuery => "Clear Search",
            Self::Help => "Help Overlay",
        }
    }

    pub fn get(self, k: &KeybindsConfig) -> &KeyList {
        match self {
            Self::Back => &k.back,
            Self::Home => &k.home,
            Self::Activate => &k.activate,
            Self::SecondaryActivate => &k.secondary_activate,
            Self::OpenCurrentFolder => &k.open_current_folder,
            Self::Next => &k.next,
            Self::Previous => &k.previous,
            Self::PageNext => &k.page_next,
            Self::PagePrevious => &k.page_previous,
            Self::AcceptCompletion => &k.accept_completion,
            Self::UndoCompletion => &k.undo_completion,
            Self::ClearQuery => &k.clear_query,
            Self::Help => &k.help,
        }
    }

    pub fn set(self, k: &mut KeybindsConfig, keys: KeyList) {
        match self {
            Self::Back => k.back = keys,
            Self::Home => k.home = keys,
            Self::Activate => k.activate = keys,
            Self::SecondaryActivate => k.secondary_activate = keys,
            Self::OpenCurrentFolder => k.open_current_folder = keys,
            Self::Next => k.next = keys,
            Self::Previous => k.previous = keys,
            Self::PageNext => k.page_next = keys,
            Self::PagePrevious => k.page_previous = keys,
            Self::AcceptCompletion => k.accept_completion = keys,
            Self::UndoCompletion => k.undo_completion = keys,
            Self::ClearQuery => k.clear_query = keys,
            Self::Help => k.help = keys,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderField {
    Translucency,
    Blur,
}

impl SliderField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Translucency => "Translucency",
            Self::Blur => "Blur",
        }
    }

    pub fn min_label(self) -> &'static str {
        match self {
            Self::Translucency => "0% · Opaque",
            Self::Blur => "0% · Off",
        }
    }

    pub fn max_label(self) -> &'static str {
        match self {
            Self::Translucency => "100% · Clear",
            Self::Blur => "100% · Max",
        }
    }

    pub fn get(self, a: &AppearanceConfig) -> u8 {
        match self {
            Self::Translucency => a.translucency.min(100),
            Self::Blur => a.blur.min(100),
        }
    }

    pub fn set(self, a: &mut AppearanceConfig, value: u8) {
        let clamped = value.min(100);
        match self {
            Self::Translucency => a.translucency = clamped,
            Self::Blur => a.blur = clamped,
        }
        a.sync_material();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceToggle {
    OverflowLayout,
    Animations,
    Animation3d,
    HighContrast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsView {
    Root,
    Appearance,
    ColorTheme,
    MenuColors,
    ColorGroupEdit(ColorGroup),
    Keybinds,
    MainMenu,
    MainMenuEntry(usize),
    MainMenuAdd,
    Updates,
    ConfirmResetMemory,
    ConfirmResetAll,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SettingsAction {
    OpenView(SettingsView),
    GoBack,
    SetTheme(String),
    ImportTheme,
    ToggleAppearance(AppearanceToggle),
    AdjustSlider(SliderField),
    ResetAppearance,
    PickColor(ColorField),
    ResetColors,
    ToggleNumberShortcuts,
    CaptureMenuHotkey {
        menu_id: Option<String>,
        label: String,
    },
    ClearMenuHotkey(Option<String>),
    CaptureNavKeybind(NavKeyField),
    ClearNavKeybind(NavKeyField),
    ResetKeybinds,
    ImportMenu,
    AddMainMenuEntry(String),
    RemoveMainMenuEntry(usize),
    MoveMainMenuEntry {
        index: usize,
        delta: isize,
    },
    ResetMainMenu,
    CheckForUpdates,
    InstallUpdate,
    CycleUpdateMode,
    TogglePrereleases,
    ResetMemory,
    ResetAllSettings,
}

#[derive(Debug, Clone)]
pub struct SettingsItem {
    pub label: String,
    pub detail: String,
    pub icon: BuiltinIcon,
    pub active: bool,
    pub accent: Option<String>,
    pub action: SettingsAction,
    pub secondary_action: Option<SettingsAction>,
}

pub fn view_title(view: &SettingsView) -> &'static str {
    match view {
        SettingsView::Root => "Settings",
        SettingsView::Appearance => "Appearance",
        SettingsView::ColorTheme => "Theme",
        SettingsView::MenuColors => "Colors",
        SettingsView::ColorGroupEdit(ColorGroup::Surface) => "Surface Colors",
        SettingsView::ColorGroupEdit(ColorGroup::Accents) => "Mode Accents",
        SettingsView::Keybinds => "Keybinds",
        SettingsView::MainMenu => "Main Menu",
        SettingsView::MainMenuEntry(_) => "Main Menu Entry",
        SettingsView::MainMenuAdd => "Add Entry",
        SettingsView::Updates => "Updates",
        SettingsView::ConfirmResetMemory => "Reset Memory",
        SettingsView::ConfirmResetAll => "Reset Settings",
    }
}

pub fn idle_status(view: &SettingsView) -> &'static str {
    match view {
        SettingsView::Root => "Configure ReCircles",
        SettingsView::Appearance => "Click any option to configure",
        SettingsView::ColorTheme => "Select or import a theme",
        SettingsView::MenuColors => "Choose a theme or color category",
        SettingsView::ColorGroupEdit(_) => "Click a color to pick RGB/alpha",
        SettingsView::Keybinds => "Click to edit · Right-click to clear",
        SettingsView::MainMenu => "Select entry to move/remove",
        SettingsView::MainMenuEntry(_) => "Move earlier, move later, or remove",
        SettingsView::MainMenuAdd => "Choose installed submenu or menu to pin",
        SettingsView::Updates => "Automatic GitHub Releases updater",
        SettingsView::ConfirmResetMemory => "Clear most-used memory?",
        SettingsView::ConfirmResetAll => "Reset all settings to defaults?",
    }
}

fn active_theme_label(cfg: &AppConfig, store: &ConfigStore) -> String {
    store
        .all_theme_choices()
        .into_iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(&cfg.appearance.theme))
        .map(|(_, label)| label)
        .unwrap_or_else(|| cfg.appearance.theme.clone())
}

fn toggle_item(
    label: &str,
    on: bool,
    on_detail: &str,
    off_detail: &str,
    action: SettingsAction,
) -> SettingsItem {
    SettingsItem {
        label: label.into(),
        detail: if on {
            format!("On · {on_detail}")
        } else {
            format!("Off · {off_detail}")
        },
        icon: if on {
            BuiltinIcon::ToggleOn
        } else {
            BuiltinIcon::ToggleOff
        },
        active: on,
        accent: None,
        action,
        secondary_action: None,
    }
}

pub fn build_items(
    cfg: &AppConfig,
    store: &ConfigStore,
    installed: &InstalledModules,
    view: &SettingsView,
) -> Vec<SettingsItem> {
    match view {
        SettingsView::Root => vec![
            SettingsItem {
                label: "Appearance".into(),
                detail: "Layout, animations, translucency, blur and contrast".into(),
                icon: BuiltinIcon::Appearance,
                active: false,
                accent: None,
                action: SettingsAction::OpenView(SettingsView::Appearance),
                secondary_action: None,
            },
            SettingsItem {
                label: "Menu Colors".into(),
                detail: "Ring, glass & mode accents".into(),
                icon: BuiltinIcon::Palette,
                active: false,
                accent: None,
                action: SettingsAction::OpenView(SettingsView::MenuColors),
                secondary_action: None,
            },
            SettingsItem {
                label: "Keybinds".into(),
                detail: format!("Main: {}", cfg.keybinds.main_menu.display()),
                icon: BuiltinIcon::Keybind,
                active: false,
                accent: None,
                action: SettingsAction::OpenView(SettingsView::Keybinds),
                secondary_action: None,
            },
            SettingsItem {
                label: "Main Menu".into(),
                detail: format!("{} pinned entries", cfg.main_menu.len()),
                icon: BuiltinIcon::MainMenu,
                active: false,
                accent: None,
                action: SettingsAction::OpenView(SettingsView::MainMenu),
                secondary_action: None,
            },
            {
                let upd_state = UpdateState::load(store.dir());
                let avail = upd_state.available_update_for(CURRENT_VERSION);
                SettingsItem {
                    label: "Updates".into(),
                    detail: match avail {
                        Some(rel) => format!("Update v{} available · {}", rel.version, cfg.updates.mode.label()),
                        None => format!("v{CURRENT_VERSION} · {}", cfg.updates.mode.label()),
                    },
                    icon: BuiltinIcon::Refresh,
                    active: avail.is_some(),
                    accent: None,
                    action: SettingsAction::OpenView(SettingsView::Updates),
                    secondary_action: None,
                }
            },
            SettingsItem {
                label: "Reset Memory".into(),
                detail: "Clear most-used apps & folders".into(),
                icon: BuiltinIcon::Forget,
                active: false,
                accent: None,
                action: SettingsAction::OpenView(SettingsView::ConfirmResetMemory),
                secondary_action: None,
            },
            SettingsItem {
                label: "Reset All".into(),
                detail: "Restore factory configuration".into(),
                icon: BuiltinIcon::Reset,
                active: false,
                accent: None,
                action: SettingsAction::OpenView(SettingsView::ConfirmResetAll),
                secondary_action: None,
            },
        ],
        SettingsView::Appearance => {
            let a = &cfg.appearance;
            let is_box = a.layout == OverflowLayout::Box;
            vec![
                SettingsItem {
                    label: if is_box {
                        "Overflow: Box".into()
                    } else {
                        "Overflow: Rings".into()
                    },
                    detail: if is_box {
                        "Grid box for 9+ items · Click to switch to paged rings".into()
                    } else {
                        "Paged rings for 9+ items · Click to switch to grid box".into()
                    },
                    icon: BuiltinIcon::Window,
                    active: is_box,
                    accent: None,
                    action: SettingsAction::ToggleAppearance(AppearanceToggle::OverflowLayout),
                    secondary_action: None,
                },
                toggle_item(
                    "Animations",
                    !a.reduced_motion,
                    "Smooth springs and transitions",
                    "Instant transitions (reduced motion)",
                    SettingsAction::ToggleAppearance(AppearanceToggle::Animations),
                ),
                toggle_item(
                    "3D Loading",
                    a.loading_animation == LoadingStyle::ThreeD,
                    "3D tumbling loading ring",
                    "2D pulsing loading ring",
                    SettingsAction::ToggleAppearance(AppearanceToggle::Animation3d),
                ),
                SettingsItem {
                    label: "Translucency".into(),
                    detail: if a.translucency == 0 {
                        "0% · Solid opaque surfaces · Click to adjust".into()
                    } else if a.translucency == 100 {
                        "100% · Pure clear glass · Click to adjust".into()
                    } else {
                        format!("{}% · See-through glass · Click to adjust", a.translucency)
                    },
                    icon: BuiltinIcon::Material,
                    active: a.translucency > 0,
                    accent: None,
                    action: SettingsAction::AdjustSlider(SliderField::Translucency),
                    secondary_action: None,
                },
                SettingsItem {
                    label: "Blur".into(),
                    detail: if a.blur == 0 {
                        "0% · No background blur · Click to adjust".into()
                    } else {
                        format!("{}% · Frosted glass backdrop · Click to adjust", a.blur)
                    },
                    icon: BuiltinIcon::Blur,
                    active: a.blur > 0,
                    accent: None,
                    action: SettingsAction::AdjustSlider(SliderField::Blur),
                    secondary_action: None,
                },
                toggle_item(
                    "High Contrast",
                    a.high_contrast,
                    "Opaque high-contrast rim and surfaces",
                    "Standard contrast",
                    SettingsAction::ToggleAppearance(AppearanceToggle::HighContrast),
                ),
                SettingsItem {
                    label: "Reset Appearance".into(),
                    detail: "Restore default appearance settings".into(),
                    icon: BuiltinIcon::Reset,
                    active: false,
                    accent: None,
                    action: SettingsAction::ResetAppearance,
                    secondary_action: None,
                },
            ]
        }
        SettingsView::ColorTheme => {
            let mut items: Vec<SettingsItem> = store
                .all_theme_choices()
                .into_iter()
                .map(|(id, label)| {
                    let active = cfg.appearance.theme.eq_ignore_ascii_case(&id);
                    let accent = store.resolve_theme_colors(&id).map(|c| c.accent);
                    SettingsItem {
                        label,
                        detail: if active { "Active theme".into() } else { id.clone() },
                        icon: if active {
                            BuiltinIcon::Check
                        } else {
                            BuiltinIcon::Theme
                        },
                        active,
                        accent,
                        action: SettingsAction::SetTheme(id),
                        secondary_action: None,
                    }
                })
                .collect();
            items.push(SettingsItem {
                label: "Import Theme...".into(),
                detail: "Load .json or .zip theme".into(),
                icon: BuiltinIcon::Add,
                active: false,
                accent: None,
                action: SettingsAction::ImportTheme,
                secondary_action: None,
            });
            items
        }
        SettingsView::MenuColors => vec![
            SettingsItem {
                label: "Theme".into(),
                detail: format!(
                    "Current: {} · Choose a built-in or local theme",
                    active_theme_label(cfg, store)
                ),
                icon: BuiltinIcon::Theme,
                active: false,
                accent: None,
                action: SettingsAction::OpenView(SettingsView::ColorTheme),
                secondary_action: None,
            },
            SettingsItem {
                label: "Surface & Text".into(),
                detail: "Ring, border, center, capsule, text".into(),
                icon: BuiltinIcon::Palette,
                active: false,
                accent: Some(cfg.colors.accent.clone()),
                action: SettingsAction::OpenView(SettingsView::ColorGroupEdit(ColorGroup::Surface)),
                secondary_action: None,
            },
            SettingsItem {
                label: "Submenu Accents".into(),
                detail: "Files, Apps, Windows, Clipboard, ...".into(),
                icon: BuiltinIcon::Palette,
                active: false,
                accent: Some(cfg.colors.accents.clipboard.clone()),
                action: SettingsAction::OpenView(SettingsView::ColorGroupEdit(ColorGroup::Accents)),
                secondary_action: None,
            },
            SettingsItem {
                label: "Reset Colors".into(),
                detail: "Restore theme defaults".into(),
                icon: BuiltinIcon::Reset,
                active: false,
                accent: None,
                action: SettingsAction::ResetColors,
                secondary_action: None,
            },
        ],
        SettingsView::ColorGroupEdit(group) => {
            let fields: &[ColorField] = match group {
                ColorGroup::Surface => &[
                    ColorField::Ring,
                    ColorField::RingBorder,
                    ColorField::Center,
                    ColorField::Capsule,
                    ColorField::Text,
                    ColorField::Muted,
                    ColorField::Accent,
                ],
                ColorGroup::Accents => &[
                    ColorField::AccentFiles,
                    ColorField::AccentApps,
                    ColorField::AccentWindows,
                    ColorField::AccentClipboard,
                    ColorField::AccentSettings,
                ],
            };
            fields
                .iter()
                .map(|&field| {
                    let hex = field.get(&cfg.colors).to_string();
                    SettingsItem {
                        label: field.label().into(),
                        detail: hex.clone(),
                        icon: BuiltinIcon::Palette,
                        active: false,
                        accent: Some(hex),
                        action: SettingsAction::PickColor(field),
                        secondary_action: None,
                    }
                })
                .collect()
        }
        SettingsView::Keybinds => {
            let mut items = Vec::new();
            items.push(SettingsItem {
                label: "Number Shortcuts".into(),
                detail: if cfg.keybinds.number_shortcuts {
                    "1-8 select visible items".into()
                } else {
                    "Number shortcuts disabled".into()
                },
                icon: BuiltinIcon::Keybind,
                active: cfg.keybinds.number_shortcuts,
                accent: None,
                action: SettingsAction::ToggleNumberShortcuts,
                secondary_action: None,
            });
            // Global OS Hotkeys first: Main Ring, installed submenus, custom menus
            items.push(SettingsItem {
                label: "Hotkey: Main Ring".into(),
                detail: cfg.keybinds.main_menu.display(),
                icon: BuiltinIcon::Keybind,
                active: !cfg.keybinds.main_menu.is_empty(),
                accent: None,
                action: SettingsAction::CaptureMenuHotkey {
                    menu_id: None,
                    label: "Main Ring".into(),
                },
                secondary_action: Some(SettingsAction::ClearMenuHotkey(None)),
            });

            for sub in installed.installed_list() {
                let id = sub.id().to_string();
                let current = cfg.keybinds.menu_hotkey(&id);
                items.push(SettingsItem {
                    label: format!("Hotkey: {}", sub.title()),
                    detail: current.display(),
                    icon: BuiltinIcon::Keybind,
                    active: !current.is_empty(),
                    accent: None,
                    action: SettingsAction::CaptureMenuHotkey {
                        menu_id: Some(id.clone()),
                        label: sub.title().into(),
                    },
                    secondary_action: Some(SettingsAction::ClearMenuHotkey(Some(id))),
                });
            }

            for (menu_id, menu_cfg) in &cfg.menus {
                let title = if menu_cfg.title.trim().is_empty() {
                    menu_id.as_str()
                } else {
                    menu_cfg.title.as_str()
                };
                let current = cfg.keybinds.menu_hotkey(menu_id);
                items.push(SettingsItem {
                    label: format!("Hotkey: {title}"),
                    detail: current.display(),
                    icon: BuiltinIcon::Keybind,
                    active: !current.is_empty(),
                    accent: None,
                    action: SettingsAction::CaptureMenuHotkey {
                        menu_id: Some(menu_id.clone()),
                        label: title.into(),
                    },
                    secondary_action: Some(SettingsAction::ClearMenuHotkey(Some(menu_id.clone()))),
                });
            }

            for &field in &NavKeyField::ALL {
                items.push(SettingsItem {
                    label: field.label().into(),
                    detail: field.get(&cfg.keybinds).display(),
                    icon: BuiltinIcon::Keybind,
                    active: false,
                    accent: None,
                    action: SettingsAction::CaptureNavKeybind(field),
                    secondary_action: Some(SettingsAction::ClearNavKeybind(field)),
                });
            }

            items.push(SettingsItem {
                label: "Reset Keybinds".into(),
                detail: "Restore default keys".into(),
                icon: BuiltinIcon::Reset,
                active: false,
                accent: None,
                action: SettingsAction::ResetKeybinds,
                secondary_action: None,
            });
            items
        }
        SettingsView::MainMenu => {
            let mut items: Vec<SettingsItem> = cfg
                .main_menu
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    let title = entry.resolved_title(&cfg.menus);
                    let secondary = if entry.menu == "settings" {
                        None
                    } else {
                        Some(SettingsAction::RemoveMainMenuEntry(i))
                    };
                    SettingsItem {
                        label: title,
                        detail: if entry.menu == "settings" {
                            format!("#{} · Required entry", i + 1)
                        } else {
                            format!("#{} · {} · Edit or R-click remove", i + 1, entry.menu)
                        },
                        icon: if entry.menu == "settings" {
                            BuiltinIcon::Lock
                        } else {
                            BuiltinIcon::MainMenu
                        },
                        active: true,
                        accent: entry.accent.clone(),
                        action: SettingsAction::OpenView(SettingsView::MainMenuEntry(i)),
                        secondary_action: secondary,
                    }
                })
                .collect();
            items.push(SettingsItem {
                label: "Add Entry".into(),
                detail: "Pin installed submenu or menu".into(),
                icon: BuiltinIcon::Add,
                active: false,
                accent: None,
                action: SettingsAction::OpenView(SettingsView::MainMenuAdd),
                secondary_action: None,
            });
            items.push(SettingsItem {
                label: "Import Menu".into(),
                detail: "Import a menu .json or .zip bundle".into(),
                icon: BuiltinIcon::Add,
                active: false,
                accent: None,
                action: SettingsAction::ImportMenu,
                secondary_action: None,
            });
            items.push(SettingsItem {
                label: "Reset Main Menu".into(),
                detail: "Restore default main ring entries".into(),
                icon: BuiltinIcon::Reset,
                active: false,
                accent: None,
                action: SettingsAction::ResetMainMenu,
                secondary_action: None,
            });
            items
        }
        SettingsView::MainMenuEntry(index) => build_main_menu_entry_items(cfg, *index),
        SettingsView::MainMenuAdd => build_main_menu_add_items(cfg, installed),
        SettingsView::Updates => {
            let state = UpdateState::load(store.dir());
            let mut items = Vec::new();
            if let Some(rel) = state.available_update_for(CURRENT_VERSION) {
                items.push(SettingsItem {
                    label: format!("Install v{}", rel.version),
                    detail: format!("Upgrade from v{CURRENT_VERSION} now"),
                    icon: BuiltinIcon::Check,
                    active: true,
                    accent: None,
                    action: SettingsAction::InstallUpdate,
                    secondary_action: None,
                });
            }
            items.push(SettingsItem {
                label: "Check Now".into(),
                detail: format!("Installed: v{CURRENT_VERSION} · GitHub Releases"),
                icon: BuiltinIcon::Refresh,
                active: false,
                accent: None,
                action: SettingsAction::CheckForUpdates,
                secondary_action: None,
            });
            items.push(SettingsItem {
                label: format!("Mode: {}", cfg.updates.mode.label()),
                detail: format!("{} · Click to cycle", cfg.updates.mode.description()),
                icon: BuiltinIcon::Settings,
                active: cfg.updates.mode != crate::config::UpdateMode::Manual,
                accent: None,
                action: SettingsAction::CycleUpdateMode,
                secondary_action: None,
            });
            items.push(toggle_item(
                "Beta / Pre-releases",
                cfg.updates.include_prereleases,
                "Receiving beta & pre-release builds",
                "Stable releases only",
                SettingsAction::TogglePrereleases,
            ));
            items
        }
        SettingsView::ConfirmResetMemory => vec![
            SettingsItem {
                label: "Clear Memory".into(),
                detail: "Reset most-used apps & folders".into(),
                icon: BuiltinIcon::Forget,
                active: false,
                accent: None,
                action: SettingsAction::ResetMemory,
                secondary_action: None,
            },
            SettingsItem {
                label: "Cancel".into(),
                detail: "Keep saved usage memory".into(),
                icon: BuiltinIcon::Disconnect,
                active: false,
                accent: None,
                action: SettingsAction::GoBack,
                secondary_action: None,
            },
        ],
        SettingsView::ConfirmResetAll => vec![
            SettingsItem {
                label: "Reset All".into(),
                detail: "Restore factory configuration".into(),
                icon: BuiltinIcon::Reset,
                active: false,
                accent: None,
                action: SettingsAction::ResetAllSettings,
                secondary_action: None,
            },
            SettingsItem {
                label: "Cancel".into(),
                detail: "Keep current settings".into(),
                icon: BuiltinIcon::Disconnect,
                active: false,
                accent: None,
                action: SettingsAction::GoBack,
                secondary_action: None,
            },
        ],
    }
}

fn build_main_menu_entry_items(cfg: &AppConfig, index: usize) -> Vec<SettingsItem> {
    let Some(entry) = cfg.main_menu.get(index) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    if cfg.main_menu.len() > 1 {
        let prev_slot = if index == 0 { cfg.main_menu.len() } else { index };
        let next_slot = if index + 1 >= cfg.main_menu.len() { 1 } else { index + 2 };
        items.push(SettingsItem {
            label: "Move Earlier".into(),
            detail: format!("Move from #{} to #{}", index + 1, prev_slot),
            icon: BuiltinIcon::MainMenu,
            active: false,
            accent: None,
            action: SettingsAction::MoveMainMenuEntry { index, delta: -1 },
            secondary_action: None,
        });
        items.push(SettingsItem {
            label: "Move Later".into(),
            detail: format!("Move from #{} to #{}", index + 1, next_slot),
            icon: BuiltinIcon::MainMenu,
            active: false,
            accent: None,
            action: SettingsAction::MoveMainMenuEntry { index, delta: 1 },
            secondary_action: None,
        });
    }
    if entry.menu != "settings" {
        items.push(SettingsItem {
            label: "Remove Entry".into(),
            detail: format!("Unpin '{}' from main menu", entry.resolved_title(&cfg.menus)),
            icon: BuiltinIcon::Disconnect,
            active: false,
            accent: None,
            action: SettingsAction::RemoveMainMenuEntry(index),
            secondary_action: None,
        });
    }
    if items.is_empty() {
        items.push(SettingsItem {
            label: "Required Entry".into(),
            detail: "Settings cannot be unpinned".into(),
            icon: BuiltinIcon::Lock,
            active: true,
            accent: None,
            action: SettingsAction::GoBack,
            secondary_action: None,
        });
    }
    items
}

fn build_main_menu_add_items(cfg: &AppConfig, installed: &InstalledModules) -> Vec<SettingsItem> {
    let mut items = Vec::new();
    for sub in installed.installed_list() {
        if !cfg.main_menu.iter().any(|e| e.menu == sub.id()) {
            items.push(SettingsItem {
                label: sub.title().into(),
                detail: format!("Built-in ({})", sub.id()),
                icon: BuiltinIcon::Add,
                active: false,
                accent: None,
                action: SettingsAction::AddMainMenuEntry(sub.id().into()),
                secondary_action: None,
            });
        }
    }
    for (id, menu) in &cfg.menus {
        if !cfg.main_menu.iter().any(|e| &e.menu == id) {
            let label = if menu.title.trim().is_empty() {
                id.clone()
            } else {
                menu.title.clone()
            };
            items.push(SettingsItem {
                label,
                detail: format!("Custom menu ({id})"),
                icon: BuiltinIcon::Add,
                active: false,
                accent: menu.accent.clone(),
                action: SettingsAction::AddMainMenuEntry(id.clone()),
                secondary_action: None,
            });
        }
    }
    if items.is_empty() {
        items.push(SettingsItem {
            label: "Nothing to Add".into(),
            detail: "All installed menus are pinned".into(),
            icon: BuiltinIcon::Check,
            active: false,
            accent: None,
            action: SettingsAction::GoBack,
            secondary_action: None,
        });
    }
    items
}

pub fn apply_main_menu_move(entries: &mut Vec<MainMenuEntry>, index: usize, delta: isize) -> bool {
    if entries.is_empty() || index >= entries.len() {
        return false;
    }
    let next = (index as isize + delta).rem_euclid(entries.len() as isize) as usize;
    if next == index {
        return false;
    }
    let entry = entries.remove(index);
    entries.insert(next, entry);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_env(tag: &str) -> (PathBuf, ConfigStore) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("recircles_set_{tag}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let store = ConfigStore::in_dir(dir.clone());
        (dir, store)
    }

    #[test]
    fn root_has_reset_memory_and_keybinds_filter_by_installed_dlls() {
        let (dir, store) = temp_env("views");
        let sub_dir = dir.join("submenus");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(sub_dir.join("recircles_clipboard.dll"), b"dll").unwrap();
        let installed = InstalledModules::detect_in(sub_dir);
        let mut cfg = AppConfig::default_for_installed(&installed);

        let root = build_items(&cfg, &store, &installed, &SettingsView::Root);
        assert_eq!(root.len(), 7);
        assert!(root.iter().any(|i| i.label == "Updates"));
        assert!(root.iter().any(|i| i.label == "Reset Memory"));

        let reset_mem = build_items(&cfg, &store, &installed, &SettingsView::ConfirmResetMemory);
        assert_eq!(reset_mem.len(), 2);
        assert_eq!(reset_mem[0].label, "Clear Memory");

        let kb = build_items(&cfg, &store, &installed, &SettingsView::Keybinds);
        assert_eq!(kb[0].label, "Number Shortcuts");
        assert_eq!(kb[0].action, SettingsAction::ToggleNumberShortcuts);
        // Only Main Ring + Clipboard + Settings hotkeys (+ Number Shortcuts + 15 nav + Reset Keybinds)
        let hotkey_labels: Vec<&str> = kb
            .iter()
            .filter(|i| i.label.starts_with("Hotkey:"))
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(
            hotkey_labels,
            vec!["Hotkey: Main Ring", "Hotkey: Clipboard", "Hotkey: Settings"]
        );

        let main_menu = build_items(&cfg, &store, &installed, &SettingsView::MainMenu);
        assert!(main_menu.iter().any(|i| i.action == SettingsAction::ImportMenu));
        assert!(main_menu.iter().any(|i| i.action == SettingsAction::ResetMainMenu));
        let entry_actions = build_items(&cfg, &store, &installed, &SettingsView::MainMenuEntry(0));
        assert!(entry_actions.iter().any(|i| i.label == "Move Earlier"));
        assert!(entry_actions.iter().any(|i| i.label == "Move Later"));

        // Unpin clipboard -> MainMenuAdd must offer Clipboard, NOT Files/Apps/etc.
        cfg.main_menu.retain(|e| e.menu != "clipboard");
        let add = build_items(&cfg, &store, &installed, &SettingsView::MainMenuAdd);
        assert_eq!(add.len(), 1);
        assert_eq!(add[0].label, "Clipboard");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn appearance_view_matches_linux_toggles() {
        let (dir, store) = temp_env("appearance");
        let installed = InstalledModules::detect_in(dir.join("submenus"));
        let mut cfg = AppConfig::default_for_installed(&installed);

        let items = build_items(&cfg, &store, &installed, &SettingsView::Appearance);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Overflow: Box",
                "Animations",
                "3D Loading",
                "Translucency",
                "Blur",
                "High Contrast",
                "Reset Appearance",
            ]
        );
        assert_eq!(items[0].icon, BuiltinIcon::Window);
        assert!(items[0].active);
        assert_eq!(items[1].icon, BuiltinIcon::ToggleOn);
        assert!(items[1].active);
        assert_eq!(items[2].icon, BuiltinIcon::ToggleOn);
        assert!(items[2].active);
        assert_eq!(items[3].icon, BuiltinIcon::Material);
        assert!(items[3].active);
        assert_eq!(items[3].action, SettingsAction::AdjustSlider(SliderField::Translucency));
        assert_eq!(items[4].icon, BuiltinIcon::Blur);
        assert!(items[4].active);
        assert_eq!(items[4].action, SettingsAction::AdjustSlider(SliderField::Blur));
        assert_eq!(items[5].icon, BuiltinIcon::ToggleOff);
        assert!(!items[5].active);
        assert_eq!(items[6].icon, BuiltinIcon::Reset);

        cfg.appearance.layout = OverflowLayout::Ring;
        cfg.appearance.reduced_motion = true;
        cfg.appearance.loading_animation = LoadingStyle::TwoD;
        SliderField::Translucency.set(&mut cfg.appearance, 0);
        SliderField::Blur.set(&mut cfg.appearance, 0);
        cfg.appearance.high_contrast = true;

        let flipped = build_items(&cfg, &store, &installed, &SettingsView::Appearance);
        assert_eq!(flipped[0].label, "Overflow: Rings");
        assert!(!flipped[0].active);
        assert_eq!(flipped[1].icon, BuiltinIcon::ToggleOff);
        assert!(!flipped[1].active);
        assert_eq!(flipped[2].icon, BuiltinIcon::ToggleOff);
        assert!(!flipped[2].active);
        assert_eq!(flipped[3].icon, BuiltinIcon::Material);
        assert!(!flipped[3].active);
        assert!(flipped[3].detail.starts_with("0%"));
        assert_eq!(flipped[4].icon, BuiltinIcon::Blur);
        assert!(!flipped[4].active);
        assert!(flipped[4].detail.starts_with("0%"));
        assert_eq!(flipped[5].icon, BuiltinIcon::ToggleOn);
        assert!(flipped[5].active);

        let colors = build_items(&cfg, &store, &installed, &SettingsView::MenuColors);
        assert_eq!(colors[0].label, "Theme");
        assert_eq!(colors[0].action, SettingsAction::OpenView(SettingsView::ColorTheme));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn color_field_getters_and_setters_cover_clipboard() {
        let mut c = ColorsConfig::default();
        assert_eq!(ColorField::AccentClipboard.get(&c), "#faad8a");
        ColorField::AccentClipboard.set(&mut c, "#123456".into());
        assert_eq!(c.accents.clipboard, "#123456");
    }
}
