/// Renders the ReCircles orange ring icon into a BGRA (0xAARRGGBB, premultiplied alpha) pixel buffer.
/// Matches the exact anti-aliased orange ring gradient used for the daemon tray icon.
pub fn render_ring_icon_bgra(size: u32) -> Vec<u32> {
    let size_i = size as i32;
    let mut pixels = vec![0u32; (size * size) as usize];
    if size == 0 {
        return pixels;
    }
    let scale = size as f64 / 32.0;
    let cx = (size as f64 - 1.0) * 0.5;
    let cy = (size as f64 - 1.0) * 0.5;
    let r_out = 13.8 * scale;
    let r_in = 6.2 * scale;
    let denom = (size_i - 1).max(1) as f64;

    for y in 0..size_i {
        let t = (y as f64 / denom).clamp(0.0, 1.0);
        let (base_r, base_g, base_b) = if t < 0.5 {
            let u = t * 2.0;
            (
                255.0,
                209.0 + (158.0 - 209.0) * u,
                158.0 + (71.0 - 158.0) * u,
            )
        } else {
            let u = (t - 0.5) * 2.0;
            (
                255.0 + (245.0 - 255.0) * u,
                158.0 + (130.0 - 158.0) * u,
                71.0 + (50.0 - 71.0) * u,
            )
        };
        for x in 0..size_i {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let cov_out = (r_out - dist + 0.6).clamp(0.0, 1.0);
            let cov_in = (dist - r_in + 0.6).clamp(0.0, 1.0);
            let alpha = (cov_out * cov_in).clamp(0.0, 1.0);
            let a = (alpha * 255.0).round() as u32;
            let r = ((base_r * alpha).round() as u32).min(a);
            let g = ((base_g * alpha).round() as u32).min(a);
            let b = ((base_b * alpha).round() as u32).min(a);
            pixels[(y * size_i + x) as usize] = (a << 24) | (r << 16) | (g << 8) | b;
        }
    }
    pixels
}

/// Resource ID for the embedded application icon in ReCircles Windows executables.
pub const APP_ICON_RESOURCE_ID: u16 = 1;

#[cfg(windows)]
pub mod win {
    use super::{APP_ICON_RESOURCE_ID, render_ring_icon_bgra};
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPV5HEADER, CreateBitmap, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateIconIndirect, HICON, ICONINFO, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED, LoadImageW,
    };

    /// Generates an anti-aliased ReCircles orange ring HICON of the given size in memory.
    pub fn create_ring_hicon_sized(size: i32) -> HICON {
        let size = size.clamp(16, 256);
        let mut hdr: BITMAPV5HEADER = unsafe { std::mem::zeroed() };
        hdr.bV5Size = std::mem::size_of::<BITMAPV5HEADER>() as u32;
        hdr.bV5Width = size;
        hdr.bV5Height = -size;
        hdr.bV5Planes = 1;
        hdr.bV5BitCount = 32;
        hdr.bV5Compression = BI_RGB;
        hdr.bV5RedMask = 0x00FF_0000;
        hdr.bV5GreenMask = 0x0000_FF00;
        hdr.bV5BlueMask = 0x0000_00FF;
        hdr.bV5AlphaMask = 0xFF00_0000;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hdc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        let color_bmp: HBITMAP = unsafe {
            CreateDIBSection(
                hdc,
                (&hdr as *const BITMAPV5HEADER).cast(),
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            )
        };
        if !hdc.is_null() {
            unsafe { DeleteDC(hdc) };
        }
        if color_bmp.is_null() || bits.is_null() {
            return std::ptr::null_mut();
        }

        let src = render_ring_icon_bgra(size as u32);
        let dst =
            unsafe { std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize) };
        dst.copy_from_slice(&src);

        let mask_bmp = unsafe { CreateBitmap(size, size, 1, 1, std::ptr::null()) };
        let ii = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let icon = unsafe { CreateIconIndirect(&ii) };
        unsafe {
            DeleteObject(color_bmp);
            if !mask_bmp.is_null() {
                DeleteObject(mask_bmp);
            }
        }
        icon
    }

    /// Generates an anti-aliased 32x32 ReCircles orange ring HICON in memory.
    pub fn create_ring_hicon() -> HICON {
        create_ring_hicon_sized(32)
    }

    /// Loads the embedded application icon resource (ID 1) or falls back to generating the HICON in memory.
    /// Returns `(hIcon, hIconSm)`.
    pub fn load_or_create_app_icons() -> (HICON, HICON) {
        let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
        let res_id = APP_ICON_RESOURCE_ID as usize as *const u16;
        let big = unsafe {
            LoadImageW(
                hinst,
                res_id,
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTSIZE | LR_SHARED,
            )
        } as HICON;
        let small = unsafe { LoadImageW(hinst, res_id, IMAGE_ICON, 16, 16, LR_SHARED) } as HICON;

        let h_icon = if !big.is_null() {
            big
        } else {
            create_ring_hicon_sized(32)
        };
        let h_icon_sm = if !small.is_null() {
            small
        } else {
            create_ring_hicon_sized(16)
        };
        (h_icon, h_icon_sm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_transparent_center_and_opaque_orange_ring() {
        let px = render_ring_icon_bgra(32);
        assert_eq!(px.len(), 32 * 32);
        // Center pixel (16, 16) is inside the hollow ring -> transparent
        assert_eq!(px[16 * 32 + 16] >> 24, 0);
        // Corner pixel (0, 0) is outside the ring -> transparent
        assert_eq!(px[0] >> 24, 0);
        // Ring body at top-middle (16, 5) -> distance ~10.5 -> opaque orange
        let ring_px = px[5 * 32 + 16];
        let a = (ring_px >> 24) & 0xFF;
        let r = (ring_px >> 16) & 0xFF;
        let g = (ring_px >> 8) & 0xFF;
        let b = ring_px & 0xFF;
        assert_eq!(a, 255);
        assert_eq!(r, 255);
        assert!(g > 150 && b < g);
    }
}
