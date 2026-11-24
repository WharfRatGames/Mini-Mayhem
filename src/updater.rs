use crate::renderer::{WorldBuffer, Framebuffer};

// ── Linux desktop stubs — no OTA, update via git pull ────────────────────────
#[cfg(all(feature = "desktop", not(target_os = "windows")))]
pub fn prior_update_attempted() -> bool { true }

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
pub fn sync_assets_bg(_version: &'static str) {}

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
pub fn check_for_update(_current: &str) -> (bool, bool) { (false, false) }

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
pub fn check_for_update_bg(_current: &'static str) -> std::thread::JoinHandle<(bool, bool)> {
    std::thread::spawn(|| (false, false))
}

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
pub fn fetch_changelog(_timeout_secs: u64) -> Option<Vec<String>> { None }

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
pub fn stream_binary<F: FnMut(usize, usize)>(_on_progress: F) -> Option<Vec<u8>> { None }

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
pub fn apply_binary(_binary: &[u8], _buf: &mut WorldBuffer, _fb: &mut Framebuffer) {}

#[cfg(all(feature = "desktop", not(target_os = "windows")))]
pub fn download_and_apply(_buf: &mut WorldBuffer, _fb: &mut Framebuffer) {}

// ── Windows OTA update ────────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
const UPDATE_HOST: &str = "crumbonium.duckdns.org";

#[cfg(target_os = "windows")]
fn http_get_body(path: &str, timeout_secs: u64) -> Option<Vec<u8>> {
    crate::https::https_get(UPDATE_HOST, path, timeout_secs, timeout_secs)
        .ok()
        .or_else(|| crate::https::http_get(UPDATE_HOST, path, timeout_secs, timeout_secs).ok())
}

#[cfg(target_os = "windows")]
pub fn prior_update_attempted() -> bool { false }

#[cfg(target_os = "windows")]
pub fn sync_assets_bg(_version: &'static str) {}

#[cfg(target_os = "windows")]
pub fn check_for_update(current: &str) -> (bool, bool) {
    if crate::net::EXPERIMENTAL { return (false, false); }
    let body = match http_get_body("/arty/version.txt", 3) {
        Some(b) => b,
        None => return (false, false),
    };
    let server_ver = String::from_utf8_lossy(&body).trim().to_string();
    (server_ver != current, false)
}

#[cfg(target_os = "windows")]
pub fn check_for_update_bg(current: &'static str) -> std::thread::JoinHandle<(bool, bool)> {
    std::thread::spawn(move || check_for_update(current))
}

#[cfg(target_os = "windows")]
pub fn fetch_changelog(timeout_secs: u64) -> Option<Vec<String>> {
    let body = http_get_body("/arty/changelog.txt", timeout_secs)?;
    let lines: Vec<String> = String::from_utf8_lossy(&body)
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .collect();
    if lines.is_empty() { None } else { Some(lines) }
}

#[cfg(target_os = "windows")]
pub fn stream_binary<F: FnMut(usize, usize)>(mut on_progress: F) -> Option<Vec<u8>> {
    use std::io::Write;
    let body = http_get_body("/arty/mini-mayhem.exe", 120)?;
    // Must start with MZ (PE header)
    if body.len() < 2 || body[0] != b'M' || body[1] != b'Z' { return None; }
    on_progress(body.len(), body.len());
    let tmp = std::env::temp_dir().join("mini-mayhem.new.exe");
    std::fs::File::create(&tmp).ok()?.write_all(&body).ok()?;
    // Sentinel value — actual file is on disk
    Some(b"MZ_ONDISK".to_vec())
}

#[cfg(target_os = "windows")]
pub fn apply_binary(_binary: &[u8], _buf: &mut WorldBuffer, _fb: &mut Framebuffer) {
    let dest = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("mini-mayhem.exe"));
    let dest_str = dest.to_str().unwrap_or("mini-mayhem.exe");
    let tmp = std::env::temp_dir().join("mini-mayhem.new.exe");
    let tmp_str = tmp.to_str().unwrap_or("mini-mayhem.new.exe");
    // Write a .bat that waits for this process to exit, swaps the exe, relaunches.
    let bat = format!(
        "@echo off\r\ntimeout /t 2 /nobreak >nul\r\ncopy /y \"{tmp}\" \"{dest}\" >nul\r\ndel \"{tmp}\" >nul\r\nstart \"\" \"{dest}\"\r\n",
        tmp = tmp_str, dest = dest_str
    );
    let bat_path = std::env::temp_dir().join("mini-mayhem-update.bat");
    if std::fs::write(&bat_path, bat.as_bytes()).is_err() { return; }
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "/B", bat_path.to_str().unwrap_or("")])
        .spawn();
    std::process::exit(0);
}

#[cfg(target_os = "windows")]
pub fn download_and_apply(_buf: &mut WorldBuffer, _fb: &mut Framebuffer) {}

// ── Miyoo implementation ──────────────────────────────────────────────────────

#[cfg(not(feature = "desktop"))]
use std::io::Write;
#[cfg(not(feature = "desktop"))]
use sha2::{Sha256, Digest};

#[cfg(not(feature = "desktop"))]
fn sha256_file(path: &std::path::Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Some(hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

#[cfg(not(feature = "desktop"))]
fn needs_update(fpath: &std::path::Path, expected_size: u64, expected_hash: Option<&str>) -> bool {
    let meta = match std::fs::metadata(fpath) {
        Ok(m) => m,
        Err(_) => return true,
    };
    if meta.len() != expected_size { return true; }
    match expected_hash {
        Some(h) => sha256_file(fpath).as_deref() != Some(h),
        None => false,
    }
}

#[cfg(not(feature = "desktop"))]
const UPDATE_HOST: &str = "crumbonium.duckdns.org";
/// Fallback LAN IP for when the router doesn't support hairpin NAT (Miyoos on home network).
/// http_get_body tries this if the public hostname TCP connect fails.
const UPDATE_HOST_LAN: &str = "10.0.0.123";

#[cfg(not(feature = "desktop"))]
fn http_get_body(path: &str, timeout_secs: u64) -> Option<(Vec<u8>, bool)> {
    match crate::https::https_get(UPDATE_HOST, path, timeout_secs, timeout_secs) {
        Ok(b) => return Some((b, false)),
        Err(_) => {}
    }
    // HTTPS failed; try plain HTTP via public hostname, then fall back to direct LAN IP
    // (preserving the Host header so nginx serves the right vhost).
    if let Ok(b) = crate::https::http_get(UPDATE_HOST, path, timeout_secs, timeout_secs) {
        return Some((b, true));
    }
    crate::https::http_get_via(UPDATE_HOST_LAN, UPDATE_HOST, path, timeout_secs, timeout_secs)
        .ok()
        .map(|b| (b, true))
}

#[cfg(not(feature = "desktop"))]
pub fn check_for_update_bg(current: &'static str) -> std::thread::JoinHandle<(bool, bool)> {
    std::thread::spawn(move || check_for_update(current))
}

/// URL prefix of the update channel this build follows.
#[cfg(not(feature = "desktop"))]
fn channel_base() -> &'static str {
    if crate::net::EXPERIMENTAL { crate::ota::EXP_BASE } else { "/arty" }
}

/// Binary file name on the server.
#[cfg(not(feature = "desktop"))]
fn channel_bin() -> &'static str {
    if crate::net::EXPERIMENTAL { crate::ota::EXP_BIN } else { "mini-mayhem" }
}

/// Where the download lands (tmpfs on the device).
#[cfg(not(feature = "desktop"))]
fn tmp_new() -> &'static str {
    if crate::net::EXPERIMENTAL { "/tmp/arty-exp.new" } else { "/tmp/mini-mayhem.new" }
}

/// Experimental channel: the published `version.txt` (version, size, sha256), if reachable and well-formed.
#[cfg(not(feature = "desktop"))]
fn fetch_ota_info(timeout_secs: u64) -> Option<crate::ota::OtaInfo> {
    let (body, _) = http_get_body(&format!("{}/version.txt", crate::ota::EXP_BASE), timeout_secs)?;
    crate::ota::parse_version_file(&body)
}

/// Info of the build `stream_binary` last downloaded and verified (so `apply_binary` knows its size and target version).
#[cfg(not(feature = "desktop"))]
static LAST_OTA: std::sync::Mutex<Option<crate::ota::OtaInfo>> = std::sync::Mutex::new(None);

/// Returns (update_available, tls_broken).
/// tls_broken=true means HTTPS failed and we fell back to HTTP — force the update, no skip.
#[cfg(not(feature = "desktop"))]
pub fn check_for_update(current: &str) -> (bool, bool) {
    // Experimental builds follow their OWN channel (/arty/exp, see ota.rs), never the production release.
    if crate::net::EXPERIMENTAL {
        return match fetch_ota_info(3) {
            Some(info) => (crate::ota::is_newer(&info.version, crate::VERSION), false),
            None => (false, false),
        };
    }
    // HTTPS first; fall back to HTTP (port 80) then LAN IP for hairpin-NAT routers.
    let body =
        crate::https::https_get(UPDATE_HOST, "/arty/version.txt", 2, 2)
        .or_else(|_| crate::https::http_get(UPDATE_HOST, "/arty/version.txt", 2, 2))
        .or_else(|_| crate::https::http_get_via(UPDATE_HOST_LAN, UPDATE_HOST, "/arty/version.txt", 2, 2));
    let body = match body {
        Ok(b) => b,
        Err(_) => return (false, false),
    };
    let server_ver = String::from_utf8_lossy(&body).trim().to_string();
    (server_ver != current, false)
}

#[cfg(not(feature = "desktop"))]
pub fn fetch_changelog(timeout_secs: u64) -> Option<Vec<String>> {
    let (body, _) = http_get_body(&format!("{}/changelog.txt", channel_base()), timeout_secs)?;
    let lines: Vec<String> = String::from_utf8_lossy(&body)
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.trim().is_empty())
        .take(5) // most recent entries only — fits the update screen at scale 2
        .collect();
    if lines.is_empty() { None } else { Some(lines) }
}

/// Check sfx assets once per version. Runs in background on first launch after
/// a new binary is installed (direct copy or OTA). No-op on subsequent runs of
/// the same version. Writes a sentinel like `.sfx_0.5.4.348` when done.
#[cfg(not(feature = "desktop"))]
pub fn sync_assets_bg(version: &'static str) {
    if crate::net::EXPERIMENTAL { return; }
    std::thread::spawn(move || {
        let dest = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("/mnt/SDCARD/App/Arty/mini-mayhem"));
        let app_dir = dest.parent()
            .unwrap_or(std::path::Path::new("/mnt/SDCARD/App/Arty"))
            .to_path_buf();
        // Skip if sfx are already up-to-date for this version.
        let sentinel = app_dir.join(format!(".sfx_{}", version));
        if sentinel.exists() { return; }
        let manifest = match http_get_body("/arty/manifest.txt", 5).map(|(b,_)| b) {
            Some(m) => m,
            None => return,
        };
        let mut all_ok = true;
        for line in String::from_utf8_lossy(&manifest).lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let fpath = app_dir.join(parts[0]);
            let expected: u64 = parts[1].parse().unwrap_or(0);
            let expected_hash = parts.get(2).copied();
            if !needs_update(&fpath, expected, expected_hash) { continue; }
            match http_get_body(&format!("/arty/{}", parts[0]), 10) {
                Some((data, _)) => {
                    if let Some(parent) = fpath.parent() { let _ = std::fs::create_dir_all(parent); }
                    if std::fs::write(&fpath, &data).is_err() { all_ok = false; }
                }
                None => { all_ok = false; }
            }
        }
        // Only stamp sentinel if everything downloaded successfully.
        if all_ok { let _ = std::fs::write(&sentinel, b""); }
    });
}

/// HTTP fallback for stream_binary: downloads via LAN IP over plain HTTP.
/// Used when the HTTPS connect to the public hostname fails (router hairpin NAT absent).
#[cfg(not(feature = "desktop"))]
fn stream_binary_http<F: FnMut(usize, usize)>(mut on_progress: F) -> Option<Vec<u8>> {
    use std::io::Write;
    let body = crate::https::http_get_via(UPDATE_HOST_LAN, UPDATE_HOST, &format!("{}/{}", channel_base(), channel_bin()), 10, 120).ok()?;
    if body.len() < 4 { return None; }
    let tmp_path = tmp_new();
    let mut f = std::fs::File::create(tmp_path).ok()?;
    f.write_all(&body).ok()?;
    on_progress(body.len(), body.len());
    if body[0] != 0x7f || body[1] != b'E' { return None; }
    Some(b"\x7fELF_ONDISK".to_vec())
}

#[cfg(not(feature = "desktop"))]
// Streams the binary directly to /tmp/mini-mayhem.new to avoid buffering 26MB in RAM.
// Returns Some(path) on success, None on failure.
pub fn stream_binary<F: FnMut(usize, usize)>(mut on_progress: F) -> Option<Vec<u8>> {
    if !crate::net::EXPERIMENTAL { return stream_binary_once(&mut on_progress); }
    // Experimental channel: only ever accept a build that matches the published size + sha256, retrying a flaky link a few times.
    for attempt in 0..3 {
        let info = match fetch_ota_info(5) { Some(i) => i, None => { std::thread::sleep(std::time::Duration::from_millis(500 * (attempt + 1))); continue; } };
        if !crate::ota::is_newer(&info.version, crate::VERSION) { return None; }
        // last attempt goes straight to plain HTTP (via the LAN IP): covers a TLS stream that connects but keeps dropping
        let got = if attempt == 2 { stream_binary_http(|d, t| on_progress(d, t)) } else { stream_binary_once(&mut on_progress) };
        let ok = got.is_some() && crate::ota::verify_file(std::path::Path::new(tmp_new()), &info);
        if ok {
            *LAST_OTA.lock().unwrap() = Some(info);
            return got;
        }
        let _ = std::fs::remove_file(tmp_new());
        crate::dlog!("ota", "download attempt {} failed verification", attempt + 1);
    }
    None
}

#[cfg(not(feature = "desktop"))]
fn stream_binary_once<F: FnMut(usize, usize)>(on_progress: &mut F) -> Option<Vec<u8>> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use rustls::pki_types::ServerName;

    let req = format!("GET {}/{} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", channel_base(), channel_bin(), UPDATE_HOST);
    let addr = match (UPDATE_HOST, 443u16).to_socket_addrs().ok().and_then(|mut a| a.next()) {
        Some(a) => a,
        None => return stream_binary_http(|d, t| on_progress(d, t)), // DNS failed, go straight to LAN HTTP
    };
    let tcp = match TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(10)) {
        Ok(t) => t,
        Err(_) => return stream_binary_http(|d, t| on_progress(d, t)), // TCP failed (hairpin NAT), try LAN HTTP
    };
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(120))).ok();

    let config = crate::https::make_tls_config();
    let server_name: ServerName<'static> = ServerName::try_from(UPDATE_HOST.to_string()).ok()?;
    let mut conn = rustls::ClientConnection::new(config, server_name).ok()?;
    let mut tcp = tcp;
    let mut stream = rustls::Stream::new(&mut conn, &mut tcp);

    stream.write_all(req.as_bytes()).ok()?;

    // Read headers byte-by-byte until \r\n\r\n
    let mut header_buf: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if stream.read_exact(&mut byte).is_err() { break; }
        header_buf.push(byte[0]);
        if header_buf.ends_with(b"\r\n\r\n") || header_buf.ends_with(b"\n\n") { break; }
        if header_buf.len() > 8192 { break; }
    }
    let header_str = String::from_utf8_lossy(&header_buf);
    let total: usize = header_str.lines()
        .find(|l| l.to_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);

    // Stream directly to disk — avoid holding the full binary in RAM
    let tmp_path = tmp_new();
    let mut file = std::fs::File::create(tmp_path).ok()?;
    let mut written = 0usize;
    let mut last_reported = 0usize;
    let mut chunk = [0u8; 32768];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if file.write_all(&chunk[..n]).is_err() { return None; }
                written += n;
                if total == 0 || written - last_reported > total / 20 {
                    last_reported = written;
                    on_progress(written, total);
                }
            }
            Err(_) => break,
        }
    }
    on_progress(written, total);
    if written < 4 { return None; }

    // Read back just the ELF header to verify, return a sentinel vec so the
    // caller's ELF check passes — actual binary is already on disk.
    let mut hdr = [0u8; 4];
    std::fs::File::open(tmp_path).ok()?.read_exact(&mut hdr).ok()?;
    if hdr[0] == 0x7f && &hdr[1..4] == b"ELF" {
        // Signal success with a magic sentinel — apply_binary will use the file on disk
        Some(b"\x7fELF_ONDISK".to_vec())
    } else {
        None
    }
}

#[cfg(not(feature = "desktop"))]
const SENTINEL: &str = "/tmp/mini-mayhem_update_sentinel";

#[cfg(not(feature = "desktop"))]
const SENTINEL_EXP: &str = "/tmp/arty-exp_update_sentinel";

#[cfg(not(feature = "desktop"))]
pub fn prior_update_attempted() -> bool {
    if crate::net::EXPERIMENTAL {
        // Experimental: the marker holds the version we tried to install. Skip the start-up gate only if that attempt did not take
        // (running something else), which prevents retry loops; after a good update the next published build is offered normally.
        let s = std::fs::read_to_string(SENTINEL_EXP).ok();
        return crate::ota::gate_skipped(s.as_deref(), crate::VERSION);
    }
    std::path::Path::new(SENTINEL).exists()
}

/// Experimental builds: call once at start-up. After the build has run `ota::BOOT_OK_SECS` the update is considered good and the
/// roll-back marker next to the binary is removed (see `ota::apply_script`). No-op elsewhere.
pub fn mark_boot_ok() {
    if !crate::net::EXPERIMENTAL { return; }
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(crate::ota::BOOT_OK_SECS));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() { let _ = std::fs::remove_file(dir.join(".ota_pending")); }
        }
    });
}

#[cfg(not(feature = "desktop"))]
pub fn apply_binary(binary: &[u8], buf: &mut WorldBuffer, fb: &mut Framebuffer) {
    if crate::net::EXPERIMENTAL { return apply_binary_exp(buf, fb); }
    let dest = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("/mnt/SDCARD/App/Arty/mini-mayhem"));
    let dest_str = dest.to_str().unwrap_or("/mnt/SDCARD/App/Arty/mini-mayhem");

    // If stream_binary already wrote the file to disk, skip the in-memory write.
    let ondisk = binary == b"\x7fELF_ONDISK";
    if !ondisk {
        let mut f = match std::fs::File::create("/tmp/mini-mayhem.new") {
            Ok(f) => f,
            Err(_) => { super::draw_msg(buf, fb, "FAIL:TMPWRITE"); std::thread::sleep(std::time::Duration::from_secs(2)); return; }
        };
        use std::io::Write;
        if f.write_all(binary).is_err() {
            super::draw_msg(buf, fb, "FAIL:WRITE");
            std::thread::sleep(std::time::Duration::from_secs(2));
            return;
        }
    }
    unsafe { libc::chmod(b"/tmp/mini-mayhem.new\0".as_ptr() as *const libc::c_char, 0o755); }

    // Update script: copy new binary then exec it.
    // Uses && so exec only runs if cp succeeded — prevents exec-old-binary loop.
    let script = format!("#!/bin/sh\ncp /tmp/mini-mayhem.new '{}' && chmod +x '{}' && exec '{}'\n",
        dest_str, dest_str, dest_str);
    if std::fs::write("/tmp/mini-mayhem_update.sh", script.as_bytes()).is_err() {
        super::draw_msg(buf, fb, "FAIL:SCRIPT");
        std::thread::sleep(std::time::Duration::from_secs(2));
        return;
    }
    unsafe { libc::chmod(b"/tmp/mini-mayhem_update.sh\0".as_ptr() as *const libc::c_char, 0o755); }

    // Fetch updated app files (short timeout — don't block the restart)
    if let Some((manifest, _)) = http_get_body("/arty/manifest.txt", 2) {
        let app_dir = std::path::Path::new(dest_str).parent()
            .unwrap_or(std::path::Path::new("/mnt/SDCARD/App/Arty"));
        for line in String::from_utf8_lossy(&manifest).lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let fpath = app_dir.join(parts[0]);
            let expected: u64 = parts[1].parse().unwrap_or(0);
            let expected_hash = parts.get(2).copied();
            if needs_update(&fpath, expected, expected_hash) {
                if let Some((data, _)) = http_get_body(&format!("/arty/{}", parts[0]), 5) {
                    if let Some(parent) = fpath.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(&fpath, &data);
                }
            }
        }
    }

    // Write sentinel before exec. If the update fails (cp fails, shell script doesn't
    // reach exec, or execs the old binary), the sentinel persists and prior_update_attempted()
    // returns true next launch, breaking the retry loop. /tmp clears on reboot.
    let _ = std::fs::write(SENTINEL, b"1");

    // Close all inherited file descriptors (framebuffer, input, etc.) before exec
    // so the new process starts clean without duplicate device handles.
    for fd in 3..=255i32 {
        unsafe { libc::close(fd); }
    }

    // Replace this process with a shell that copies the binary and relaunches
    let sh  = std::ffi::CString::new("/bin/sh").unwrap();
    let arg = std::ffi::CString::new("/tmp/mini-mayhem_update.sh").unwrap();
    let args: [*const libc::c_char; 3] = [sh.as_ptr(), arg.as_ptr(), std::ptr::null()];
    unsafe { libc::execv(sh.as_ptr(), args.as_ptr()); }
    std::process::exit(0);
}

/// Experimental channel install: the verified download is at `tmp_new()`; hand over to `ota::apply_script` (atomic replace, keeps
/// `.prev`, rolls back a build that crashes at start, always relaunches).
#[cfg(not(feature = "desktop"))]
fn apply_binary_exp(buf: &mut WorldBuffer, fb: &mut Framebuffer) {
    let fail = |buf: &mut WorldBuffer, fb: &mut Framebuffer, m: &str| {
        super::draw_msg(buf, fb, m);
        std::thread::sleep(std::time::Duration::from_secs(2));
    };
    let info = match LAST_OTA.lock().unwrap().clone() { Some(i) => i, None => return fail(buf, fb, "FAIL:NOINFO") };
    // Re-check what is on disk right before installing (the file sits in /tmp between download and here).
    if !crate::ota::verify_file(std::path::Path::new(tmp_new()), &info) { return fail(buf, fb, "FAIL:VERIFY"); }
    let dest = match std::env::current_exe() { Ok(d) => d, Err(_) => return fail(buf, fb, "FAIL:EXE") };
    let dest_str = match dest.to_str() { Some(d) => d.to_string(), None => return fail(buf, fb, "FAIL:EXE") };
    let script = crate::ota::apply_script(&dest_str, tmp_new(), info.size);
    let script_path = "/tmp/arty-exp_update.sh";
    if std::fs::write(script_path, script.as_bytes()).is_err() { return fail(buf, fb, "FAIL:SCRIPT"); }
    unsafe { libc::chmod(b"/tmp/arty-exp_update.sh\0".as_ptr() as *const libc::c_char, 0o755); }
    crate::dlog!("ota", "installing {} (from {})", info.version, crate::VERSION);
    let _ = std::fs::write(SENTINEL_EXP, info.version.as_bytes());
    for fd in 3..=255i32 { unsafe { libc::close(fd); } }
    let sh  = std::ffi::CString::new("/bin/sh").unwrap();
    let arg = std::ffi::CString::new(script_path).unwrap();
    let args: [*const libc::c_char; 3] = [sh.as_ptr(), arg.as_ptr(), std::ptr::null()];
    unsafe { libc::execv(sh.as_ptr(), args.as_ptr()); }
    std::process::exit(0);
}

#[cfg(not(feature = "desktop"))]
pub fn download_and_apply(buf: &mut WorldBuffer, fb: &mut Framebuffer) {
    if crate::net::EXPERIMENTAL { return; } // experimental builds use stream_binary + apply_binary (verified, rollback-safe)
    super::draw_msg(buf, fb, "DOWNLOADING UPDATE...");
    let binary = match http_get_body("/arty/mini-mayhem", 120).map(|(b,_)| b) {
        Some(b) => b,
        None => { super::draw_msg(buf, fb, "FAIL:DOWNLOAD"); std::thread::sleep(std::time::Duration::from_secs(2)); return; }
    };
    if binary.len() < 4 || binary[0] != 0x7f || &binary[1..4] != b"ELF" {
        super::draw_msg(buf, fb, "FAIL:ELF");
        std::thread::sleep(std::time::Duration::from_secs(2));
        return;
    }
    // Write binary to /tmp
    {
        let mut f = match std::fs::File::create("/tmp/mini-mayhem.new") {
            Ok(f) => f,
            Err(_) => { super::draw_msg(buf, fb, "FAIL:TMPWRITE"); std::thread::sleep(std::time::Duration::from_secs(2)); return; }
        };
        if f.write_all(&binary).is_err() {
            super::draw_msg(buf, fb, "FAIL:WRITE");
            std::thread::sleep(std::time::Duration::from_secs(2));
            return;
        }
    }
    unsafe { libc::chmod(b"/tmp/mini-mayhem.new\0".as_ptr() as *const libc::c_char, 0o755); }

    // Use a shell script to replace ourselves (avoids can-not-overwrite-running-exe issues)
    let dest = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("/mnt/SDCARD/App/Arty/mini-mayhem"));
    let dest_str = dest.to_str().unwrap_or("/mnt/SDCARD/App/Arty/mini-mayhem");
    let script = format!("#!/bin/sh\ncp /tmp/mini-mayhem.new '{}' && chmod +x '{}' && exec '{}'\n",
        dest_str, dest_str, dest_str);
    if std::fs::write("/tmp/mini-mayhem_update.sh", script.as_bytes()).is_err() {
        super::draw_msg(buf, fb, "FAIL:SCRIPT");
        std::thread::sleep(std::time::Duration::from_secs(2));
        return;
    }
    unsafe { libc::chmod(b"/tmp/mini-mayhem_update.sh\0".as_ptr() as *const libc::c_char, 0o755); }
    // Try direct copy first as fallback
    let copy_ok = std::fs::copy("/tmp/mini-mayhem.new", &dest).is_ok();
    if !copy_ok {
        let msg = format!("DST:{}", dest_str);
        super::draw_msg(buf, fb, &msg);
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    // Download extra app files from manifest
    if let Some((manifest, _)) = http_get_body("/arty/manifest.txt", 10) {
        let manifest_str = String::from_utf8_lossy(&manifest).to_string();
        let app_dir = std::path::Path::new(&dest_str).parent()
            .unwrap_or(std::path::Path::new("/mnt/SDCARD/App/Arty"));
        for line in manifest_str.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let fname = parts[0];
            let expected_size: u64 = parts[1].parse().unwrap_or(0);
            let expected_hash = parts.get(2).copied();
            let fpath = app_dir.join(fname);
            if needs_update(&fpath, expected_size, expected_hash) {
                let url = format!("/arty/{}", fname);
                if let Some((data, _)) = http_get_body(&url, 30) {
                    if let Some(parent) = fpath.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(&fpath, &data);
                }
            }
        }
    }
    super::draw_msg(buf, fb, "UPDATE DONE - RELAUNCH FROM MENU");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let sh  = std::ffi::CString::new("/bin/sh").unwrap();
    let arg = std::ffi::CString::new("/tmp/mini-mayhem_update.sh").unwrap();
    let args: [*const libc::c_char; 3] = [sh.as_ptr(), arg.as_ptr(), std::ptr::null()];
    unsafe { libc::execv(sh.as_ptr(), args.as_ptr()); }
    std::process::exit(0);
}
