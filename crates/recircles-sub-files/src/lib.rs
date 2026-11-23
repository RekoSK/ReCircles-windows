use recircles_submenu_abi::{FileEntryDto, SubmenuRequest, SubmenuResponse, export_submenu_dll};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

export_submenu_dll!("files", handle_request);

pub fn handle_request(req: SubmenuRequest) -> SubmenuResponse {
    match req {
        SubmenuRequest::FilesCurrentFolder { query, .. } => {
            let resolved = current_folder(&query);
            SubmenuResponse::FilesCurrentFolder {
                folder_label: resolved.folder_label,
                is_root: resolved.is_root,
                filter: resolved.filter,
                parent_query: resolved.parent_query,
                entries: resolved.entries,
            }
        }
        SubmenuRequest::FilesFrequentFolders {
            usage_keys,
            config_dir,
        } => {
            let entries = frequent_folders(&usage_keys, Path::new(&config_dir));
            SubmenuResponse::FilesList { entries }
        }
        SubmenuRequest::FilesOpen { path } => match shell_open(&path, None) {
            Ok(()) => SubmenuResponse::ActionOutcome {
                ok: true,
                message: format!("Opened {path}"),
            },
            Err(e) => SubmenuResponse::ActionOutcome {
                ok: false,
                message: e,
            },
        },
        SubmenuRequest::FilesOpenFolder { path } => match open_in_explorer(&path) {
            Ok(()) => SubmenuResponse::ActionOutcome {
                ok: true,
                message: format!("Opened folder for {path}"),
            },
            Err(e) => SubmenuResponse::ActionOutcome {
                ok: false,
                message: e,
            },
        },
        other => SubmenuResponse::Error {
            message: format!("Unsupported request for files DLL: {other:?}"),
        },
    }
}

pub struct FolderResolution {
    pub folder_label: String,
    pub is_root: bool,
    pub filter: String,
    pub parent_query: String,
    pub entries: Vec<FileEntryDto>,
}

pub fn user_home() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("USERPROFILE")
            && !p.trim().is_empty()
        {
            return PathBuf::from(p);
        }
    }
    if let Ok(p) = std::env::var("HOME")
        && !p.trim().is_empty()
    {
        return PathBuf::from(p);
    }
    PathBuf::from("C:\\")
}

pub fn list_drives() -> Vec<FileEntryDto> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::GetLogicalDriveStringsW;
        let mut buf = [0u16; 512];
        let len = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
        if len > 0 && len <= buf.len() {
            let mut out = Vec::new();
            for slice in buf[..len].split(|&c| c == 0) {
                if slice.is_empty() {
                    continue;
                }
                let s = String::from_utf16_lossy(slice);
                out.push(FileEntryDto {
                    path: s.clone(),
                    label: s.trim_end_matches('\\').to_string(),
                    is_dir: true,
                    is_home: false,
                    is_pin: false,
                    completion: s,
                });
            }
            if !out.is_empty() {
                return out;
            }
        }
    }
    vec![FileEntryDto {
        path: "C:\\".into(),
        label: "C:".into(),
        is_dir: true,
        is_home: false,
        is_pin: false,
        completion: "C:\\".into(),
    }]
}

pub fn normalize_separators(q: &str) -> String {
    if cfg!(windows) {
        q.replace('/', "\\")
    } else {
        q.replace('\\', "/")
    }
}

pub fn is_path_query(query: &str) -> bool {
    let q = query.trim();
    q.starts_with('~')
        || q.starts_with('/')
        || q.starts_with('\\')
        || (q.len() >= 2 && q.as_bytes()[1] == b':')
}

pub fn expand_query_path(query: &str) -> String {
    let q = query.trim();
    if q == "~" || q.starts_with("~/") || q.starts_with("~\\") {
        let home = user_home().to_string_lossy().into_owned();
        let rest = q.strip_prefix('~').unwrap_or("");
        let rest_norm = normalize_separators(rest);
        if rest_norm.is_empty() {
            return format!("{}{}", home.trim_end_matches(['\\', '/']), std::path::MAIN_SEPARATOR);
        }
        return format!("{}{}", home.trim_end_matches(['\\', '/']), rest_norm);
    }
    normalize_separators(q)
}

pub fn parent_query(query: &str) -> String {
    let norm = expand_query_path(query);
    let trimmed = norm.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() || trimmed == "\\" || trimmed == "/" {
        return "\\".into();
    }
    // Windows drive root like "C:"
    if trimmed.len() == 2 && trimmed.as_bytes()[1] == b':' {
        return "\\".into();
    }
    let p = Path::new(trimmed);
    match p.parent() {
        Some(par) if !par.as_os_str().is_empty() => {
            let mut s = par.to_string_lossy().into_owned();
            if !s.ends_with(['\\', '/']) {
                s.push(std::path::MAIN_SEPARATOR);
            }
            s
        }
        _ => "\\".into(),
    }
}

pub fn current_folder(query: &str) -> FolderResolution {
    let expanded = expand_query_path(query);
    if expanded == "\\" || expanded == "/" {
        return FolderResolution {
            folder_label: "Drives".into(),
            is_root: true,
            filter: String::new(),
            parent_query: "\\".into(),
            entries: list_drives(),
        };
    }

    let path = Path::new(&expanded);
    let ends_with_sep = expanded.ends_with('\\') || expanded.ends_with('/');
    let (dir, filter) = if ends_with_sep || path.is_dir() {
        (path.to_path_buf(), String::new())
    } else {
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let filt = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        (dir, filt)
    };

    let show_hidden = filter.starts_with('.');
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let home = user_home();

    if let Ok(read) = fs::read_dir(&dir) {
        for entry in read.flatten() {
            let ep = entry.path();
            let Some(name) = ep.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !show_hidden && is_hidden_entry(&ep, name) {
                continue;
            }
            if !filter.is_empty()
                && !name
                    .to_ascii_lowercase()
                    .contains(&filter.to_ascii_lowercase())
            {
                continue;
            }
            let is_dir = ep.is_dir();
            let mut path_str = ep.to_string_lossy().into_owned();
            let completion = if is_dir {
                if !path_str.ends_with(['\\', '/']) {
                    path_str.push(std::path::MAIN_SEPARATOR);
                }
                path_str.clone()
            } else {
                path_str.clone()
            };
            let dto = FileEntryDto {
                path: path_str,
                label: name.to_string(),
                is_dir,
                is_home: ep == home,
                is_pin: false,
                completion,
            };
            if is_dir {
                dirs.push(dto);
            } else {
                files.push(dto);
            }
        }
    }

    dirs.sort_by_cached_key(|a| a.label.to_ascii_lowercase());
    files.sort_by_cached_key(|a| a.label.to_ascii_lowercase());
    dirs.extend(files);

    let folder_label = dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| dir.to_string_lossy().into_owned());

    let dir_str = {
        let mut s = dir.to_string_lossy().into_owned();
        if !s.ends_with(['\\', '/']) {
            s.push(std::path::MAIN_SEPARATOR);
        }
        s
    };

    FolderResolution {
        folder_label,
        is_root: false,
        filter,
        parent_query: parent_query(&dir_str),
        entries: dirs,
    }
}

pub fn frequent_folders(usage_keys: &[String], _config_dir: &Path) -> Vec<FileEntryDto> {
    let home = user_home();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();

    let push_dir = |out: &mut Vec<FileEntryDto>,
                    seen: &mut HashSet<String>,
                    p: PathBuf,
                    label_opt: Option<&str>,
                    is_home: bool| {
        if out.len() >= 8 {
            return;
        }
        if !p.is_dir() {
            return;
        }
        let mut path_str = normalize_separators(&p.to_string_lossy());
        if !path_str.ends_with(['\\', '/']) {
            path_str.push(std::path::MAIN_SEPARATOR);
        }
        let norm = if cfg!(windows) {
            path_str.to_ascii_lowercase()
        } else {
            path_str.clone()
        };
        if !seen.insert(norm) {
            return;
        }
        let label = label_opt
            .map(ToOwned::to_owned)
            .or_else(|| p.file_name().and_then(|n| n.to_str()).map(ToOwned::to_owned))
            .unwrap_or_else(|| path_str.trim_end_matches(['\\', '/']).to_string());
        out.push(FileEntryDto {
            completion: path_str.clone(),
            path: path_str,
            label,
            is_dir: true,
            is_home,
            is_pin: false,
        });
    };

    push_dir(&mut out, &mut seen, home, Some("Home"), true);

    // Last 7 opened folders (most recently opened first)
    for key in usage_keys {
        if out.len() >= 8 {
            break;
        }
        push_dir(&mut out, &mut seen, PathBuf::from(key), None, false);
    }

    out
}

fn is_hidden_entry(path: &Path, name: &str) -> bool {
    if name.starts_with('.') || name.eq_ignore_ascii_case("desktop.ini") || name.eq_ignore_ascii_case("thumbs.db") {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        if let Ok(meta) = fs::metadata(path) {
            let attrs = meta.file_attributes();
            if (attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)) != 0 {
                return true;
            }
        }
    }
    let _ = path;
    false
}

fn shell_open(path: &str, params: Option<&str>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let verb: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let file: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
        let par_w: Option<Vec<u16>> = params.map(|p| OsStr::new(p).encode_wide().chain(Some(0)).collect());
        let rc = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                par_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if rc <= 32 {
            return Err(format!("ShellExecute failed ({rc}) opening '{path}'"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (path, params);
        Err("ShellExecute only runs on Windows".into())
    }
}

fn open_in_explorer(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let p = Path::new(path);
        if p.is_dir() {
            shell_open("explorer.exe", Some(&format!("\"{path}\"")))
        } else {
            shell_open("explorer.exe", Some(&format!("/select,\"{path}\"")))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Explorer only runs on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("recircles_files_{tag}_{nanos}"));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn current_folder_resolves_entries_filters_hidden_and_computes_parent() {
        let root = temp_dir("nav");
        let sub = root.join("Projects");
        fs::create_dir_all(&sub).unwrap();
        fs::write(root.join("alpha.txt"), b"hi").unwrap();
        fs::write(root.join(".hidden"), b"no").unwrap();

        let q = format!("{}{}", root.display(), std::path::MAIN_SEPARATOR);
        let res = current_folder(&q);
        assert!(res.entries.iter().any(|e| e.label == "Projects" && e.is_dir));
        assert!(res.entries.iter().any(|e| e.label == "alpha.txt" && !e.is_dir));
        assert!(!res.entries.iter().any(|e| e.label == ".hidden"));

        let partial = format!("{}{}pro", root.display(), std::path::MAIN_SEPARATOR);
        let res_part = current_folder(&partial);
        assert_eq!(res_part.filter, "pro");
        assert_eq!(res_part.entries.len(), 1);
        assert_eq!(res_part.entries[0].label, "Projects");

        let sub_q = format!("{}{}", sub.display(), std::path::MAIN_SEPARATOR);
        let parent = parent_query(&sub_q);
        assert!(parent.contains(&root.file_name().unwrap().to_string_lossy().to_string()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn frequent_folders_returns_home_plus_last_seven_opened_folders() {
        let cfg = temp_dir("freq");
        let empty = frequent_folders(&[], &cfg);
        assert_eq!(empty.len(), 1);
        assert!(empty[0].is_home);
        assert_eq!(empty[0].label, "Home");

        let mut dirs = Vec::new();
        for i in 0..9 {
            let d = cfg.join(format!("Folder{i}"));
            fs::create_dir_all(&d).unwrap();
            dirs.push(d.to_string_lossy().into_owned());
        }
        let file = cfg.join("note.txt");
        fs::write(&file, b"hi").unwrap();

        let home = user_home().to_string_lossy().into_owned();
        let mut usage = vec![
            home.clone(),
            format!("{}{}", home, std::path::MAIN_SEPARATOR),
            file.to_string_lossy().into_owned(),
            cfg.join("MissingFolder").to_string_lossy().into_owned(),
        ];
        usage.extend(dirs.iter().cloned());

        let entries = frequent_folders(&usage, &cfg);
        assert_eq!(entries.len(), 8);
        assert!(entries[0].is_home);
        assert_eq!(entries[0].label, "Home");
        let labels: Vec<&str> = entries[1..].iter().map(|e| e.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Folder0", "Folder1", "Folder2", "Folder3", "Folder4", "Folder5", "Folder6"
            ]
        );

        let _ = fs::remove_dir_all(cfg);
    }
}
