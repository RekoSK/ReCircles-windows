use recircles_submenu_abi::{
    ClipboardItemDto, ClipboardKind, SubmenuRequest, SubmenuResponse, export_submenu_dll,
};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

export_submenu_dll!("clipboard", handle_request);

const DEFAULT_MAX_ITEMS: usize = 120;

pub fn handle_request(req: SubmenuRequest) -> SubmenuResponse {
    match req {
        SubmenuRequest::ClipboardList { data_dir } => {
            let items = load_history(Path::new(&data_dir));
            SubmenuResponse::ClipboardItems { items }
        }
        SubmenuRequest::ClipboardCaptureNow {
            data_dir,
            max_items,
            ignore_internal_marker,
        } => {
            let max = if max_items == 0 {
                DEFAULT_MAX_ITEMS
            } else {
                max_items
            };
            capture_current_clipboard(Path::new(&data_dir), max, ignore_internal_marker)
        }
        SubmenuRequest::ClipboardCopyToClipboard { data_dir, id } => {
            match restore_entry(Path::new(&data_dir), &id) {
                Ok(msg) => SubmenuResponse::ActionOutcome {
                    ok: true,
                    message: msg,
                },
                Err(e) => SubmenuResponse::ActionOutcome {
                    ok: false,
                    message: e,
                },
            }
        }
        SubmenuRequest::ClipboardDelete { data_dir, id } => {
            match delete_entry(Path::new(&data_dir), &id) {
                Ok(()) => SubmenuResponse::ActionOutcome {
                    ok: true,
                    message: "Deleted clipboard entry".into(),
                },
                Err(e) => SubmenuResponse::ActionOutcome {
                    ok: false,
                    message: e,
                },
            }
        }
        SubmenuRequest::ClipboardClear { data_dir } => {
            match clear_history(Path::new(&data_dir)) {
                Ok(()) => SubmenuResponse::ActionOutcome {
                    ok: true,
                    message: "Clipboard history cleared".into(),
                },
                Err(e) => SubmenuResponse::ActionOutcome {
                    ok: false,
                    message: e,
                },
            }
        }
        other => SubmenuResponse::Error {
            message: format!("Unsupported request for clipboard DLL: {other:?}"),
        },
    }
}

fn history_file(data_dir: &Path) -> PathBuf {
    data_dir.join("clipboard_history.json")
}

fn images_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("clipboard_images")
}

pub fn load_history(data_dir: &Path) -> Vec<ClipboardItemDto> {
    let p = history_file(data_dir);
    let Ok(raw) = fs::read_to_string(p) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_history(data_dir: &Path, items: &[ClipboardItemDto]) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let target = history_file(data_dir);
    let tmp = data_dir.join(format!("clipboard_history.tmp.{}", std::process::id()));
    let json = serde_json::to_string_pretty(items).map_err(|e| e.to_string())?;
    {
        let mut f = File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(&target);
    fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

pub fn fnv1a_hex(data: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn text_preview_label(text: &str) -> (String, String) {
    let trimmed = text.trim();
    let collapsed: String = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let char_count = text.chars().count();
    let line_count = text.lines().count().max(1);
    let mut preview: String = collapsed.chars().take(54).collect();
    if collapsed.chars().count() > 54 {
        preview.push_str("...");
    }
    if preview.is_empty() {
        preview = format!("({char_count} whitespace chars)");
    }
    let detail = if line_count > 1 {
        format!("{line_count} lines · {char_count} chars")
    } else {
        format!("{char_count} chars")
    };
    (preview, detail)
}

pub fn record_text_entry(
    data_dir: &Path,
    text: String,
    max_items: usize,
) -> Result<(bool, usize), String> {
    if text.trim().is_empty() {
        return Ok((false, load_history(data_dir).len()));
    }
    let hash = format!("t_{}", fnv1a_hex(text.as_bytes()));
    let (label, detail) = text_preview_label(&text);
    let now = now_secs();
    let item = ClipboardItemDto {
        id: format!("{hash}_{now}"),
        kind: ClipboardKind::Text,
        label,
        detail,
        text: Some(text),
        files: Vec::new(),
        image_path: None,
        width: 0,
        height: 0,
        hash,
        timestamp_secs: now,
    };
    upsert_history_item(data_dir, item, max_items)
}

pub fn record_files_entry(
    data_dir: &Path,
    files: Vec<String>,
    max_items: usize,
) -> Result<(bool, usize), String> {
    if files.is_empty() {
        return Ok((false, load_history(data_dir).len()));
    }
    let joined = files.join("\n");
    let hash = format!("f_{}", fnv1a_hex(joined.as_bytes()));
    let first_name = Path::new(&files[0])
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&files[0]);
    let label = if files.len() == 1 {
        first_name.to_string()
    } else {
        format!("{} files: {first_name}...", files.len())
    };
    let detail = if files.len() == 1 {
        files[0].clone()
    } else {
        format!("{} paths copied", files.len())
    };
    let now = now_secs();
    let item = ClipboardItemDto {
        id: format!("{hash}_{now}"),
        kind: ClipboardKind::Files,
        label,
        detail,
        text: None,
        files,
        image_path: None,
        width: 0,
        height: 0,
        hash,
        timestamp_secs: now,
    };
    upsert_history_item(data_dir, item, max_items)
}

pub fn record_dib_image_entry(
    data_dir: &Path,
    dib_bytes: &[u8],
    max_items: usize,
) -> Result<(bool, usize), String> {
    if dib_bytes.len() < 40 {
        return Ok((false, load_history(data_dir).len()));
    }
    let width = u32::from_le_bytes([dib_bytes[4], dib_bytes[5], dib_bytes[6], dib_bytes[7]]);
    let raw_h = i32::from_le_bytes([dib_bytes[8], dib_bytes[9], dib_bytes[10], dib_bytes[11]]);
    let height = raw_h.unsigned_abs();
    let hash = format!("i_{}", fnv1a_hex(dib_bytes));

    let img_dir = images_dir(data_dir);
    fs::create_dir_all(&img_dir).map_err(|e| e.to_string())?;
    let bmp_path = img_dir.join(format!("{hash}.bmp"));
    if !bmp_path.exists() {
        let bmp = wrap_dib_in_bmp(dib_bytes);
        fs::write(&bmp_path, bmp).map_err(|e| e.to_string())?;
    }

    let now = now_secs();
    let item = ClipboardItemDto {
        id: format!("{hash}_{now}"),
        kind: ClipboardKind::Image,
        label: format!("Image {width}x{height}"),
        detail: format!("{} KB bitmap", (dib_bytes.len() / 1024).max(1)),
        text: None,
        files: Vec::new(),
        image_path: Some(bmp_path.to_string_lossy().into_owned()),
        width,
        height,
        hash,
        timestamp_secs: now,
    };
    upsert_history_item(data_dir, item, max_items)
}

pub fn wrap_dib_in_bmp(dib_bytes: &[u8]) -> Vec<u8> {
    // 14-byte BITMAPFILEHEADER + raw BITMAPINFO/DIB bytes
    let header_size = if dib_bytes.len() >= 4 {
        u32::from_le_bytes([dib_bytes[0], dib_bytes[1], dib_bytes[2], dib_bytes[3]])
    } else {
        40
    };
    let bit_count = if dib_bytes.len() >= 16 {
        u16::from_le_bytes([dib_bytes[14], dib_bytes[15]])
    } else {
        32
    };
    let clr_used = if dib_bytes.len() >= 36 {
        u32::from_le_bytes([dib_bytes[32], dib_bytes[33], dib_bytes[34], dib_bytes[35]])
    } else {
        0
    };
    let palette_entries = if bit_count <= 8 {
        if clr_used == 0 {
            1u32 << bit_count
        } else {
            clr_used
        }
    } else {
        clr_used
    };
    let pixel_offset = 14u32.saturating_add(header_size).saturating_add(palette_entries * 4);
    let total_size = 14u32.saturating_add(dib_bytes.len() as u32);

    let mut bmp = Vec::with_capacity(14 + dib_bytes.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&total_size.to_le_bytes());
    bmp.extend_from_slice(&[0u8; 4]);
    bmp.extend_from_slice(&pixel_offset.to_le_bytes());
    bmp.extend_from_slice(dib_bytes);
    bmp
}

fn upsert_history_item(
    data_dir: &Path,
    item: ClipboardItemDto,
    max_items: usize,
) -> Result<(bool, usize), String> {
    let mut items = load_history(data_dir);
    let same_top = items.first().is_some_and(|top| top.hash == item.hash);
    if same_top {
        return Ok((false, items.len()));
    }
    items.retain(|existing| existing.hash != item.hash);
    items.insert(0, item);

    if items.len() > max_items {
        let dropped = items.split_off(max_items);
        for d in dropped {
            if let Some(p) = d.image_path {
                let still_used = items.iter().any(|k| k.image_path.as_deref() == Some(p.as_str()));
                if !still_used {
                    let _ = fs::remove_file(p);
                }
            }
        }
    }
    save_history(data_dir, &items)?;
    Ok((true, items.len()))
}

pub fn delete_entry(data_dir: &Path, id: &str) -> Result<(), String> {
    let mut items = load_history(data_dir);
    let mut removed_imgs = Vec::new();
    let before = items.len();
    items.retain(|item| {
        if item.id == id {
            if let Some(img) = &item.image_path {
                removed_imgs.push(img.clone());
            }
            false
        } else {
            true
        }
    });
    if items.len() == before {
        return Err("Clipboard entry no longer exists".into());
    }
    for img in removed_imgs {
        if !items.iter().any(|k| k.image_path.as_deref() == Some(img.as_str())) {
            let _ = fs::remove_file(img);
        }
    }
    save_history(data_dir, &items)
}

pub fn clear_history(data_dir: &Path) -> Result<(), String> {
    let img_dir = images_dir(data_dir);
    if img_dir.exists() {
        let _ = fs::remove_dir_all(&img_dir);
    }
    save_history(data_dir, &[])
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(windows)]
mod win_clip {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HWND};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
    };
    use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};

    const CF_UNICODETEXT: u32 = 13;
    const CF_DIB: u32 = 8;
    const CF_HDROP: u32 = 15;
    const INTERNAL_CLIPBOARD_FORMAT: &str = "ReCirclesInternalClipboardOp";

    struct ClipboardLock;
    impl ClipboardLock {
        fn open() -> Result<Self, String> {
            for _ in 0..8 {
                if unsafe { OpenClipboard(std::ptr::null_mut() as HWND) } != 0 {
                    return Ok(Self);
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            Err("Clipboard is locked by another application".into())
        }
    }
    impl Drop for ClipboardLock {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }

    fn format_id(name: &str) -> u32 {
        let wide: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
        unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
    }

    fn is_internal_clipboard_op() -> bool {
        let internal = format_id(INTERNAL_CLIPBOARD_FORMAT);
        internal != 0 && unsafe { IsClipboardFormatAvailable(internal) } != 0
    }

    fn write_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
        let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len().max(1)) };
        if hmem.is_null() {
            return Err("GlobalAlloc failed".into());
        }
        let ptr = unsafe { GlobalLock(hmem) } as *mut u8;
        if ptr.is_null() {
            unsafe { GlobalFree(hmem) };
            return Err("GlobalLock failed".into());
        }
        unsafe {
            if !bytes.is_empty() {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            }
            GlobalUnlock(hmem);
            if SetClipboardData(format, hmem as HANDLE).is_null() {
                GlobalFree(hmem);
                return Err(format!("SetClipboardData({format}) failed"));
            }
        }
        Ok(())
    }

    fn is_sensitive_clipboard() -> bool {
        let excl = format_id("ExcludeClipboardContentFromMonitorProcessing");
        if excl != 0 && unsafe { IsClipboardFormatAvailable(excl) } != 0 {
            return true;
        }
        let allow = format_id("CanIncludeInClipboardHistory");
        if allow != 0 && unsafe { IsClipboardFormatAvailable(allow) } != 0 {
            let h = unsafe { GetClipboardData(allow) };
            if !h.is_null() {
                let ptr = unsafe { GlobalLock(h) } as *const u32;
                let blocked = if !ptr.is_null() {
                    unsafe { *ptr == 0 }
                } else {
                    false
                };
                unsafe { GlobalUnlock(h) };
                if blocked {
                    return true;
                }
            }
        }
        false
    }

    enum Snapshot {
        Files(Vec<String>),
        Dib(Vec<u8>),
        Text(String),
        None,
    }

    fn read_snapshot(ignore_internal_marker: bool) -> Snapshot {
        let Ok(_lock) = ClipboardLock::open() else {
            return Snapshot::None;
        };

        if ignore_internal_marker && is_internal_clipboard_op() {
            return Snapshot::None;
        }
        if is_sensitive_clipboard() {
            return Snapshot::None;
        }

        if unsafe { IsClipboardFormatAvailable(CF_HDROP) } != 0 {
            let h = unsafe { GetClipboardData(CF_HDROP) };
            if !h.is_null() {
                let hdrop = h as HDROP;
                let count = unsafe { DragQueryFileW(hdrop, u32::MAX, std::ptr::null_mut(), 0) };
                let mut files = Vec::new();
                for i in 0..count {
                    let len = unsafe { DragQueryFileW(hdrop, i, std::ptr::null_mut(), 0) };
                    if len > 0 {
                        let mut buf = vec![0u16; (len as usize) + 1];
                        unsafe { DragQueryFileW(hdrop, i, buf.as_mut_ptr(), buf.len() as u32) };
                        files.push(String::from_utf16_lossy(&buf[..len as usize]));
                    }
                }
                if !files.is_empty() {
                    return Snapshot::Files(files);
                }
            }
        }

        if unsafe { IsClipboardFormatAvailable(CF_DIB) } != 0 {
            let h = unsafe { GetClipboardData(CF_DIB) };
            if !h.is_null() {
                let size = unsafe { GlobalSize(h) };
                let ptr = unsafe { GlobalLock(h) } as *const u8;
                let mut dib = Vec::new();
                if !ptr.is_null() && (40..=64 * 1024 * 1024).contains(&size) {
                    dib.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, size) });
                }
                unsafe { GlobalUnlock(h) };
                if !dib.is_empty() {
                    return Snapshot::Dib(dib);
                }
            }
        }

        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } != 0 {
            let h = unsafe { GetClipboardData(CF_UNICODETEXT) };
            if !h.is_null() {
                let ptr = unsafe { GlobalLock(h) } as *const u16;
                let text = if !ptr.is_null() {
                    let max_chars = (unsafe { GlobalSize(h) }) / 2;
                    let slice = unsafe { std::slice::from_raw_parts(ptr, max_chars) };
                    let nul = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
                    Some(String::from_utf16_lossy(&slice[..nul]))
                } else {
                    None
                };
                unsafe { GlobalUnlock(h) };
                if let Some(t) = text
                    && !t.trim().is_empty()
                {
                    return Snapshot::Text(t);
                }
            }
        }

        Snapshot::None
    }

    pub fn capture(data_dir: &Path, max_items: usize, ignore_internal_marker: bool) -> SubmenuResponse {
        let outcome = match read_snapshot(ignore_internal_marker) {
            Snapshot::Files(files) => record_files_entry(data_dir, files, max_items),
            Snapshot::Dib(dib) => record_dib_image_entry(data_dir, &dib, max_items),
            Snapshot::Text(text) => record_text_entry(data_dir, text, max_items),
            Snapshot::None => {
                return SubmenuResponse::ClipboardCapture {
                    captured: false,
                    changed: false,
                    total_items: load_history(data_dir).len(),
                };
            }
        };

        match outcome {
            Ok((changed, total)) => SubmenuResponse::ClipboardCapture {
                captured: true,
                changed,
                total_items: total,
            },
            Err(_) => SubmenuResponse::ClipboardCapture {
                captured: false,
                changed: false,
                total_items: load_history(data_dir).len(),
            },
        }
    }

    pub fn restore(data_dir: &Path, id: &str) -> Result<String, String> {
        let mut items = load_history(data_dir);
        let pos = items
            .iter()
            .position(|i| i.id == id)
            .ok_or_else(|| "Clipboard entry not found".to_string())?;
        let item = items[pos].clone();

        {
            let _lock = ClipboardLock::open()?;
            if unsafe { EmptyClipboard() } == 0 {
                return Err("Failed clearing current clipboard".into());
            }

            let internal_fmt = format_id(INTERNAL_CLIPBOARD_FORMAT);
            if internal_fmt != 0 {
                let _ = write_clipboard_bytes(internal_fmt, &[1]);
            }

            match item.kind {
                ClipboardKind::Text => {
                    let text = item.text.clone().unwrap_or_default();
                    let wide: Vec<u16> = OsStr::new(&text).encode_wide().chain(Some(0)).collect();
                    let raw = unsafe {
                        std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2)
                    };
                    write_clipboard_bytes(CF_UNICODETEXT, raw)?;
                }
                ClipboardKind::Image => {
                    let bmp_path = item
                        .image_path
                        .clone()
                        .ok_or_else(|| "Image file path missing".to_string())?;
                    let bmp = fs::read(&bmp_path)
                        .map_err(|e| format!("Failed reading cached image {bmp_path}: {e}"))?;
                    if bmp.len() <= 14 || &bmp[..2] != b"BM" {
                        return Err("Invalid cached BMP image".into());
                    }
                    write_clipboard_bytes(CF_DIB, &bmp[14..])?;
                }
                ClipboardKind::Files => {
                    // DROPFILES header (20 bytes) + double-NUL terminated UTF-16 path list
                    #[repr(C)]
                    struct DropFiles {
                        p_files: u32,
                        pt_x: i32,
                        pt_y: i32,
                        f_nc: i32,
                        f_wide: i32,
                    }
                    let mut wide_paths: Vec<u16> = Vec::new();
                    for p in &item.files {
                        wide_paths.extend(OsStr::new(p).encode_wide());
                        wide_paths.push(0);
                    }
                    wide_paths.push(0);

                    let header_bytes = std::mem::size_of::<DropFiles>();
                    let mut buf = Vec::with_capacity(header_bytes + wide_paths.len() * 2);
                    let hdr = DropFiles {
                        p_files: header_bytes as u32,
                        pt_x: 0,
                        pt_y: 0,
                        f_nc: 0,
                        f_wide: 1,
                    };
                    let hdr_slice = unsafe {
                        std::slice::from_raw_parts((&hdr as *const DropFiles).cast::<u8>(), header_bytes)
                    };
                    let paths_slice = unsafe {
                        std::slice::from_raw_parts(
                            wide_paths.as_ptr().cast::<u8>(),
                            wide_paths.len() * 2,
                        )
                    };
                    buf.extend_from_slice(hdr_slice);
                    buf.extend_from_slice(paths_slice);
                    write_clipboard_bytes(CF_HDROP, &buf)?;
                }
            }
        }

        if pos > 0 {
            let moved = items.remove(pos);
            items.insert(0, moved);
            let _ = save_history(data_dir, &items);
        }

        Ok(match item.kind {
            ClipboardKind::Text => "Copied text to clipboard".into(),
            ClipboardKind::Image => "Copied image to clipboard".into(),
            ClipboardKind::Files => "Copied file list to clipboard".into(),
        })
    }
}

pub fn capture_current_clipboard(
    data_dir: &Path,
    max_items: usize,
    ignore_internal_marker: bool,
) -> SubmenuResponse {
    #[cfg(windows)]
    {
        win_clip::capture(data_dir, max_items, ignore_internal_marker)
    }
    #[cfg(not(windows))]
    {
        let _ = (data_dir, max_items, ignore_internal_marker);
        SubmenuResponse::ClipboardCapture {
            captured: false,
            changed: false,
            total_items: 0,
        }
    }
}

pub fn restore_entry(data_dir: &Path, id: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        win_clip::restore(data_dir, id)
    }
    #[cfg(not(windows))]
    {
        let _ = (data_dir, id);
        Err("Clipboard restore only runs on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("recircles_clip_{tag}_{nanos}"));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn records_dedups_trims_and_deletes_clipboard_entries_and_bmps() {
        let dir = temp_dir("ops");
        assert_eq!(record_text_entry(&dir, "hello world".into(), 3).unwrap(), (true, 1));
        assert_eq!(record_text_entry(&dir, "hello world".into(), 3).unwrap(), (false, 1)); // dedup top
        assert_eq!(record_text_entry(&dir, "second item".into(), 3).unwrap(), (true, 2));

        // 1x1 32bpp synthetic BITMAPINFOHEADER + 4-byte pixel
        let mut dib = vec![0u8; 44];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1u32.to_le_bytes());
        dib[8..12].copy_from_slice(&1i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        assert_eq!(record_dib_image_entry(&dir, &dib, 3).unwrap(), (true, 3));

        let items = load_history(&dir);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, ClipboardKind::Image);
        let bmp_path = PathBuf::from(items[0].image_path.as_ref().unwrap());
        assert!(bmp_path.is_file());

        // Delete image item -> deletes orphaned bmp too
        delete_entry(&dir, &items[0].id).unwrap();
        assert!(!bmp_path.exists());
        assert_eq!(load_history(&dir).len(), 2);

        clear_history(&dir).unwrap();
        assert!(load_history(&dir).is_empty());

        let _ = fs::remove_dir_all(dir);
    }
}
