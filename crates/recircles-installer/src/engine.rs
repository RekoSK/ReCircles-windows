use recircles_core::config::{
    AppConfig, AppearanceConfig, ConfigStore, KeyList, KeybindsConfig, MainMenuEntry, UpdateMode,
};
use recircles_core::modules::{BuiltinSubmenu, InstalledModules};
use recircles_core::paths;
use recircles_core::themes::builtin_theme;
use recircles_core::win_keys::parse_global_hotkey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub mod payload {
    include!(concat!(env!("OUT_DIR"), "/embedded_payload.rs"));

    pub fn dll_bytes(sub: recircles_core::modules::BuiltinSubmenu) -> Option<&'static [u8]> {
        use recircles_core::modules::BuiltinSubmenu::*;
        match sub {
            Files => Some(DLL_FILES),
            Apps => Some(DLL_APPS),
            Windows => Some(DLL_WINDOWS),
            Clipboard => Some(DLL_CLIPBOARD),
            Settings => None,
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub const SUBMENU_EXCLUSION_WARNING: &str =
    "Warning: Unchecked submenus will NOT be installed on your system at all. \
     To restore any skipped submenu later, re-run this ReCircles installer. \
     Settings is mandatory and cannot be unselected.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavPreset {
    Standard,
    Vim,
}

impl NavPreset {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard (Arrows / Tab / Enter)",
            Self::Vim => "Vim Friendly (Arrows + Ctrl+HJKL)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub install_dir: PathBuf,
    pub config_dir: PathBuf,
    pub theme: String,
    pub appearance: AppearanceConfig,
    pub keybinds: KeybindsConfig,
    pub custom_keybinds: bool,
    pub main_hotkey: String,
    pub submenu_hotkeys: BTreeMap<String, String>,
    pub nav_preset: NavPreset,
    pub selected_submenus: BTreeSet<BuiltinSubmenu>,
    pub update_mode: UpdateMode,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub start_with_windows: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub create_shortcuts: bool,
    pub launch_daemon: bool,
    pub register_windows_integration: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        let mut selected = BTreeSet::new();
        for sub in BuiltinSubmenu::ALL {
            selected.insert(sub);
        }
        Self {
            install_dir: default_install_dir(),
            config_dir: paths::config_dir(),
            theme: "default".into(),
            appearance: AppearanceConfig::default(),
            keybinds: KeybindsConfig::default(),
            custom_keybinds: false,
            main_hotkey: "Win+e".into(),
            submenu_hotkeys: BTreeMap::new(),
            nav_preset: NavPreset::Standard,
            selected_submenus: selected,
            update_mode: UpdateMode::AutoInstall,
            start_with_windows: true,
            create_shortcuts: true,
            launch_daemon: true,
            register_windows_integration: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpgradeOptions {
    pub install_dir: Option<PathBuf>,
    pub config_dir: PathBuf,
    pub wait_pid: Option<u32>,
    pub launch_daemon: bool,
    pub register_windows_integration: bool,
}

impl Default for UpgradeOptions {
    fn default() -> Self {
        Self {
            install_dir: None,
            config_dir: paths::config_dir(),
            wait_pid: None,
            launch_daemon: true,
            register_windows_integration: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpgradeProgress {
    pub fraction: f32,
    pub status: &'static str,
}

#[derive(Debug, Clone)]
pub struct UpgradeSummary {
    pub install_dir: PathBuf,
    #[allow(dead_code)]
    pub previous_version: Option<String>,
    pub new_version: String,
    pub updated_dlls: Vec<String>,
    pub skipped_dlls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallManifest {
    pub version: String,
    pub install_dir: String,
    pub submenus: Vec<String>,
    pub theme: String,
}

#[derive(Debug, Clone)]
pub struct InstallSummary {
    pub install_dir: PathBuf,
    pub config_path: PathBuf,
    pub installed_dlls: Vec<String>,
    pub omitted_dlls: Vec<String>,
    pub active_hotkeys: Vec<(String, String)>,
}

pub fn default_install_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA")
            && !local.trim().is_empty()
        {
            return PathBuf::from(local).join("Programs").join("ReCircles");
        }
    }
    paths::config_dir().join("app")
}

pub fn enforce_mandatory_submenus(selected: &mut BTreeSet<BuiltinSubmenu>) {
    selected.insert(BuiltinSubmenu::Settings);
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn load_existing_manifest(config_dir: &Path) -> Option<InstallManifest> {
    let p = paths::install_manifest_in(config_dir);
    let raw = fs::read_to_string(p).ok()?;
    serde_json::from_str(&raw).ok()
}

pub type PayloadOverride<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

pub fn execute_install(
    opts: &InstallOptions,
    override_payload: Option<PayloadOverride<'_>>,
) -> Result<InstallSummary, String> {
    if opts.install_dir.as_os_str().is_empty() {
        return Err("Install directory cannot be empty".into());
    }
    let mut selected = opts.selected_submenus.clone();
    enforce_mandatory_submenus(&mut selected);

    // Validate theme BEFORE writing binaries/DLLs to disk
    let store = ConfigStore::in_dir(opts.config_dir.clone());
    if builtin_theme(&opts.theme).is_none() && store.resolve_theme_colors(&opts.theme).is_none() {
        return Err(format!("Unknown theme '{}'", opts.theme));
    }

    // Validate keybind syntax AND conflicts BEFORE writing binaries/DLLs to disk
    let mut preview_kb = opts.keybinds.clone();
    if opts.custom_keybinds {
        let main_hk = opts.main_hotkey.trim();
        preview_kb.main_menu = if main_hk.is_empty() {
            KeyList::default()
        } else {
            parse_global_hotkey(main_hk)
                .map_err(|e| format!("Invalid Main Ring hotkey '{main_hk}': {e}"))?;
            KeyList::one(main_hk)
        };
        if opts.nav_preset == NavPreset::Vim {
            preview_kb.previous = KeyList::many(&["Up", "Shift+Tab", "Ctrl+k"]);
            preview_kb.next = KeyList::many(&["Down", "Tab", "Ctrl+j"]);
            preview_kb.undo_completion = KeyList::many(&["Left", "Ctrl+h"]);
            preview_kb.accept_completion = KeyList::many(&["Right", "Ctrl+l"]);
        }
        preview_kb.menu_keybinds.clear();
        for (id, spec) in &opts.submenu_hotkeys {
            let trimmed = spec.trim();
            if trimmed.is_empty() {
                continue;
            }
            let allowed = BuiltinSubmenu::from_id(id).is_none_or(|sub| selected.contains(&sub));
            if !allowed {
                continue;
            }
            parse_global_hotkey(trimmed)
                .map_err(|e| format!("Invalid hotkey '{trimmed}' for '{id}': {e}"))?;
            preview_kb.set_menu_hotkey(id, KeyList::one(trimmed));
        }
    }
    preview_kb.validate()?;

    #[cfg(windows)]
    if opts.register_windows_integration {
        let _ = recircles_core::ipc::win::close_existing_ui_instances();
        let _ = recircles_core::ipc::win::stop_daemon_gracefully();
        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    let submenus_dir = opts.install_dir.join("submenus");
    fs::create_dir_all(&opts.install_dir)
        .map_err(|e| format!("Cannot create {}: {e}", opts.install_dir.display()))?;
    fs::create_dir_all(&submenus_dir)
        .map_err(|e| format!("Cannot create {}: {e}", submenus_dir.display()))?;

    let read_artifact = |name: &str, embedded: &'static [u8]| -> std::borrow::Cow<'static, [u8]> {
        if let Some(cb) = override_payload
            && let Some(bytes) = cb(name)
        {
            return std::borrow::Cow::Owned(bytes);
        }
        std::borrow::Cow::Borrowed(embedded)
    };

    // Write core binaries if embedded bytes are non-empty (or stub override)
    let ui_bytes = read_artifact("recircles.exe", payload::BIN_UI);
    if !ui_bytes.is_empty() {
        atomic_write(&opts.install_dir.join("recircles.exe"), &ui_bytes)?;
    }
    let daemon_bytes = read_artifact("recircles-daemon.exe", payload::BIN_DAEMON);
    if !daemon_bytes.is_empty() {
        atomic_write(&opts.install_dir.join("recircles-daemon.exe"), &daemon_bytes)?;
    }

    // Copy installer executable into install_dir for modify/reinstall/uninstall later
    if opts.register_windows_integration
        && let Ok(cur_exe) = std::env::current_exe()
    {
        let target_setup = opts.install_dir.join("recircles-setup.exe");
        if cur_exe != target_setup && cur_exe.is_file() {
            let _ = fs::copy(&cur_exe, &target_setup);
        }
    }

    // CRITICAL REQUIREMENT: install ONLY selected DLLs, delete any unselected ones from disk!
    let mut installed_dlls = Vec::new();
    let mut omitted_dlls = Vec::new();
    for sub in BuiltinSubmenu::OPTIONAL {
        let dll_name = sub.dll_filename().expect("optional sub has dll");
        let dst = submenus_dir.join(dll_name);
        if selected.contains(&sub) {
            let bytes = read_artifact(dll_name, payload::dll_bytes(sub).unwrap_or(b""));
            if !bytes.is_empty() {
                atomic_write(&dst, &bytes)?;
            } else {
                // Keep placeholder ONLY if override/stub explicitly empty during no-payload unit tests
                if override_payload.is_some() {
                    atomic_write(&dst, b"")?;
                }
            }
            installed_dlls.push(dll_name.to_string());
        } else {
            if dst.exists() {
                fs::remove_file(&dst).map_err(|e| {
                    format!("Failed removing unselected submenu '{}': {e}", dst.display())
                })?;
            }
            omitted_dlls.push(dll_name.to_string());
        }
    }

    // Build and write configuration aligned with installed DLLs, theme, and keybinds
    let store = ConfigStore::in_dir(opts.config_dir.clone());
    store.ensure_dirs().map_err(|e| e.to_string())?;
    let detected = InstalledModules::detect_in(submenus_dir.clone());

    let mut cfg = if store.config_path().is_file() {
        let mut existing = store
            .load_or_create(&detected)
            .unwrap_or_else(|_| AppConfig::default_for_installed(&detected));
        // Rebuild built-in order matching user's chosen submenus while preserving any custom menus
        let custom_entries: Vec<MainMenuEntry> = existing
            .main_menu
            .iter()
            .filter(|e| BuiltinSubmenu::from_id(&e.menu).is_none())
            .cloned()
            .collect();
        let mut next_main: Vec<MainMenuEntry> = BuiltinSubmenu::ALL
            .iter()
            .copied()
            .filter(|s| selected.contains(s))
            .map(|s| MainMenuEntry::new(s.id()))
            .collect();
        next_main.extend(custom_entries);
        existing.main_menu = next_main;
        existing
    } else {
        let main_menu: Vec<MainMenuEntry> = BuiltinSubmenu::ALL
            .iter()
            .copied()
            .filter(|s| selected.contains(s))
            .map(|s| MainMenuEntry::new(s.id()))
            .collect();
        let mut fresh = AppConfig::default_for_installed(&detected);
        fresh.main_menu = main_menu;
        fresh
    };

    // Apply appearance settings selected in the installer
    cfg.appearance.layout = opts.appearance.layout;
    cfg.appearance.translucency = opts.appearance.translucency.min(100);
    cfg.appearance.blur = opts.appearance.blur.min(100);
    cfg.appearance.high_contrast = opts.appearance.high_contrast;
    cfg.appearance.reduced_motion = opts.appearance.reduced_motion;
    cfg.appearance.loading_animation = opts.appearance.loading_animation;
    cfg.appearance.validate();

    if let Some(bt) = builtin_theme(&opts.theme) {
        cfg.appearance.theme = bt.id.to_string();
        cfg.colors = bt.colors;
    } else if let Some(local) = store.resolve_theme_colors(&opts.theme) {
        cfg.appearance.theme = recircles_core::themes::normalize_theme_name(&opts.theme);
        cfg.colors = local;
    } else {
        return Err(format!("Unknown theme '{}'", opts.theme));
    }

    // Apply update mode chosen in the installer
    cfg.updates.mode = opts.update_mode;

    // Keybinds (already validated into preview_kb; prune any custom menu refs that don't exist)
    preview_kb.menu_keybinds.retain(|id, _| match BuiltinSubmenu::from_id(id) {
        Some(sub) => selected.contains(&sub),
        None => cfg.menus.contains_key(id),
    });
    cfg.keybinds = preview_kb;
    cfg.prune_uninstalled(&detected);
    cfg.validate()?;
    store.save(&cfg).map_err(|e| e.to_string())?;

    let mut sub_ids: Vec<String> = selected.iter().map(|s| s.id().to_string()).collect();
    sub_ids.sort();
    let manifest = InstallManifest {
        version: env!("CARGO_PKG_VERSION").to_string(),
        install_dir: opts.install_dir.to_string_lossy().into_owned(),
        submenus: sub_ids,
        theme: cfg.appearance.theme.clone(),
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    atomic_write(&paths::install_manifest_in(&opts.config_dir), manifest_json.as_bytes())?;

    #[cfg(windows)]
    if opts.register_windows_integration {
        win_sys::apply_windows_integration(opts)?;
        if opts.launch_daemon {
            let _ = recircles_core::ipc::win::launch_daemon_detached(&opts.install_dir);
        }
    }

    let mut active_hotkeys = Vec::new();
    if let Some(m) = cfg.keybinds.main_menu.primary() {
        active_hotkeys.push(("Main Ring".into(), m.to_string()));
    }
    for (id, keys) in &cfg.keybinds.menu_keybinds {
        if let Some(p) = keys.primary() {
            let lbl = BuiltinSubmenu::from_id(id)
                .map(|s| s.title().to_string())
                .unwrap_or_else(|| id.clone());
            active_hotkeys.push((lbl, p.to_string()));
        }
    }

    Ok(InstallSummary {
        install_dir: opts.install_dir.clone(),
        config_path: store.config_path(),
        installed_dlls,
        omitted_dlls,
        active_hotkeys,
    })
}

pub fn execute_upgrade<F>(
    opts: &UpgradeOptions,
    override_payload: Option<PayloadOverride<'_>>,
    mut on_progress: F,
) -> Result<UpgradeSummary, String>
where
    F: FnMut(UpgradeProgress),
{
    on_progress(UpgradeProgress {
        fraction: 0.08,
        status: "Preparing update…",
    });

    let existing_manifest = load_existing_manifest(&opts.config_dir);
    let install_dir = opts
        .install_dir
        .clone()
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            existing_manifest
                .as_ref()
                .map(|m| PathBuf::from(m.install_dir.trim()))
                .filter(|p| !p.as_os_str().is_empty())
        })
        .unwrap_or_else(default_install_dir);

    let submenus_dir = install_dir.join("submenus");

    // Preserve exact set of installed optional submenus (never re-install skipped DLLs!)
    let mut selected = BTreeSet::new();
    if let Some(ref m) = existing_manifest
        && !m.submenus.is_empty()
    {
        for id in &m.submenus {
            if let Some(sub) = BuiltinSubmenu::from_id(id) {
                selected.insert(sub);
            }
        }
    } else {
        let detected = InstalledModules::detect_in(submenus_dir.clone());
        let any_optional = BuiltinSubmenu::OPTIONAL
            .iter()
            .any(|&sub| detected.is_installed(sub));
        if any_optional || install_dir.join("recircles.exe").is_file() {
            for &sub in BuiltinSubmenu::OPTIONAL.iter() {
                if detected.is_installed(sub) {
                    selected.insert(sub);
                }
            }
        } else {
            for sub in BuiltinSubmenu::ALL {
                selected.insert(sub);
            }
        }
    }
    enforce_mandatory_submenus(&mut selected);

    on_progress(UpgradeProgress {
        fraction: 0.22,
        status: "Stopping ReCircles…",
    });

    #[cfg(windows)]
    if opts.register_windows_integration {
        let _ = recircles_core::ipc::win::close_existing_ui_instances();
        let _ = recircles_core::ipc::win::stop_daemon_gracefully();
        wait_for_processes_to_exit(opts.wait_pid);
    }

    fs::create_dir_all(&install_dir)
        .map_err(|e| format!("Cannot create {}: {e}", install_dir.display()))?;
    fs::create_dir_all(&submenus_dir)
        .map_err(|e| format!("Cannot create {}: {e}", submenus_dir.display()))?;

    let read_artifact = |name: &str, embedded: &'static [u8]| -> std::borrow::Cow<'static, [u8]> {
        if let Some(cb) = override_payload
            && let Some(bytes) = cb(name)
        {
            return std::borrow::Cow::Owned(bytes);
        }
        std::borrow::Cow::Borrowed(embedded)
    };

    on_progress(UpgradeProgress {
        fraction: 0.44,
        status: "Updating core binaries…",
    });

    let ui_bytes = read_artifact("recircles.exe", payload::BIN_UI);
    if !ui_bytes.is_empty() {
        atomic_write_with_retry(&install_dir.join("recircles.exe"), &ui_bytes)?;
    }
    let daemon_bytes = read_artifact("recircles-daemon.exe", payload::BIN_DAEMON);
    if !daemon_bytes.is_empty() {
        atomic_write_with_retry(&install_dir.join("recircles-daemon.exe"), &daemon_bytes)?;
    }

    if opts.register_windows_integration
        && let Ok(cur_exe) = std::env::current_exe()
    {
        let target_setup = install_dir.join("recircles-setup.exe");
        if cur_exe != target_setup && cur_exe.is_file() {
            let _ = copy_file_with_retry(&cur_exe, &target_setup);
        }
    }

    on_progress(UpgradeProgress {
        fraction: 0.68,
        status: "Updating submenus…",
    });

    let mut updated_dlls = Vec::new();
    let mut skipped_dlls = Vec::new();
    for sub in BuiltinSubmenu::OPTIONAL {
        let dll_name = sub.dll_filename().expect("optional sub has dll");
        let dst = submenus_dir.join(dll_name);
        if selected.contains(&sub) {
            let bytes = read_artifact(dll_name, payload::dll_bytes(sub).unwrap_or(b""));
            if !bytes.is_empty() {
                atomic_write_with_retry(&dst, &bytes)?;
            } else if override_payload.is_some() {
                atomic_write_with_retry(&dst, b"")?;
            }
            updated_dlls.push(dll_name.to_string());
        } else {
            skipped_dlls.push(dll_name.to_string());
        }
    }

    on_progress(UpgradeProgress {
        fraction: 0.84,
        status: "Finalizing update…",
    });

    // Preserve user config completely — only refresh installed.json version metadata
    fs::create_dir_all(&opts.config_dir)
        .map_err(|e| format!("Cannot create {}: {e}", opts.config_dir.display()))?;
    let mut sub_ids: Vec<String> = selected.iter().map(|s| s.id().to_string()).collect();
    sub_ids.sort();
    let previous_version = existing_manifest.as_ref().map(|m| m.version.clone());
    let preserved_theme = existing_manifest
        .as_ref()
        .map(|m| m.theme.clone())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "default".into());
    let new_version = env!("CARGO_PKG_VERSION").to_string();
    let manifest = InstallManifest {
        version: new_version.clone(),
        install_dir: install_dir.to_string_lossy().into_owned(),
        submenus: sub_ids,
        theme: preserved_theme,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    atomic_write_with_retry(
        &paths::install_manifest_in(&opts.config_dir),
        manifest_json.as_bytes(),
    )?;

    #[cfg(windows)]
    if opts.register_windows_integration {
        let _ = win_sys::update_uninstall_metadata(&install_dir);
    }

    on_progress(UpgradeProgress {
        fraction: 0.94,
        status: "Launching ReCircles…",
    });

    #[cfg(windows)]
    if opts.register_windows_integration && opts.launch_daemon {
        let _ = recircles_core::ipc::win::launch_daemon_detached(&install_dir);
    }

    on_progress(UpgradeProgress {
        fraction: 1.0,
        status: "Updated!",
    });

    Ok(UpgradeSummary {
        install_dir,
        previous_version,
        new_version,
        updated_dlls,
        skipped_dlls,
    })
}

pub fn execute_uninstall(install_dir: &Path, config_dir: &Path, purge_config: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = recircles_core::ipc::win::close_existing_ui_instances();
        let _ = recircles_core::ipc::win::stop_daemon_gracefully();
        std::thread::sleep(std::time::Duration::from_millis(120));
        win_sys::remove_windows_integration();
    }

    let sub_dir = install_dir.join("submenus");
    if sub_dir.exists() {
        let _ = fs::remove_dir_all(&sub_dir);
    }
    for name in ["recircles.exe", "recircles-daemon.exe"] {
        let p = install_dir.join(name);
        if p.exists() {
            let _ = fs::remove_file(p);
        }
    }
    let manifest = paths::install_manifest_in(config_dir);
    if manifest.exists() {
        let _ = fs::remove_file(manifest);
    }
    if purge_config && config_dir.exists() {
        let _ = fs::remove_dir_all(config_dir);
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| format!("Failed writing {}: {e}", tmp.display()))?;
    let _ = fs::remove_file(path);
    fs::rename(&tmp, path).map_err(|e| format!("Failed replacing {}: {e}", path.display()))
}

fn atomic_write_with_retry(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 0..15 {
        match atomic_write(path, bytes) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = e;
                if attempt < 14 {
                    std::thread::sleep(std::time::Duration::from_millis(120));
                }
            }
        }
    }
    Err(last_err)
}

#[cfg(windows)]
fn copy_file_with_retry(src: &Path, dst: &Path) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 0..10 {
        match fs::copy(src, dst) {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = e.to_string();
                if attempt < 9 {
                    std::thread::sleep(std::time::Duration::from_millis(120));
                }
            }
        }
    }
    Err(last_err)
}

#[cfg(windows)]
fn wait_for_processes_to_exit(wait_pid: Option<u32>) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

    if let Some(pid) = wait_pid
        && pid != 0
        && pid != std::process::id()
    {
        const SYNCHRONIZE: u32 = 0x0010_0000;
        let h = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
        if !h.is_null() {
            unsafe {
                WaitForSingleObject(h, 4000);
                CloseHandle(h);
            }
        }
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let ui_gone = recircles_core::ipc::win::find_ui_hwnd().is_null();
        let daemon_gone = recircles_core::ipc::win::find_daemon_hwnd().is_null();
        if ui_gone && daemon_gone {
            if let Some(guard) = recircles_core::ipc::win::acquire_daemon_mutex() {
                drop(guard);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
}

#[cfg(windows)]
mod win_sys {
    use super::InstallOptions;
    use recircles_core::ipc::win::to_wide_nul;
    use std::ffi::c_void;
    use std::path::PathBuf;
    use windows_sys::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WRITE, REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
    };
    use windows_sys::core::{GUID, HRESULT};

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ReCircles";

    fn write_reg_sz(root: HKEY, subkey: &str, name: &str, value: &str) -> Result<(), String> {
        let sub_w = to_wide_nul(subkey);
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc = unsafe {
            RegCreateKeyExW(
                root,
                sub_w.as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };
        if rc != 0 {
            return Err(format!("RegCreateKeyExW('{subkey}') failed: {rc}"));
        }
        let name_w = to_wide_nul(name);
        let val_w = to_wide_nul(value);
        let rc2 = unsafe {
            RegSetValueExW(
                hkey,
                name_w.as_ptr(),
                0,
                REG_SZ,
                val_w.as_ptr().cast(),
                (val_w.len() * 2) as u32,
            )
        };
        unsafe { RegCloseKey(hkey) };
        if rc2 != 0 {
            return Err(format!("RegSetValueExW('{name}') failed: {rc2}"));
        }
        Ok(())
    }

    fn delete_reg_value(root: HKEY, subkey: &str, name: &str) {
        let sub_w = to_wide_nul(subkey);
        let mut hkey: HKEY = std::ptr::null_mut();
        if unsafe { RegOpenKeyExW(root, sub_w.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) } == 0 {
            let name_w = to_wide_nul(name);
            unsafe {
                RegDeleteValueW(hkey, name_w.as_ptr());
                RegCloseKey(hkey);
            }
        }
    }

    fn start_menu_dir() -> Option<PathBuf> {
        std::env::var("APPDATA")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|a| PathBuf::from(a).join("Microsoft\\Windows\\Start Menu\\Programs\\ReCircles"))
    }

    pub fn update_uninstall_metadata(install_dir: &std::path::Path) -> Result<(), String> {
        let ui_exe = install_dir.join("recircles.exe");
        let setup_exe = install_dir.join("recircles-setup.exe");
        write_reg_sz(HKEY_CURRENT_USER, UNINSTALL_KEY, "DisplayName", "ReCircles")?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "DisplayVersion",
            env!("CARGO_PKG_VERSION"),
        )?;
        write_reg_sz(HKEY_CURRENT_USER, UNINSTALL_KEY, "Publisher", "ReCircles")?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "InstallLocation",
            &install_dir.to_string_lossy(),
        )?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "UninstallString",
            &format!("\"{}\" --uninstall", setup_exe.display()),
        )?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "ModifyPath",
            &format!("\"{}\"", setup_exe.display()),
        )?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "DisplayIcon",
            &format!("{},0", ui_exe.display()),
        )?;
        Ok(())
    }

    pub fn apply_windows_integration(opts: &InstallOptions) -> Result<(), String> {
        let daemon_exe = opts.install_dir.join("recircles-daemon.exe");
        let ui_exe = opts.install_dir.join("recircles.exe");
        let setup_exe = opts.install_dir.join("recircles-setup.exe");

        if opts.start_with_windows {
            write_reg_sz(
                HKEY_CURRENT_USER,
                RUN_KEY,
                "ReCirclesDaemon",
                &format!("\"{}\"", daemon_exe.display()),
            )?;
        } else {
            delete_reg_value(HKEY_CURRENT_USER, RUN_KEY, "ReCirclesDaemon");
        }

        write_reg_sz(HKEY_CURRENT_USER, UNINSTALL_KEY, "DisplayName", "ReCircles")?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "DisplayVersion",
            env!("CARGO_PKG_VERSION"),
        )?;
        write_reg_sz(HKEY_CURRENT_USER, UNINSTALL_KEY, "Publisher", "ReCircles")?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "InstallLocation",
            &opts.install_dir.to_string_lossy(),
        )?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "UninstallString",
            &format!("\"{}\" --uninstall", setup_exe.display()),
        )?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "ModifyPath",
            &format!("\"{}\"", setup_exe.display()),
        )?;
        write_reg_sz(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            "DisplayIcon",
            &format!("{},0", ui_exe.display()),
        )?;

        if opts.create_shortcuts
            && let Some(sm) = start_menu_dir()
        {
            let _ = std::fs::create_dir_all(&sm);
            create_lnk(&sm.join("ReCircles.lnk"), &ui_exe, "", &opts.install_dir);
            create_lnk(
                &sm.join("ReCircles Settings.lnk"),
                &ui_exe,
                "--run settings",
                &opts.install_dir,
            );
            create_lnk(
                &sm.join("ReCircles Installer.lnk"),
                &setup_exe,
                "",
                &opts.install_dir,
            );
        }
        Ok(())
    }

    pub fn remove_windows_integration() {
        delete_reg_value(HKEY_CURRENT_USER, RUN_KEY, "ReCirclesDaemon");
        let sub_w = to_wide_nul(UNINSTALL_KEY);
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, sub_w.as_ptr());
        }
        if let Some(sm) = start_menu_dir()
            && sm.exists()
        {
            let _ = std::fs::remove_dir_all(sm);
        }
    }

    pub(super) fn create_lnk(
        lnk_path: &std::path::Path,
        target: &std::path::Path,
        args: &str,
        work_dir: &std::path::Path,
    ) -> bool {
        #[repr(C)]
        struct ComUnknown {
            vtbl: *const *const c_void,
        }

        const CLSID_SHELL_LINK: GUID = GUID {
            data1: 0x0002_1401,
            data2: 0x0000,
            data3: 0x0000,
            data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
        };
        const IID_ISHELL_LINK_W: GUID = GUID {
            data1: 0x0002_14F9,
            data2: 0x0000,
            data3: 0x0000,
            data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
        };
        const IID_IPERSIST_FILE: GUID = GUID {
            data1: 0x0000_010B,
            data2: 0x0000,
            data3: 0x0000,
            data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
        };

        unsafe {
            let hr_com = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            let com_inited = hr_com >= 0;

            let mut link_raw: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_SHELL_LINK,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ISHELL_LINK_W,
                &mut link_raw,
            );
            let mut ok = false;
            if hr >= 0 && !link_raw.is_null() {
                let link_obj = link_raw as *mut ComUnknown;
                let vtbl = (*link_obj).vtbl;

                type QueryInterfaceFn =
                    unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
                type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
                type SetWideFn = unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT;
                type SetIconLocationFn =
                    unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT;
                type SaveFn = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT;

                let query_interface: QueryInterfaceFn = std::mem::transmute(*vtbl.add(0));
                let link_release: ReleaseFn = std::mem::transmute(*vtbl.add(2));
                let set_work_dir: SetWideFn = std::mem::transmute(*vtbl.add(9));
                let set_args: SetWideFn = std::mem::transmute(*vtbl.add(11));
                let set_icon_location: SetIconLocationFn = std::mem::transmute(*vtbl.add(17));
                let set_path: SetWideFn = std::mem::transmute(*vtbl.add(20));

                let target_w = to_wide_nul(&target.to_string_lossy());
                let work_w = to_wide_nul(&work_dir.to_string_lossy());
                let args_w = to_wide_nul(args);
                let lnk_w = to_wide_nul(&lnk_path.to_string_lossy());

                let mut hr_set = set_path(link_raw, target_w.as_ptr());
                if hr_set >= 0 && !args.is_empty() {
                    hr_set = set_args(link_raw, args_w.as_ptr());
                }
                if hr_set >= 0 {
                    hr_set = set_work_dir(link_raw, work_w.as_ptr());
                }
                if hr_set >= 0 {
                    let _ = set_icon_location(link_raw, target_w.as_ptr(), 0);
                }

                if hr_set >= 0 {
                    let mut pf_raw: *mut c_void = std::ptr::null_mut();
                    if query_interface(link_raw, &IID_IPERSIST_FILE, &mut pf_raw) >= 0
                        && !pf_raw.is_null()
                    {
                        let pf_vtbl = (*(pf_raw as *mut ComUnknown)).vtbl;
                        let pf_release: ReleaseFn = std::mem::transmute(*pf_vtbl.add(2));
                        let pf_save: SaveFn = std::mem::transmute(*pf_vtbl.add(6));
                        if pf_save(pf_raw, lnk_w.as_ptr(), 1) >= 0 {
                            ok = true;
                        }
                        pf_release(pf_raw);
                    }
                }
                link_release(link_raw);
            }

            if com_inited {
                CoUninitialize();
            }
            ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("recircles_inst_{tag}_{nanos}"));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn selective_install_omits_unselected_dlls_enforces_settings_and_prunes_on_reinstall() {
        let root = temp_root("selective");
        let install_dir = root.join("app");
        let config_dir = root.join("cfg");

        let mut selected = BTreeSet::new();
        selected.insert(BuiltinSubmenu::Files);
        selected.insert(BuiltinSubmenu::Clipboard);
        // Explicitly omit Settings to verify mandatory enforcement!
        let mut hotkeys = BTreeMap::new();
        hotkeys.insert("clipboard".into(), "Ctrl+Alt+v".into());
        hotkeys.insert("apps".into(), "Ctrl+Alt+a".into()); // unselected -> must not persist!

        let opts = InstallOptions {
            install_dir: install_dir.clone(),
            config_dir: config_dir.clone(),
            theme: "midnight".into(),
            appearance: AppearanceConfig::default(),
            keybinds: KeybindsConfig::default(),
            custom_keybinds: true,
            main_hotkey: "Ctrl+Shift+Space".into(),
            submenu_hotkeys: hotkeys,
            nav_preset: NavPreset::Vim,
            selected_submenus: selected,
            update_mode: UpdateMode::AutoInstall,
            start_with_windows: false,
            create_shortcuts: false,
            launch_daemon: false,
            register_windows_integration: false,
        };

        let provider = |name: &str| Some(format!("stub:{name}").into_bytes());
        let summary = execute_install(&opts, Some(&provider)).unwrap();

        let sub_dir = install_dir.join("submenus");
        assert!(sub_dir.join("recircles_files.dll").is_file());
        assert!(sub_dir.join("recircles_clipboard.dll").is_file());
        // Unselected submenus are literally NOT on disk at all!
        assert!(!sub_dir.join("recircles_apps.dll").exists());
        assert!(!sub_dir.join("recircles_windows.dll").exists());
        assert_eq!(summary.omitted_dlls.len(), 2);

        let installed = InstalledModules::detect_in(sub_dir.clone());
        let store = ConfigStore::in_dir(config_dir.clone());
        let cfg = store.load_or_create(&installed).unwrap();
        assert_eq!(cfg.appearance.theme, "midnight");
        assert_eq!(cfg.keybinds.main_menu.primary(), Some("Ctrl+Shift+Space"));
        assert_eq!(
            cfg.keybinds.menu_hotkey("clipboard").primary(),
            Some("Ctrl+Alt+v")
        );
        assert!(cfg.keybinds.menu_hotkey("apps").is_empty());
        assert_eq!(
            cfg.main_menu
                .iter()
                .map(|e| e.menu.as_str())
                .collect::<Vec<_>>(),
            vec!["files", "clipboard", "settings"]
        );

        // Now modify/reinstall via installer WITHOUT clipboard, WITH apps -> must DELETE recircles_clipboard.dll!
        let mut next_selected = BTreeSet::new();
        next_selected.insert(BuiltinSubmenu::Files);
        next_selected.insert(BuiltinSubmenu::Apps);
        let opts2 = InstallOptions {
            selected_submenus: next_selected,
            ..opts
        };
        execute_install(&opts2, Some(&provider)).unwrap();
        assert!(!sub_dir.join("recircles_clipboard.dll").exists());
        assert!(sub_dir.join("recircles_apps.dll").is_file());

        // "Amber" resolves to "default" Amber palette
        let opts_amber = InstallOptions {
            theme: "Amber".into(),
            ..opts2.clone()
        };
        execute_install(&opts_amber, Some(&provider)).unwrap();
        let cfg_amber = store.load_or_create(&InstalledModules::detect_in(sub_dir.clone())).unwrap();
        assert_eq!(cfg_amber.appearance.theme, "default");
        assert_eq!(cfg_amber.colors.accent, "#ff9e47");

        // Unknown theme must fail
        let opts_bad_theme = InstallOptions {
            theme: "nonexistent_theme_xyz".into(),
            ..opts2.clone()
        };
        assert!(execute_install(&opts_bad_theme, Some(&provider)).is_err());

        // Conflicting main + submenu hotkeys must fail
        let mut dup_hks = BTreeMap::new();
        dup_hks.insert("files".into(), "Ctrl+Shift+Space".into());
        let opts_dup = InstallOptions {
            submenu_hotkeys: dup_hks,
            ..opts2
        };
        assert!(execute_install(&opts_dup, Some(&provider)).is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_options_do_not_create_or_delete_existing_files() {
        let root = temp_root("atomic_validate");
        let fresh_dir = root.join("fresh_app");
        let fresh_cfg = root.join("fresh_cfg");

        let mut selected = BTreeSet::new();
        selected.insert(BuiltinSubmenu::Files);
        selected.insert(BuiltinSubmenu::Clipboard);

        let provider = |name: &str| Some(format!("stub:{name}").into_bytes());

        // Empty install dir rejected cleanly
        let empty_opts = InstallOptions {
            install_dir: PathBuf::new(),
            config_dir: fresh_cfg.clone(),
            theme: "default".into(),
            appearance: AppearanceConfig::default(),
            keybinds: KeybindsConfig::default(),
            custom_keybinds: false,
            main_hotkey: "Win+e".into(),
            submenu_hotkeys: BTreeMap::new(),
            nav_preset: NavPreset::Standard,
            selected_submenus: selected.clone(),
            update_mode: UpdateMode::AutoInstall,
            start_with_windows: false,
            create_shortcuts: false,
            launch_daemon: false,
            register_windows_integration: false,
        };
        assert!(execute_install(&empty_opts, Some(&provider)).is_err());

        // Bad theme must NOT create fresh_dir or executables
        let bad_theme_opts = InstallOptions {
            install_dir: fresh_dir.clone(),
            config_dir: fresh_cfg.clone(),
            theme: "nonexistent_theme_xyz".into(),
            ..empty_opts.clone()
        };
        assert!(execute_install(&bad_theme_opts, Some(&provider)).is_err());
        assert!(!fresh_dir.exists());

        // Conflicting hotkey on a valid existing install must NOT delete unselected DLLs
        let valid_opts = InstallOptions {
            install_dir: fresh_dir.clone(),
            config_dir: fresh_cfg.clone(),
            theme: "default".into(),
            custom_keybinds: true,
            main_hotkey: "Ctrl+Shift+Space".into(),
            ..empty_opts
        };
        execute_install(&valid_opts, Some(&provider)).unwrap();
        let clip_dll = fresh_dir.join("submenus").join("recircles_clipboard.dll");
        assert!(clip_dll.is_file());

        let mut next_selected = BTreeSet::new();
        next_selected.insert(BuiltinSubmenu::Files); // omits Clipboard!
        let mut bad_hks = BTreeMap::new();
        bad_hks.insert("files".into(), "Ctrl+Shift+Space".into()); // conflicts with main_hotkey
        let failing_reinstall = InstallOptions {
            selected_submenus: next_selected,
            submenu_hotkeys: bad_hks,
            ..valid_opts
        };
        assert!(execute_install(&failing_reinstall, Some(&provider)).is_err());
        assert!(
            clip_dll.is_file(),
            "existing DLL must not be deleted when validation fails"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn creates_windows_shortcut_via_com_ishelllink() {
        let root = temp_root("lnk");
        let target = root.join("recircles.exe");
        fs::write(&target, b"stub").unwrap();
        let lnk = root.join("ReCircles.lnk");
        assert!(win_sys::create_lnk(&lnk, &target, "--run settings", &root));
        let meta = fs::metadata(&lnk).expect("shortcut file created");
        assert!(meta.len() > 64, "shortcut file should be valid ShellLink binary");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persists_custom_appearance_and_in_ring_keybinds() {
        let root = temp_root("appearance_keybinds");
        let install_dir = root.join("app");
        let config_dir = root.join("cfg");

        let mut appearance = AppearanceConfig::default();
        appearance.layout = recircles_core::config::OverflowLayout::Ring;
        appearance.translucency = 80;
        appearance.blur = 35;
        appearance.high_contrast = true;
        appearance.reduced_motion = true;
        appearance.loading_animation = recircles_core::config::LoadingStyle::TwoD;

        let mut keybinds = KeybindsConfig::default();
        keybinds.number_shortcuts = false;
        keybinds.main_menu = KeyList::one("Alt+Space");
        keybinds.back = KeyList::one("Backspace");
        keybinds.help = KeyList::one("F2");

        let opts = InstallOptions {
            install_dir: install_dir.clone(),
            config_dir: config_dir.clone(),
            theme: "light_green".into(),
            appearance,
            keybinds,
            custom_keybinds: false,
            main_hotkey: "Win+e".into(),
            submenu_hotkeys: BTreeMap::new(),
            nav_preset: NavPreset::Standard,
            selected_submenus: BuiltinSubmenu::ALL.into_iter().collect(),
            update_mode: UpdateMode::Notify,
            start_with_windows: false,
            create_shortcuts: false,
            launch_daemon: false,
            register_windows_integration: false,
        };

        let provider = |name: &str| Some(format!("stub:{name}").into_bytes());
        execute_install(&opts, Some(&provider)).expect("install succeeds");

        let store = ConfigStore::in_dir(config_dir);
        let detected = InstalledModules::detect_in(install_dir.join("submenus"));
        let cfg = store.load_or_create(&detected).expect("config loads");

        assert_eq!(cfg.appearance.theme, "light_green");
        assert_eq!(cfg.appearance.layout, recircles_core::config::OverflowLayout::Ring);
        assert_eq!(cfg.appearance.translucency, 80);
        assert_eq!(cfg.appearance.blur, 35);
        assert!(cfg.appearance.high_contrast);
        assert!(cfg.appearance.reduced_motion);
        assert_eq!(
            cfg.appearance.loading_animation,
            recircles_core::config::LoadingStyle::TwoD
        );

        assert!(!cfg.keybinds.number_shortcuts);
        assert_eq!(cfg.keybinds.main_menu.primary(), Some("Alt+Space"));
        assert_eq!(cfg.keybinds.back.primary(), Some("Backspace"));
        assert_eq!(cfg.keybinds.help.primary(), Some("F2"));
        assert_eq!(cfg.updates.mode, UpdateMode::Notify);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn upgrade_replaces_installed_binaries_and_preserves_config_and_exclusions() {
        let root = temp_root("upgrade_preserve");
        let install_dir = root.join("app");
        let config_dir = root.join("cfg");

        let mut selected = BTreeSet::new();
        selected.insert(BuiltinSubmenu::Files);
        selected.insert(BuiltinSubmenu::Clipboard);

        let opts = InstallOptions {
            install_dir: install_dir.clone(),
            config_dir: config_dir.clone(),
            theme: "midnight".into(),
            appearance: AppearanceConfig::default(),
            keybinds: KeybindsConfig::default(),
            custom_keybinds: true,
            main_hotkey: "Ctrl+Shift+Space".into(),
            submenu_hotkeys: BTreeMap::new(),
            nav_preset: NavPreset::Vim,
            selected_submenus: selected,
            update_mode: UpdateMode::Notify,
            start_with_windows: false,
            create_shortcuts: false,
            launch_daemon: false,
            register_windows_integration: false,
        };

        let v1 = |name: &str| Some(format!("v1:{name}").into_bytes());
        execute_install(&opts, Some(&v1)).expect("v1 install succeeds");

        let cfg_before = fs::read_to_string(paths::config_file_in(&config_dir)).unwrap();
        assert!(install_dir.join("submenus/recircles_files.dll").is_file());
        assert!(install_dir.join("submenus/recircles_clipboard.dll").is_file());
        assert!(!install_dir.join("submenus/recircles_apps.dll").exists());
        assert!(!install_dir.join("submenus/recircles_windows.dll").exists());

        let up_opts = UpgradeOptions {
            install_dir: Some(install_dir.clone()),
            config_dir: config_dir.clone(),
            wait_pid: None,
            launch_daemon: false,
            register_windows_integration: false,
        };
        let v2 = |name: &str| Some(format!("v2:{name}").into_bytes());
        let mut stages = Vec::new();
        let sum = execute_upgrade(&up_opts, Some(&v2), |p| stages.push(p.fraction))
            .expect("upgrade succeeds");

        assert!(stages.last().copied().unwrap_or(0.0) >= 0.99);
        assert_eq!(fs::read(install_dir.join("recircles.exe")).unwrap(), b"v2:recircles.exe");
        assert_eq!(
            fs::read(install_dir.join("recircles-daemon.exe")).unwrap(),
            b"v2:recircles-daemon.exe"
        );
        assert_eq!(
            fs::read(install_dir.join("submenus/recircles_files.dll")).unwrap(),
            b"v2:recircles_files.dll"
        );
        assert_eq!(
            fs::read(install_dir.join("submenus/recircles_clipboard.dll")).unwrap(),
            b"v2:recircles_clipboard.dll"
        );
        assert!(
            !install_dir.join("submenus/recircles_apps.dll").exists(),
            "upgrade must never install previously excluded submenus"
        );
        assert!(
            !install_dir.join("submenus/recircles_windows.dll").exists(),
            "upgrade must never install previously excluded submenus"
        );
        assert_eq!(sum.updated_dlls.len(), 2);
        assert_eq!(sum.skipped_dlls.len(), 2);

        let cfg_after = fs::read_to_string(paths::config_file_in(&config_dir)).unwrap();
        assert_eq!(cfg_before, cfg_after, "recircles.json must remain untouched on upgrade");

        let _ = fs::remove_dir_all(root);
    }
}
