use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BuiltinSubmenu {
    Files,
    Apps,
    Windows,
    Clipboard,
    Settings,
}

impl BuiltinSubmenu {
    pub const ALL: [BuiltinSubmenu; 5] = [
        Self::Files,
        Self::Apps,
        Self::Windows,
        Self::Clipboard,
        Self::Settings,
    ];

    pub const OPTIONAL: [BuiltinSubmenu; 4] = [
        Self::Files,
        Self::Apps,
        Self::Windows,
        Self::Clipboard,
    ];

    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "files" => Some(Self::Files),
            "apps" => Some(Self::Apps),
            "windows" => Some(Self::Windows),
            "clipboard" => Some(Self::Clipboard),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Apps => "apps",
            Self::Windows => "windows",
            Self::Clipboard => "clipboard",
            Self::Settings => "settings",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Files => "Files",
            Self::Apps => "Apps",
            Self::Windows => "Windows",
            Self::Clipboard => "Clipboard",
            Self::Settings => "Settings",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Files => "Browse folders, drives, pins, and recent paths",
            Self::Apps => "Launch installed applications sorted by usage",
            Self::Windows => "Switch focus between open desktop windows",
            Self::Clipboard => "ReCircles Windows clipboard history (text, files, images)",
            Self::Settings => "Core configuration, themes, hotkeys, and reset tools (Required)",
        }
    }

    pub const fn is_mandatory(self) -> bool {
        matches!(self, Self::Settings)
    }

    pub const fn dll_filename(self) -> Option<&'static str> {
        match self {
            Self::Files => Some("recircles_files.dll"),
            Self::Apps => Some("recircles_apps.dll"),
            Self::Windows => Some("recircles_windows.dll"),
            Self::Clipboard => Some("recircles_clipboard.dll"),
            Self::Settings => None,
        }
    }

    pub fn dll_path(self, submenus_dir: &Path) -> Option<PathBuf> {
        self.dll_filename().map(|name| submenus_dir.join(name))
    }
}

#[derive(Debug, Clone)]
pub struct InstalledModules {
    submenus_dir: PathBuf,
    installed: [bool; 5],
}

impl InstalledModules {
    pub fn detect_default() -> Self {
        Self::detect_in(crate::paths::submenus_dir())
    }

    pub fn detect_in(submenus_dir: PathBuf) -> Self {
        let mut installed = [false; 5];
        for (idx, sub) in BuiltinSubmenu::ALL.iter().enumerate() {
            installed[idx] = match sub.dll_path(&submenus_dir) {
                None => true, // Mandatory built-in (Settings)
                Some(path) => path.is_file(),
            };
        }
        Self {
            submenus_dir,
            installed,
        }
    }

    pub fn all_for_tests(submenus_dir: PathBuf) -> Self {
        Self {
            submenus_dir,
            installed: [true; 5],
        }
    }

    pub fn submenus_dir(&self) -> &Path {
        &self.submenus_dir
    }

    pub fn is_installed(&self, sub: BuiltinSubmenu) -> bool {
        let idx = BuiltinSubmenu::ALL
            .iter()
            .position(|&s| s == sub)
            .expect("all variants present");
        self.installed[idx]
    }

    pub fn is_id_installed(&self, id: &str) -> bool {
        match BuiltinSubmenu::from_id(id) {
            Some(sub) => self.is_installed(sub),
            None => true, // Custom menus live in config/menus JSON files
        }
    }

    pub fn installed_list(&self) -> Vec<BuiltinSubmenu> {
        BuiltinSubmenu::ALL
            .iter()
            .copied()
            .filter(|&sub| self.is_installed(sub))
            .collect()
    }
}

#[cfg(windows)]
pub mod host {
    use super::{BuiltinSubmenu, InstalledModules};
    use recircles_submenu_abi::{
        FN_ABI_VERSION, FN_FREE_CSTR, FN_INVOKE, FN_SUBMENU_ID, SUBMENU_ABI_VERSION,
        SubmenuRequest, SubmenuResponse,
    };
    use std::collections::HashMap;
    use std::ffi::{CStr, CString, c_char};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    type FnAbiVersion = unsafe extern "C" fn() -> u32;
    type FnSubmenuId = unsafe extern "C" fn() -> *const c_char;
    type FnInvoke = unsafe extern "C" fn(*const c_char) -> *mut c_char;
    type FnFreeCstr = unsafe extern "C" fn(*mut c_char);

    struct LoadedDll {
        handle: HMODULE,
        invoke: FnInvoke,
        free_cstr: FnFreeCstr,
    }

    // Win32 HMODULE + function pointers are process-wide and safe to call across worker threads.
    unsafe impl Send for LoadedDll {}
    unsafe impl Sync for LoadedDll {}

    impl Drop for LoadedDll {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    FreeLibrary(self.handle);
                }
            }
        }
    }

    pub struct SubmenuHost {
        modules: InstalledModules,
        loaded: std::sync::Mutex<HashMap<BuiltinSubmenu, std::sync::Arc<LoadedDll>>>,
    }

    impl SubmenuHost {
        pub fn new(modules: InstalledModules) -> Self {
            Self {
                modules,
                loaded: std::sync::Mutex::new(HashMap::new()),
            }
        }

        pub fn installed(&self) -> &InstalledModules {
            &self.modules
        }

        pub fn refresh_installed(&mut self) {
            self.modules = InstalledModules::detect_in(self.modules.submenus_dir().to_path_buf());
            if let Ok(mut map) = self.loaded.lock() {
                map.retain(|sub, _| self.modules.is_installed(*sub));
            }
        }

        pub fn invoke(
            &self,
            sub: BuiltinSubmenu,
            request: &SubmenuRequest,
        ) -> Result<SubmenuResponse, String> {
            let dll = self.get_or_load(sub)?;
            let req_json = serde_json::to_string(request)
                .map_err(|e| format!("Failed to serialize submenu request: {e}"))?;
            let req_c =
                CString::new(req_json).map_err(|_| "Submenu request contained NUL byte".to_string())?;
            let resp_ptr = unsafe { (dll.invoke)(req_c.as_ptr()) };
            if resp_ptr.is_null() {
                return Err(format!("Submenu '{}' returned null response", sub.id()));
            }
            let resp_str = unsafe { CStr::from_ptr(resp_ptr) }
                .to_str()
                .map(|s| s.to_owned());
            unsafe { (dll.free_cstr)(resp_ptr) };
            let resp_str = resp_str.map_err(|e| format!("Invalid UTF-8 response: {e}"))?;
            serde_json::from_str(&resp_str)
                .map_err(|e| format!("Invalid JSON response from '{}': {e}", sub.id()))
        }

        fn get_or_load(&self, sub: BuiltinSubmenu) -> Result<std::sync::Arc<LoadedDll>, String> {
            if let Ok(guard) = self.loaded.lock()
                && let Some(dll) = guard.get(&sub)
            {
                return Ok(dll.clone());
            }

            let dll_path = sub
                .dll_path(self.modules.submenus_dir())
                .ok_or_else(|| format!("'{}' is built into core, not a DLL", sub.id()))?;
            if !dll_path.is_file() {
                return Err(format!(
                    "Submenu '{}' is not installed on disk (missing {}). Run the ReCircles installer to install it.",
                    sub.title(),
                    dll_path.display()
                ));
            }

            let loaded = std::sync::Arc::new(load_submenu_dll(&dll_path, sub.id())?);
            if let Ok(mut guard) = self.loaded.lock() {
                let entry = guard.entry(sub).or_insert_with(|| loaded.clone());
                return Ok(entry.clone());
            }
            Ok(loaded)
        }
    }

    fn load_submenu_dll(path: &Path, expected_id: &str) -> Result<LoadedDll, String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
        if handle.is_null() {
            return Err(format!("Failed to load submenu DLL '{}'", path.display()));
        }

        let load_err = |msg: String| {
            unsafe {
                FreeLibrary(handle);
            }
            Err(msg)
        };

        let ver_ptr = unsafe { GetProcAddress(handle, FN_ABI_VERSION.as_ptr()) };
        let id_ptr = unsafe { GetProcAddress(handle, FN_SUBMENU_ID.as_ptr()) };
        let invoke_ptr = unsafe { GetProcAddress(handle, FN_INVOKE.as_ptr()) };
        let free_ptr = unsafe { GetProcAddress(handle, FN_FREE_CSTR.as_ptr()) };

        let (Some(ver_fn), Some(id_fn), Some(invoke_fn), Some(free_fn)) =
            (ver_ptr, id_ptr, invoke_ptr, free_ptr)
        else {
            return load_err(format!("DLL '{}' is missing submenu ABI exports", path.display()));
        };

        let ver: FnAbiVersion = unsafe { std::mem::transmute(ver_fn) };
        let id: FnSubmenuId = unsafe { std::mem::transmute(id_fn) };
        let invoke: FnInvoke = unsafe { std::mem::transmute(invoke_fn) };
        let free_cstr: FnFreeCstr = unsafe { std::mem::transmute(free_fn) };

        let abi = unsafe { ver() };
        if abi != SUBMENU_ABI_VERSION {
            return load_err(format!(
                "DLL '{}' ABI version {abi} != expected {SUBMENU_ABI_VERSION}",
                path.display()
            ));
        }

        let got_id_ptr = unsafe { id() };
        let got_id = if got_id_ptr.is_null() {
            ""
        } else {
            unsafe { CStr::from_ptr(got_id_ptr) }
                .to_str()
                .unwrap_or("")
        };
        if got_id != expected_id {
            return load_err(format!(
                "DLL '{}' reported submenu id '{got_id}', expected '{expected_id}'",
                path.display()
            ));
        }

        Ok(LoadedDll {
            handle,
            invoke,
            free_cstr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{BuiltinSubmenu, InstalledModules};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("recircles_mod_test_{tag}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_is_always_installed_and_optional_depend_on_dll_files() {
        let dir = temp_dir("dlls");
        fs::write(dir.join("recircles_files.dll"), b"stub").unwrap();
        fs::write(dir.join("recircles_clipboard.dll"), b"stub").unwrap();

        let mods = InstalledModules::detect_in(dir.clone());
        assert!(mods.is_installed(BuiltinSubmenu::Settings));
        assert!(BuiltinSubmenu::Settings.is_mandatory());
        assert!(mods.is_installed(BuiltinSubmenu::Files));
        assert!(mods.is_installed(BuiltinSubmenu::Clipboard));
        assert!(!mods.is_installed(BuiltinSubmenu::Apps));
        assert!(!mods.is_installed(BuiltinSubmenu::Windows));

        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn loads_and_invokes_real_compiled_submenu_dlls() {
        let ws = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let candidates = [
            ws.join("target").join("dist").join("submenus"),
            ws.join("target").join("release"),
        ];
        let Some(sub_dir) = candidates
            .into_iter()
            .find(|p| p.join("recircles_files.dll").is_file())
        else {
            return;
        };
        let mods = InstalledModules::detect_in(sub_dir.clone());
        let host = super::host::SubmenuHost::new(mods);

        let files_resp = host
            .invoke(
                BuiltinSubmenu::Files,
                &recircles_submenu_abi::SubmenuRequest::FilesCurrentFolder {
                    query: "\\".into(),
                    config_dir: sub_dir.to_string_lossy().into_owned(),
                },
            )
            .expect("files DLL invoke");
        assert!(matches!(
            files_resp,
            recircles_submenu_abi::SubmenuResponse::FilesCurrentFolder { is_root: true, .. }
        ));

        let apps_resp = host
            .invoke(BuiltinSubmenu::Apps, &recircles_submenu_abi::SubmenuRequest::AppsList)
            .expect("apps DLL invoke");
        assert!(matches!(
            apps_resp,
            recircles_submenu_abi::SubmenuResponse::AppsList { .. }
        ));

        let win_resp = host
            .invoke(
                BuiltinSubmenu::Windows,
                &recircles_submenu_abi::SubmenuRequest::WindowsList,
            )
            .expect("windows DLL invoke");
        assert!(matches!(
            win_resp,
            recircles_submenu_abi::SubmenuResponse::WindowsList { .. }
        ));

        let clip_dir = temp_dir("real_clip");
        let clip_resp = host
            .invoke(
                BuiltinSubmenu::Clipboard,
                &recircles_submenu_abi::SubmenuRequest::ClipboardList {
                    data_dir: clip_dir.to_string_lossy().into_owned(),
                },
            )
            .expect("clipboard DLL invoke");
        assert!(matches!(
            clip_resp,
            recircles_submenu_abi::SubmenuResponse::ClipboardItems { .. }
        ));
        let _ = fs::remove_dir_all(clip_dir);
    }
}
