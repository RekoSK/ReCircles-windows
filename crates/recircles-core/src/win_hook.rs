use crate::win_keys::{
    Modifiers, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, is_modifier_vk,
};

/// Distinct `dwExtraInfo` tag stamped on synthetic `SendInput` events emitted by ReCircles
/// so our `WH_KEYBOARD_LL` callbacks immediately pass them through without re-entering state.
pub const RECIRCLES_INJECTED_EXTRA_INFO: usize = 0x5243_484B; // "RCHK"

/// Unassigned virtual key code (`0xE8`, same default as AutoHotkey's `#MenuMaskKey`) used to
/// break Windows' lone-Win Start Menu / lone-Alt menu bar tracker without side effects.
pub const VK_MASK_DUMMY: u32 = 0xE8;

/// Maximum window (ms) during which a hook-tracked modifier `KEYDOWN` overrides a lagging or
/// zeroed `GetAsyncKeyState()` result (longer than Windows' max 1000 ms keyboard repeat delay;
/// auto-repeat refreshes `last_down_ms` continuously while a modifier is physically held).
pub const MOD_FRESH_DOWN_WINDOW_MS: u32 = 1500;

/// Window (ms) after a hook-observed modifier `KEYUP` during which a lagging `GetAsyncKeyState()`
/// `true` bit is ignored (prevents stale `Alt`/`Ctrl`/`Shift` after `Alt+F4`/`Ctrl+W`/`Win+D`).
pub const MOD_RECENT_UP_WINDOW_MS: u32 = 250;

/// Window (ms) within which repeated `KEYDOWN` events for an already-suppressed key are treated
/// as keyboard auto-repeat (necessary because swallowed keys never update `GetAsyncKeyState`).
pub const SUPPRESSED_REPEAT_WINDOW_MS: u32 = 1500;

#[derive(Debug, Clone, Copy, Default)]
struct TrackedKey {
    down: bool,
    last_down_ms: u32,
    last_up_ms: u32,
    has_up: bool,
}

impl TrackedKey {
    fn on_down(&mut self, now_ms: u32) {
        self.down = true;
        self.last_down_ms = now_ms;
    }

    fn on_up(&mut self, now_ms: u32) {
        self.down = false;
        self.last_up_ms = now_ms;
        self.has_up = true;
    }

    fn is_fresh_down(&self, now_ms: u32) -> bool {
        self.down && now_ms.wrapping_sub(self.last_down_ms) <= MOD_FRESH_DOWN_WINDOW_MS
    }

    fn is_recent_up(&self, now_ms: u32) -> bool {
        !self.down && self.has_up && now_ms.wrapping_sub(self.last_up_ms) <= MOD_RECENT_UP_WINDOW_MS
    }
}

/// Deterministic hook-stream modifier state tracker that avoids `GetAsyncKeyState()` timing races
/// and zeroed desktop-focus reads inside `LowLevelKeyboardProc`, while self-healing if a `KEYUP`
/// is ever missed across UAC, lock screens, or elevated windows.
#[derive(Debug, Clone, Copy, Default)]
struct ModifierTracker {
    lctrl: TrackedKey,
    rctrl: TrackedKey,
    ctrl: TrackedKey,
    lalt: TrackedKey,
    ralt: TrackedKey,
    alt: TrackedKey,
    lshift: TrackedKey,
    rshift: TrackedKey,
    shift: TrackedKey,
    lwin: TrackedKey,
    rwin: TrackedKey,
}

impl ModifierTracker {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn is_key_down(&self, vk: u32) -> bool {
        match vk {
            VK_LCONTROL => self.lctrl.down,
            VK_RCONTROL => self.rctrl.down,
            VK_CONTROL => self.ctrl.down,
            VK_LMENU => self.lalt.down,
            VK_RMENU => self.ralt.down,
            VK_MENU => self.alt.down,
            VK_LSHIFT => self.lshift.down,
            VK_RSHIFT => self.rshift.down,
            VK_SHIFT => self.shift.down,
            VK_LWIN => self.lwin.down,
            VK_RWIN => self.rwin.down,
            _ => false,
        }
    }

    fn any_win_down(&self) -> bool {
        self.lwin.down || self.rwin.down
    }

    fn any_alt_down(&self) -> bool {
        self.lalt.down || self.ralt.down || self.alt.down
    }

    fn on_event(&mut self, vk: u32, is_down: bool, now_ms: u32) {
        if is_down {
            match vk {
                VK_LCONTROL => self.lctrl.on_down(now_ms),
                VK_RCONTROL => self.rctrl.on_down(now_ms),
                VK_CONTROL => self.ctrl.on_down(now_ms),
                VK_LMENU => self.lalt.on_down(now_ms),
                VK_RMENU => self.ralt.on_down(now_ms),
                VK_MENU => self.alt.on_down(now_ms),
                VK_LSHIFT => self.lshift.on_down(now_ms),
                VK_RSHIFT => self.rshift.on_down(now_ms),
                VK_SHIFT => self.shift.on_down(now_ms),
                VK_LWIN => self.lwin.on_down(now_ms),
                VK_RWIN => self.rwin.on_down(now_ms),
                _ => {}
            }
        } else {
            match vk {
                VK_LCONTROL => self.lctrl.on_up(now_ms),
                VK_RCONTROL => self.rctrl.on_up(now_ms),
                VK_CONTROL => {
                    self.ctrl.on_up(now_ms);
                    if self.lctrl.down {
                        self.lctrl.on_up(now_ms);
                    }
                    if self.rctrl.down {
                        self.rctrl.on_up(now_ms);
                    }
                }
                VK_LMENU => self.lalt.on_up(now_ms),
                VK_RMENU => self.ralt.on_up(now_ms),
                VK_MENU => {
                    self.alt.on_up(now_ms);
                    if self.lalt.down {
                        self.lalt.on_up(now_ms);
                    }
                    if self.ralt.down {
                        self.ralt.on_up(now_ms);
                    }
                }
                VK_LSHIFT => self.lshift.on_up(now_ms),
                VK_RSHIFT => self.rshift.on_up(now_ms),
                VK_SHIFT => {
                    self.shift.on_up(now_ms);
                    if self.lshift.down {
                        self.lshift.on_up(now_ms);
                    }
                    if self.rshift.down {
                        self.rshift.on_up(now_ms);
                    }
                }
                VK_LWIN => self.lwin.on_up(now_ms),
                VK_RWIN => self.rwin.on_up(now_ms),
                _ => {}
            }
        }
    }

    fn resolve_group(keys: &mut [&mut TrackedKey], async_down: bool, now_ms: u32) -> bool {
        let any_down = keys.iter().any(|k| k.down);
        if any_down {
            if async_down {
                return true;
            }
            let mut any_fresh = false;
            for k in keys.iter_mut() {
                if k.down {
                    if k.is_fresh_down(now_ms) {
                        any_fresh = true;
                    } else {
                        // Self-heal stale modifier whose KEYUP was missed (UAC / lock / elevated).
                        k.down = false;
                    }
                }
            }
            return any_fresh;
        }

        if !async_down {
            return false;
        }

        // Ignore lagging GetAsyncKeyState=true right after our hook already observed KEYUP.
        if keys.iter().any(|k| k.is_recent_up(now_ms)) {
            return false;
        }

        // Modifier was pressed before our hook observed it (e.g. held across hook start/elevated).
        true
    }

    fn effective(&mut self, async_mods: Modifiers, now_ms: u32) -> Modifiers {
        Modifiers {
            ctrl: Self::resolve_group(
                &mut [&mut self.lctrl, &mut self.rctrl, &mut self.ctrl],
                async_mods.ctrl,
                now_ms,
            ),
            alt: Self::resolve_group(
                &mut [&mut self.lalt, &mut self.ralt, &mut self.alt],
                async_mods.alt,
                now_ms,
            ),
            shift: Self::resolve_group(
                &mut [&mut self.lshift, &mut self.rshift, &mut self.shift],
                async_mods.shift,
                now_ms,
            ),
            win: Self::resolve_group(
                &mut [&mut self.lwin, &mut self.rwin],
                async_mods.win,
                now_ms,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookBindingEntry {
    pub id: i32,
    pub mods: Modifiers,
    pub vk: u32,
    pub lone_win: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookDecision {
    PassThrough,
    SwallowOnly,
    Trigger { id: i32, mask_menu: bool },
    TriggerLoneWin { id: i32, win_vk: u32 },
    /// Swallows a fast-rollover modifier `KEYUP` that arrived before the async dummy mask key
    /// passed through the hook, delegating `[0xE8 down, 0xE8 up, mod_vk up]` to the mask thread.
    MaskModifierRelease { mod_vk: u32 },
}

/// Pure, allocation-free state machine for `recircles-daemon`'s `WH_KEYBOARD_LL` hook.
#[derive(Debug, Clone)]
pub struct HookMatcher {
    chords: Vec<HookBindingEntry>,
    lone_win_id: Option<i32>,
    modifiers: ModifierTracker,
    suppressed_vks: [Option<u32>; 256],
    pending_lone_win_vk: Option<u32>,
    pending_mask_win: bool,
    pending_mask_alt: bool,
}

impl HookMatcher {
    pub fn new(entries: Vec<HookBindingEntry>) -> Self {
        let mut matcher = Self {
            chords: Vec::new(),
            lone_win_id: None,
            modifiers: ModifierTracker::default(),
            suppressed_vks: [None; 256],
            pending_lone_win_vk: None,
            pending_mask_win: false,
            pending_mask_alt: false,
        };
        matcher.update_bindings(entries);
        matcher
    }

    /// Replaces active bindings in-place without disturbing currently held modifiers or
    /// still-draining suppressed keys (zero unhook gap during config reload).
    pub fn update_bindings(&mut self, entries: Vec<HookBindingEntry>) {
        self.chords.clear();
        self.lone_win_id = None;
        for entry in entries {
            if entry.lone_win {
                self.lone_win_id = Some(entry.id);
            } else {
                self.chords.push(entry);
            }
        }
        if self.lone_win_id.is_none() {
            self.pending_lone_win_vk = None;
        }
    }

    /// Clears all transient modifier, suppression, and menu-mask state (e.g. on lock/unlock/sleep).
    pub fn reset_state(&mut self) {
        self.modifiers.reset();
        self.suppressed_vks = [None; 256];
        self.pending_lone_win_vk = None;
        self.pending_mask_win = false;
        self.pending_mask_alt = false;
    }

    /// Called when the dummy mask key (`VK_MASK_DUMMY`) passes through the hook chain.
    pub fn on_menu_mask_seen(&mut self) {
        self.pending_mask_win = false;
        self.pending_mask_alt = false;
    }

    pub fn is_empty(&self) -> bool {
        self.chords.is_empty() && self.lone_win_id.is_none()
    }

    pub fn process_key(
        &mut self,
        vk: u32,
        is_down: bool,
        async_mods: Modifiers,
        was_already_down: bool,
        now_ms: u32,
        extra_info: usize,
    ) -> HookDecision {
        if vk == VK_MASK_DUMMY {
            self.on_menu_mask_seen();
            return HookDecision::PassThrough;
        }
        if extra_info == RECIRCLES_INJECTED_EXTRA_INFO {
            return HookDecision::PassThrough;
        }

        if vk == VK_LWIN || vk == VK_RWIN {
            if is_down {
                let mods_before = self.modifiers.effective(async_mods, now_ms);
                let was_this_win_down = self.modifiers.is_key_down(vk);
                self.modifiers.on_event(vk, true, now_ms);
                if !was_this_win_down {
                    self.pending_mask_win = false;
                }
                if self.lone_win_id.is_some() {
                    if self.pending_lone_win_vk == Some(vk) {
                        // Auto-repeat while holding the same Win key: keep candidate active.
                    } else if self.pending_lone_win_vk.is_none()
                        && !mods_before.ctrl
                        && !mods_before.alt
                        && !mods_before.shift
                    {
                        self.pending_lone_win_vk = Some(vk);
                    } else {
                        self.pending_lone_win_vk = None;
                    }
                }
                return HookDecision::PassThrough;
            } else {
                self.modifiers.on_event(vk, false, now_ms);
                if self.pending_lone_win_vk == Some(vk) {
                    self.pending_lone_win_vk = None;
                    self.pending_mask_win = false;
                    if let Some(id) = self.lone_win_id {
                        return HookDecision::TriggerLoneWin { id, win_vk: vk };
                    }
                } else {
                    self.pending_lone_win_vk = None;
                    if self.pending_mask_win && !self.modifiers.any_win_down() {
                        self.pending_mask_win = false;
                        return HookDecision::MaskModifierRelease { mod_vk: vk };
                    }
                }
                return HookDecision::PassThrough;
            }
        }

        if is_modifier_vk(vk) {
            let is_alt_vk = matches!(vk, VK_LMENU | VK_RMENU | VK_MENU);
            let was_this_down = self.modifiers.is_key_down(vk);
            self.modifiers.on_event(vk, is_down, now_ms);
            if is_down {
                self.pending_lone_win_vk = None;
                if is_alt_vk && !was_this_down {
                    self.pending_mask_alt = false;
                }
            } else if is_alt_vk && self.pending_mask_alt && !self.modifiers.any_alt_down() {
                self.pending_mask_alt = false;
                return HookDecision::MaskModifierRelease { mod_vk: vk };
            }
            return HookDecision::PassThrough;
        }

        let idx = vk as usize;
        if is_down {
            self.pending_lone_win_vk = None;

            if idx < self.suppressed_vks.len()
                && let Some(last_ms) = self.suppressed_vks[idx]
            {
                if was_already_down
                    || now_ms.wrapping_sub(last_ms) <= SUPPRESSED_REPEAT_WINDOW_MS
                {
                    self.suppressed_vks[idx] = Some(now_ms);
                    return HookDecision::SwallowOnly;
                }
                // Self-heal if a prior KEYUP was missed across UAC / lock screen.
                self.suppressed_vks[idx] = None;
            }

            let mods = self.modifiers.effective(async_mods, now_ms);
            if let Some(entry) = self
                .chords
                .iter()
                .find(|e| e.vk == vk && e.mods == mods)
            {
                if idx < self.suppressed_vks.len() {
                    self.suppressed_vks[idx] = Some(now_ms);
                }
                if mods.win {
                    self.pending_mask_win = true;
                }
                if mods.alt {
                    self.pending_mask_alt = true;
                }
                return HookDecision::Trigger {
                    id: entry.id,
                    mask_menu: mods.win || mods.alt,
                };
            }

            HookDecision::PassThrough
        } else {
            if idx < self.suppressed_vks.len() && self.suppressed_vks[idx].is_some() {
                self.suppressed_vks[idx] = None;
                return HookDecision::SwallowOnly;
            }
            HookDecision::PassThrough
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureDecision {
    PassThrough,
    SwallowOnly { finished_draining: bool },
    Captured {
        vk: u32,
        mods: Modifiers,
        mask_menu: bool,
    },
    CapturedLoneWin { win_vk: u32 },
    MaskLoneWinOnly { win_vk: u32 },
    MaskModifierRelease { mod_vk: u32 },
}

/// Pure state machine for temporary interactive keybind capture in `recircles-ui` and `recircles-installer`.
#[derive(Debug, Clone)]
pub struct CaptureMatcher {
    allow_lone_win: bool,
    active: bool,
    modifiers: ModifierTracker,
    suppressed_vks: [Option<u32>; 256],
    pending_lone_win_vk: Option<u32>,
    pending_mask_win: bool,
    pending_mask_alt: bool,
}

impl CaptureMatcher {
    pub fn new(allow_lone_win: bool) -> Self {
        Self {
            allow_lone_win,
            active: true,
            modifiers: ModifierTracker::default(),
            suppressed_vks: [None; 256],
            pending_lone_win_vk: None,
            pending_mask_win: false,
            pending_mask_alt: false,
        }
    }

    pub fn set_active(&mut self, active: bool, allow_lone_win: bool) {
        self.active = active;
        self.allow_lone_win = allow_lone_win;
        if !active {
            self.pending_lone_win_vk = None;
        }
    }

    pub fn on_menu_mask_seen(&mut self) {
        self.pending_mask_win = false;
        self.pending_mask_alt = false;
    }

    pub fn has_suppressed(&self) -> bool {
        self.suppressed_vks.iter().any(Option::is_some)
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn process_key(
        &mut self,
        vk: u32,
        is_down: bool,
        async_mods: Modifiers,
        was_already_down: bool,
        now_ms: u32,
        extra_info: usize,
    ) -> CaptureDecision {
        if vk == VK_MASK_DUMMY {
            self.on_menu_mask_seen();
            return CaptureDecision::PassThrough;
        }
        if extra_info == RECIRCLES_INJECTED_EXTRA_INFO {
            return CaptureDecision::PassThrough;
        }

        let idx = vk as usize;

        if !self.active {
            if is_modifier_vk(vk) {
                let is_win_vk = vk == VK_LWIN || vk == VK_RWIN;
                let is_alt_vk = matches!(vk, VK_LMENU | VK_RMENU | VK_MENU);
                self.modifiers.on_event(vk, is_down, now_ms);
                if !is_down {
                    if is_win_vk && self.pending_mask_win && !self.modifiers.any_win_down() {
                        self.pending_mask_win = false;
                        return CaptureDecision::MaskModifierRelease { mod_vk: vk };
                    }
                    if is_alt_vk && self.pending_mask_alt && !self.modifiers.any_alt_down() {
                        self.pending_mask_alt = false;
                        return CaptureDecision::MaskModifierRelease { mod_vk: vk };
                    }
                }
                return CaptureDecision::PassThrough;
            }
            if is_down {
                if idx < self.suppressed_vks.len()
                    && let Some(last_ms) = self.suppressed_vks[idx]
                {
                    if was_already_down
                        || now_ms.wrapping_sub(last_ms) <= SUPPRESSED_REPEAT_WINDOW_MS
                    {
                        self.suppressed_vks[idx] = Some(now_ms);
                        return CaptureDecision::SwallowOnly {
                            finished_draining: false,
                        };
                    }
                    self.suppressed_vks[idx] = None;
                }
                return CaptureDecision::PassThrough;
            } else {
                if idx < self.suppressed_vks.len() && self.suppressed_vks[idx].is_some() {
                    self.suppressed_vks[idx] = None;
                    return CaptureDecision::SwallowOnly {
                        finished_draining: !self.has_suppressed(),
                    };
                }
                return CaptureDecision::PassThrough;
            }
        }

        if vk == VK_LWIN || vk == VK_RWIN {
            if is_down {
                let mods_before = self.modifiers.effective(async_mods, now_ms);
                let was_this_win_down = self.modifiers.is_key_down(vk);
                self.modifiers.on_event(vk, true, now_ms);
                if !was_this_win_down {
                    self.pending_mask_win = false;
                }
                if self.pending_lone_win_vk == Some(vk) {
                    // Auto-repeat while holding Win: keep candidate.
                } else if self.pending_lone_win_vk.is_none()
                    && !mods_before.ctrl
                    && !mods_before.alt
                    && !mods_before.shift
                {
                    self.pending_lone_win_vk = Some(vk);
                } else {
                    self.pending_lone_win_vk = None;
                }
                return CaptureDecision::PassThrough;
            } else {
                self.modifiers.on_event(vk, false, now_ms);
                if self.pending_lone_win_vk == Some(vk) {
                    self.pending_lone_win_vk = None;
                    self.pending_mask_win = false;
                    if self.allow_lone_win {
                        return CaptureDecision::CapturedLoneWin { win_vk: vk };
                    }
                    return CaptureDecision::MaskLoneWinOnly { win_vk: vk };
                }
                self.pending_lone_win_vk = None;
                if self.pending_mask_win && !self.modifiers.any_win_down() {
                    self.pending_mask_win = false;
                    return CaptureDecision::MaskModifierRelease { mod_vk: vk };
                }
                return CaptureDecision::PassThrough;
            }
        }

        if is_modifier_vk(vk) {
            let is_alt_vk = matches!(vk, VK_LMENU | VK_RMENU | VK_MENU);
            let was_this_down = self.modifiers.is_key_down(vk);
            self.modifiers.on_event(vk, is_down, now_ms);
            if is_down {
                self.pending_lone_win_vk = None;
                if is_alt_vk && !was_this_down {
                    self.pending_mask_alt = false;
                }
            } else if is_alt_vk && self.pending_mask_alt && !self.modifiers.any_alt_down() {
                self.pending_mask_alt = false;
                return CaptureDecision::MaskModifierRelease { mod_vk: vk };
            }
            return CaptureDecision::PassThrough;
        }

        if is_down {
            self.pending_lone_win_vk = None;
            if idx < self.suppressed_vks.len()
                && let Some(last_ms) = self.suppressed_vks[idx]
            {
                if was_already_down
                    || now_ms.wrapping_sub(last_ms) <= SUPPRESSED_REPEAT_WINDOW_MS
                {
                    self.suppressed_vks[idx] = Some(now_ms);
                    return CaptureDecision::SwallowOnly {
                        finished_draining: false,
                    };
                }
                self.suppressed_vks[idx] = None;
            }
            if idx < self.suppressed_vks.len() {
                self.suppressed_vks[idx] = Some(now_ms);
            }
            let mods = self.modifiers.effective(async_mods, now_ms);
            if mods.win {
                self.pending_mask_win = true;
            }
            if mods.alt {
                self.pending_mask_alt = true;
            }
            CaptureDecision::Captured {
                vk,
                mods,
                mask_menu: mods.win || mods.alt,
            }
        } else {
            if idx < self.suppressed_vks.len() && self.suppressed_vks[idx].is_some() {
                self.suppressed_vks[idx] = None;
                return CaptureDecision::SwallowOnly {
                    finished_draining: false,
                };
            }
            CaptureDecision::PassThrough
        }
    }
}

#[cfg(windows)]
pub mod win {
    use super::{
        CaptureDecision, CaptureMatcher, HookBindingEntry, HookDecision, HookMatcher,
        RECIRCLES_INJECTED_EXTRA_INFO, VK_MASK_DUMMY,
    };
    use crate::win_keys::{Modifiers, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RWIN};
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex, mpsc};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
        THREAD_PRIORITY_HIGHEST,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, SendInput, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN as WIN_VK_LWIN,
        VK_RCONTROL as WIN_VK_RCONTROL, VK_RMENU as WIN_VK_RMENU, VK_RSHIFT,
        VK_RWIN as WIN_VK_RWIN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG,
        PM_NOREMOVE, PeekMessageW, PostMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    const WM_HOOK_UPDATE_BINDINGS: u32 = 0x8000 + 201;
    const WM_HOOK_RESET_STATE: u32 = 0x8000 + 202;
    const WM_HOOK_REINSTALL: u32 = 0x8000 + 203;
    const WM_HOOK_SHUTDOWN: u32 = 0x8000 + 204;

    const WM_MASK_MENU: u32 = 0x8000 + 210;
    const WM_MASK_RELEASE: u32 = 0x8000 + 211;
    const WM_MASK_SHUTDOWN: u32 = 0x8000 + 212;

    fn is_async_down(vk: u16) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    pub fn read_async_modifiers() -> Modifiers {
        Modifiers {
            ctrl: is_async_down(VK_LCONTROL) || is_async_down(WIN_VK_RCONTROL),
            alt: is_async_down(VK_LMENU) || is_async_down(WIN_VK_RMENU),
            shift: is_async_down(VK_LSHIFT) || is_async_down(VK_RSHIFT),
            win: is_async_down(WIN_VK_LWIN) || is_async_down(WIN_VK_RWIN),
        }
    }

    fn is_extended_modifier_vk(vk: u32) -> bool {
        matches!(vk, VK_LWIN | VK_RWIN | VK_RMENU | VK_RCONTROL)
    }

    fn kbd_input(vk: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: RECIRCLES_INJECTED_EXTRA_INFO,
                },
            },
        }
    }

    /// Injects an unassigned dummy key (`0xE8` down + up) while `Win` or `Alt` is still down
    /// so releasing the modifier does not trigger Windows Start Menu or menu-bar focus.
    pub fn send_menu_mask_key() {
        let inputs = [
            kbd_input(VK_MASK_DUMMY as u16, 0),
            kbd_input(VK_MASK_DUMMY as u16, KEYEVENTF_KEYUP),
        ];
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
        }
    }

    /// Atomically injects `[0xE8 down, 0xE8 up, mod_vk up]` after swallowing a hardware modifier
    /// release, guaranteeing `0xE8` precedes the release so Start Menu / menu bar never activates
    /// and the OS never sees a stuck modifier key.
    pub fn send_masked_win_release(mod_vk: u32) {
        let up_flags = if is_extended_modifier_vk(mod_vk) {
            KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
        } else {
            KEYEVENTF_KEYUP
        };
        let inputs = [
            kbd_input(VK_MASK_DUMMY as u16, 0),
            kbd_input(VK_MASK_DUMMY as u16, KEYEVENTF_KEYUP),
            kbd_input(mod_vk as u16, up_flags),
        ];
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
        }
    }

    // -------------------------------------------------------------------------
    // Dedicated async SendInput mask thread (never runs inside LowLevelKeyboardProc)
    // -------------------------------------------------------------------------

    struct MaskThread {
        thread_id: u32,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl MaskThread {
        fn spawn() -> Option<Self> {
            let (ready_tx, ready_rx) = mpsc::sync_channel::<u32>(1);
            let join = std::thread::Builder::new()
                .name("recircles-mask".into())
                .spawn(move || {
                    unsafe {
                        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
                        // Force message queue creation before signaling thread ID to callers.
                        let mut msg: MSG = std::mem::zeroed();
                        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                        let tid = GetCurrentThreadId();
                        let _ = ready_tx.send(tid);
                        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                            if msg.hwnd.is_null() {
                                match msg.message {
                                    WM_MASK_MENU => send_menu_mask_key(),
                                    WM_MASK_RELEASE => send_masked_win_release(msg.wParam as u32),
                                    WM_MASK_SHUTDOWN => break,
                                    _ => {}
                                }
                            }
                        }
                    }
                })
                .ok()?;

            let thread_id = ready_rx.recv().ok()?;
            Some(Self {
                thread_id,
                join: Some(join),
            })
        }

        fn thread_id(&self) -> u32 {
            self.thread_id
        }
    }

    impl Drop for MaskThread {
        fn drop(&mut self) {
            if self.thread_id != 0 {
                unsafe {
                    PostThreadMessageW(self.thread_id, WM_MASK_SHUTDOWN, 0, 0);
                }
            }
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    #[inline]
    fn post_mask_menu(mask_thread_id: u32) {
        if mask_thread_id != 0 {
            unsafe {
                PostThreadMessageW(mask_thread_id, WM_MASK_MENU, 0, 0);
            }
        }
    }

    #[inline]
    fn post_mask_release(mask_thread_id: u32, mod_vk: u32) {
        if mask_thread_id != 0 {
            unsafe {
                PostThreadMessageW(mask_thread_id, WM_MASK_RELEASE, mod_vk as usize, 0);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Global daemon WH_KEYBOARD_LL hook (dedicated high-priority hook thread)
    // -------------------------------------------------------------------------

    struct GlobalHookCtx {
        target_hwnd: HWND,
        msg_id: u32,
        mask_thread_id: u32,
        matcher: HookMatcher,
    }

    thread_local! {
        static GLOBAL_HOOK_CTX: RefCell<Option<GlobalHookCtx>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn global_ll_proc(
        ncode: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if ncode != HC_ACTION as i32 || lparam == 0 {
            return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) };
        }

        let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if kb.vkCode == VK_MASK_DUMMY {
            GLOBAL_HOOK_CTX.with(|slot| {
                if let Ok(mut guard) = slot.try_borrow_mut()
                    && let Some(ctx) = guard.as_mut()
                {
                    ctx.matcher.on_menu_mask_seen();
                }
            });
            return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) };
        }
        if kb.dwExtraInfo == RECIRCLES_INJECTED_EXTRA_INFO {
            return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) };
        }

        let msg = wparam as u32;
        let is_down = match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => true,
            WM_KEYUP | WM_SYSKEYUP => false,
            _ => return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) },
        };

        let vk = kb.vkCode;
        let async_mods = read_async_modifiers();
        let was_already_down = if is_down && vk <= 0xFFFF {
            is_async_down(vk as u16)
        } else {
            false
        };
        let now_ms = unsafe { GetTickCount() };

        let mut target_hwnd: HWND = std::ptr::null_mut();
        let mut msg_id = 0u32;
        let mut mask_thread_id = 0u32;
        let decision = GLOBAL_HOOK_CTX.with(|slot| {
            let Ok(mut guard) = slot.try_borrow_mut() else {
                return HookDecision::PassThrough;
            };
            let Some(ctx) = guard.as_mut() else {
                return HookDecision::PassThrough;
            };
            target_hwnd = ctx.target_hwnd;
            msg_id = ctx.msg_id;
            mask_thread_id = ctx.mask_thread_id;
            ctx.matcher.process_key(
                vk,
                is_down,
                async_mods,
                was_already_down,
                now_ms,
                kb.dwExtraInfo,
            )
        });

        // RefCell borrow is now completely released before any PostMessageW / PostThreadMessageW / CallNextHookEx.
        match decision {
            HookDecision::PassThrough => unsafe {
                CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam)
            },
            HookDecision::SwallowOnly => 1,
            HookDecision::Trigger { id, mask_menu } => {
                if mask_menu {
                    post_mask_menu(mask_thread_id);
                }
                if !target_hwnd.is_null() {
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, id as usize, 0);
                    }
                }
                1
            }
            HookDecision::TriggerLoneWin { id, win_vk } => {
                post_mask_release(mask_thread_id, win_vk);
                if !target_hwnd.is_null() {
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, id as usize, 0);
                    }
                }
                1
            }
            HookDecision::MaskModifierRelease { mod_vk } => {
                post_mask_release(mask_thread_id, mod_vk);
                1
            }
        }
    }

    pub struct GlobalHotkeyHook {
        hook_thread_id: u32,
        hook_join: Option<std::thread::JoinHandle<()>>,
        pending_bindings: Arc<Mutex<Option<Vec<HookBindingEntry>>>>,
        // Declared after hook_join so Drop stops the hook thread before stopping the mask thread.
        _mask_thread: MaskThread,
    }

    impl GlobalHotkeyHook {
        pub fn install(
            target_hwnd: HWND,
            msg_id: u32,
            entries: Vec<HookBindingEntry>,
        ) -> Result<Self, String> {
            let matcher = HookMatcher::new(entries);
            if matcher.is_empty() {
                return Err("No hook bindings provided".into());
            }

            let mask_thread =
                MaskThread::spawn().ok_or_else(|| "Failed spawning mask input thread".to_string())?;
            let mask_thread_id = mask_thread.thread_id();

            let pending_bindings: Arc<Mutex<Option<Vec<HookBindingEntry>>>> =
                Arc::new(Mutex::new(None));
            let pending_for_thread = Arc::clone(&pending_bindings);
            let target_raw = target_hwnd as usize;
            let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<u32, String>>(1);

            let hook_join = std::thread::Builder::new()
                .name("recircles-llhook".into())
                .spawn(move || {
                    unsafe {
                        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
                        let mut msg: MSG = std::mem::zeroed();
                        // Ensure message queue exists before exposing our thread ID.
                        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                        let tid = GetCurrentThreadId();

                        GLOBAL_HOOK_CTX.with(|slot| {
                            *slot.borrow_mut() = Some(GlobalHookCtx {
                                target_hwnd: target_raw as HWND,
                                msg_id,
                                mask_thread_id,
                                matcher,
                            });
                        });

                        let hmod = GetModuleHandleW(std::ptr::null());
                        let mut hhook: HHOOK =
                            SetWindowsHookExW(WH_KEYBOARD_LL, Some(global_ll_proc), hmod, 0);
                        if hhook.is_null() {
                            GLOBAL_HOOK_CTX.with(|slot| {
                                *slot.borrow_mut() = None;
                            });
                            let _ = ready_tx
                                .send(Err("SetWindowsHookExW(WH_KEYBOARD_LL) failed".into()));
                            return;
                        }

                        let _ = ready_tx.send(Ok(tid));

                        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                            if msg.hwnd.is_null() {
                                match msg.message {
                                    WM_HOOK_UPDATE_BINDINGS => {
                                        let next = pending_for_thread
                                            .lock()
                                            .ok()
                                            .and_then(|mut g| g.take());
                                        if let Some(entries) = next {
                                            GLOBAL_HOOK_CTX.with(|slot| {
                                                if let Ok(mut guard) = slot.try_borrow_mut()
                                                    && let Some(ctx) = guard.as_mut()
                                                {
                                                    ctx.matcher.update_bindings(entries);
                                                }
                                            });
                                        }
                                        continue;
                                    }
                                    WM_HOOK_RESET_STATE => {
                                        GLOBAL_HOOK_CTX.with(|slot| {
                                            if let Ok(mut guard) = slot.try_borrow_mut()
                                                && let Some(ctx) = guard.as_mut()
                                            {
                                                ctx.matcher.reset_state();
                                            }
                                        });
                                        continue;
                                    }
                                    WM_HOOK_REINSTALL => {
                                        if !hhook.is_null() {
                                            UnhookWindowsHookEx(hhook);
                                        }
                                        GLOBAL_HOOK_CTX.with(|slot| {
                                            if let Ok(mut guard) = slot.try_borrow_mut()
                                                && let Some(ctx) = guard.as_mut()
                                            {
                                                ctx.matcher.reset_state();
                                            }
                                        });
                                        hhook = SetWindowsHookExW(
                                            WH_KEYBOARD_LL,
                                            Some(global_ll_proc),
                                            hmod,
                                            0,
                                        );
                                        continue;
                                    }
                                    WM_HOOK_SHUTDOWN => break,
                                    _ => {}
                                }
                            }
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }

                        if !hhook.is_null() {
                            UnhookWindowsHookEx(hhook);
                        }
                        GLOBAL_HOOK_CTX.with(|slot| {
                            *slot.borrow_mut() = None;
                        });
                    }
                })
                .map_err(|e| format!("Failed spawning low-level hook thread: {e}"))?;

            match ready_rx.recv() {
                Ok(Ok(hook_thread_id)) => Ok(Self {
                    hook_thread_id,
                    hook_join: Some(hook_join),
                    pending_bindings,
                    _mask_thread: mask_thread,
                }),
                Ok(Err(err)) => {
                    let _ = hook_join.join();
                    Err(err)
                }
                Err(_) => {
                    let _ = hook_join.join();
                    Err("Low-level hook thread exited unexpectedly".into())
                }
            }
        }

        /// Swaps the active hook bindings in-place on the hook thread with zero unhook gap.
        pub fn update_bindings(&self, entries: Vec<HookBindingEntry>) {
            if let Ok(mut guard) = self.pending_bindings.lock() {
                *guard = Some(entries);
            }
            if self.hook_thread_id != 0 {
                unsafe {
                    PostThreadMessageW(self.hook_thread_id, WM_HOOK_UPDATE_BINDINGS, 0, 0);
                }
            }
        }

        /// Clears tracked modifier/suppression state (e.g. when locking the desktop or suspending).
        pub fn reset_state(&self) {
            if self.hook_thread_id != 0 {
                unsafe {
                    PostThreadMessageW(self.hook_thread_id, WM_HOOK_RESET_STATE, 0, 0);
                }
            }
        }

        /// Cleanly reinstalls `WH_KEYBOARD_LL` and resets matcher state (e.g. on unlock, resume,
        /// or `TaskbarCreated`).
        pub fn reinstall(&self) {
            if self.hook_thread_id != 0 {
                unsafe {
                    PostThreadMessageW(self.hook_thread_id, WM_HOOK_REINSTALL, 0, 0);
                }
            }
        }
    }

    impl Drop for GlobalHotkeyHook {
        fn drop(&mut self) {
            if self.hook_thread_id != 0 {
                unsafe {
                    PostThreadMessageW(self.hook_thread_id, WM_HOOK_SHUTDOWN, 0, 0);
                }
            }
            if let Some(j) = self.hook_join.take() {
                let _ = j.join();
            }
        }
    }

    // -------------------------------------------------------------------------
    // Short-lived interactive keybind capture WH_KEYBOARD_LL hook (UI & Installer)
    // -------------------------------------------------------------------------

    struct CaptureHookCtx {
        target_hwnd: HWND,
        msg_id: u32,
        mask_thread_id: u32,
        matcher: CaptureMatcher,
    }

    thread_local! {
        static CAPTURE_HOOK_CTX: RefCell<Option<CaptureHookCtx>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn capture_ll_proc(
        ncode: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if ncode != HC_ACTION as i32 || lparam == 0 {
            return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) };
        }

        let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if kb.vkCode == VK_MASK_DUMMY {
            CAPTURE_HOOK_CTX.with(|slot| {
                if let Ok(mut guard) = slot.try_borrow_mut()
                    && let Some(ctx) = guard.as_mut()
                {
                    ctx.matcher.on_menu_mask_seen();
                }
            });
            return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) };
        }
        if kb.dwExtraInfo == RECIRCLES_INJECTED_EXTRA_INFO {
            return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) };
        }

        let msg = wparam as u32;
        let is_down = match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => true,
            WM_KEYUP | WM_SYSKEYUP => false,
            _ => return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) },
        };

        let vk = kb.vkCode;
        let async_mods = read_async_modifiers();
        let was_already_down = if is_down && vk <= 0xFFFF {
            is_async_down(vk as u16)
        } else {
            false
        };
        let now_ms = unsafe { GetTickCount() };

        let mut target_hwnd: HWND = std::ptr::null_mut();
        let mut msg_id = 0u32;
        let mut mask_thread_id = 0u32;
        let decision = CAPTURE_HOOK_CTX.with(|slot| {
            let Ok(mut guard) = slot.try_borrow_mut() else {
                return CaptureDecision::PassThrough;
            };
            let Some(ctx) = guard.as_mut() else {
                return CaptureDecision::PassThrough;
            };
            target_hwnd = ctx.target_hwnd;
            msg_id = ctx.msg_id;
            mask_thread_id = ctx.mask_thread_id;
            ctx.matcher.process_key(
                vk,
                is_down,
                async_mods,
                was_already_down,
                now_ms,
                kb.dwExtraInfo,
            )
        });

        match decision {
            CaptureDecision::PassThrough => unsafe {
                CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam)
            },
            CaptureDecision::SwallowOnly { finished_draining } => {
                if finished_draining && !target_hwnd.is_null() {
                    // Sentinel wparam=0 tells the owning window that key-up drained cleanly.
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, 0, 0);
                    }
                }
                1
            }
            CaptureDecision::Captured {
                vk,
                mods,
                mask_menu,
            } => {
                if mask_menu {
                    post_mask_menu(mask_thread_id);
                }
                if !target_hwnd.is_null() {
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, vk as usize, mods.to_bits() as isize);
                    }
                }
                1
            }
            CaptureDecision::CapturedLoneWin { win_vk } => {
                post_mask_release(mask_thread_id, win_vk);
                if !target_hwnd.is_null() {
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, VK_LWIN as usize, 0);
                    }
                }
                1
            }
            CaptureDecision::MaskLoneWinOnly { win_vk } => {
                post_mask_release(mask_thread_id, win_vk);
                1
            }
            CaptureDecision::MaskModifierRelease { mod_vk } => {
                post_mask_release(mask_thread_id, mod_vk);
                1
            }
        }
    }

    pub struct CaptureHook {
        hhook: HHOOK,
        _mask_thread: MaskThread,
    }

    impl CaptureHook {
        pub fn install(target_hwnd: HWND, msg_id: u32, allow_lone_win: bool) -> Option<Self> {
            let mask_thread = MaskThread::spawn()?;
            let mask_thread_id = mask_thread.thread_id();
            CAPTURE_HOOK_CTX.with(|slot| {
                *slot.borrow_mut() = Some(CaptureHookCtx {
                    target_hwnd,
                    msg_id,
                    mask_thread_id,
                    matcher: CaptureMatcher::new(allow_lone_win),
                });
            });
            let hmod = unsafe { GetModuleHandleW(std::ptr::null()) };
            let hhook =
                unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(capture_ll_proc), hmod, 0) };
            if hhook.is_null() {
                CAPTURE_HOOK_CTX.with(|slot| {
                    *slot.borrow_mut() = None;
                });
                return None;
            }
            Some(Self {
                hhook,
                _mask_thread: mask_thread,
            })
        }

        pub fn set_active(&self, active: bool, allow_lone_win: bool) {
            CAPTURE_HOOK_CTX.with(|slot| {
                if let Ok(mut guard) = slot.try_borrow_mut()
                    && let Some(ctx) = guard.as_mut()
                {
                    ctx.matcher.set_active(active, allow_lone_win);
                }
            });
        }

        pub fn has_suppressed(&self) -> bool {
            CAPTURE_HOOK_CTX.with(|slot| {
                slot.try_borrow()
                    .ok()
                    .and_then(|g| g.as_ref().map(|ctx| ctx.matcher.has_suppressed()))
                    .unwrap_or(false)
            })
        }
    }

    impl Drop for CaptureHook {
        fn drop(&mut self) {
            if !self.hhook.is_null() {
                unsafe {
                    UnhookWindowsHookEx(self.hhook);
                }
                self.hhook = std::ptr::null_mut();
            }
            CAPTURE_HOOK_CTX.with(|slot| {
                *slot.borrow_mut() = None;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win_keys::{VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RWIN};

    fn win_only() -> Modifiers {
        Modifiers {
            win: true,
            ..Default::default()
        }
    }

    #[test]
    fn hook_matcher_triggers_win_e_suppresses_repeat_and_handles_rollover() {
        let mut m = HookMatcher::new(vec![HookBindingEntry {
            id: 7,
            mods: win_only(),
            vk: b'E' as u32,
            lone_win: false,
        }]);

        // LWin down passes through
        assert_eq!(
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 1000, 0),
            HookDecision::PassThrough
        );
        // E down while Win held triggers with menu mask
        assert_eq!(
            m.process_key(b'E' as u32, true, win_only(), false, 1020, 0),
            HookDecision::Trigger {
                id: 7,
                mask_menu: true
            }
        );
        // Async mask dummy key passes through and clears pending Win mask
        assert_eq!(
            m.process_key(VK_MASK_DUMMY, true, win_only(), false, 1021, RECIRCLES_INJECTED_EXTRA_INFO),
            HookDecision::PassThrough
        );
        // Key repeat while holding Win+E is swallowed even when GetAsyncKeyState('E') is false
        // (because E down was swallowed and never marked down in OS async key state).
        assert_eq!(
            m.process_key(b'E' as u32, true, win_only(), false, 1400, 0),
            HookDecision::SwallowOnly
        );
        // Fast rollover: user releases LWin before E; E repeat is still swallowed
        assert_eq!(
            m.process_key(VK_LWIN, false, Modifiers::default(), false, 1420, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'E' as u32, true, Modifiers::default(), false, 1450, 0),
            HookDecision::SwallowOnly
        );
        // E key-up is swallowed and clears suppression
        assert_eq!(
            m.process_key(b'E' as u32, false, Modifiers::default(), false, 1480, 0),
            HookDecision::SwallowOnly
        );
        // Plain E typing afterwards passes through cleanly
        assert_eq!(
            m.process_key(b'E' as u32, true, Modifiers::default(), false, 1520, 0),
            HookDecision::PassThrough
        );
    }

    #[test]
    fn hook_matcher_triggers_win_e_when_get_async_key_state_lags_or_returns_zero_on_desktop() {
        let mut m = HookMatcher::new(vec![HookBindingEntry {
            id: 7,
            mods: win_only(),
            vk: b'E' as u32,
            lone_win: false,
        }]);

        // Desktop scenario: LWin down is delivered to WH_KEYBOARD_LL, then E down arrives while
        // GetAsyncKeyState() returns 0 (all modifiers false).
        assert_eq!(
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 5000, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'E' as u32, true, Modifiers::default(), false, 5040, 0),
            HookDecision::Trigger {
                id: 7,
                mask_menu: true
            }
        );
        assert_eq!(
            m.process_key(b'E' as u32, false, Modifiers::default(), false, 5090, 0),
            HookDecision::SwallowOnly
        );
        // Simulate mask already seen before LWin release
        m.on_menu_mask_seen();
        assert_eq!(
            m.process_key(VK_LWIN, false, Modifiers::default(), false, 5110, 0),
            HookDecision::PassThrough
        );
    }

    #[test]
    fn hook_matcher_ignores_stale_async_alt_ctrl_right_after_closing_last_window() {
        let mut m = HookMatcher::new(vec![HookBindingEntry {
            id: 7,
            mods: win_only(),
            vk: b'E' as u32,
            lone_win: false,
        }]);

        // User closes last window with Alt+F4 -> LAlt goes down then up
        assert_eq!(
            m.process_key(VK_LMENU, true, Modifiers::default(), false, 2000, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_LMENU, false, Modifiers::default(), false, 2080, 0),
            HookDecision::PassThrough
        );

        // Immediately presses Win+E on the empty desktop while GetAsyncKeyState still lags Alt=true
        assert_eq!(
            m.process_key(
                VK_LWIN,
                true,
                Modifiers {
                    alt: true,
                    ..Default::default()
                },
                false,
                2100,
                0
            ),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(
                b'E' as u32,
                true,
                Modifiers {
                    win: true,
                    alt: true,
                    ..Default::default()
                },
                false,
                2130,
                0
            ),
            HookDecision::Trigger {
                id: 7,
                mask_menu: true
            }
        );
    }

    #[test]
    fn hook_matcher_masks_ultra_fast_win_release_before_async_mask_arrives() {
        let mut m = HookMatcher::new(vec![HookBindingEntry {
            id: 7,
            mods: win_only(),
            vk: b'E' as u32,
            lone_win: false,
        }]);

        assert_eq!(
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 100, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'E' as u32, true, win_only(), false, 110, 0),
            HookDecision::Trigger {
                id: 7,
                mask_menu: true
            }
        );
        // LWin released BEFORE VK_MASK_DUMMY passed through -> must swallow & atomically mask!
        assert_eq!(
            m.process_key(VK_LWIN, false, Modifiers::default(), false, 112, 0),
            HookDecision::MaskModifierRelease { mod_vk: VK_LWIN }
        );
        // Subsequent injected LWin up passes through untouched
        assert_eq!(
            m.process_key(
                VK_LWIN,
                false,
                Modifiers::default(),
                false,
                113,
                RECIRCLES_INJECTED_EXTRA_INFO
            ),
            HookDecision::PassThrough
        );
    }

    #[test]
    fn hook_matcher_self_heals_stale_modifier_missed_across_uac_or_lock() {
        let mut m = HookMatcher::new(vec![HookBindingEntry {
            id: 7,
            mods: win_only(),
            vk: b'E' as u32,
            lone_win: false,
        }]);

        // LWin went down before UAC/lock; KEYUP was missed on Secure Desktop
        assert_eq!(
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 10_000, 0),
            HookDecision::PassThrough
        );

        // 3 seconds later back on normal desktop, GetAsyncKeyState reports all up, user types 'e'
        assert_eq!(
            m.process_key(b'E' as u32, true, Modifiers::default(), false, 13_000, 0),
            HookDecision::PassThrough
        );
    }

    #[test]
    fn hook_matcher_reset_and_update_bindings_behave_cleanly() {
        let mut m = HookMatcher::new(vec![HookBindingEntry {
            id: 1,
            mods: win_only(),
            vk: b'E' as u32,
            lone_win: false,
        }]);

        // Hold LWin, then reload bindings in-place: held LWin must stay recognized!
        assert_eq!(
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 1000, 0),
            HookDecision::PassThrough
        );
        m.update_bindings(vec![HookBindingEntry {
            id: 9,
            mods: win_only(),
            vk: b'E' as u32,
            lone_win: false,
        }]);
        assert_eq!(
            m.process_key(b'E' as u32, true, Modifiers::default(), false, 1050, 0),
            HookDecision::Trigger {
                id: 9,
                mask_menu: true
            }
        );

        // Explicit reset_state (on lock/unlock/resume) clears held modifiers & suppressed keys
        m.reset_state();
        assert_eq!(
            m.process_key(b'E' as u32, true, Modifiers::default(), false, 1100, 0),
            HookDecision::PassThrough
        );
    }

    #[test]
    fn hook_matcher_enforces_exact_modifiers_and_ignores_injected() {
        let mut m = HookMatcher::new(vec![
            HookBindingEntry {
                id: 1,
                mods: win_only(),
                vk: b'E' as u32,
                lone_win: false,
            },
            HookBindingEntry {
                id: 2,
                mods: win_only(),
                vk: b'B' as u32,
                lone_win: false,
            },
        ]);

        // Unbound Win+R passes through
        assert_eq!(
            m.process_key(b'R' as u32, true, win_only(), false, 100, 0),
            HookDecision::PassThrough
        );
        // Win+Shift+E does not trigger Win+E
        assert_eq!(
            m.process_key(
                b'E' as u32,
                true,
                Modifiers {
                    win: true,
                    shift: true,
                    ..Default::default()
                },
                false,
                120,
                0
            ),
            HookDecision::PassThrough
        );
        // Self-injected events and dummy mask VK pass through untouched
        assert_eq!(
            m.process_key(
                b'E' as u32,
                true,
                win_only(),
                false,
                130,
                RECIRCLES_INJECTED_EXTRA_INFO
            ),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_MASK_DUMMY, true, win_only(), false, 140, 0),
            HookDecision::PassThrough
        );
        // Win+B triggers id 2
        assert_eq!(
            m.process_key(b'B' as u32, true, win_only(), false, 150, 0),
            HookDecision::Trigger {
                id: 2,
                mask_menu: true
            }
        );
    }

    #[test]
    fn hook_matcher_supports_lone_win_coexisting_with_win_chords() {
        let mut m = HookMatcher::new(vec![
            HookBindingEntry {
                id: 1,
                mods: Modifiers::default(),
                vk: VK_LWIN,
                lone_win: true,
            },
            HookBindingEntry {
                id: 2,
                mods: win_only(),
                vk: b'E' as u32,
                lone_win: false,
            },
        ]);

        // Case 1: Tap LWin alone (with a hold repeat) -> triggers lone Win on release
        assert_eq!(
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 1000, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_LWIN, true, win_only(), true, 1300, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_LWIN, false, Modifiers::default(), false, 1350, 0),
            HookDecision::TriggerLoneWin {
                id: 1,
                win_vk: VK_LWIN
            }
        );

        // Case 2: Press Win+E -> triggers id 2, does NOT trigger lone Win on release
        assert_eq!(
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 2000, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'E' as u32, true, win_only(), false, 2020, 0),
            HookDecision::Trigger {
                id: 2,
                mask_menu: true
            }
        );
        m.on_menu_mask_seen();
        assert_eq!(
            m.process_key(b'E' as u32, false, win_only(), false, 2060, 0),
            HookDecision::SwallowOnly
        );
        assert_eq!(
            m.process_key(VK_LWIN, false, Modifiers::default(), false, 2080, 0),
            HookDecision::PassThrough
        );

        // Case 3: Press unbound Win+R -> passes through and does NOT trigger lone Win
        assert_eq!(
            m.process_key(VK_RWIN, true, Modifiers::default(), false, 3000, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'R' as u32, true, win_only(), false, 3020, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'R' as u32, false, win_only(), false, 3050, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_RWIN, false, Modifiers::default(), false, 3080, 0),
            HookDecision::PassThrough
        );

        // Case 4: Press Ctrl+Win -> Ctrl cancels lone Win tap
        assert_eq!(
            m.process_key(VK_LCONTROL, true, Modifiers::default(), false, 4000, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(
                VK_LWIN,
                true,
                Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                false,
                4020,
                0
            ),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(
                VK_LWIN,
                false,
                Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                false,
                4050,
                0
            ),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_LCONTROL, false, Modifiers::default(), false, 4070, 0),
            HookDecision::PassThrough
        );
    }

    #[test]
    fn capture_matcher_captures_chords_lone_win_and_drains_cleanly() {
        // Global hotkey capture (allows lone Win)
        let mut c = CaptureMatcher::new(true);

        // Press Win+E (even when async_mods lags false)
        assert_eq!(
            c.process_key(VK_LWIN, true, Modifiers::default(), false, 1000, 0),
            CaptureDecision::PassThrough
        );
        assert_eq!(
            c.process_key(b'E' as u32, true, Modifiers::default(), false, 1020, 0),
            CaptureDecision::Captured {
                vk: b'E' as u32,
                mods: win_only(),
                mask_menu: true,
            }
        );
        assert!(c.has_suppressed());

        // Transition to draining after UI accepts Win+e
        c.set_active(false, true);
        assert_eq!(
            c.process_key(b'E' as u32, true, win_only(), false, 1300, 0),
            CaptureDecision::SwallowOnly {
                finished_draining: false
            }
        );
        assert_eq!(
            c.process_key(b'E' as u32, false, win_only(), false, 1350, 0),
            CaptureDecision::SwallowOnly {
                finished_draining: true
            }
        );
        assert!(!c.has_suppressed());

        // Lone Win tap when allowed
        let mut c2 = CaptureMatcher::new(true);
        assert_eq!(
            c2.process_key(VK_LWIN, true, Modifiers::default(), false, 2000, 0),
            CaptureDecision::PassThrough
        );
        assert_eq!(
            c2.process_key(VK_LWIN, false, Modifiers::default(), false, 2050, 0),
            CaptureDecision::CapturedLoneWin { win_vk: VK_LWIN }
        );

        // Lone Win tap during Nav capture (not allowed) still masks Start Menu
        let mut c3 = CaptureMatcher::new(false);
        assert_eq!(
            c3.process_key(VK_LWIN, true, Modifiers::default(), false, 3000, 0),
            CaptureDecision::PassThrough
        );
        assert_eq!(
            c3.process_key(VK_LWIN, false, Modifiers::default(), false, 3050, 0),
            CaptureDecision::MaskLoneWinOnly { win_vk: VK_LWIN }
        );

        // Shift+Esc cancels cleanly
        assert_eq!(
            c3.process_key(VK_LSHIFT, true, Modifiers::default(), false, 4000, 0),
            CaptureDecision::PassThrough
        );
        assert_eq!(
            c3.process_key(
                VK_ESCAPE,
                true,
                Modifiers {
                    shift: true,
                    ..Default::default()
                },
                false,
                4020,
                0
            ),
            CaptureDecision::Captured {
                vk: VK_ESCAPE,
                mods: Modifiers {
                    shift: true,
                    ..Default::default()
                },
                mask_menu: false,
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn global_hotkey_hook_installs_updates_reinstalls_and_drops_cleanly() {
        let hook = win::GlobalHotkeyHook::install(
            std::ptr::null_mut(),
            0x8000 + 105,
            vec![HookBindingEntry {
                id: 1,
                mods: win_only(),
                vk: b'E' as u32,
                lone_win: false,
            }],
        )
        .expect("install dedicated WH_KEYBOARD_LL hook thread");

        hook.update_bindings(vec![
            HookBindingEntry {
                id: 1,
                mods: win_only(),
                vk: b'E' as u32,
                lone_win: false,
            },
            HookBindingEntry {
                id: 2,
                mods: win_only(),
                vk: b'V' as u32,
                lone_win: false,
            },
        ]);
        hook.reset_state();
        hook.reinstall();
        drop(hook);
    }
}
