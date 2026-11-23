#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ShowMain,
    RunMenu(String),
    Add(String),
    Remove(String),
    Config { force: bool },
    ResetHistory,
    Update { check_only: bool },
    Help,
    Version,
}

pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some(arg) = args.first().map(String::as_str) else {
        return Ok(Command::ShowMain);
    };
    match arg {
        "-h" | "--help" => Ok(Command::Help),
        "-V" | "--version" => Ok(Command::Version),
        "--run" | "-r" => {
            let target = args
                .get(1)
                .ok_or_else(|| "Missing menu name after --run".to_string())?;
            Ok(Command::RunMenu(target.clone()))
        }
        "add" => {
            let path = args
                .get(1)
                .ok_or_else(|| "Usage: recircles add <path-to-json-or-zip>".to_string())?;
            Ok(Command::Add(path.clone()))
        }
        "remove" => {
            let id = args
                .get(1)
                .ok_or_else(|| "Usage: recircles remove <menu-id>".to_string())?;
            Ok(Command::Remove(id.clone()))
        }
        "config" => {
            let mut force = false;
            for rest in &args[1..] {
                if rest == "--force" || rest == "-f" {
                    force = true;
                } else {
                    return Err(format!("Unknown flag for config: '{rest}'"));
                }
            }
            Ok(Command::Config { force })
        }
        "reset" => Ok(Command::ResetHistory),
        "update" => {
            let mut check_only = false;
            for rest in &args[1..] {
                if rest == "--check" || rest == "-c" {
                    check_only = true;
                } else {
                    return Err(format!("Unknown flag for update: '{rest}'"));
                }
            }
            Ok(Command::Update { check_only })
        }
        other => Err(format!("Unknown argument '{other}'. Try 'recircles --help'.")),
    }
}

pub fn help_text() -> &'static str {
    concat!(
        "ReCircles Windows v",
        env!("CARGO_PKG_VERSION"),
        "\n\n",
        "USAGE:\n",
        "  recircles.exe                     Open the main radial menu\n",
        "  recircles.exe --run <menu>        Open a submenu or custom menu directly\n",
        "  recircles.exe add <file>          Import a .json or .zip theme/menu bundle\n",
        "  recircles.exe remove <menu>       Remove a menu from config\n",
        "  recircles.exe config [--force]    Generate default config file\n",
        "  recircles.exe reset               Reset saved most-used apps & folders memory\n",
        "  recircles.exe update [--check]    Check for or install updates from GitHub\n",
        "  recircles.exe -h, --help          Print this help message\n",
        "  recircles.exe -V, --version       Print version\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_cli_commands() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
        assert_eq!(parse_args(&s(&[])).unwrap(), Command::ShowMain);
        assert_eq!(parse_args(&s(&["--run", "clipboard"])).unwrap(), Command::RunMenu("clipboard".into()));
        assert_eq!(parse_args(&s(&["add", "theme.json"])).unwrap(), Command::Add("theme.json".into()));
        assert_eq!(parse_args(&s(&["remove", "power"])).unwrap(), Command::Remove("power".into()));
        assert_eq!(parse_args(&s(&["config", "-f"])).unwrap(), Command::Config { force: true });
        assert_eq!(parse_args(&s(&["config"])).unwrap(), Command::Config { force: false });
        assert_eq!(parse_args(&s(&["reset"])).unwrap(), Command::ResetHistory);
        assert_eq!(parse_args(&s(&["update"])).unwrap(), Command::Update { check_only: false });
        assert_eq!(parse_args(&s(&["update", "--check"])).unwrap(), Command::Update { check_only: true });
        assert_eq!(parse_args(&s(&["--help"])).unwrap(), Command::Help);
        assert!(parse_args(&s(&["--run"])).is_err());
        assert!(parse_args(&s(&["--bogus"])).is_err());
    }
}
