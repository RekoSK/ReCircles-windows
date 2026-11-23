use crate::win_keys::{Modifiers, VK_LWIN, VK_RWIN, is_modifier_vk};

/// Distinct `dwExtraInfo` tag stamped on synthetic `SendInput` events emitted by ReCircles
/// so our `WH_KEYBOARD_LL` callbacks immediately pass them through without re-entering state.
pub const RECIRCLES_INJECTED_EXTRA_INFO: usize = 0x5243_484B; // "RCHK"

/// Unassigned virtual key code (`0xE8`, same default as AutoHotkey's `#MenuMaskKey`) used to
/// break Windows' lone-Win Start Menu / lone-Alt menu bar tracker without side effects.
pub const VK_MASK_DUMMY: u32 = 0xE8;

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
}

/// Pure, allocation-free state machine for `recircles-daemon`'s `WH_KEYBOARD_LL` hook.
#[derive(Debug, Clone)]
pub struct HookMatcher {
    chords: Vec<HookBindingEntry>,
    lone_win_id: Option<i32>,
    suppressed_vks: [bool; 256],
    pending_lone_win_vk: Option<u32>,
}

impl HookMatcher {
    pub fn new(entries: Vec<HookBindingEntry>) -> Self {
        let mut chords = Vec::new();
        let mut lone_win_id = None;
        for entry in entries {
            if entry.lone_win {
                lone_win_id = Some(entry.id);
            } else {
                chords.push(entry);
            }
        }
        Self {
            chords,
            lone_win_id,
            suppressed_vks: [false; 256],
            pending_lone_win_vk: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.chords.is_empty() && self.lone_win_id.is_none()
    }

    pub fn process_key(
        &mut self,
        vk: u32,
        is_down: bool,
        mods: Modifiers,
        was_already_down: bool,
        extra_info: usize,
    ) -> HookDecision {
        if extra_info == RECIRCLES_INJECTED_EXTRA_INFO || vk == VK_MASK_DUMMY {
            return HookDecision::PassThrough;
        }

        if vk == VK_LWIN || vk == VK_RWIN {
            if is_down {
                if self.lone_win_id.is_some() {
                    if self.pending_lone_win_vk == Some(vk) {
                        // Auto-repeat while holding the same Win key: keep candidate active.
                    } else if self.pending_lone_win_vk.is_none()
                        && !mods.ctrl
                        && !mods.alt
                        && !mods.shift
                    {
                        self.pending_lone_win_vk = Some(vk);
                    } else {
                        self.pending_lone_win_vk = None;
                    }
                }
                return HookDecision::PassThrough;
            } else {
                if self.pending_lone_win_vk == Some(vk) {
                    self.pending_lone_win_vk = None;
                    if let Some(id) = self.lone_win_id {
                        return HookDecision::TriggerLoneWin { id, win_vk: vk };
                    }
                } else {
                    self.pending_lone_win_vk = None;
                }
                return HookDecision::PassThrough;
            }
        }

        if is_modifier_vk(vk) {
            if is_down {
                self.pending_lone_win_vk = None;
            }
            return HookDecision::PassThrough;
        }

        let idx = vk as usize;
        if is_down {
            self.pending_lone_win_vk = None;

            if idx < self.suppressed_vks.len() && self.suppressed_vks[idx] {
                if was_already_down {
                    return HookDecision::SwallowOnly;
                }
                // Self-heal if a prior KEYUP was missed across UAC / lock screen.
                self.suppressed_vks[idx] = false;
            }

            if let Some(entry) = self
                .chords
                .iter()
                .find(|e| e.vk == vk && e.mods == mods)
            {
                if idx < self.suppressed_vks.len() {
                    self.suppressed_vks[idx] = true;
                }
                return HookDecision::Trigger {
                    id: entry.id,
                    mask_menu: mods.win || mods.alt,
                };
            }

            HookDecision::PassThrough
        } else {
            if idx < self.suppressed_vks.len() && self.suppressed_vks[idx] {
                self.suppressed_vks[idx] = false;
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
}

/// Pure state machine for temporary interactive keybind capture in `recircles-ui` and `recircles-installer`.
#[derive(Debug, Clone)]
pub struct CaptureMatcher {
    allow_lone_win: bool,
    active: bool,
    suppressed_vks: [bool; 256],
    pending_lone_win_vk: Option<u32>,
}

impl CaptureMatcher {
    pub fn new(allow_lone_win: bool) -> Self {
        Self {
            allow_lone_win,
            active: true,
            suppressed_vks: [false; 256],
            pending_lone_win_vk: None,
        }
    }

    pub fn set_active(&mut self, active: bool, allow_lone_win: bool) {
        self.active = active;
        self.allow_lone_win = allow_lone_win;
        if !active {
            self.pending_lone_win_vk = None;
        }
    }

    pub fn has_suppressed(&self) -> bool {
        self.suppressed_vks.iter().any(|&v| v)
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn process_key(
        &mut self,
        vk: u32,
        is_down: bool,
        mods: Modifiers,
        was_already_down: bool,
        extra_info: usize,
    ) -> CaptureDecision {
        if extra_info == RECIRCLES_INJECTED_EXTRA_INFO || vk == VK_MASK_DUMMY {
            return CaptureDecision::PassThrough;
        }

        let idx = vk as usize;

        if !self.active {
            if is_down {
                if idx < self.suppressed_vks.len() && self.suppressed_vks[idx] {
                    if was_already_down {
                        return CaptureDecision::SwallowOnly {
                            finished_draining: false,
                        };
                    }
                    self.suppressed_vks[idx] = false;
                }
                return CaptureDecision::PassThrough;
            } else {
                if idx < self.suppressed_vks.len() && self.suppressed_vks[idx] {
                    self.suppressed_vks[idx] = false;
                    return CaptureDecision::SwallowOnly {
                        finished_draining: !self.has_suppressed(),
                    };
                }
                return CaptureDecision::PassThrough;
            }
        }

        if vk == VK_LWIN || vk == VK_RWIN {
            if is_down {
                if self.pending_lone_win_vk == Some(vk) {
                    // Auto-repeat while holding Win: keep candidate.
                } else if self.pending_lone_win_vk.is_none()
                    && !mods.ctrl
                    && !mods.alt
                    && !mods.shift
                {
                    self.pending_lone_win_vk = Some(vk);
                } else {
                    self.pending_lone_win_vk = None;
                }
                return CaptureDecision::PassThrough;
            } else {
                if self.pending_lone_win_vk == Some(vk) {
                    self.pending_lone_win_vk = None;
                    if self.allow_lone_win {
                        return CaptureDecision::CapturedLoneWin { win_vk: vk };
                    }
                    return CaptureDecision::MaskLoneWinOnly { win_vk: vk };
                }
                self.pending_lone_win_vk = None;
                return CaptureDecision::PassThrough;
            }
        }

        if is_modifier_vk(vk) {
            if is_down {
                self.pending_lone_win_vk = None;
            }
            return CaptureDecision::PassThrough;
        }

        if is_down {
            self.pending_lone_win_vk = None;
            if idx < self.suppressed_vks.len() && self.suppressed_vks[idx] {
                if was_already_down {
                    return CaptureDecision::SwallowOnly {
                        finished_draining: false,
                    };
                }
                self.suppressed_vks[idx] = false;
            }
            if idx < self.suppressed_vks.len() {
                self.suppressed_vks[idx] = true;
            }
            CaptureDecision::Captured {
                vk,
                mods,
                mask_menu: mods.win || mods.alt,
            }
        } else {
            if idx < self.suppressed_vks.len() && self.suppressed_vks[idx] {
                self.suppressed_vks[idx] = false;
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
    use crate::win_keys::{Modifiers, VK_LWIN};
    use std::cell::RefCell;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, SendInput, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN as WIN_VK_LWIN,
        VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN as WIN_VK_RWIN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, PostMessageW, SetWindowsHookExW,
        UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    fn is_async_down(vk: u16) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    pub fn read_async_modifiers() -> Modifiers {
        Modifiers {
            ctrl: is_async_down(VK_LCONTROL) || is_async_down(VK_RCONTROL),
            alt: is_async_down(VK_LMENU) || is_async_down(VK_RMENU),
            shift: is_async_down(VK_LSHIFT) || is_async_down(VK_RSHIFT),
            win: is_async_down(WIN_VK_LWIN) || is_async_down(WIN_VK_RWIN),
        }
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

    /// Atomically injects `[0xE8 down, 0xE8 up, win_vk up]` after swallowing a hardware lone-Win
    /// release, guaranteeing `0xE8` precedes the Win release so Start Menu never opens and the
    /// OS never sees a stuck Win key.
    pub fn send_masked_win_release(win_vk: u32) {
        let inputs = [
            kbd_input(VK_MASK_DUMMY as u16, 0),
            kbd_input(VK_MASK_DUMMY as u16, KEYEVENTF_KEYUP),
            kbd_input(win_vk as u16, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
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
    // Global daemon WH_KEYBOARD_LL hook
    // -------------------------------------------------------------------------

    struct GlobalHookCtx {
        target_hwnd: HWND,
        msg_id: u32,
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
        if kb.dwExtraInfo == RECIRCLES_INJECTED_EXTRA_INFO || kb.vkCode == VK_MASK_DUMMY {
            return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) };
        }

        let msg = wparam as u32;
        let is_down = match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => true,
            WM_KEYUP | WM_SYSKEYUP => false,
            _ => return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) },
        };

        let vk = kb.vkCode;
        let mods = read_async_modifiers();
        let was_already_down = if is_down && vk <= 0xFFFF {
            is_async_down(vk as u16)
        } else {
            false
        };

        let mut target_hwnd: HWND = std::ptr::null_mut();
        let mut msg_id = 0u32;
        let decision = GLOBAL_HOOK_CTX.with(|slot| {
            let Ok(mut guard) = slot.try_borrow_mut() else {
                return HookDecision::PassThrough;
            };
            let Some(ctx) = guard.as_mut() else {
                return HookDecision::PassThrough;
            };
            target_hwnd = ctx.target_hwnd;
            msg_id = ctx.msg_id;
            ctx.matcher
                .process_key(vk, is_down, mods, was_already_down, kb.dwExtraInfo)
        });

        // RefCell borrow is now completely released before any SendInput / PostMessageW / CallNextHookEx.
        match decision {
            HookDecision::PassThrough => unsafe {
                CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam)
            },
            HookDecision::SwallowOnly => 1,
            HookDecision::Trigger { id, mask_menu } => {
                if mask_menu {
                    send_menu_mask_key();
                }
                if !target_hwnd.is_null() {
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, id as usize, 0);
                    }
                }
                1
            }
            HookDecision::TriggerLoneWin { id, win_vk } => {
                send_masked_win_release(win_vk);
                if !target_hwnd.is_null() {
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, id as usize, 0);
                    }
                }
                1
            }
        }
    }

    pub struct GlobalHotkeyHook {
        hhook: HHOOK,
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
            GLOBAL_HOOK_CTX.with(|slot| {
                *slot.borrow_mut() = Some(GlobalHookCtx {
                    target_hwnd,
                    msg_id,
                    matcher,
                });
            });
            let hmod = unsafe { GetModuleHandleW(std::ptr::null()) };
            let hhook =
                unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(global_ll_proc), hmod, 0) };
            if hhook.is_null() {
                GLOBAL_HOOK_CTX.with(|slot| {
                    *slot.borrow_mut() = None;
                });
                return Err("SetWindowsHookExW(WH_KEYBOARD_LL) failed".into());
            }
            Ok(Self { hhook })
        }
    }

    impl Drop for GlobalHotkeyHook {
        fn drop(&mut self) {
            if !self.hhook.is_null() {
                unsafe {
                    UnhookWindowsHookEx(self.hhook);
                }
                self.hhook = std::ptr::null_mut();
            }
            GLOBAL_HOOK_CTX.with(|slot| {
                *slot.borrow_mut() = None;
            });
        }
    }

    // -------------------------------------------------------------------------
    // Short-lived interactive keybind capture WH_KEYBOARD_LL hook (UI & Installer)
    // -------------------------------------------------------------------------

    struct CaptureHookCtx {
        target_hwnd: HWND,
        msg_id: u32,
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
        if kb.dwExtraInfo == RECIRCLES_INJECTED_EXTRA_INFO || kb.vkCode == VK_MASK_DUMMY {
            return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) };
        }

        let msg = wparam as u32;
        let is_down = match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => true,
            WM_KEYUP | WM_SYSKEYUP => false,
            _ => return unsafe { CallNextHookEx(std::ptr::null_mut(), ncode, wparam, lparam) },
        };

        let vk = kb.vkCode;
        let mods = read_async_modifiers();
        let was_already_down = if is_down && vk <= 0xFFFF {
            is_async_down(vk as u16)
        } else {
            false
        };

        let mut target_hwnd: HWND = std::ptr::null_mut();
        let mut msg_id = 0u32;
        let decision = CAPTURE_HOOK_CTX.with(|slot| {
            let Ok(mut guard) = slot.try_borrow_mut() else {
                return CaptureDecision::PassThrough;
            };
            let Some(ctx) = guard.as_mut() else {
                return CaptureDecision::PassThrough;
            };
            target_hwnd = ctx.target_hwnd;
            msg_id = ctx.msg_id;
            ctx.matcher
                .process_key(vk, is_down, mods, was_already_down, kb.dwExtraInfo)
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
                    send_menu_mask_key();
                }
                if !target_hwnd.is_null() {
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, vk as usize, mods.to_bits() as isize);
                    }
                }
                1
            }
            CaptureDecision::CapturedLoneWin { win_vk } => {
                send_masked_win_release(win_vk);
                if !target_hwnd.is_null() {
                    unsafe {
                        PostMessageW(target_hwnd, msg_id, VK_LWIN as usize, 0);
                    }
                }
                1
            }
            CaptureDecision::MaskLoneWinOnly { win_vk } => {
                send_masked_win_release(win_vk);
                1
            }
        }
    }

    pub struct CaptureHook {
        hhook: HHOOK,
    }

    impl CaptureHook {
        pub fn install(target_hwnd: HWND, msg_id: u32, allow_lone_win: bool) -> Option<Self> {
            CAPTURE_HOOK_CTX.with(|slot| {
                *slot.borrow_mut() = Some(CaptureHookCtx {
                    target_hwnd,
                    msg_id,
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
            Some(Self { hhook })
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
    use crate::win_keys::{VK_ESCAPE, VK_LCONTROL, VK_LSHIFT, VK_RWIN};

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
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 0),
            HookDecision::PassThrough
        );
        // E down while Win held triggers with menu mask
        assert_eq!(
            m.process_key(b'E' as u32, true, win_only(), false, 0),
            HookDecision::Trigger {
                id: 7,
                mask_menu: true
            }
        );
        // Key repeat while holding Win+E is swallowed without re-triggering
        assert_eq!(
            m.process_key(b'E' as u32, true, win_only(), true, 0),
            HookDecision::SwallowOnly
        );
        // Fast rollover: user releases LWin before E; E repeat is still swallowed
        assert_eq!(
            m.process_key(VK_LWIN, false, Modifiers::default(), false, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'E' as u32, true, Modifiers::default(), true, 0),
            HookDecision::SwallowOnly
        );
        // E key-up is swallowed and clears suppression
        assert_eq!(
            m.process_key(b'E' as u32, false, Modifiers::default(), false, 0),
            HookDecision::SwallowOnly
        );
        // Plain E typing afterwards passes through cleanly
        assert_eq!(
            m.process_key(b'E' as u32, true, Modifiers::default(), false, 0),
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
            m.process_key(b'R' as u32, true, win_only(), false, 0),
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
                RECIRCLES_INJECTED_EXTRA_INFO
            ),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_MASK_DUMMY, true, win_only(), false, 0),
            HookDecision::PassThrough
        );
        // Win+B triggers id 2
        assert_eq!(
            m.process_key(b'B' as u32, true, win_only(), false, 0),
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
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_LWIN, true, win_only(), true, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_LWIN, false, Modifiers::default(), false, 0),
            HookDecision::TriggerLoneWin {
                id: 1,
                win_vk: VK_LWIN
            }
        );

        // Case 2: Press Win+E -> triggers id 2, does NOT trigger lone Win on release
        assert_eq!(
            m.process_key(VK_LWIN, true, Modifiers::default(), false, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'E' as u32, true, win_only(), false, 0),
            HookDecision::Trigger {
                id: 2,
                mask_menu: true
            }
        );
        assert_eq!(
            m.process_key(b'E' as u32, false, win_only(), false, 0),
            HookDecision::SwallowOnly
        );
        assert_eq!(
            m.process_key(VK_LWIN, false, Modifiers::default(), false, 0),
            HookDecision::PassThrough
        );

        // Case 3: Press unbound Win+R -> passes through and does NOT trigger lone Win
        assert_eq!(
            m.process_key(VK_RWIN, true, Modifiers::default(), false, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'R' as u32, true, win_only(), false, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(b'R' as u32, false, win_only(), false, 0),
            HookDecision::PassThrough
        );
        assert_eq!(
            m.process_key(VK_RWIN, false, Modifiers::default(), false, 0),
            HookDecision::PassThrough
        );

        // Case 4: Press Ctrl+Win -> Ctrl cancels lone Win tap
        assert_eq!(
            m.process_key(VK_LCONTROL, true, Modifiers::default(), false, 0),
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
                0
            ),
            HookDecision::PassThrough
        );
    }

    #[test]
    fn capture_matcher_captures_chords_lone_win_and_drains_cleanly() {
        // Global hotkey capture (allows lone Win)
        let mut c = CaptureMatcher::new(true);

        // Press Win+E
        assert_eq!(
            c.process_key(VK_LWIN, true, Modifiers::default(), false, 0),
            CaptureDecision::PassThrough
        );
        assert_eq!(
            c.process_key(b'E' as u32, true, win_only(), false, 0),
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
            c.process_key(b'E' as u32, true, win_only(), true, 0),
            CaptureDecision::SwallowOnly {
                finished_draining: false
            }
        );
        assert_eq!(
            c.process_key(b'E' as u32, false, win_only(), false, 0),
            CaptureDecision::SwallowOnly {
                finished_draining: true
            }
        );
        assert!(!c.has_suppressed());

        // Lone Win tap when allowed
        let mut c2 = CaptureMatcher::new(true);
        assert_eq!(
            c2.process_key(VK_LWIN, true, Modifiers::default(), false, 0),
            CaptureDecision::PassThrough
        );
        assert_eq!(
            c2.process_key(VK_LWIN, false, Modifiers::default(), false, 0),
            CaptureDecision::CapturedLoneWin { win_vk: VK_LWIN }
        );

        // Lone Win tap during Nav capture (not allowed) still masks Start Menu
        let mut c3 = CaptureMatcher::new(false);
        assert_eq!(
            c3.process_key(VK_LWIN, true, Modifiers::default(), false, 0),
            CaptureDecision::PassThrough
        );
        assert_eq!(
            c3.process_key(VK_LWIN, false, Modifiers::default(), false, 0),
            CaptureDecision::MaskLoneWinOnly { win_vk: VK_LWIN }
        );

        // Shift+Esc cancels cleanly
        assert_eq!(
            c3.process_key(VK_LSHIFT, true, Modifiers::default(), false, 0),
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
}
