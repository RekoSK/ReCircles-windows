use recircles_submenu_abi::{AppDto, SubmenuRequest, SubmenuResponse, export_submenu_dll};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

export_submenu_dll!("apps", handle_request);

pub fn handle_request(req: SubmenuRequest) -> SubmenuResponse {
    match req {
        SubmenuRequest::AppsList => SubmenuResponse::AppsList {
            apps: list_installed_apps(),
        },
        SubmenuRequest::AppsLaunch { id, launch_target } => match launch_app(&launch_target) {
            Ok(()) => SubmenuResponse::ActionOutcome {
                ok: true,
                message: format!("Launched {id}"),
            },
            Err(e) => SubmenuResponse::ActionOutcome {
                ok: false,
                message: e,
            },
        },
        other => SubmenuResponse::Error {
            message: format!("Unsupported request for apps DLL: {other:?}"),
        },
    }
}

pub fn list_installed_apps() -> Vec<AppDto> {
    let mut roots = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Ok(base) = std::env::var(var)
            && !base.trim().is_empty()
        {
            roots.push(
                PathBuf::from(base)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs"),
            );
        }
    }

    let mut apps = scan_start_menu_roots(&roots);
    append_builtin_windows_tools(&mut apps);
    apps.sort_by_cached_key(|a| a.name.to_ascii_lowercase());
    apps
}

pub fn scan_start_menu_roots(roots: &[PathBuf]) -> Vec<AppDto> {
    let mut apps = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    for root in roots {
        if root.is_dir() {
            collect_shortcuts(root, 0, &mut apps, &mut seen_names);
        }
    }
    apps
}

fn collect_shortcuts(
    dir: &Path,
    depth: usize,
    out: &mut Vec<AppDto>,
    seen_names: &mut HashSet<String>,
) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            collect_shortcuts(&entry.path(), depth + 1, out, seen_names);
            continue;
        }
        if ft.is_symlink() {
            let p = entry.path();
            if p.is_dir() {
                collect_shortcuts(&p, depth + 1, out, seen_names);
                continue;
            }
        } else if !ft.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        let Some((stem, ext)) = name_str.rsplit_once('.') else {
            continue;
        };
        if !(ext.eq_ignore_ascii_case("lnk")
            || ext.eq_ignore_ascii_case("appref-ms")
            || ext.eq_ignore_ascii_case("url"))
        {
            continue;
        }
        let clean = stem.trim();
        if clean.is_empty() {
            continue;
        }
        let lower = clean.to_ascii_lowercase();
        if lower == "uninstall"
            || lower.starts_with("uninstall ")
            || lower.contains("uninstaller")
        {
            continue;
        }
        if !seen_names.insert(lower) {
            continue;
        }
        let target = entry.path().to_string_lossy().into_owned();
        out.push(AppDto {
            id: format!("app:{clean}"),
            name: clean.to_string(),
            launch_target: target.clone(),
            icon_path: target,
        });
    }
}

fn append_builtin_windows_tools(out: &mut Vec<AppDto>) {
    let mut seen: HashSet<String> = out.iter().map(|a| a.name.to_ascii_lowercase()).collect();
    #[cfg(windows)]
    let sys_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    #[cfg(not(windows))]
    let sys_root = String::new();

    if sys_root.is_empty() {
        return;
    }
    let sys32 = PathBuf::from(&sys_root).join("System32");
    for (name, exe) in [
        ("Notepad", sys32.join("notepad.exe")),
        ("Command Prompt", sys32.join("cmd.exe")),
        ("PowerShell", sys32.join("WindowsPowerShell\\v1.0\\powershell.exe")),
        ("Calculator", sys32.join("calc.exe")),
        ("Task Manager", sys32.join("Taskmgr.exe")),
        ("Control Panel", sys32.join("control.exe")),
        ("File Explorer", PathBuf::from(&sys_root).join("explorer.exe")),
    ] {
        if exe.is_file() && seen.insert(name.to_ascii_lowercase()) {
            let s = exe.to_string_lossy().into_owned();
            out.push(AppDto {
                id: format!("app:{name}"),
                name: name.into(),
                launch_target: s.clone(),
                icon_path: s,
            });
        }
    }
}

fn launch_app(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let verb: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let file: Vec<u16> = OsStr::new(target).encode_wide().chain(Some(0)).collect();
        let rc = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if rc <= 32 {
            return Err(format!("Failed launching '{target}' (ShellExecute rc={rc})"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("App launching only runs on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn scans_start_menu_shortcuts_dedups_and_filters_uninstallers() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("recircles_apps_{nanos}"));
        let sub = dir.join("Tools");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("Firefox.lnk"), b"lnk").unwrap();
        fs::write(sub.join("Firefox.lnk"), b"dup").unwrap();
        fs::write(sub.join("Uninstall Firefox.lnk"), b"uninst").unwrap();
        fs::write(sub.join("readme.txt"), b"txt").unwrap();

        let apps = scan_start_menu_roots(std::slice::from_ref(&dir));
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Firefox");

        let _ = fs::remove_dir_all(dir);
    }
}
