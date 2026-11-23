#[cfg(windows)]
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[cfg(windows)]
use std::thread::JoinHandle;
#[cfg(windows)]
use windows_sys::Win32::Foundation::HWND;
#[cfg(windows)]
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_TICK: u32 = 0x8000 + 220;
pub const WM_APP_INSTALL_DONE: u32 = 0x8000 + 221;

#[cfg(windows)]
struct PacerState {
    animating: bool,
    stop: bool,
}

#[cfg(windows)]
pub struct VsyncPacer {
    shared: Arc<(Mutex<PacerState>, Condvar)>,
    tick_pending: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl VsyncPacer {
    pub fn start(hwnd: HWND) -> Self {
        let hwnd_raw = hwnd as usize;
        let shared = Arc::new((
            Mutex::new(PacerState {
                animating: true,
                stop: false,
            }),
            Condvar::new(),
        ));
        let tick_pending = Arc::new(AtomicBool::new(false));
        let bg = shared.clone();
        let tp = tick_pending.clone();
        let handle = std::thread::spawn(move || {
            let hwnd = hwnd_raw as HWND;
            loop {
                {
                    let (lock, cv) = &*bg;
                    let mut st = lock.lock().unwrap();
                    while !st.animating && !st.stop {
                        st = cv.wait(st).unwrap();
                    }
                    if st.stop {
                        break;
                    }
                }
                let hr = unsafe { DwmFlush() };
                if hr != 0 {
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
                {
                    let (lock, _) = &*bg;
                    if let Ok(st) = lock.lock()
                        && (st.stop || !st.animating)
                    {
                        continue;
                    }
                }
                if !tp.swap(true, Ordering::AcqRel) {
                    let ok = unsafe { PostMessageW(hwnd, WM_APP_TICK, 0, 0) };
                    if ok == 0 {
                        break;
                    }
                }
            }
        });
        Self {
            shared,
            tick_pending,
            handle: Some(handle),
        }
    }

    pub fn ack_tick(&self) {
        self.tick_pending.store(false, Ordering::Release);
    }

    pub fn set_animating(&self, animating: bool) {
        let (lock, cv) = &*self.shared;
        if let Ok(mut st) = lock.lock()
            && st.animating != animating
        {
            st.animating = animating;
            if animating {
                cv.notify_one();
            }
        }
    }

    pub fn stop(&mut self) {
        {
            let (lock, cv) = &*self.shared;
            if let Ok(mut st) = lock.lock() {
                st.stop = true;
                st.animating = false;
                cv.notify_all();
            }
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(windows)]
impl Drop for VsyncPacer {
    fn drop(&mut self) {
        self.stop();
    }
}
