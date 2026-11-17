//! Tiny diagnostic log for playtesting on the handheld.
//!
//! * Off until `init()` is called (the server and tests never write a file).
//! * Writes `arty-exp.log` next to the executable (or `$ARTY_LOG`); rotates to `.1` at 512 KB.
//! * Timestamps are milliseconds since start (the Miyoo clock is unreliable), plus the game tick
//!   where the caller supplies one.
//! * Never panics and never touches game state, so it cannot affect simulation parity.
//!
//! Use `dlog!("rope", "attach at {},{}", x, y)`; pull it with `exp-deploy/miyoo_log.sh`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const MAX_BYTES: u64 = 512 * 1024;

struct Logger { file: File, path: std::path::PathBuf, written: u64, start: Instant }
static LOG: OnceLock<Mutex<Logger>> = OnceLock::new();

fn log_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("ARTY_LOG") { return p.into(); }
    let mut p = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| ".".into());
    p.push("arty-exp.log");
    p
}

/// Start logging (idempotent). Also installs a panic hook that records the panic and location.
pub fn init(version: &str) {
    if LOG.get().is_some() { return; }
    let path = log_path();
    // Keep the previous run's log as .1 so a crash can be inspected after relaunch.
    let _ = std::fs::rename(&path, path.with_extension("log.1"));
    let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else { return };
    let _ = LOG.set(Mutex::new(Logger { file, path, written: 0, start: Instant::now() }));
    log("sys", format_args!("start version={} pid={}", version, std::process::id()));
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log("PANIC", format_args!("{}", info));
        prev(info);
    }));
}

pub fn log(tag: &str, args: std::fmt::Arguments) {
    let Some(m) = LOG.get() else { return };
    let Ok(mut l) = m.lock() else { return };
    let line = format!("{:>9.3} [{}] {}\n", l.start.elapsed().as_secs_f64(), tag, args);
    if l.file.write_all(line.as_bytes()).is_err() { return; }
    let _ = l.file.flush();
    l.written += line.len() as u64;
    if l.written > MAX_BYTES {
        let rotated = l.path.with_extension("log.1");
        let _ = std::fs::rename(&l.path, &rotated);
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&l.path) { l.file = f; l.written = 0; }
    }
}

#[macro_export]
macro_rules! dlog {
    ($tag:expr, $($arg:tt)*) => { $crate::dlog::log($tag, format_args!($($arg)*)) };
}
