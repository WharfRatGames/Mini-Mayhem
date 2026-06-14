use std::io::{Read, Write};
use std::net::TcpStream;
use crate::renderer::{WorldBuffer, Framebuffer};

const UPDATE_HOST: &str = "crumbonium.duckdns.org";
const UPDATE_PORT: u16 = 80;

fn http_get_body(path: &str, timeout_secs: u64) -> Option<Vec<u8>> {
    let req = format!("GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", path, UPDATE_HOST);
    use std::net::ToSocketAddrs;
    let addr = (UPDATE_HOST, UPDATE_PORT).to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(timeout_secs)).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(timeout_secs))).ok();
    stream.write_all(req.as_bytes()).ok()?;
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    if resp.is_empty() { return None; }
    let sep = if let Some(i) = resp.windows(4).position(|w| w == b"\r\n\r\n") {
        i + 4
    } else if let Some(i) = resp.windows(2).position(|w| w == b"\n\n") {
        i + 2
    } else {
        return None;
    };
    Some(resp[sep..].to_vec())
}

pub fn check_for_update_bg(current: &'static str) -> std::thread::JoinHandle<bool> {
    std::thread::spawn(move || check_for_update(current))
}

pub fn check_for_update(current: &str) -> bool {
    let body = match http_get_body("/arty/version.txt", 2) {
        Some(b) => b,
        None => return false,
    };
    let server_ver = String::from_utf8_lossy(&body).trim().to_string();
    server_ver != current
}

/// Fetch the human-readable changelog served alongside the binary
/// (`/arty/changelog.txt` on the update host): one entry per line, newest first.
/// Edited in one place on the Pi, so the update screen is always current without a
/// rebuild. Returns None on network failure so the caller can fall back.
pub fn fetch_changelog(timeout_secs: u64) -> Option<Vec<String>> {
    let body = http_get_body("/arty/changelog.txt", timeout_secs)?;
    let lines: Vec<String> = String::from_utf8_lossy(&body)
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.trim().is_empty())
        .take(24) // fits the screen
        .collect();
    if lines.is_empty() { None } else { Some(lines) }
}

/// Background asset sync — runs on every launch regardless of whether a binary
/// update is available. Downloads any manifest file whose size differs from disk.
/// Silently skips on network failure. Call in a detached thread at startup.
pub fn sync_assets_bg() {
    std::thread::spawn(|| {
        let dest = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("/mnt/SDCARD/App/Arty/arty"));
        let app_dir = dest.parent()
            .unwrap_or(std::path::Path::new("/mnt/SDCARD/App/Arty"))
            .to_path_buf();
        let manifest = match http_get_body("/arty/manifest.txt", 3) {
            Some(m) => m,
            None => return,
        };
        for line in String::from_utf8_lossy(&manifest).lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let fpath = app_dir.join(parts[0]);
            let expected: u64 = parts[1].parse().unwrap_or(0);
            if std::fs::metadata(&fpath).map(|m| m.len()).unwrap_or(0) == expected { continue; }
            if let Some(data) = http_get_body(&format!("/arty/{}", parts[0]), 10) {
                if let Some(parent) = fpath.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&fpath, &data);
            }
        }
    });
}

/// Download the binary with streaming, calling on_progress(bytes_done, total_bytes) per chunk.
/// Returns the binary bytes, or None on failure.
pub fn stream_binary<F: FnMut(usize, usize)>(mut on_progress: F) -> Option<Vec<u8>> {
    let req = format!("GET /arty/arty HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", UPDATE_HOST);
    let mut stream = TcpStream::connect((UPDATE_HOST, UPDATE_PORT)).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(120))).ok();
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
    let mut body: Vec<u8> = Vec::with_capacity(total.max(512 * 1024));
    let mut chunk = [0u8; 32768]; // 32KB chunks for faster download
    let mut last_reported = 0usize;
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                body.extend_from_slice(&chunk[..n]);
                // Update progress every 5% to reduce overhead
                if total == 0 || body.len() - last_reported > total / 20 {
                    last_reported = body.len();
                    on_progress(body.len(), total);
                }
            }
            Err(_) => break,
        }
    }
    on_progress(body.len(), total); // final update
    if body.is_empty() { None } else { Some(body) }
}

const SENTINEL: &str = "/tmp/arty_update_sentinel";

/// Returns true if a binary update was attempted this boot session but
/// may have failed.  /tmp is cleared on reboot so this resets automatically.
/// Call at startup BEFORE launching the update-check thread.
pub fn prior_update_attempted() -> bool {
    std::path::Path::new(SENTINEL).exists()
}

/// Apply a validated ELF binary — write, chmod, copy, exec update script.
pub fn apply_binary(binary: &[u8], buf: &mut WorldBuffer, fb: &mut Framebuffer) {
    let dest = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("/mnt/SDCARD/App/Arty/arty"));
    let dest_str = dest.to_str().unwrap_or("/mnt/SDCARD/App/Arty/arty");

    // Write to /tmp first, then copy to dest
    {
        let mut f = match std::fs::File::create("/tmp/arty.new") {
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
    unsafe { libc::chmod(b"/tmp/arty.new\0".as_ptr() as *const libc::c_char, 0o755); }

    // Update script: copy new binary then exec it.
    // Uses && so exec only runs if cp succeeded — prevents exec-old-binary loop.
    let script = format!("#!/bin/sh\ncp /tmp/arty.new '{}' && chmod +x '{}' && exec '{}'\n",
        dest_str, dest_str, dest_str);
    if std::fs::write("/tmp/arty_update.sh", script.as_bytes()).is_err() {
        super::draw_msg(buf, fb, "FAIL:SCRIPT");
        std::thread::sleep(std::time::Duration::from_secs(2));
        return;
    }
    unsafe { libc::chmod(b"/tmp/arty_update.sh\0".as_ptr() as *const libc::c_char, 0o755); }

    // Fetch updated app files (short timeout — don't block the restart)
    if let Some(manifest) = http_get_body("/arty/manifest.txt", 2) {
        let app_dir = std::path::Path::new(dest_str).parent()
            .unwrap_or(std::path::Path::new("/mnt/SDCARD/App/Arty"));
        for line in String::from_utf8_lossy(&manifest).lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let fpath = app_dir.join(parts[0]);
            let expected: u64 = parts[1].parse().unwrap_or(0);
            if std::fs::metadata(&fpath).map(|m| m.len()).unwrap_or(0) != expected {
                if let Some(data) = http_get_body(&format!("/arty/{}", parts[0]), 5) {
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
    let arg = std::ffi::CString::new("/tmp/arty_update.sh").unwrap();
    let args: [*const libc::c_char; 3] = [sh.as_ptr(), arg.as_ptr(), std::ptr::null()];
    unsafe { libc::execv(sh.as_ptr(), args.as_ptr()); }
    std::process::exit(0);
}

pub fn download_and_apply(buf: &mut WorldBuffer, fb: &mut Framebuffer) {
    super::draw_msg(buf, fb, "DOWNLOADING UPDATE...");
    let binary = match http_get_body("/arty/arty", 120) {
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
        let mut f = match std::fs::File::create("/tmp/arty.new") {
            Ok(f) => f,
            Err(_) => { super::draw_msg(buf, fb, "FAIL:TMPWRITE"); std::thread::sleep(std::time::Duration::from_secs(2)); return; }
        };
        if f.write_all(&binary).is_err() {
            super::draw_msg(buf, fb, "FAIL:WRITE");
            std::thread::sleep(std::time::Duration::from_secs(2));
            return;
        }
    }
    unsafe { libc::chmod(b"/tmp/arty.new\0".as_ptr() as *const libc::c_char, 0o755); }

    // Use a shell script to replace ourselves (avoids can-not-overwrite-running-exe issues)
    let dest = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("/mnt/SDCARD/App/Arty/arty"));
    let dest_str = dest.to_str().unwrap_or("/mnt/SDCARD/App/Arty/arty");
    let script = format!("#!/bin/sh\ncp /tmp/arty.new '{}' && chmod +x '{}' && exec '{}'\n",
        dest_str, dest_str, dest_str);
    if std::fs::write("/tmp/arty_update.sh", script.as_bytes()).is_err() {
        super::draw_msg(buf, fb, "FAIL:SCRIPT");
        std::thread::sleep(std::time::Duration::from_secs(2));
        return;
    }
    unsafe { libc::chmod(b"/tmp/arty_update.sh\0".as_ptr() as *const libc::c_char, 0o755); }
    // Try direct copy first as fallback
    let copy_ok = std::fs::copy("/tmp/arty.new", &dest).is_ok();
    if !copy_ok {
        let msg = format!("DST:{}", dest_str);
        super::draw_msg(buf, fb, &msg);
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    // Download extra app files from manifest
    if let Some(manifest) = http_get_body("/arty/manifest.txt", 10) {
        let manifest_str = String::from_utf8_lossy(&manifest).to_string();
        let app_dir = std::path::Path::new(&dest_str).parent()
            .unwrap_or(std::path::Path::new("/mnt/SDCARD/App/Arty"));
        for line in manifest_str.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let fname = parts[0];
            let expected_size: u64 = parts[1].parse().unwrap_or(0);
            let fpath = app_dir.join(fname);
            // Only download if size differs
            let current_size = std::fs::metadata(&fpath).map(|m| m.len()).unwrap_or(0);
            if current_size != expected_size {
                let url = format!("/arty/{}", fname);
                if let Some(data) = http_get_body(&url, 30) {
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
    let arg = std::ffi::CString::new("/tmp/arty_update.sh").unwrap();
    let args: [*const libc::c_char; 3] = [sh.as_ptr(), arg.as_ptr(), std::ptr::null()];
    unsafe { libc::execv(sh.as_ptr(), args.as_ptr()); }
    std::process::exit(0);
}
