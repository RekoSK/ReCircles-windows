use crate::paths;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const GITHUB_OWNER: &str = "RekoSK";
pub const GITHUB_REPO: &str = "ReCircles-windows";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<PreIdent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreIdent {
    Numeric(u64),
    AlphaNum(String),
}

impl Ord for PreIdent {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(a), Self::Numeric(b)) => a.cmp(b),
            (Self::Numeric(_), Self::AlphaNum(_)) => Ordering::Less,
            (Self::AlphaNum(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::AlphaNum(a), Self::AlphaNum(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PreIdent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SemVer {
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let without_v = trimmed
            .strip_prefix('v')
            .or_else(|| trimmed.strip_prefix('V'))
            .unwrap_or(trimmed);
        // Strip optional build metadata (+...)
        let main_and_pre = without_v.split('+').next().unwrap_or(without_v);
        let (core, pre_str) = match main_and_pre.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (main_and_pre, None),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch = parts.next()?.parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        let mut pre = Vec::new();
        if let Some(p) = pre_str {
            if p.is_empty() {
                return None;
            }
            for seg in p.split('.') {
                if seg.is_empty() {
                    return None;
                }
                if seg.bytes().all(|b| b.is_ascii_digit()) {
                    pre.push(PreIdent::Numeric(seg.parse::<u64>().ok()?));
                } else {
                    pre.push(PreIdent::AlphaNum(seg.to_ascii_lowercase()));
                }
            }
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(other.pre.iter()) {
                        let c = a.cmp(b);
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            })
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (SemVer::parse(candidate), SemVer::parse(current)) {
        (Some(cand), Some(cur)) => cand > cur,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub asset_name: String,
    pub download_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_url: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateState {
    pub last_checked_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default)]
    pub etag_include_prereleases: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_release: Option<ReleaseInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    UpToDate {
        current: String,
        latest: Option<String>,
    },
    UpdateAvailable(ReleaseInfo),
}

impl UpdateState {
    pub fn load(config_dir: &Path) -> Self {
        let p = paths::update_state_in(config_dir);
        fs::read_to_string(p)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(config_dir)
            .map_err(|e| format!("Failed creating config dir {}: {e}", config_dir.display()))?;
        let p = paths::update_state_in(config_dir);
        let tmp = p.with_extension(format!("tmp.{}", std::process::id()));
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&tmp, json.as_bytes()).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&p);
        fs::rename(&tmp, &p).map_err(|e| e.to_string())
    }

    pub fn available_update_for(&self, current_version: &str) -> Option<&ReleaseInfo> {
        let rel = self.latest_release.as_ref()?;
        if is_newer_version(&rel.version, current_version) {
            Some(rel)
        } else {
            None
        }
    }
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    state: Option<String>,
}

fn normalize_sha256_digest(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let hex = trimmed
        .strip_prefix("sha256:")
        .or_else(|| trimmed.strip_prefix("SHA256:"))
        .unwrap_or(trimmed)
        .trim();
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(hex.to_ascii_lowercase())
    } else {
        None
    }
}

fn is_setup_asset_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower.starts_with("recircles-setup") && lower.ends_with(".exe")
}

fn release_from_gh(rel: GhRelease, include_prereleases: bool) -> Option<(SemVer, ReleaseInfo)> {
    if rel.draft {
        return None;
    }
    let semver = SemVer::parse(&rel.tag_name)?;
    let is_pre = rel.prerelease || semver.is_prerelease();
    if is_pre && !include_prereleases {
        return None;
    }

    let asset = rel
        .assets
        .into_iter()
        .filter(|a| {
            is_setup_asset_name(&a.name)
                && a.state
                    .as_deref()
                    .is_none_or(|s| s.eq_ignore_ascii_case("uploaded"))
        })
        .max_by_key(|a| a.size)?;

    let version = rel
        .tag_name
        .trim()
        .strip_prefix('v')
        .or_else(|| rel.tag_name.trim().strip_prefix('V'))
        .unwrap_or(rel.tag_name.trim())
        .to_string();

    let sha256 = asset.digest.as_deref().and_then(normalize_sha256_digest);
    let info = ReleaseInfo {
        tag_name: rel.tag_name,
        version,
        asset_name: asset.name,
        download_url: asset.browser_download_url,
        sha256,
        html_url: rel.html_url,
        prerelease: is_pre,
        size: asset.size,
    };
    Some((semver, info))
}

/// Parses either a single GitHub release object (`/releases/latest`) or a JSON array (`/releases`)
/// and selects the highest matching SemVer release containing a `recircles-setup*.exe` asset.
pub fn parse_github_releases(json_body: &str, include_prereleases: bool) -> Result<Option<ReleaseInfo>, String> {
    let trimmed = json_body.trim();
    let releases: Vec<GhRelease> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).map_err(|e| format!("Failed parsing GitHub releases array: {e}"))?
    } else {
        let single: GhRelease = serde_json::from_str(trimmed)
            .map_err(|e| format!("Failed parsing GitHub release JSON: {e}"))?;
        vec![single]
    };

    let best = releases
        .into_iter()
        .filter_map(|r| release_from_gh(r, include_prereleases))
        .max_by(|(va, _), (vb, _)| va.cmp(vb))
        .map(|(_, info)| info);
    Ok(best)
}

#[derive(Debug, Deserialize)]
struct MinimalManifest {
    install_dir: String,
}

/// Returns the recorded install directory from `installed.json`, if present.
pub fn installed_dir_from_manifest(config_dir: &Path) -> Option<PathBuf> {
    let raw = fs::read_to_string(paths::install_manifest_in(config_dir)).ok()?;
    let m: MinimalManifest = serde_json::from_str(&raw).ok()?;
    let trimmed = m.install_dir.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Returns true when the current executable is running from the recorded installed directory
/// (as opposed to a developer checkout like `target\release`).
pub fn is_installed_copy(config_dir: &Path) -> bool {
    let Some(inst) = installed_dir_from_manifest(config_dir) else {
        return false;
    };
    let cur = paths::exe_dir();
    let norm = |p: &Path| {
        fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    };
    norm(&inst) == norm(&cur)
}

/// Removes downloaded setup files from `<config_dir>\updates` after startup/upgrade.
pub fn cleanup_old_installers(config_dir: &Path) {
    let dir = paths::updates_dir_in(config_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
    }
}

// ---------------------------------------------------------------------------
// Pure-Rust SHA-256 (zero extra dependencies, works on every OS & in unit tests)
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_bits: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_bits: 0,
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
        self.h[5] = self.h[5].wrapping_add(f);
        self.h[6] = self.h[6].wrapping_add(g);
        self.h[7] = self.h[7].wrapping_add(h);
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((data.len() as u64) * 8);
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            let take = need.min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn finalize_hex(mut self) -> String {
        let total_bits = self.total_bits;
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            self.buf[self.buf_len..64].fill(0);
            let block = self.buf;
            self.compress(&block);
            self.buf_len = 0;
        }
        self.buf[self.buf_len..56].fill(0);
        self.buf[56..64].copy_from_slice(&total_bits.to_be_bytes());
        let block = self.buf;
        self.compress(&block);

        let mut out = String::with_capacity(64);
        for word in self.h {
            out.push_str(&format!("{word:08x}"));
        }
        out
    }
}

pub fn sha256_bytes(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize_hex()
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 32 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize_hex())
}

pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> Result<(), String> {
    let Some(norm_expected) = normalize_sha256_digest(expected_hex) else {
        return Err(format!("Invalid SHA-256 digest format: '{expected_hex}'"));
    };
    let actual = sha256_file(path)
        .map_err(|e| format!("Failed hashing {}: {e}", path.display()))?;
    if actual != norm_expected {
        let _ = fs::remove_file(path);
        return Err(format!(
            "SHA-256 mismatch for {}: expected {norm_expected}, got {actual}",
            path.display()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// URL parsing & Windows WinHTTP HTTPS client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedUrl {
    https: bool,
    host: String,
    port: u16,
    path_and_query: String,
}

fn parse_url(url: &str, allow_local_http: bool) -> Result<ParsedUrl, String> {
    let trimmed = url.trim();
    let (https, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!("Unsupported URL scheme in '{trimmed}'"));
    };

    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(format!("Missing host in URL '{trimmed}'"));
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') => {
            let port = p
                .parse::<u16>()
                .map_err(|_| format!("Invalid port in URL '{trimmed}'"))?;
            (h.to_string(), port)
        }
        _ => (authority.to_string(), if https { 443 } else { 80 }),
    };

    if !https {
        let is_local = host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1";
        if !(allow_local_http && is_local) {
            return Err(format!("Refusing insecure HTTP URL '{trimmed}'"));
        }
    }

    Ok(ParsedUrl {
        https,
        host,
        port,
        path_and_query: path_and_query.to_string(),
    })
}

fn api_base_url() -> (String, bool) {
    if let Ok(over) = std::env::var("RECIRCLES_UPDATE_API_BASE") {
        let t = over.trim().trim_end_matches('/');
        if !t.is_empty() {
            return (t.to_string(), true);
        }
    }
    ("https://api.github.com".to_string(), false)
}

#[cfg(windows)]
mod win_http {
    use super::*;
    use crate::ipc::win::to_wide_nul;
    use std::ffi::c_void;
    use windows_sys::Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_ADDREQ_FLAG_REPLACE,
        WINHTTP_FLAG_SECURE, WINHTTP_QUERY_ETAG, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE, WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
        WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    };

    struct WinHttpHandle(*mut c_void);

    impl Drop for WinHttpHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    pub struct HttpGetResult {
        pub status: u32,
        pub etag: Option<String>,
        pub body: Vec<u8>,
    }

    fn open_request(
        url: &ParsedUrl,
        extra_headers: &[String],
        recv_timeout_ms: i32,
    ) -> Result<(WinHttpHandle, WinHttpHandle, WinHttpHandle), String> {
        let ua = to_wide_nul(&format!("ReCircles-Windows/{CURRENT_VERSION}"));
        let session = unsafe {
            WinHttpOpen(
                ua.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        if session.is_null() {
            return Err("WinHttpOpen failed".into());
        }
        let session = WinHttpHandle(session);

        unsafe {
            WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, recv_timeout_ms);
        }

        let host_w = to_wide_nul(&url.host);
        let conn = unsafe { WinHttpConnect(session.0, host_w.as_ptr(), url.port, 0) };
        if conn.is_null() {
            return Err(format!("WinHttpConnect failed for {}", url.host));
        }
        let conn = WinHttpHandle(conn);

        let verb_w = to_wide_nul("GET");
        let path_w = to_wide_nul(&url.path_and_query);
        let flags = if url.https { WINHTTP_FLAG_SECURE } else { 0 };
        let req = unsafe {
            WinHttpOpenRequest(
                conn.0,
                verb_w.as_ptr(),
                path_w.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                flags,
            )
        };
        if req.is_null() {
            return Err("WinHttpOpenRequest failed".into());
        }
        let req = WinHttpHandle(req);

        for hdr in extra_headers {
            let hdr_w = to_wide_nul(hdr);
            let _ = unsafe {
                WinHttpAddRequestHeaders(
                    req.0,
                    hdr_w.as_ptr(),
                    u32::MAX,
                    WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
                )
            };
        }

        let sent = unsafe {
            WinHttpSendRequest(
                req.0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                0,
                0,
            )
        };
        if sent == 0 {
            return Err(format!("WinHttpSendRequest failed for {}", url.host));
        }
        let recvd = unsafe { WinHttpReceiveResponse(req.0, std::ptr::null_mut()) };
        if recvd == 0 {
            return Err(format!("WinHttpReceiveResponse failed for {}", url.host));
        }

        Ok((session, conn, req))
    }

    fn query_status_code(req: *mut c_void) -> Result<u32, String> {
        let mut status: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let ok = unsafe {
            WinHttpQueryHeaders(
                req,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut size,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err("Failed querying HTTP status code".into());
        }
        Ok(status)
    }

    fn query_etag(req: *mut c_void) -> Option<String> {
        let mut buf = [0u16; 256];
        let mut byte_len = (buf.len() * 2) as u32;
        let ok = unsafe {
            WinHttpQueryHeaders(
                req,
                WINHTTP_QUERY_ETAG,
                std::ptr::null(),
                buf.as_mut_ptr().cast(),
                &mut byte_len,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || byte_len == 0 {
            return None;
        }
        let u16_len = (byte_len as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..u16_len]);
        let trimmed = s.trim_matches('\0').trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    pub fn http_get_json(
        url_str: &str,
        etag: Option<&str>,
        allow_local_http: bool,
    ) -> Result<HttpGetResult, String> {
        let url = parse_url(url_str, allow_local_http)?;
        let mut headers = vec![
            "Accept: application/vnd.github+json".to_string(),
            "X-GitHub-Api-Version: 2022-11-28".to_string(),
        ];
        if let Some(tag) = etag
            && !tag.trim().is_empty()
        {
            headers.push(format!("If-None-Match: {}", tag.trim()));
        }

        let (_s, _c, req) = open_request(&url, &headers, 20_000)?;
        let status = query_status_code(req.0)?;
        let resp_etag = query_etag(req.0);

        let mut body = Vec::new();
        if status != 304 {
            let mut chunk = [0u8; 16 * 1024];
            loop {
                let mut avail: u32 = 0;
                if unsafe { WinHttpQueryDataAvailable(req.0, &mut avail) } == 0 {
                    break;
                }
                if avail == 0 {
                    break;
                }
                let to_read = (avail as usize).min(chunk.len()) as u32;
                let mut read: u32 = 0;
                if unsafe {
                    WinHttpReadData(req.0, chunk.as_mut_ptr().cast(), to_read, &mut read)
                } == 0
                {
                    return Err("WinHttpReadData failed".into());
                }
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read as usize]);
                if body.len() > 4 * 1024 * 1024 {
                    return Err("GitHub API response exceeded 4 MB limit".into());
                }
            }
        }

        Ok(HttpGetResult {
            status,
            etag: resp_etag,
            body,
        })
    }

    pub fn http_download_file(
        url_str: &str,
        dest_part: &Path,
        allow_local_http: bool,
    ) -> Result<(), String> {
        let url = parse_url(url_str, allow_local_http)?;
        let headers = vec!["Accept: application/octet-stream".to_string()];
        let (_s, _c, req) = open_request(&url, &headers, 60_000)?;
        let status = query_status_code(req.0)?;
        if !(200..300).contains(&status) {
            return Err(format!("Download failed with HTTP {status}"));
        }

        let mut file = File::create(dest_part)
            .map_err(|e| format!("Failed creating {}: {e}", dest_part.display()))?;
        let mut chunk = [0u8; 64 * 1024];
        let mut total_bytes: u64 = 0;
        loop {
            let mut avail: u32 = 0;
            if unsafe { WinHttpQueryDataAvailable(req.0, &mut avail) } == 0 {
                let _ = fs::remove_file(dest_part);
                return Err("WinHttpQueryDataAvailable failed during download".into());
            }
            if avail == 0 {
                break;
            }
            let to_read = (avail as usize).min(chunk.len()) as u32;
            let mut read: u32 = 0;
            if unsafe { WinHttpReadData(req.0, chunk.as_mut_ptr().cast(), to_read, &mut read) } == 0
            {
                let _ = fs::remove_file(dest_part);
                return Err("WinHttpReadData failed during download".into());
            }
            if read == 0 {
                break;
            }
            total_bytes += read as u64;
            if total_bytes > 256 * 1024 * 1024 {
                let _ = fs::remove_file(dest_part);
                return Err("Downloaded installer exceeded 256 MB safety limit".into());
            }
            if let Err(e) = file.write_all(&chunk[..read as usize]) {
                let _ = fs::remove_file(dest_part);
                return Err(format!("Failed writing download chunk: {e}"));
            }
        }
        file.flush()
            .map_err(|e| format!("Failed flushing {}: {e}", dest_part.display()))?;
        Ok(())
    }
}

/// Checks GitHub Releases for a newer version of ReCircles for Windows.
/// Uses cached `ETag` (`If-None-Match`) to avoid consuming GitHub API rate limits on unchanged releases.
pub fn check_for_update(config_dir: &Path, include_prereleases: bool) -> Result<CheckOutcome, String> {
    check_for_update_against(config_dir, CURRENT_VERSION, include_prereleases)
}

pub fn check_for_update_against(
    config_dir: &Path,
    current_version: &str,
    include_prereleases: bool,
) -> Result<CheckOutcome, String> {
    #[cfg(not(windows))]
    {
        let _ = (config_dir, current_version, include_prereleases);
        return Err("Update network checks are supported on Windows only".into());
    }

    #[cfg(windows)]
    {
        let mut state = UpdateState::load(config_dir);
        let (base, allow_local_http) = api_base_url();
        let url = if include_prereleases {
            format!("{base}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases?per_page=10")
        } else {
            format!("{base}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/latest")
        };

        let cached_etag = if state.etag_include_prereleases == include_prereleases {
            state.etag.as_deref()
        } else {
            None
        };

        let resp = match win_http::http_get_json(&url, cached_etag, allow_local_http) {
            Ok(r) => r,
            Err(e) => {
                state.last_error = Some(e.clone());
                let _ = state.save(config_dir);
                return Err(e);
            }
        };

        state.last_checked_unix = now_unix_secs();
        state.etag_include_prereleases = include_prereleases;

        match resp.status {
            304 => {
                state.last_error = None;
                let _ = state.save(config_dir);
                if let Some(rel) = state.latest_release.clone()
                    && is_newer_version(&rel.version, current_version)
                {
                    return Ok(CheckOutcome::UpdateAvailable(rel));
                }
                return Ok(CheckOutcome::UpToDate {
                    current: current_version.to_string(),
                    latest: state.latest_release.map(|r| r.version),
                });
            }
            200..=299 => {
                let text = String::from_utf8(resp.body)
                    .map_err(|_| "GitHub API response was not valid UTF-8".to_string())?;
                let parsed = parse_github_releases(&text, include_prereleases)?;
                state.etag = resp.etag;
                state.latest_release = parsed.clone();
                state.last_error = None;
                let _ = state.save(config_dir);

                if let Some(rel) = parsed {
                    if is_newer_version(&rel.version, current_version) {
                        return Ok(CheckOutcome::UpdateAvailable(rel));
                    }
                    return Ok(CheckOutcome::UpToDate {
                        current: current_version.to_string(),
                        latest: Some(rel.version),
                    });
                }
                return Ok(CheckOutcome::UpToDate {
                    current: current_version.to_string(),
                    latest: None,
                });
            }
            404 => {
                state.etag = None;
                state.latest_release = None;
                state.last_error = None;
                let _ = state.save(config_dir);
                return Ok(CheckOutcome::UpToDate {
                    current: current_version.to_string(),
                    latest: None,
                });
            }
            code => {
                let err = format!("GitHub Releases API returned HTTP {code}");
                state.last_error = Some(err.clone());
                let _ = state.save(config_dir);
                return Err(err);
            }
        }
    }
}

/// Downloads the release's `recircles-setup-*.exe` into `<config_dir>\updates\`,
/// verifies its SHA-256 digest when provided by GitHub, and returns the verified executable path.
pub fn download_update(config_dir: &Path, release: &ReleaseInfo) -> Result<PathBuf, String> {
    #[cfg(not(windows))]
    {
        let _ = (config_dir, release);
        return Err("Update downloads are supported on Windows only".into());
    }

    #[cfg(windows)]
    {
        let updates_dir = paths::updates_dir_in(config_dir);
        fs::create_dir_all(&updates_dir)
            .map_err(|e| format!("Failed creating {}: {e}", updates_dir.display()))?;

        // Sanitize asset filename to prevent any path traversal
        let safe_name = Path::new(&release.asset_name)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| is_setup_asset_name(s))
            .unwrap_or("recircles-setup-update.exe");

        let final_path = updates_dir.join(safe_name);

        // Reuse already-downloaded file if its SHA-256 already matches
        if final_path.is_file()
            && let Some(expected) = release.sha256.as_deref()
            && verify_file_sha256(&final_path, expected).is_ok()
        {
            return Ok(final_path);
        }

        let part_path = updates_dir.join(format!("{safe_name}.part.{}", std::process::id()));
        let (_, allow_local_http) = api_base_url();
        win_http::http_download_file(&release.download_url, &part_path, allow_local_http)?;

        if let Some(expected) = release.sha256.as_deref() {
            verify_file_sha256(&part_path, expected)?;
        }

        let _ = fs::remove_file(&final_path);
        fs::rename(&part_path, &final_path).map_err(|e| {
            let _ = fs::remove_file(&part_path);
            format!("Failed staging installer {}: {e}", final_path.display())
        })?;
        Ok(final_path)
    }
}

/// Spawns the verified setup executable in non-destructive `--upgrade` mode.
#[cfg(windows)]
pub fn launch_installer_upgrade(
    installer_path: &Path,
    install_dir: &Path,
    config_dir: &Path,
    wait_pid: Option<u32>,
    from_version: &str,
    to_version: &str,
    headless: bool,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    if !installer_path.is_file() {
        return Err(format!("Installer missing at {}", installer_path.display()));
    }
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut cmd = Command::new(installer_path);
    cmd.arg("--upgrade")
        .arg("--install-dir")
        .arg(install_dir)
        .arg("--config-dir")
        .arg(config_dir)
        .arg("--from-version")
        .arg(from_version)
        .arg("--to-version")
        .arg(to_version);
    if let Some(pid) = wait_pid {
        cmd.arg("--wait-pid").arg(pid.to_string());
    }
    if headless {
        cmd.arg("--headless");
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed launching updater {}: {e}", installer_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parsing_and_ordering() {
        let v_beta = SemVer::parse("v0.1.0-beta").unwrap();
        let v_beta2 = SemVer::parse("0.1.0-beta.2").unwrap();
        let v_beta10 = SemVer::parse("0.1.0-beta.10").unwrap();
        let v_rc = SemVer::parse("v0.1.0-rc.1").unwrap();
        let v_stable = SemVer::parse("v0.1.0").unwrap();
        let v_next = SemVer::parse("0.2.0").unwrap();

        assert!(v_beta < v_beta2);
        assert!(v_beta2 < v_beta10);
        assert!(v_beta10 < v_rc);
        assert!(v_rc < v_stable);
        assert!(v_stable < v_next);

        assert!(is_newer_version("v0.1.1-beta", "0.1.0-beta"));
        assert!(is_newer_version("0.1.0", "0.1.0-beta"));
        assert!(!is_newer_version("v0.1.0-beta", "0.1.0-beta"));
        assert!(!is_newer_version("v0.0.9", "0.1.0-beta"));
        assert!(SemVer::parse("not-a-version").is_none());
    }

    #[test]
    fn sha256_matches_nist_vectors() {
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_bytes(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn parses_github_releases_with_prereleases_drafts_and_digests() {
        let payload = r#"[
            {
                "tag_name": "v0.3.0-draft",
                "draft": true,
                "prerelease": false,
                "assets": [
                    {"name": "recircles-setup-v0.3.0.exe", "browser_download_url": "https://example.com/draft.exe", "size": 100}
                ]
            },
            {
                "tag_name": "v0.1.0-beta",
                "draft": false,
                "prerelease": true,
                "html_url": "https://github.com/RekoSK/ReCircles-windows/releases/tag/v0.1.0-beta",
                "assets": [
                    {
                        "name": "recircles-setup-v0.1.0-beta.exe",
                        "browser_download_url": "https://github.com/RekoSK/ReCircles-windows/releases/download/v0.1.0-beta/recircles-setup-v0.1.0-beta.exe",
                        "digest": "sha256:3f9264ce9260b698fa00a33e0123456789abcdef0123456789abcdef01234567",
                        "size": 6200000,
                        "state": "uploaded"
                    }
                ]
            },
            {
                "tag_name": "v0.1.1-beta",
                "draft": false,
                "prerelease": true,
                "html_url": "https://github.com/RekoSK/ReCircles-windows/releases/tag/v0.1.1-beta",
                "assets": [
                    {
                        "name": "recircles-setup-v0.1.1-beta.exe",
                        "browser_download_url": "https://github.com/RekoSK/ReCircles-windows/releases/download/v0.1.1-beta/recircles-setup-v0.1.1-beta.exe",
                        "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "size": 6300000,
                        "state": "uploaded"
                    }
                ]
            }
        ]"#;

        // Prereleases enabled -> picks v0.1.1-beta, ignores v0.3.0-draft
        let rel = parse_github_releases(payload, true).unwrap().unwrap();
        assert_eq!(rel.version, "0.1.1-beta");
        assert_eq!(rel.asset_name, "recircles-setup-v0.1.1-beta.exe");
        assert_eq!(
            rel.sha256.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );

        // Stable-only -> skips prereleases
        assert!(parse_github_releases(payload, false).unwrap().is_none());
    }

    #[test]
    fn verify_file_sha256_accepts_valid_and_deletes_tampered() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("recircles_upd_test_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("setup.exe");
        fs::write(&p, b"hello recircles update").unwrap();
        let digest = format!("sha256:{}", sha256_bytes(b"hello recircles update"));
        assert!(verify_file_sha256(&p, &digest).is_ok());
        assert!(p.is_file());

        let bad = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
        assert!(verify_file_sha256(&p, bad).is_err());
        assert!(!p.exists(), "tampered download must be removed immediately");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_url_enforces_https() {
        assert!(parse_url("https://api.github.com/repos/RekoSK/ReCircles-windows/releases", false).is_ok());
        assert!(parse_url("http://api.github.com/repos/RekoSK/ReCircles-windows/releases", false).is_err());
        assert!(parse_url("http://127.0.0.1:8080/releases", false).is_err());
        assert!(parse_url("http://127.0.0.1:8080/releases", true).is_ok());
    }
}
