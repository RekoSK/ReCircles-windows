#[cfg(windows)]
use crate::d2d::ShellIconWorker;
#[cfg(windows)]
use recircles_core::ipc::win::to_wide_nul;
#[cfg(windows)]
use std::collections::{HashSet, VecDeque};
#[cfg(windows)]
use std::fs;
#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};
#[cfg(windows)]
use std::time::UNIX_EPOCH;
#[cfg(windows)]
use windows_sys::Win32::Foundation::HWND;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_ICON_READY: u32 = 0x8000 + 211;

#[cfg(windows)]
const CACHE_MAGIC: [u8; 4] = *b"RCIC";
#[cfg(windows)]
const CACHE_VERSION: u32 = 1;
#[cfg(windows)]
const CACHE_HEADER_LEN: usize = 36;
#[cfg(windows)]
const MAX_LOW_QUEUE: usize = 64;
#[cfg(windows)]
const MAX_HIGH_QUEUE: usize = 64;

#[cfg(windows)]
pub struct DecodedShellIcon {
    pub path: String,
    pub desired_px: i32,
    pub bitmap: Option<(u32, u32, Vec<u8>)>,
}

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct IconJob {
    path: String,
    desired_px: i32,
}

#[cfg(windows)]
struct QueueState {
    high: VecDeque<IconJob>,
    low: VecDeque<IconJob>,
    stop: bool,
}

#[cfg(windows)]
pub struct ShellIconLoader {
    hwnd_raw: usize,
    cache_dir: PathBuf,
    shared: Arc<(Mutex<QueueState>, Condvar)>,
    hwnd_alive: Arc<AtomicBool>,
    notify_pending: Arc<AtomicBool>,
    tx: Sender<DecodedShellIcon>,
    rx: Receiver<DecodedShellIcon>,
    workers_started: bool,
    in_flight: HashSet<(String, i32)>,
}

#[cfg(windows)]
impl ShellIconLoader {
    pub fn new(hwnd: HWND, config_dir: &Path) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            hwnd_raw: hwnd as usize,
            cache_dir: config_dir.join("cache").join("shell_icons_v1"),
            shared: Arc::new((
                Mutex::new(QueueState {
                    high: VecDeque::new(),
                    low: VecDeque::new(),
                    stop: false,
                }),
                Condvar::new(),
            )),
            hwnd_alive: Arc::new(AtomicBool::new(true)),
            notify_pending: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            workers_started: false,
            in_flight: HashSet::new(),
        }
    }

    pub fn request(&mut self, path: &str, desired_px: i32, high_priority: bool) {
        if path.is_empty() {
            return;
        }
        let px = desired_px.clamp(32, 256);
        let key = (path.to_string(), px);
        if self.in_flight.contains(&key) {
            if high_priority {
                // Promote from low queue to the hot end of high queue if still queued
                let (lock, cv) = &*self.shared;
                if let Ok(mut st) = lock.lock() {
                    let job = IconJob {
                        path: key.0,
                        desired_px: px,
                    };
                    if let Some(pos) = st.low.iter().position(|j| j == &job) {
                        st.low.remove(pos);
                        st.high.push_back(job);
                        cv.notify_one();
                    } else if let Some(pos) = st.high.iter().position(|j| j == &job) {
                        // Move to back (LIFO) so currently visible icons load first
                        st.high.remove(pos);
                        st.high.push_back(job);
                    }
                }
            }
            return;
        }

        self.ensure_workers_started();
        self.in_flight.insert(key.clone());

        let job = IconJob {
            path: key.0,
            desired_px: px,
        };
        let (lock, cv) = &*self.shared;
        if let Ok(mut st) = lock.lock() {
            if high_priority {
                if st.high.len() >= MAX_HIGH_QUEUE
                    && let Some(evicted) = st.high.pop_front()
                {
                    self.in_flight.remove(&(evicted.path, evicted.desired_px));
                }
                st.high.push_back(job);
            } else {
                if st.low.len() >= MAX_LOW_QUEUE
                    && let Some(evicted) = st.low.pop_front()
                {
                    self.in_flight.remove(&(evicted.path, evicted.desired_px));
                }
                st.low.push_back(job);
            }
            cv.notify_one();
        }
    }

    pub fn try_recv(&mut self) -> Option<DecodedShellIcon> {
        let item = self.rx.try_recv().ok()?;
        self.in_flight.remove(&(item.path.clone(), item.desired_px));
        Some(item)
    }

    pub fn ack_notify(&self) {
        self.notify_pending.store(false, Ordering::Release);
    }

    pub fn clear_tracking(&mut self) {
        self.in_flight.clear();
        let (lock, _) = &*self.shared;
        if let Ok(mut st) = lock.lock() {
            st.high.clear();
            st.low.clear();
        }
        while self.rx.try_recv().is_ok() {}
    }

    fn ensure_workers_started(&mut self) {
        if self.workers_started {
            return;
        }
        self.workers_started = true;
        let _ = fs::create_dir_all(&self.cache_dir);

        let worker_count = std::thread::available_parallelism()
            .map(|n| n.get().clamp(2, 4))
            .unwrap_or(2);

        for idx in 0..worker_count {
            let shared = self.shared.clone();
            let tx = self.tx.clone();
            let cache_dir = self.cache_dir.clone();
            let hwnd_raw = self.hwnd_raw;
            let hwnd_alive = self.hwnd_alive.clone();
            let notify_pending = self.notify_pending.clone();
            let _ = std::thread::Builder::new()
                .name(format!("recircles-icon-{idx}"))
                .spawn(move || {
                    worker_loop(
                        shared,
                        tx,
                        cache_dir,
                        hwnd_raw,
                        hwnd_alive,
                        notify_pending,
                    );
                });
        }
    }
}

#[cfg(windows)]
impl Drop for ShellIconLoader {
    fn drop(&mut self) {
        self.hwnd_alive.store(false, Ordering::Release);
        let (lock, cv) = &*self.shared;
        if let Ok(mut st) = lock.lock() {
            st.stop = true;
            st.high.clear();
            st.low.clear();
            cv.notify_all();
        }
    }
}

#[cfg(windows)]
fn worker_loop(
    shared: Arc<(Mutex<QueueState>, Condvar)>,
    tx: Sender<DecodedShellIcon>,
    cache_dir: PathBuf,
    hwnd_raw: usize,
    hwnd_alive: Arc<AtomicBool>,
    notify_pending: Arc<AtomicBool>,
) {
    let mut extractor: Option<ShellIconWorker> = None;

    loop {
        let job = {
            let (lock, cv) = &*shared;
            let mut st = match lock.lock() {
                Ok(g) => g,
                Err(_) => break,
            };
            while st.high.is_empty() && st.low.is_empty() && !st.stop {
                st = match cv.wait(st) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
            if st.stop {
                break;
            }
            // LIFO for high-priority visible items so rapid scrolling resolves the latest viewport first;
            // FIFO for low-priority lookahead prefetch.
            st.high.pop_back().or_else(|| st.low.pop_front())
        };

        let Some(job) = job else {
            continue;
        };

        let meta = file_stamp(&job.path);
        let cache_file = cache_path_for(&cache_dir, &job.path, job.desired_px);

        let bitmap = if let Some(stamp) = meta
            && let Some(hit) = read_icon_cache(&cache_file, stamp)
        {
            Some(hit)
        } else {
            if extractor.is_none() {
                extractor = ShellIconWorker::new();
            }
            let wide = to_wide_nul(&job.path);
            let extracted = extractor
                .as_mut()
                .and_then(|w| w.extract_bgra(&wide, job.desired_px));
            if let (Some(stamp), Some((w, h, bgra))) = (meta, &extracted) {
                let _ = write_icon_cache(&cache_file, stamp, *w, *h, bgra);
            }
            extracted
        };

        if tx
            .send(DecodedShellIcon {
                path: job.path,
                desired_px: job.desired_px,
                bitmap,
            })
            .is_err()
        {
            break;
        }

        if hwnd_raw != 0
            && hwnd_alive.load(Ordering::Acquire)
            && !notify_pending.swap(true, Ordering::AcqRel)
        {
            unsafe {
                PostMessageW(hwnd_raw as HWND, WM_APP_ICON_READY, 0, 0);
            }
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
}

#[cfg(windows)]
fn file_stamp(path: &str) -> Option<FileStamp> {
    let md = fs::metadata(path).ok()?;
    let modified = md.modified().ok()?;
    let dur = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    Some(FileStamp {
        size: md.len(),
        mtime_secs: dur.as_secs(),
        mtime_nanos: dur.subsec_nanos(),
    })
}

#[cfg(windows)]
fn hash_path_128(path: &str) -> (u64, u64) {
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for b in path.bytes() {
        let lb = b.to_ascii_lowercase() as u64;
        h1 ^= lb;
        h1 = h1.wrapping_mul(0x100000001b3);
        h2 ^= lb.wrapping_add(0x9e);
        h2 = h2.wrapping_mul(0x100000001b3).rotate_left(7);
    }
    (h1, h2)
}

#[cfg(windows)]
fn cache_path_for(cache_dir: &Path, path: &str, desired_px: i32) -> PathBuf {
    let (h1, h2) = hash_path_128(path);
    cache_dir.join(format!("{h1:016x}{h2:016x}_{desired_px}.bgra"))
}

#[cfg(windows)]
fn read_icon_cache(cache_file: &Path, expected: FileStamp) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = fs::read(cache_file).ok()?;
    if bytes.len() < CACHE_HEADER_LEN {
        return None;
    }
    if bytes[0..4] != CACHE_MAGIC {
        return None;
    }
    let ver = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    if ver != CACHE_VERSION {
        return None;
    }
    let size = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let mtime_secs = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let mtime_nanos = u32::from_le_bytes(bytes[24..28].try_into().ok()?);
    if size != expected.size
        || mtime_secs != expected.mtime_secs
        || mtime_nanos != expected.mtime_nanos
    {
        return None;
    }
    let w = u32::from_le_bytes(bytes[28..32].try_into().ok()?);
    let h = u32::from_le_bytes(bytes[32..36].try_into().ok()?);
    if w == 0 || h == 0 || w > 256 || h > 256 {
        return None;
    }
    let expected_len = CACHE_HEADER_LEN + (w as usize) * (h as usize) * 4;
    if bytes.len() != expected_len {
        return None;
    }
    Some((w, h, bytes[CACHE_HEADER_LEN..].to_vec()))
}

#[cfg(windows)]
fn write_icon_cache(
    cache_file: &Path,
    stamp: FileStamp,
    w: u32,
    h: u32,
    bgra: &[u8],
) -> std::io::Result<()> {
    if bgra.len() != (w as usize) * (h as usize) * 4 {
        return Ok(());
    }
    let mut out = Vec::with_capacity(CACHE_HEADER_LEN + bgra.len());
    out.extend_from_slice(&CACHE_MAGIC);
    out.extend_from_slice(&CACHE_VERSION.to_le_bytes());
    out.extend_from_slice(&stamp.size.to_le_bytes());
    out.extend_from_slice(&stamp.mtime_secs.to_le_bytes());
    out.extend_from_slice(&stamp.mtime_nanos.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(bgra);

    let pid = std::process::id();
    let tmp = cache_file.with_extension(format!("tmp.{pid}"));
    if fs::write(&tmp, &out).is_ok() {
        if fs::rename(&tmp, cache_file).is_err() {
            let _ = fs::remove_file(cache_file);
            if fs::rename(&tmp, cache_file).is_err() {
                let _ = fs::remove_file(&tmp);
                let _ = fs::write(cache_file, &out);
            }
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant, SystemTime};

    #[test]
    fn icon_disk_cache_roundtrips_and_invalidates_on_stamp_change() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("recircles_icache_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let cache_file = cache_path_for(&dir, "C:\\Windows\\System32\\notepad.exe", 64);
        let stamp = FileStamp {
            size: 12345,
            mtime_secs: 1_700_000_000,
            mtime_nanos: 42,
        };
        let bgra = vec![0xAAu8; 4 * 4 * 4];
        write_icon_cache(&cache_file, stamp, 4, 4, &bgra).unwrap();

        let hit = read_icon_cache(&cache_file, stamp).expect("cache hit");
        assert_eq!(hit.0, 4);
        assert_eq!(hit.1, 4);
        assert_eq!(hit.2, bgra);

        let changed = FileStamp {
            size: 12346,
            ..stamp
        };
        assert!(read_icon_cache(&cache_file, changed).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn async_loader_extracts_real_windows_exe_icon_and_populates_disk_cache() {
        let sys_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let notepad = PathBuf::from(sys_root).join("System32").join("notepad.exe");
        if !notepad.is_file() {
            return;
        }
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("recircles_iload_{nanos}"));
        let mut loader = ShellIconLoader::new(std::ptr::null_mut(), &dir);
        let path_str = notepad.to_string_lossy().into_owned();
        loader.request(&path_str, 64, true);
        // Duplicate request is deduplicated cleanly
        loader.request(&path_str, 64, true);

        let deadline = Instant::now() + Duration::from_secs(15);
        let mut decoded = None;
        while Instant::now() < deadline {
            if let Some(item) = loader.try_recv() {
                decoded = Some(item);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let item = decoded.expect("icon worker should decode notepad.exe within 5s");
        let (w, h, bgra) = item.bitmap.expect("notepad.exe should produce non-empty icon");
        assert!(w >= 16 && w <= 64 && h >= 16 && h <= 64);
        assert_eq!(bgra.len(), (w as usize) * (h as usize) * 4);

        // Disk cache file should now exist and validate
        let cache_file = cache_path_for(&dir.join("cache").join("shell_icons_v1"), &path_str, 64);
        let stamp = file_stamp(&path_str).unwrap();
        assert!(read_icon_cache(&cache_file, stamp).is_some());

        let _ = fs::remove_dir_all(dir);
    }
}
