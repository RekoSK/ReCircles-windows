use std::env;
use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    if let Ok(override_dir) = env::var("RECIRCLES_CONFIG_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    #[cfg(windows)]
    {
        if let Ok(local) = env::var("LOCALAPPDATA")
            && !local.trim().is_empty()
        {
            return PathBuf::from(local).join("recircles");
        }
        if let Ok(roaming) = env::var("APPDATA")
            && !roaming.trim().is_empty()
        {
            return PathBuf::from(roaming).join("recircles");
        }
        if let Ok(home) = env::var("USERPROFILE")
            && !home.trim().is_empty()
        {
            return PathBuf::from(home).join(".config").join("recircles");
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg).join("recircles");
        }
        if let Ok(home) = env::var("HOME")
            && !home.trim().is_empty()
        {
            return PathBuf::from(home).join(".config").join("recircles");
        }
    }
    PathBuf::from("recircles-config")
}

pub fn home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(home) = env::var("USERPROFILE")
            && !home.trim().is_empty()
        {
            return PathBuf::from(home);
        }
        if let (Ok(drive), Ok(path)) = (env::var("HOMEDRIVE"), env::var("HOMEPATH")) {
            return PathBuf::from(format!("{drive}{path}"));
        }
    }
    if let Ok(home) = env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home);
    }
    PathBuf::from(".")
}

pub fn exe_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn submenus_dir() -> PathBuf {
    if let Ok(override_dir) = env::var("RECIRCLES_SUBMENUS_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let dir = exe_dir();
    let nested = dir.join("submenus");
    if nested.is_dir() {
        return nested;
    }
    // When running directly from a Cargo build directory (e.g. target/release),
    // submenu DLLs sit alongside recircles.exe.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && name.starts_with("recircles_")
                && name.ends_with(".dll")
            {
                return dir;
            }
        }
    }
    nested
}

pub fn config_file_in(dir: &std::path::Path) -> PathBuf {
    let json = dir.join("recircles.json");
    if json.exists() {
        return json;
    }
    let legacy = dir.join("recircles.config");
    if legacy.exists() {
        return legacy;
    }
    json
}

pub fn themes_dir_in(dir: &std::path::Path) -> PathBuf {
    dir.join("themes")
}

pub fn menus_dir_in(dir: &std::path::Path) -> PathBuf {
    dir.join("menus")
}

pub fn pins_file_in(dir: &std::path::Path) -> PathBuf {
    dir.join("pins")
}

pub fn usage_file_in(dir: &std::path::Path) -> PathBuf {
    dir.join("usage")
}

pub fn install_manifest_in(dir: &std::path::Path) -> PathBuf {
    dir.join("installed.json")
}
