use crate::renderer::{WorldBuffer, Framebuffer};

// ── Desktop stubs — no OTA on desktop, update via git pull ───────────────────
//
// Returning true from prior_update_attempted() causes main() to skip the
// update check entirely, so none of the other OTA functions are called.

#[cfg(feature = "desktop")]
pub fn prior_update_attempted() -> bool { true }

#[cfg(feature = "desktop")]
pub fn sync_assets_bg(_version: &'static str) {}

#[cfg(feature = "desktop")]
pub fn check_for_update(_current: &str) -> (bool, bool) { (false, false) }

#[cfg(feature = "desktop")]
pub fn check_for_update_bg(_current: &'static str) -> std::thread::JoinHandle<(bool, bool)> {
    std::thread::spawn(|| (false, false))
}

#[cfg(feature = "desktop")]
pub fn fetch_changelog(_timeout_secs: u64) -> Option<Vec<String>> { None }

#[cfg(feature = "desktop")]
pub fn stream_binary<F: FnMut(usize, usize)>(_on_progress: F) -> Option<Vec<u8>> { None }

#[cfg(feature = "desktop")]
pub fn apply_binary(_binary: &[u8], _buf: &mut WorldBuffer, _fb: &mut Framebuffer) {}

#[cfg(feature = "desktop")]
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

#[cfg(not(feature = "desktop"))]
fn http_get_body(path: &str, timeout_secs: u64) -> Option<(Vec<u8>, bool)> {
    match crate::https::https_get(UPDATE_HOST, path, timeout_secs, timeout_secs) {
        Ok(b) => Some((b, false)),
        Err(_) => crate::https::http_get(UPDATE_HOST, path, timeout_secs, timeout_secs)
            .ok()
            .map(|b| (b, true)), // true = TLS failed, fell back to HTTP
    }
}

#[cfg(not(feature = "desktop"))]
pub fn check_for_update_bg(current: &'static str) -> std::thread::JoinHandle<(bool, bool)> {
    std::thread::spawn(move || check_for_update(current))
}

/// Returns (update_available, tls_broken).
/// tls_broken=true means HTTPS failed and we fell back to HTTP — force the update, no skip.
#[cfg(not(feature = "desktop"))]
pub fn check_for_update(current: &str) -> (bool, bool) {
    let (body, tls_broken) = match http_get_body("/arty/version.txt", 2) {
        Some(x) => x,
        None => return (false, false),
    };
    let server_ver = String::from_utf8_lossy(&body).trim().to_string();
    (server_ver != current, tls_broken)
}

#[cfg(not(feature = "desktop"))]
pub fn fetch_changelog(timeout_secs: u64) -> Option<Vec<String>> {
    let (body, _) = http_get_body("/arty/changelog.txt", timeout_secs)?;
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

#[cfg(not(feature = "desktop"))]
// Streams the binary directly to /tmp/mini-mayhem.new to avoid buffering 26MB in RAM.
// Returns Some(path) on success, None on failure.
pub fn stream_binary<F: FnMut(usize, usize)>(mut on_progress: F) -> Option<Vec<u8>> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use rustls::pki_types::ServerName;

    let req = format!("GET /arty/mini-mayhem HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", UPDATE_HOST);
    let addr = (UPDATE_HOST, 443u16).to_socket_addrs().ok()?.next()?;
    let tcp = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(10)).ok()?;
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
    let tmp_path = "/tmp/mini-mayhem.new";
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
pub fn prior_update_attempted() -> bool {
    std::path::Path::new(SENTINEL).exists()
}

#[cfg(not(feature = "desktop"))]
pub fn apply_binary(binary: &[u8], buf: &mut WorldBuffer, fb: &mut Framebuffer) {
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

#[cfg(not(feature = "desktop"))]
pub fn download_and_apply(buf: &mut WorldBuffer, fb: &mut Framebuffer) {
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
