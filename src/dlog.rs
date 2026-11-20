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
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Time spent inside `simulate()` this perf window (microseconds): sum, worst tick, tick count. Fed by `note_sim` from the
/// offline game loop; the live client never runs `simulate()` so these stay 0 there. Diagnostic only, never read by the sim.
static SIM_US_SUM: AtomicU64 = AtomicU64::new(0);
static SIM_US_MAX: AtomicU64 = AtomicU64::new(0);
static SIM_TICKS: AtomicU64 = AtomicU64::new(0);

pub fn note_sim(us: u64) {
    SIM_US_SUM.fetch_add(us, Ordering::Relaxed);
    SIM_US_MAX.fetch_max(us, Ordering::Relaxed);
    SIM_TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Per-second performance/smoothness window (internal diagnostics). Feed it every frame; it writes one `[perf]` line a second:
/// `fps`, frame time avg/max (ms), blit max (ms), how many frames received 0 / 1 / 2+ network states, ping, and the
/// active worm's per-frame x steps (`dx`) — uneven `dx` or frame-time spikes explain visible jerkiness.
pub struct PerfWin {
    start: Instant,
    frames: u32,
    dt_sum: f64,
    dt_max: f64,
    blit_max: f64,
    st: [u32; 3],
    last_x: Option<f32>,
    dx: Vec<i32>,
    ph_sum: [f64; 4],
    ph_max: [f64; 4],
}

impl PerfWin {
    pub fn new() -> Self {
        Self { start: Instant::now(), frames: 0, dt_sum: 0.0, dt_max: 0.0, blit_max: 0.0, st: [0; 3], last_x: None, dx: Vec::new(), ph_sum: [0.0; 4], ph_max: [0.0; 4] }
    }
    /// Phase timings (ms) for this frame: [net+apply, camera/visuals, render_live, overlays].
    pub fn phases(&mut self, p: [f64; 4]) { for i in 0..4 { self.ph_sum[i] += p[i]; self.ph_max[i] = self.ph_max[i].max(p[i]); } }
    /// `states` = network states drained this frame (0 for offline play).
    pub fn frame(&mut self, dt_ms: f64, blit_ms: f64, states: usize, active_x: Option<f32>, ping_ms: u32) {
        self.frames += 1;
        self.dt_sum += dt_ms;
        self.dt_max = self.dt_max.max(dt_ms);
        self.blit_max = self.blit_max.max(blit_ms);
        self.st[states.min(2)] += 1;
        if let Some(x) = active_x {
            if let Some(p) = self.last_x { if self.dx.len() < 24 { self.dx.push((x - p).round() as i32); } }
            self.last_x = Some(x);
        }
        if self.start.elapsed().as_secs_f64() >= 1.0 {
            let n = self.frames.max(1) as f64;
            let moving = self.dx.iter().any(|d| *d != 0);
            log("perf", format_args!("fps={} dt avg={:.1} max={:.1}ms blit_max={:.1}ms states 0/1/2+={}/{}/{} ping={}ms ph[net/vis/render/ovl] avg={:.1}/{:.1}/{:.1}/{:.1} max={:.1}/{:.1}/{:.1}/{:.1}{}",
                self.frames, self.dt_sum / self.frames.max(1) as f64, self.dt_max, self.blit_max,
                self.st[0], self.st[1], self.st[2], ping_ms,
                self.ph_sum[0]/n, self.ph_sum[1]/n, self.ph_sum[2]/n, self.ph_sum[3]/n, self.ph_max[0], self.ph_max[1], self.ph_max[2], self.ph_max[3],
                if moving { format!(" dx={:?}", self.dx) } else { String::new() }));
            let (n_sim, sum, max) = (SIM_TICKS.swap(0, Ordering::Relaxed), SIM_US_SUM.swap(0, Ordering::Relaxed), SIM_US_MAX.swap(0, Ordering::Relaxed));
            if n_sim > 0 { log("sim", format_args!("ticks={} avg={}us max={}us per tick, {:.1}ms/s", n_sim, sum / n_sim, max, sum as f64 / 1000.0)); }
            *self = Self { last_x: self.last_x, ..Self::new() };
        }
    }
}
