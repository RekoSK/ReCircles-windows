use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    compile_and_link_app_icon(&out_dir);
}

fn compile_and_link_app_icon(out_dir: &Path) {
    let ico_path = out_dir.join("recircles_app.ico");
    let rc_path = out_dir.join("recircles_app.rc");
    let res_path = out_dir.join("recircles_app.res");

    let ico_bytes = generate_ring_ico(&[16, 24, 32, 48, 64, 128, 256]);
    fs::write(&ico_path, ico_bytes).expect("write recircles_app.ico");

    let escaped_ico = ico_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let rc_contents = format!("1 ICON \"{escaped_ico}\"\n");
    fs::write(&rc_path, rc_contents).expect("write recircles_app.rc");

    let rc_exe = locate_rc_exe().expect("Could not locate Windows SDK rc.exe");
    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_path)
        .status()
        .expect("failed to execute rc.exe");
    assert!(status.success(), "rc.exe failed compiling application icon resource");

    println!("cargo:rustc-link-arg-bins={}", res_path.display());
}

fn generate_ring_ico(sizes: &[u32]) -> Vec<u8> {
    let mut dir = Vec::new();
    // ICONDIR header: idReserved=0, idType=1 (ICON), idCount=sizes.len()
    dir.extend_from_slice(&0u16.to_le_bytes());
    dir.extend_from_slice(&1u16.to_le_bytes());
    dir.extend_from_slice(&(sizes.len() as u16).to_le_bytes());

    let mut payloads: Vec<Vec<u8>> = Vec::with_capacity(sizes.len());
    for &sz in sizes {
        payloads.push(build_dib_frame(sz));
    }

    let mut offset = (6 + sizes.len() * 16) as u32;
    for (&sz, data) in sizes.iter().zip(payloads.iter()) {
        let w_byte = if sz >= 256 { 0u8 } else { sz as u8 };
        let h_byte = if sz >= 256 { 0u8 } else { sz as u8 };
        dir.push(w_byte); // bWidth
        dir.push(h_byte); // bHeight
        dir.push(0); // bColorCount
        dir.push(0); // bReserved
        dir.extend_from_slice(&1u16.to_le_bytes()); // wPlanes
        dir.extend_from_slice(&32u16.to_le_bytes()); // wBitCount
        dir.extend_from_slice(&(data.len() as u32).to_le_bytes()); // dwBytesInRes
        dir.extend_from_slice(&offset.to_le_bytes()); // dwImageOffset
        offset += data.len() as u32;
    }

    for data in payloads {
        dir.extend_from_slice(&data);
    }
    dir
}

fn build_dib_frame(size: u32) -> Vec<u8> {
    let size_i = size as i32;
    let scale = size as f64 / 32.0;
    let cx = (size as f64 - 1.0) * 0.5;
    let cy = (size as f64 - 1.0) * 0.5;
    let r_out = 13.8 * scale;
    let r_in = 6.2 * scale;
    let denom = (size_i - 1).max(1) as f64;

    let mask_row_bytes = ((size + 31) / 32 * 4) as usize;
    let xor_bytes = (size * size * 4) as usize;
    let and_bytes = mask_row_bytes * (size as usize);

    let mut buf = Vec::with_capacity(40 + xor_bytes + and_bytes);
    // BITMAPINFOHEADER (40 bytes)
    buf.extend_from_slice(&40u32.to_le_bytes()); // biSize
    buf.extend_from_slice(&size_i.to_le_bytes()); // biWidth
    buf.extend_from_slice(&(size_i * 2).to_le_bytes()); // biHeight (XOR + AND)
    buf.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    buf.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    buf.extend_from_slice(&0u32.to_le_bytes()); // biCompression (BI_RGB)
    buf.extend_from_slice(&(xor_bytes as u32).to_le_bytes()); // biSizeImage
    buf.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    buf.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    buf.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    buf.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // Bottom-up 32-bit BGRA XOR bitmap (straight RGB + alpha for standard ICO format)
    for row_bottom in 0..size_i {
        let y = size_i - 1 - row_bottom;
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
            let a = (alpha * 255.0).round() as u8;
            let (r, g, b) = if a > 0 {
                (
                    base_r.round().clamp(0.0, 255.0) as u8,
                    base_g.round().clamp(0.0, 255.0) as u8,
                    base_b.round().clamp(0.0, 255.0) as u8,
                )
            } else {
                (0, 0, 0)
            };
            buf.extend_from_slice(&[b, g, r, a]);
        }
    }

    // Bottom-up 1-bpp AND mask (1 bit = transparent when alpha == 0)
    for row_bottom in 0..size_i {
        let y = size_i - 1 - row_bottom;
        let mut row = vec![0u8; mask_row_bytes];
        for x in 0..size_i {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let cov_out = (r_out - dist + 0.6).clamp(0.0, 1.0);
            let cov_in = (dist - r_in + 0.6).clamp(0.0, 1.0);
            let alpha = (cov_out * cov_in).clamp(0.0, 1.0);
            let a = (alpha * 255.0).round() as u8;
            if a == 0 {
                let byte_idx = (x as usize) / 8;
                let bit_idx = 7 - ((x as usize) % 8);
                row[byte_idx] |= 1u8 << bit_idx;
            }
        }
        buf.extend_from_slice(&row);
    }

    buf
}

fn locate_rc_exe() -> Option<PathBuf> {
    let kits_root = PathBuf::from(
        env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()),
    )
    .join(r"Windows Kits\10\bin");
    let Ok(entries) = fs::read_dir(&kits_root) else {
        return None;
    };
    let mut versions: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.starts_with("10."))
        .collect();
    versions.sort();
    versions.reverse();
    for ver in versions {
        let candidate = kits_root.join(&ver).join("x64").join("rc.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let fallback = kits_root.join("x64").join("rc.exe");
    if fallback.is_file() {
        return Some(fallback);
    }
    None
}
