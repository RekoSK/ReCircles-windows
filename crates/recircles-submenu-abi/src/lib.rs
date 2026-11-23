use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString, c_char};

pub const SUBMENU_ABI_VERSION: u32 = 1;

pub const FN_ABI_VERSION: &[u8] = b"recircles_submenu_abi_version\0";
pub const FN_SUBMENU_ID: &[u8] = b"recircles_submenu_id\0";
pub const FN_INVOKE: &[u8] = b"recircles_submenu_invoke\0";
pub const FN_FREE_CSTR: &[u8] = b"recircles_submenu_free_cstr\0";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SubmenuRequest {
    // Files
    FilesCurrentFolder {
        query: String,
        config_dir: String,
    },
    FilesFrequentFolders {
        usage_keys: Vec<String>,
        config_dir: String,
    },
    FilesOpen {
        path: String,
    },
    FilesOpenFolder {
        path: String,
    },

    // Apps
    AppsList,
    AppsLaunch {
        id: String,
        launch_target: String,
    },

    // Windows
    WindowsList,
    WindowsFocus {
        hwnd: u64,
    },

    // Clipboard
    ClipboardList {
        data_dir: String,
    },
    ClipboardCaptureNow {
        data_dir: String,
        max_items: usize,
        ignore_internal_marker: bool,
    },
    ClipboardCopyToClipboard {
        data_dir: String,
        id: String,
    },
    ClipboardDelete {
        data_dir: String,
        id: String,
    },
    ClipboardClear {
        data_dir: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntryDto {
    pub path: String,
    pub label: String,
    pub is_dir: bool,
    pub is_home: bool,
    pub is_pin: bool,
    pub completion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDto {
    pub id: String,
    pub name: String,
    pub launch_target: String,
    pub icon_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowDto {
    pub hwnd: u64,
    pub title: String,
    pub app_id: String,
    pub exe_path: String,
    pub minimized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    Text,
    Image,
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItemDto {
    pub id: String,
    pub kind: ClipboardKind,
    pub label: String,
    pub detail: String,
    pub text: Option<String>,
    pub files: Vec<String>,
    pub image_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub hash: String,
    pub timestamp_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubmenuResponse {
    ActionOutcome {
        ok: bool,
        message: String,
    },
    FilesCurrentFolder {
        folder_label: String,
        is_root: bool,
        filter: String,
        parent_query: String,
        entries: Vec<FileEntryDto>,
    },
    FilesList {
        entries: Vec<FileEntryDto>,
    },
    AppsList {
        apps: Vec<AppDto>,
    },
    WindowsList {
        windows: Vec<WindowDto>,
    },
    ClipboardItems {
        items: Vec<ClipboardItemDto>,
    },
    ClipboardCapture {
        captured: bool,
        changed: bool,
        total_items: usize,
    },
    Error {
        message: String,
    },
}

/// Convert a C request string, invoke handler, and return an allocated C string.
///
/// # Safety
/// `request_json` must be a valid NUL-terminated UTF-8 C string.
pub unsafe fn dispatch_c_invoke<F>(request_json: *const c_char, handler: F) -> *mut c_char
where
    F: FnOnce(SubmenuRequest) -> SubmenuResponse,
{
    let resp = if request_json.is_null() {
        SubmenuResponse::Error {
            message: "Null request".into(),
        }
    } else {
        match unsafe { CStr::from_ptr(request_json) }.to_str() {
            Ok(json_str) => match serde_json::from_str::<SubmenuRequest>(json_str) {
                Ok(req) => handler(req),
                Err(e) => SubmenuResponse::Error {
                    message: format!("Invalid submenu request JSON: {e}"),
                },
            },
            Err(e) => SubmenuResponse::Error {
                message: format!("Invalid UTF-8 in submenu request: {e}"),
            },
        }
    };

    let serialized = serde_json::to_string(&resp).unwrap_or_else(|e| {
        format!(
            r#"{{"kind":"error","message":"Serialization failed: {}"}}"#,
            e.to_string().replace('"', "'")
        )
    });
    match CString::new(serialized) {
        Ok(c) => c.into_raw(),
        Err(_) => CString::new(r#"{"kind":"error","message":"NUL byte in response"}"#)
            .expect("static valid JSON")
            .into_raw(),
    }
}

/// Free a C string returned by `dispatch_c_invoke`.
///
/// # Safety
/// `ptr` must have been allocated by `dispatch_c_invoke` inside the same DLL.
pub unsafe fn free_cstr(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

#[macro_export]
macro_rules! export_submenu_dll {
    ($id_lit:literal, $handler:path) => {
        static SUBMENU_ID_CSTR: &[u8] = concat!($id_lit, "\0").as_bytes();

        #[unsafe(no_mangle)]
        pub extern "C" fn recircles_submenu_abi_version() -> u32 {
            $crate::SUBMENU_ABI_VERSION
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn recircles_submenu_id() -> *const std::ffi::c_char {
            SUBMENU_ID_CSTR.as_ptr() as *const std::ffi::c_char
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn recircles_submenu_invoke(
            request_json: *const std::ffi::c_char,
        ) -> *mut std::ffi::c_char {
            unsafe { $crate::dispatch_c_invoke(request_json, $handler) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn recircles_submenu_free_cstr(ptr: *mut std::ffi::c_char) {
            unsafe {
                $crate::free_cstr(ptr);
            }
        }
    };
}
