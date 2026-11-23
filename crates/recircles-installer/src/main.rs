#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod d2d;
mod engine;
mod gui;
#[cfg_attr(not(windows), allow(dead_code))]
mod icons;
mod spring;
#[cfg_attr(not(windows), allow(dead_code))]
mod vsync;

use engine::{InstallOptions, NavPreset, execute_install, execute_uninstall};
use recircles_core::modules::BuiltinSubmenu;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Gui,
    Help,
    SilentInstall(Box<SilentArgs>),
    Uninstall {
        install_dir: PathBuf,
        config_dir: PathBuf,
        purge: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SilentArgs {
    install_dir: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    theme: Option<String>,
    submenus: Option<Vec<String>>,
    main_hotkey: Option<String>,
    submenu_hotkeys: Vec<(String, String)>,
    nav_preset: NavPreset,
    no_launch: bool,
    no_integration: bool,
}

fn parse_cli(args: &[String]) -> Result<Mode, String> {
    if args.is_empty() {
        return Ok(Mode::Gui);
    }
    let mut silent = false;
    let mut uninstall = false;
    let mut purge = false;
    let mut s = SilentArgs {
        install_dir: None,
        config_dir: None,
        theme: None,
        submenus: None,
        main_hotkey: None,
        submenu_hotkeys: Vec::new(),
        nav_preset: NavPreset::Standard,
        no_launch: false,
        no_integration: false,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return Ok(Mode::Help),
            "--silent" => silent = true,
            "--uninstall" => uninstall = true,
            "--purge-config" => purge = true,
            "--no-start-launch" => s.no_launch = true,
            "--no-integration" => s.no_integration = true,
            "--install-dir" => {
                i += 1;
                s.install_dir = Some(PathBuf::from(args.get(i).ok_or("Missing --install-dir path")?));
            }
            "--config-dir" => {
                i += 1;
                s.config_dir = Some(PathBuf::from(args.get(i).ok_or("Missing --config-dir path")?));
            }
            "--theme" => {
                i += 1;
                s.theme = Some(args.get(i).ok_or("Missing --theme value")?.clone());
            }
            "--submenus" => {
                i += 1;
                let list = args
                    .get(i)
                    .ok_or("Missing comma list after --submenus")?
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
                s.submenus = Some(list);
            }
            "--main-hotkey" => {
                i += 1;
                s.main_hotkey = Some(args.get(i).ok_or("Missing --main-hotkey value")?.clone());
            }
            "--hotkey" => {
                i += 1;
                let pair = args.get(i).ok_or("Expected <submenu>=<hotkey> after --hotkey")?;
                let (k, v) = pair
                    .split_once('=')
                    .ok_or("Expected <submenu>=<hotkey> after --hotkey")?;
                s.submenu_hotkeys.push((k.trim().to_string(), v.trim().to_string()));
            }
            "--nav-preset" => {
                i += 1;
                let p = args.get(i).ok_or("Missing --nav-preset value")?.to_ascii_lowercase();
                s.nav_preset = match p.as_str() {
                    "standard" | "default" => NavPreset::Standard,
                    "vim" => NavPreset::Vim,
                    _ => return Err(format!("Unknown nav preset '{p}'")),
                };
            }
            other => return Err(format!("Unknown argument '{other}'")),
        }
        i += 1;
    }

    if uninstall {
        return Ok(Mode::Uninstall {
            install_dir: s.install_dir.unwrap_or_else(engine::default_install_dir),
            config_dir: s.config_dir.unwrap_or_else(recircles_core::paths::config_dir),
            purge,
        });
    }
    if silent {
        return Ok(Mode::SilentInstall(Box::new(s)));
    }
    Ok(Mode::Gui)
}

#[cfg(windows)]
fn attach_console_if_needed() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    let h = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if h.is_null() || h == INVALID_HANDLE_VALUE {
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_console_if_needed() {}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match parse_cli(&args) {
        Ok(m) => m,
        Err(e) => {
            attach_console_if_needed();
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    match mode {
        Mode::Help => {
            attach_console_if_needed();
            println!(
                "ReCircles Windows Installer v{}\n\n\
                 USAGE:\n\
                   recircles-setup.exe                  Launch interactive setup wizard\n\
                   recircles-setup.exe --silent [opts]  Scripted installation/modification\n\
                   recircles-setup.exe --uninstall      Uninstall binaries & DLLs\n\n\
                 SILENT OPTIONS:\n\
                   --install-dir <path>\n\
                   --config-dir <path>\n\
                   --theme <id>\n\
                   --submenus <comma-list>   e.g. files,apps,clipboard (settings always included)\n\
                   --main-hotkey <spec>      e.g. Ctrl+Alt+Space\n\
                   --hotkey <id>=<spec>      e.g. clipboard=Ctrl+Alt+v (repeatable)\n\
                   --nav-preset standard|vim\n\
                   --no-start-launch\n\
                   --no-integration\n",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
        Mode::Uninstall {
            install_dir,
            config_dir,
            purge,
        } => {
            attach_console_if_needed();
            match execute_uninstall(&install_dir, &config_dir, purge) {
                Ok(()) => {
                    println!("ReCircles uninstalled from {}.", install_dir.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Mode::SilentInstall(s) => {
            attach_console_if_needed();
            let mut opts = InstallOptions::default();
            if let Some(d) = s.install_dir {
                opts.install_dir = d;
            }
            if let Some(c) = s.config_dir {
                opts.config_dir = c;
            }
            if let Some(t) = s.theme {
                opts.theme = t;
            }
            if let Some(list) = s.submenus {
                let mut set = BTreeSet::new();
                for id in list {
                    if let Some(sub) = BuiltinSubmenu::from_id(&id) {
                        set.insert(sub);
                    } else {
                        eprintln!("error: unknown submenu '{id}'");
                        return ExitCode::from(2);
                    }
                }
                // Always force mandatory Settings
                set.insert(BuiltinSubmenu::Settings);
                opts.selected_submenus = set;
            }
            if let Some(hk) = s.main_hotkey {
                opts.custom_keybinds = true;
                opts.main_hotkey = hk;
            }
            if !s.submenu_hotkeys.is_empty() || s.nav_preset != NavPreset::Standard {
                opts.custom_keybinds = true;
            }
            for (k, v) in s.submenu_hotkeys {
                opts.submenu_hotkeys.insert(k, v);
            }
            opts.nav_preset = s.nav_preset;
            opts.launch_daemon = !s.no_launch;
            opts.register_windows_integration = !s.no_integration;

            match execute_install(&opts, None) {
                Ok(sum) => {
                    println!(
                        "Installed ReCircles to {}\nConfig: {}\nInstalled DLLs: {:?}\nOmitted DLLs: {:?}\nHotkeys: {:?}",
                        sum.install_dir.display(),
                        sum.config_path.display(),
                        sum.installed_dlls,
                        sum.omitted_dlls,
                        sum.active_hotkeys
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Mode::Gui => {
            #[cfg(windows)]
            {
                let rc = gui::win_gui::run_wizard();
                if rc == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            #[cfg(not(windows))]
            {
                eprintln!("GUI installer runs only on Windows. Use --silent or run cargo test.");
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_silent_installer_flags() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
        let m = parse_cli(&s(&[
            "--silent",
            "--theme",
            "forest",
            "--submenus",
            "files,clipboard",
            "--main-hotkey",
            "Ctrl+Alt+Space",
            "--hotkey",
            "clipboard=Ctrl+Alt+v",
            "--nav-preset",
            "vim",
            "--no-start-launch",
        ]))
        .unwrap();
        let Mode::SilentInstall(args) = m else {
            panic!("expected SilentInstall");
        };
        assert_eq!(args.theme.as_deref(), Some("forest"));
        assert_eq!(args.submenus.as_ref().unwrap(), &vec!["files", "clipboard"]);
        assert_eq!(args.main_hotkey.as_deref(), Some("Ctrl+Alt+Space"));
        assert_eq!(args.nav_preset, NavPreset::Vim);
        assert!(args.no_launch);
    }
}
