use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=native/d2d_backend.cpp");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let cpp_src = manifest_dir.join("native").join("d2d_backend.cpp");

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env != "msvc" {
        panic!("ReCircles Windows native GPU backend requires the MSVC toolchain (x86_64-pc-windows-msvc).");
    }

    let (cl_exe, lib_exe, msvc_inc, msvc_lib) = locate_msvc().expect(
        "Could not locate Visual Studio Build Tools (cl.exe / lib.exe). Install Visual Studio C++ Build Tools.",
    );
    let (sdk_inc_ver, sdk_lib_ver) = locate_windows_sdk().expect(
        "Could not locate Windows 10/11 SDK headers and libraries.",
    );

    let obj_path = out_dir.join("d2d_backend.obj");
    let lib_path = out_dir.join("d2d_backend.lib");

    let mut inc_paths = vec![
        msvc_inc,
        sdk_inc_ver.join("ucrt"),
        sdk_inc_ver.join("shared"),
        sdk_inc_ver.join("um"),
        sdk_inc_ver.join("winrt"),
    ];
    if let Ok(existing) = env::var("INCLUDE") {
        for p in env::split_paths(&existing) {
            if p.is_dir() {
                inc_paths.push(p);
            }
        }
    }
    let include_env = env::join_paths(&inc_paths).expect("join INCLUDE paths");

    let mut cl_cmd = Command::new(&cl_exe);
    cl_cmd
        .env("INCLUDE", &include_env)
        .arg("/nologo")
        .arg("/c")
        .arg("/O2")
        .arg("/Oi")
        .arg("/EHsc")
        .arg("/MD")
        .arg("/std:c++17")
        .arg("/W3")
        .arg("/DUNICODE")
        .arg("/D_UNICODE")
        .arg("/DNOMINMAX")
        .arg("/DWIN32_LEAN_AND_MEAN")
        .arg(format!("/Fo{}", obj_path.display()))
        .arg(&cpp_src);

    let status = cl_cmd.status().expect("failed to execute cl.exe");
    assert!(status.success(), "cl.exe failed compiling d2d_backend.cpp");

    let status = Command::new(&lib_exe)
        .arg("/nologo")
        .arg(format!("/OUT:{}", lib_path.display()))
        .arg(&obj_path)
        .status()
        .expect("failed to execute lib.exe");
    assert!(status.success(), "lib.exe failed creating d2d_backend.lib");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-search=native={}", msvc_lib.display());
    println!("cargo:rustc-link-search=native={}", sdk_lib_ver.join("um").join("x64").display());
    println!("cargo:rustc-link-search=native={}", sdk_lib_ver.join("ucrt").join("x64").display());

    println!("cargo:rustc-link-lib=static=d2d_backend");
    for lib in [
        "d3d11",
        "dxgi",
        "d2d1",
        "dcomp",
        "dwrite",
        "windowscodecs",
        "dxguid",
        "ole32",
        "uuid",
        "user32",
        "gdi32",
        "shell32",
        "dwmapi",
    ] {
        println!("cargo:rustc-link-lib={lib}");
    }

    compile_and_link_app_icon(&out_dir);
}

fn compile_and_link_app_icon(out_dir: &Path) {
    let ico_path = out_dir.join("recircles_app.ico");
    let rc_path = out_dir.join("recircles_app.rc");
    let res_path = out_dir.join("recircles_app.res");

    let ico_bytes = generate_ring_ico(&[16, 24, 32, 48, 64, 128, 256]);
    std::fs::write(&ico_path, ico_bytes).expect("write recircles_app.ico");

    let escaped_ico = ico_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let rc_contents = format!("1 ICON \"{escaped_ico}\"\n");
    std::fs::write(&rc_path, rc_contents).expect("write recircles_app.rc");

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
        dir.push(w_byte);
        dir.push(h_byte);
        dir.push(0);
        dir.push(0);
        dir.extend_from_slice(&1u16.to_le_bytes());
        dir.extend_from_slice(&32u16.to_le_bytes());
        dir.extend_from_slice(&(data.len() as u32).to_le_bytes());
        dir.extend_from_slice(&offset.to_le_bytes());
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
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&size_i.to_le_bytes());
    buf.extend_from_slice(&(size_i * 2).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&32u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(xor_bytes as u32).to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

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
    let Ok(entries) = std::fs::read_dir(&kits_root) else {
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

fn locate_msvc() -> Option<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    let mut roots = Vec::new();
    if let Ok(vc) = env::var("VCToolsInstallDir") {
        let p = PathBuf::from(vc);
        if p.is_dir() {
            roots.push(p);
        }
    }

    // Query vswhere.exe if available
    let vswhere = PathBuf::from(
        env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()),
    )
    .join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    if vswhere.is_file()
        && let Ok(out) = Command::new(&vswhere)
            .args(["-latest", "-products", "*", "-property", "installationPath"])
            .output()
        && out.status.success()
    {
        let install = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !install.is_empty() {
            push_newest_msvc_toolset(&PathBuf::from(install).join(r"VC\Tools\MSVC"), &mut roots);
        }
    }

    for edition in ["BuildTools", "Community", "Professional", "Enterprise"] {
        for base in [r"C:\Program Files (x86)\Microsoft Visual Studio\2022", r"C:\Program Files\Microsoft Visual Studio\2022"] {
            let dir = PathBuf::from(base).join(edition).join(r"VC\Tools\MSVC");
            push_newest_msvc_toolset(&dir, &mut roots);
        }
    }

    for root in roots {
        let bin = root.join("bin").join("Hostx64").join("x64");
        let cl = bin.join("cl.exe");
        let lib = bin.join("lib.exe");
        let inc = root.join("include");
        let lib_dir = root.join("lib").join("x64");
        if cl.is_file() && lib.is_file() && inc.is_dir() && lib_dir.is_dir() {
            return Some((cl, lib, inc, lib_dir));
        }
    }
    None
}

fn push_newest_msvc_toolset(msvc_dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(msvc_dir) else {
        return;
    };
    let mut versions: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    versions.sort();
    versions.reverse();
    out.extend(versions);
}

fn locate_windows_sdk() -> Option<(PathBuf, PathBuf)> {
    let kits_root = PathBuf::from(
        env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()),
    )
    .join(r"Windows Kits\10");
    let inc_root = kits_root.join("Include");
    let lib_root = kits_root.join("Lib");
    let Ok(entries) = std::fs::read_dir(&inc_root) else {
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
        let inc = inc_root.join(&ver);
        let lib = lib_root.join(&ver);
        if inc.join("um").join("dcomp.h").is_file()
            && lib.join("um").join("x64").join("dcomp.lib").is_file()
        {
            return Some((inc, lib));
        }
    }
    None
}
