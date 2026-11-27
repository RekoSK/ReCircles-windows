#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Modifiers {
    pub const fn any(self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }

    pub const fn to_bits(self) -> u8 {
        (self.ctrl as u8)
            | ((self.alt as u8) << 1)
            | ((self.shift as u8) << 2)
            | ((self.win as u8) << 3)
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self {
            ctrl: (bits & 1) != 0,
            alt: (bits & 2) != 0,
            shift: (bits & 4) != 0,
            win: (bits & 8) != 0,
        }
    }

    pub const fn to_register_hotkey_flags(self) -> u32 {
        let mut flags = 0x4000u32; // MOD_NOREPEAT
        if self.alt {
            flags |= 0x0001;
        }
        if self.ctrl {
            flags |= 0x0002;
        }
        if self.shift {
            flags |= 0x0004;
        }
        if self.win {
            flags |= 0x0008;
        }
        flags
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedKey {
    pub mods: Modifiers,
    pub vk: u32,
    pub shifted_char: Option<char>,
}

impl ParsedKey {
    /// True when this binding is a standalone Windows key tap (`"Win"`).
    pub const fn is_lone_win(self) -> bool {
        (self.vk == VK_LWIN || self.vk == VK_RWIN) && !self.mods.any()
    }

    /// True when this binding uses the Windows key either as a modifier (`Win+...`) or standalone (`Win`).
    pub const fn uses_win_key(self) -> bool {
        self.mods.win || self.is_lone_win()
    }
}

pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_PRIOR: u32 = 0x21;
pub const VK_NEXT: u32 = 0x22;
pub const VK_END: u32 = 0x23;
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;
pub const VK_INSERT: u32 = 0x2D;
pub const VK_DELETE: u32 = 0x2E;
pub const VK_LWIN: u32 = 0x5B;
pub const VK_RWIN: u32 = 0x5C;
pub const VK_LSHIFT: u32 = 0xA0;
pub const VK_RSHIFT: u32 = 0xA1;
pub const VK_LCONTROL: u32 = 0xA2;
pub const VK_RCONTROL: u32 = 0xA3;
pub const VK_LMENU: u32 = 0xA4;
pub const VK_RMENU: u32 = 0xA5;
pub const VK_F1: u32 = 0x70;
pub const VK_F24: u32 = 0x87;
pub const VK_OEM_1: u32 = 0xBA; // ;:
pub const VK_OEM_PLUS: u32 = 0xBB; // =+
pub const VK_OEM_COMMA: u32 = 0xBC; // ,<
pub const VK_OEM_MINUS: u32 = 0xBD; // -_
pub const VK_OEM_PERIOD: u32 = 0xBE; // .>
pub const VK_OEM_2: u32 = 0xBF; // /?
pub const VK_OEM_3: u32 = 0xC0; // `~
pub const VK_OEM_4: u32 = 0xDB; // [{
pub const VK_OEM_5: u32 = 0xDC; // \|
pub const VK_OEM_6: u32 = 0xDD; // ]}
pub const VK_OEM_7: u32 = 0xDE; // '"

pub fn is_modifier_vk(vk: u32) -> bool {
    matches!(
        vk,
        VK_SHIFT
            | VK_CONTROL
            | VK_MENU
            | VK_LWIN
            | VK_RWIN
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
    )
}

pub fn parse_key_spec(spec: &str) -> Result<ParsedKey, String> {
    let raw = spec.trim();
    if raw.is_empty() {
        return Err("Empty keybind specification".into());
    }

    let mut mods = Modifiers::default();
    let tokens: Vec<&str> = raw.split('+').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        // Special case: literal `+` at the end like `Ctrl++`
        if raw == "+" {
            return Ok(ParsedKey {
                mods,
                vk: VK_OEM_PLUS,
                shifted_char: Some('+'),
            });
        }
        return Err(format!("Malformed keybind '{spec}'"));
    }

    let (mod_tokens, key_token) = tokens.split_at(tokens.len() - 1);
    let key_token = key_token[0];

    for m in mod_tokens {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "mod1" => mods.alt = true,
            "shift" => mods.shift = true,
            "win" | "super" | "meta" | "mod4" => mods.win = true,
            other => return Err(format!("Unknown key modifier '{other}' in '{spec}'")),
        }
    }

    let (vk, extra_shift, shifted_char) = parse_key_name(key_token)?;
    if extra_shift {
        mods.shift = true;
    }

    Ok(ParsedKey {
        mods,
        vk,
        shifted_char,
    })
}

/// Returns a user-facing reason if Windows kernel / Winlogon hard-locks this shortcut
/// before user-mode low-level hooks can intercept it.
pub const fn os_locked_hotkey_reason(parsed: ParsedKey) -> Option<&'static str> {
    if parsed.mods.win
        && !parsed.mods.ctrl
        && !parsed.mods.alt
        && !parsed.mods.shift
        && parsed.vk == (b'L' as u32)
    {
        return Some("Win+L is locked by Windows (Lock Screen) and cannot be overridden");
    }
    if parsed.mods.ctrl
        && parsed.mods.alt
        && !parsed.mods.shift
        && !parsed.mods.win
        && parsed.vk == VK_DELETE
    {
        return Some("Ctrl+Alt+Delete is reserved by Windows Security (SAS) and cannot be overridden");
    }
    if parsed.mods.win
        && parsed.mods.ctrl
        && parsed.mods.shift
        && !parsed.mods.alt
        && parsed.vk == (b'B' as u32)
    {
        return Some(
            "Win+Ctrl+Shift+B is reserved by Windows (Graphics Driver Reset) and cannot be overridden",
        );
    }
    None
}

/// Returns true for standalone `Win`, well-known Windows 10/11 Shell (`explorer.exe`) `Win+...`
/// shortcuts, and Windows shell/DWM-reserved task-switcher/system chords (`Alt+Tab`, `Alt+Shift+Tab`,
/// `Ctrl+Alt+Tab`, `Ctrl+Alt+Shift+Tab`, `Alt+Esc`, `Alt+Shift+Esc`, `Ctrl+Esc`, `Ctrl+Shift+Esc`).
/// Routing these directly to `WH_KEYBOARD_LL` avoids `RegisterHotKey` rejections and logon races
/// with `explorer.exe` so native shortcuts restore immediately whenever unbound or when the daemon exits.
pub fn is_known_windows_shell_hotkey(parsed: ParsedKey) -> bool {
    if parsed.is_lone_win() {
        return true;
    }
    if !parsed.mods.win {
        return match (
            parsed.mods.ctrl,
            parsed.mods.alt,
            parsed.mods.shift,
            parsed.vk,
        ) {
            // Alt+Tab, Alt+Shift+Tab, Ctrl+Alt+Tab, Ctrl+Alt+Shift+Tab (Windows Task Switcher)
            (_, true, _, VK_TAB) => true,
            // Alt+Esc, Alt+Shift+Esc (Cycle Windows in Z-order)
            (false, true, _, VK_ESCAPE) => true,
            // Ctrl+Esc (Start Menu), Ctrl+Shift+Esc (Task Manager)
            (true, false, _, VK_ESCAPE) => true,
            _ => false,
        };
    }

    let vk = parsed.vk;
    match (parsed.mods.ctrl, parsed.mods.alt, parsed.mods.shift) {
        // Bare Win + <key>
        (false, false, false) => matches!(
            vk,
            0x30..=0x39 // Win+0..9 (Taskbar)
                | 0x41 // Win+A (Quick Settings)
                | 0x42 // Win+B (Notification Area)
                | 0x43 // Win+C (Copilot / Chat)
                | 0x44 // Win+D (Desktop)
                | 0x45 // Win+E (File Explorer)
                | 0x46 // Win+F (Feedback Hub)
                | 0x47 // Win+G (Game Bar)
                | 0x48 // Win+H (Voice Typing)
                | 0x49 // Win+I (Settings)
                | 0x4B // Win+K (Cast)
                | 0x4C // Win+L (Lock)
                | 0x4D // Win+M (Minimize All)
                | 0x4E // Win+N (Notifications)
                | 0x50 // Win+P (Project)
                | 0x51 // Win+Q (Search)
                | 0x52 // Win+R (Run)
                | 0x53 // Win+S (Search)
                | 0x54 // Win+T (Cycle Taskbar)
                | 0x55 // Win+U (Accessibility)
                | 0x56 // Win+V (Clipboard History)
                | 0x57 // Win+W (Widgets)
                | 0x58 // Win+X (Quick Link)
                | 0x5A // Win+Z (Snap Layouts)
                | VK_TAB
                | VK_SPACE
                | VK_LEFT
                | VK_RIGHT
                | VK_UP
                | VK_DOWN
                | VK_HOME
                | VK_ESCAPE
                | VK_OEM_1
                | VK_OEM_PLUS
                | VK_OEM_COMMA
                | VK_OEM_MINUS
                | VK_OEM_PERIOD
                | VK_OEM_2
        ),
        // Win + Shift + <key>
        (false, false, true) => matches!(
            vk,
            0x30..=0x39
                | 0x43 // Win+Shift+C
                | 0x4D // Win+Shift+M
                | 0x53 // Win+Shift+S (Snipping Tool)
                | 0x56 // Win+Shift+V
                | VK_LEFT
                | VK_RIGHT
                | VK_UP
                | VK_DOWN
                | VK_SPACE
        ),
        // Win + Ctrl + <key>
        (true, false, false) => matches!(
            vk,
            0x30..=0x39
                | 0x43 // Win+Ctrl+C (Color Filters)
                | 0x44 // Win+Ctrl+D (New Desktop)
                | 0x46 // Win+Ctrl+F
                | 0x4C // Win+Ctrl+L (Live Captions)
                | 0x4F // Win+Ctrl+O (OSK)
                | 0x51 // Win+Ctrl+Q (Quick Assist)
                | 0x53 // Win+Ctrl+S (Voice Access)
                | VK_LEFT
                | VK_RIGHT
                | VK_RETURN
                | 0x73 // VK_F4 (Close Desktop)
        ),
        // Win + Alt + <key>
        (false, true, false) => matches!(
            vk,
            0x30..=0x39
                | 0x42 // Win+Alt+B (Auto HDR)
                | 0x44 // Win+Alt+D (Date & Time)
                | 0x47 // Win+Alt+G (Game Bar Clip)
                | 0x4B // Win+Alt+K (Mute Mic)
                | 0x52 // Win+Alt+R (Game Bar Record)
                | VK_UP
                | VK_DOWN
                | VK_RETURN
        ),
        _ => false,
    }
}

/// Validates a global OS hotkey suitable for Windows `RegisterHotKey` or `WH_KEYBOARD_LL`.
/// Rejects bare printable characters (like `"k"`) so typing stays unbroken, and rejects
/// kernel-locked Windows shortcuts (`Win+L`, `Ctrl+Alt+Delete`, `Win+Ctrl+Shift+B`).
pub fn parse_global_hotkey(spec: &str) -> Result<ParsedKey, String> {
    let parsed = parse_key_spec(spec)?;
    if parsed.vk == VK_LWIN || parsed.vk == VK_RWIN {
        if parsed.mods.any() {
            return Err("Modifier-only hotkeys are not supported (except standalone Win)".into());
        }
        return Ok(ParsedKey {
            mods: Modifiers::default(),
            vk: VK_LWIN,
            shifted_char: None,
        });
    }
    let is_function_key = (VK_F1..=VK_F24).contains(&parsed.vk);
    if !parsed.mods.any() && !is_function_key {
        return Err(format!(
            "Global hotkey '{spec}' must include a modifier (Ctrl, Alt, Shift, or Win), be Win alone, or be F1-F24"
        ));
    }
    if let Some(reason) = os_locked_hotkey_reason(parsed) {
        return Err(reason.to_string());
    }
    Ok(parsed)
}

pub fn format_parsed_key(key: ParsedKey) -> String {
    if let Some(ch) = key.shifted_char
        && !key.mods.ctrl
        && !key.mods.alt
        && !key.mods.win
        && ch == '?'
    {
        return "?".into();
    }

    let mut parts = Vec::new();
    if key.mods.ctrl {
        parts.push("Ctrl".to_string());
    }
    if key.mods.alt {
        parts.push("Alt".to_string());
    }
    if key.mods.shift {
        parts.push("Shift".to_string());
    }
    if key.mods.win {
        parts.push("Win".to_string());
    }
    parts.push(vk_to_name(key.vk));
    parts.join("+")
}

pub fn canonicalize_key_spec(spec: &str) -> Result<String, String> {
    parse_key_spec(spec).map(format_parsed_key)
}

pub fn format_vk_parts(mods: Modifiers, vk: u32) -> Option<String> {
    if (vk == VK_LWIN || vk == VK_RWIN) && !mods.any() {
        return Some("Win".to_string());
    }
    if is_modifier_vk(vk) {
        return None;
    }
    let name = vk_to_name_opt(vk)?;
    let mut parts = Vec::new();
    if mods.ctrl {
        parts.push("Ctrl".to_string());
    }
    if mods.alt {
        parts.push("Alt".to_string());
    }
    if mods.shift {
        parts.push("Shift".to_string());
    }
    if mods.win {
        parts.push("Win".to_string());
    }
    parts.push(name);
    Some(parts.join("+"))
}

pub fn matches_vk_event(parsed: &ParsedKey, event_mods: Modifiers, event_vk: u32, ch: Option<char>) -> bool {
    if let Some(expected_ch) = parsed.shifted_char
        && let Some(actual_ch) = ch
        && !event_mods.ctrl
        && !event_mods.alt
        && !event_mods.win
        && actual_ch == expected_ch
    {
        return true;
    }

    // Ignore shift difference for case-insensitive single letter when shift wasn't requested explicitly
    if parsed.vk == event_vk
        && parsed.mods.ctrl == event_mods.ctrl
        && parsed.mods.alt == event_mods.alt
        && parsed.mods.shift == event_mods.shift
        && parsed.mods.win == event_mods.win
    {
        return true;
    }

    // Also match Linux ISO_Left_Tab -> Shift+Tab seamlessly
    false
}

fn parse_key_name(tok: &str) -> Result<(u32, bool, Option<char>), String> {
    if tok.chars().count() == 1 {
        let c = tok.chars().next().unwrap();
        return match c {
            'a'..='z' | 'A'..='Z' => Ok(((c.to_ascii_uppercase()) as u32, false, None)),
            '0'..='9' => Ok((c as u32, false, None)),
            ' ' => Ok((VK_SPACE, false, None)),
            '?' => Ok((VK_OEM_2, true, Some('?'))),
            '/' => Ok((VK_OEM_2, false, Some('/'))),
            '.' => Ok((VK_OEM_PERIOD, false, Some('.'))),
            ',' => Ok((VK_OEM_COMMA, false, Some(','))),
            '-' => Ok((VK_OEM_MINUS, false, Some('-'))),
            '=' => Ok((VK_OEM_PLUS, false, Some('='))),
            ';' => Ok((VK_OEM_1, false, Some(';'))),
            '\'' => Ok((VK_OEM_7, false, Some('\''))),
            '[' => Ok((VK_OEM_4, false, Some('['))),
            '\\' => Ok((VK_OEM_5, false, Some('\\'))),
            ']' => Ok((VK_OEM_6, false, Some(']'))),
            '`' => Ok((VK_OEM_3, false, Some('`'))),
            _ => Err(format!("Unsupported single character key '{tok}'")),
        };
    }

    let lower = tok.to_ascii_lowercase();
    let res = match lower.as_str() {
        "up" | "kp_up" => (VK_UP, false, None),
        "down" | "kp_down" => (VK_DOWN, false, None),
        "left" | "kp_left" => (VK_LEFT, false, None),
        "right" | "kp_right" => (VK_RIGHT, false, None),
        "return" | "enter" | "kp_enter" => (VK_RETURN, false, None),
        "space" => (VK_SPACE, false, None),
        "tab" => (VK_TAB, false, None),
        "iso_left_tab" => (VK_TAB, true, None),
        "backspace" => (VK_BACK, false, None),
        "escape" | "esc" => (VK_ESCAPE, false, None),
        "delete" | "del" => (VK_DELETE, false, None),
        "insert" | "ins" => (VK_INSERT, false, None),
        "home" => (VK_HOME, false, None),
        "end" => (VK_END, false, None),
        "prior" | "page_up" | "pageup" | "pgup" => (VK_PRIOR, false, None),
        "next" | "page_down" | "pagedown" | "pgdn" => (VK_NEXT, false, None),
        "slash" => (VK_OEM_2, false, Some('/')),
        "question" => (VK_OEM_2, true, Some('?')),
        "minus" => (VK_OEM_MINUS, false, Some('-')),
        "equal" | "plus" => (VK_OEM_PLUS, false, Some('=')),
        "comma" => (VK_OEM_COMMA, false, Some(',')),
        "period" => (VK_OEM_PERIOD, false, Some('.')),
        "grave" => (VK_OEM_3, false, Some('`')),
        "win" | "super" | "meta" | "mod4" | "lwin" | "rwin" => (VK_LWIN, false, None),
        _ => {
            if let Some(rest) = lower.strip_prefix('f')
                && let Ok(n) = rest.parse::<u32>()
                && (1..=24).contains(&n)
            {
                (VK_F1 + (n - 1), false, None)
            } else {
                return Err(format!("Unknown key name '{tok}'"));
            }
        }
    };
    Ok(res)
}

fn vk_to_name(vk: u32) -> String {
    vk_to_name_opt(vk).unwrap_or_else(|| format!("VK_{vk:02X}"))
}

pub fn vk_to_name_opt(vk: u32) -> Option<String> {
    let s = match vk {
        VK_BACK => "BackSpace",
        VK_TAB => "Tab",
        VK_RETURN => "Enter",
        VK_ESCAPE => "Escape",
        VK_SPACE => "Space",
        VK_PRIOR => "PageUp",
        VK_NEXT => "PageDown",
        VK_END => "End",
        VK_HOME => "Home",
        VK_LEFT => "Left",
        VK_UP => "Up",
        VK_RIGHT => "Right",
        VK_DOWN => "Down",
        VK_INSERT => "Insert",
        VK_DELETE => "Delete",
        VK_LWIN | VK_RWIN => "Win",
        VK_OEM_1 => ";",
        VK_OEM_PLUS => "=",
        VK_OEM_COMMA => ",",
        VK_OEM_MINUS => "-",
        VK_OEM_PERIOD => ".",
        VK_OEM_2 => "/",
        VK_OEM_3 => "`",
        VK_OEM_4 => "[",
        VK_OEM_5 => "\\",
        VK_OEM_6 => "]",
        VK_OEM_7 => "'",
        0x30..=0x39 => return Some(((vk as u8) as char).to_string()),
        0x41..=0x5A => return Some((((vk as u8) as char).to_ascii_lowercase()).to_string()),
        0x60..=0x69 => return Some(format!("Numpad{}", vk - 0x60)),
        VK_F1..=VK_F24 => return Some(format!("F{}", vk - VK_F1 + 1)),
        _ => return None,
    };
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_and_windows_key_specs_and_roundtrips() {
        for spec in [
            "Ctrl+Space",
            "Ctrl+Alt+v",
            "Shift+Tab",
            "ISO_Left_Tab",
            "Return",
            "Enter",
            "BackSpace",
            "Page_Down",
            "Next",
            "PageUp",
            "Prior",
            "Ctrl+BackSpace",
            "?",
            "F1",
            "Win+Alt+r",
            "Win+e",
            "Win+b",
            "Win+j",
            "Win",
        ] {
            let parsed = parse_key_spec(spec).unwrap_or_else(|e| panic!("{spec}: {e}"));
            let canon = format_parsed_key(parsed);
            let reparsed = parse_key_spec(&canon).unwrap();
            assert_eq!(parsed.vk, reparsed.vk, "roundtrip vk mismatch for {spec}");
            assert_eq!(parsed.mods, reparsed.mods, "roundtrip mods mismatch for {spec}");
        }
    }

    #[test]
    fn rejects_unmodified_global_hotkeys_and_invalid_specs() {
        assert!(parse_global_hotkey("a").is_err());
        assert!(parse_global_hotkey("Space").is_err());
        assert!(parse_global_hotkey("Ctrl+").is_err());
        assert!(parse_global_hotkey("Bogus+k").is_err());
        assert!(parse_global_hotkey("Ctrl+Win").is_err());
        assert!(parse_global_hotkey("Win+L").is_err());
        assert!(parse_global_hotkey("Win+l").is_err());
        assert!(parse_global_hotkey("Ctrl+Alt+Delete").is_err());
        assert!(parse_global_hotkey("Win+Ctrl+Shift+B").is_err());
        assert!(parse_global_hotkey("Ctrl+Space").is_ok());
        assert!(parse_global_hotkey("F9").is_ok());
        assert!(parse_global_hotkey("Win+Shift+c").is_ok());
        assert!(parse_global_hotkey("Win+E").is_ok());
        assert!(parse_global_hotkey("Win+B").is_ok());
        assert!(parse_global_hotkey("Win+J").is_ok());
        assert!(parse_global_hotkey("Win+Ctrl+L").is_ok());
        let lone_win = parse_global_hotkey("Win").unwrap();
        assert!(lone_win.is_lone_win());
        assert!(lone_win.uses_win_key());
        assert_eq!(format_parsed_key(lone_win), "Win");
        assert_eq!(format_vk_parts(Modifiers::default(), VK_LWIN), Some("Win".into()));
        assert_eq!(
            format_vk_parts(Modifiers { win: true, ..Default::default() }, VK_LWIN),
            None
        );
        let upper = parse_global_hotkey("Ctrl+Alt+F").unwrap();
        let lower = parse_global_hotkey("Ctrl+Alt+f").unwrap();
        assert_eq!(upper.mods, lower.mods);
        assert!(!upper.mods.shift);
    }

    #[test]
    fn classifies_shell_hotkeys_and_modifier_bits() {
        for bits in 0u8..16 {
            let m = Modifiers::from_bits(bits);
            assert_eq!(m.to_bits(), bits);
        }

        for shell_spec in [
            "Win",
            "Win+E",
            "Win+B",
            "Win+V",
            "Win+D",
            "Win+R",
            "Win+Tab",
            "Win+Shift+S",
            "Win+1",
            "Alt+Tab",
            "Alt+Shift+Tab",
            "Ctrl+Alt+Tab",
            "Ctrl+Alt+Shift+Tab",
            "Alt+Escape",
            "Alt+Shift+Escape",
            "Ctrl+Escape",
            "Ctrl+Shift+Escape",
        ] {
            let p = parse_global_hotkey(shell_spec).unwrap();
            assert!(is_known_windows_shell_hotkey(p), "{shell_spec} should be shell hotkey");
        }
        for non_shell_spec in [
            "Win+J",
            "Win+O",
            "Win+Y",
            "Win+Alt+E",
            "Ctrl+Space",
            "Ctrl+Tab",
            "Shift+Tab",
            "Alt+Space",
            "Alt+F4",
            "F9",
        ] {
            let p = parse_global_hotkey(non_shell_spec).unwrap();
            assert!(
                !is_known_windows_shell_hotkey(p),
                "{non_shell_spec} should not be classified as shell hotkey"
            );
        }
    }
}
