/// Miyoo Mini Plus audio — raw ALSA, 48 kHz mono s16le.
/// Each sound plays in its own thread (opens hw:0,0, writes, closes).
/// libasound is dlopen'd at runtime — no compile-time link needed.

static MUTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Suppress all audio (e.g. during silent fast-forward).
pub fn set_muted(v: bool) {
    MUTED.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub fn init() {
    #[cfg(feature = "desktop")]
    imp_desktop::init();
}

/// Drive the plasma-torch loop sound from the live torch state. Call every frame
/// with whether the torch is currently active (`game.plasma_torch.is_some()`).
/// Starts a looping burn sound on the rising edge and stops it the instant the
/// torch turns off — so the sound only plays WHILE the torch is active (no more
/// 4-second clip droning on after an early release). Works in every mode because
/// the live client reconstructs `plasma_torch` from the networked torch_dir.
/// Drive the Mac-10 loop sound from firing state. Call every render frame.
pub fn update_torch(active: bool) {
    #[cfg(target_arch = "arm")]
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static WAS: AtomicBool = AtomicBool::new(false);
        let was = WAS.swap(active, Ordering::Relaxed);
        if active && !was { imp::start_torch(); }
        else if !active && was { imp::stop_torch(); }
    }
    #[cfg(not(target_arch = "arm"))]
    { let _ = active; }
}
/// Pre-load all SFX into memory in the calling thread.
/// Spawn this in a background thread at game start so the first explosion
/// doesn't stall the game loop with disk I/O.
pub fn preload() {
    #[cfg(target_arch = "arm")]
    imp::preload();
    #[cfg(feature = "desktop")]
    imp_desktop::preload();
}

pub fn play_explosion()         { _play_once("bazooka_explosion.wav"); }
pub fn play_tnt()               { _play_once("tnt.wav"); }
pub fn play_grenade()           { _play("grenade.wav"); }
pub fn play_meteor()            { _play_once("meteor.wav"); }
pub fn play_black_hole()        { _play_once("hum.wav"); }
pub fn play_mine()              { _play_once("mine.wav"); }
pub fn play_mine_arm()          { _play("arm_beep.wav"); }
pub fn play_barrel_explosion()  { _play_once("barrell.wav"); }
pub fn play_revolver()          { _play_revolver(); }
pub fn play_plasma_torch()      { _play("torch.wav"); }
pub fn play_garcia()            { _play_once("garcia.wav"); }
pub fn play_smash()             { _play_once("smash.wav"); }
pub fn play_shotgun_fire()      { _play("shotgun.wav"); }
pub fn play_bat()               { _play("bat.wav"); }
pub fn play_splash()            {}
pub fn play_crate_drop()        {}
pub fn play_death()             { _play_death(); }
pub fn play_wet_death()         { _play("wet.wav"); }
pub fn play_death_water()       { _play_death_water(); }
pub fn play_holy_hand_grenade() { _play_once("hallelujah.wav"); }
pub fn play_minigun()           { _play_once("minigun.wav"); } // deploy/assets/sfx/hallelujah.wav required
pub fn play_uzi()               { _play("mac10.wav"); }

/// Identifies a sound effect so it can be recorded during simulation and
/// shipped to the live client (which runs no simulation of its own and would
/// otherwise be silent). See `GameState::emit_sound`. Keep the u8 values stable
/// across client/server — they travel in StateMsg.sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Sfx {
    Explosion = 0,
    Tnt       = 1,
    Grenade   = 2,
    Meteor    = 3,
    BlackHole = 4,
    Mine      = 5,
    MineArm   = 6,
    Barrel    = 7,
    Revolver  = 8,
    Shotgun   = 9,
    Bat       = 10,
    CrateDrop   = 11,
    PlasmaTorch = 12,
    Garcia      = 13,
    Smash       = 14,
    Death       = 15,
    DeathWater  = 16,
    HolyHandGrenade = 17,
    Minigun         = 18,
    Uzi             = 19,
}

impl Sfx {
    pub fn from_u8(v: u8) -> Option<Sfx> {
        Some(match v {
            0  => Sfx::Explosion, 1 => Sfx::Tnt,     2  => Sfx::Grenade,  3 => Sfx::Meteor,
            4  => Sfx::BlackHole, 5 => Sfx::Mine,    6  => Sfx::MineArm,  7 => Sfx::Barrel,
            8  => Sfx::Revolver,  9 => Sfx::Shotgun, 10 => Sfx::Bat,      11 => Sfx::CrateDrop,
            12 => Sfx::PlasmaTorch,
            13 => Sfx::Garcia,
            14 => Sfx::Smash,
            15 => Sfx::Death,
            16 => Sfx::DeathWater,
            17 => Sfx::HolyHandGrenade,
            18 => Sfx::Minigun,
            19 => Sfx::Uzi,
            _ => return None,
        })
    }

    /// Compile-time forcing function: exhaustively matches every Sfx variant so
    /// adding a new sound breaks compilation here until you assign it a stable
    /// u8 discriminant AND handle it in `from_u8` and `play`. Mirrors
    /// `_net_coverage_checklist` in projectile.rs and the GameState checklists.
    #[allow(dead_code)]
    fn _sfx_coverage_checklist(s: Sfx) {
        match s {
            Sfx::Explosion | Sfx::Tnt | Sfx::Grenade | Sfx::Meteor |
            Sfx::BlackHole | Sfx::Mine | Sfx::MineArm | Sfx::Barrel |
            Sfx::Revolver | Sfx::Shotgun | Sfx::Bat | Sfx::CrateDrop |
            Sfx::PlasmaTorch | Sfx::Garcia | Sfx::Smash | Sfx::Death |
            Sfx::DeathWater | Sfx::HolyHandGrenade | Sfx::Minigun | Sfx::Uzi => {}
        }
    }
}

/// Play a sound by its `Sfx` id. Used by the live client to render the sounds
/// the server recorded during its simulation tick.
pub fn play(s: Sfx) {
    match s {
        Sfx::Explosion => play_explosion(),
        Sfx::Tnt       => play_tnt(),
        Sfx::Grenade   => play_grenade(),
        Sfx::Meteor    => play_meteor(),
        Sfx::BlackHole => play_black_hole(),
        Sfx::Mine      => play_mine(),
        Sfx::MineArm   => play_mine_arm(),
        Sfx::Barrel    => play_barrel_explosion(),
        Sfx::Revolver  => play_revolver(),
        Sfx::Shotgun   => play_shotgun_fire(),
        Sfx::Bat       => play_bat(),
        Sfx::CrateDrop   => play_crate_drop(),
        Sfx::PlasmaTorch => play_plasma_torch(),
        Sfx::Garcia      => play_garcia(),
        Sfx::Smash       => play_smash(),
        Sfx::Death       => play_death(),
        Sfx::DeathWater  => play_death_water(),
        Sfx::HolyHandGrenade => play_holy_hand_grenade(),
        Sfx::Minigun         => play_minigun(),
        Sfx::Uzi             => play_uzi(),
    }
}

// ── ARM implementation ────────────────────────────────────────────────────────

#[cfg(target_arch = "arm")]
mod imp {
    use std::sync::OnceLock;

    const RATE:     u32 = 48000;
    const PERIOD:   usize = 1024;
    // Max boost applied to a quiet sound; the load_wav limiter caps each sound below
    // this if it would clip, so loud sounds get less (and hot sounds get reduced).
    const GAIN_NUM: i32 = 6; // up to 6×
    const GAIN_DEN: i32 = 1;

    static EXPLOSION: OnceLock<Vec<i16>> = OnceLock::new();
    static GRENADE:   OnceLock<Vec<i16>> = OnceLock::new();
    static METEOR:    OnceLock<Vec<i16>> = OnceLock::new();
    static MINE:      OnceLock<Vec<i16>> = OnceLock::new();
    static MINE_ARM:  OnceLock<Vec<i16>> = OnceLock::new();
    static BARREL:    OnceLock<Vec<i16>> = OnceLock::new();
    static REVOLVER:  OnceLock<Vec<i16>> = OnceLock::new();
    static TNT:       OnceLock<Vec<i16>> = OnceLock::new();
    static SHOTGUN:   OnceLock<Vec<i16>> = OnceLock::new();
    static BAT:       OnceLock<Vec<i16>> = OnceLock::new();
    static WET:       OnceLock<Vec<i16>> = OnceLock::new();
    static WATER:     OnceLock<Vec<i16>> = OnceLock::new();
    static HUM:       OnceLock<Vec<i16>> = OnceLock::new();
    static TORCH:     OnceLock<Vec<i16>> = OnceLock::new();
    static GARCIA:    OnceLock<Vec<i16>> = OnceLock::new();
    static SMASH:       OnceLock<Vec<i16>> = OnceLock::new();
    static HALLELUJAH:  OnceLock<Vec<i16>> = OnceLock::new();
    static MINIGUN:     OnceLock<Vec<i16>> = OnceLock::new();
    static UZI:         OnceLock<Vec<i16>> = OnceLock::new();
    static DEATHS:      OnceLock<Vec<Vec<i16>>> = OnceLock::new();

    // ── WAV → 48kHz mono i16 ─────────────────────────────────────────────────

    pub fn load_wav(path: &std::path::Path) -> Option<Vec<i16>> {
        let data = std::fs::read(path).ok()?;
        if data.len() < 44 { return None; }
        let mut pos = 12usize;
        let (mut fmt_off, mut dat_off, mut dat_sz) = (None, None, 0usize);
        while pos + 8 <= data.len() {
            let tag  = &data[pos..pos+4];
            let sz   = u32::from_le_bytes(data[pos+4..pos+8].try_into().ok()?) as usize;
            if tag == b"fmt " { fmt_off = Some(pos+8); }
            if tag == b"data" { dat_off = Some(pos+8); dat_sz = sz; }
            pos += 8 + sz + (sz & 1);
        }
        let fmt = fmt_off?; let doff = dat_off?;
        if u16::from_le_bytes(data[fmt..fmt+2].try_into().ok()?) != 1 { return None; }
        let ch      = u16::from_le_bytes(data[fmt+2..fmt+4].try_into().ok()?) as usize;
        let in_rate = u32::from_le_bytes(data[fmt+4..fmt+8].try_into().ok()?) as usize;
        if u16::from_le_bytes(data[fmt+14..fmt+16].try_into().ok()?) != 16 { return None; }
        let raw = &data[doff..(doff+dat_sz).min(data.len())];
        let in_frames  = raw.len() / ch.max(1) / 2;
        let out_frames = (in_frames as u64 * RATE as u64 / in_rate.max(1) as u64) as usize;
        let mut out = Vec::with_capacity(out_frames);
        for i in 0..out_frames {
            let src  = ((i as u64 * in_rate as u64) / RATE as u64) as usize;
            let base = src * ch * 2;
            if base + ch*2 > raw.len() { break; }
            let sum: i32 = (0..ch).map(|c| {
                i16::from_le_bytes(raw[base+c*2..base+c*2+2].try_into().unwrap_or([0;2])) as i32
            }).sum();
            out.push((sum / ch.max(1) as i32) as i16);
        }
        // Per-sound gain with a hard no-clip limiter: boost by up to GAIN_NUM/GAIN_DEN,
        // but if that would push the loudest sample past HEADROOM, scale the gain down
        // so the peak lands exactly at HEADROOM instead. Quiet sounds get the full
        // boost; loud sounds are turned up only as far as they can go without clipping.
        const HEADROOM: i64 = 32000; // ~0.2 dB below i16::MAX, leaves a little margin
        let peak = out.iter().map(|&s| (s as i64).abs()).max().unwrap_or(0).max(1);
        // Effective gain = min(GAIN_NUM/GAIN_DEN, HEADROOM/peak) as an integer ratio.
        let (gn, gd): (i64, i64) =
            if GAIN_NUM as i64 * peak <= HEADROOM * GAIN_DEN as i64 {
                (GAIN_NUM as i64, GAIN_DEN as i64) // desired boost stays under HEADROOM
            } else {
                (HEADROOM, peak)                    // cap so the peak just touches HEADROOM
            };
        // Short linear fade-in/out (480 samples = 10ms) to prevent pops.
        let fade = 480.min(out.len() / 4);
        for i in 0..out.len() {
            let ramp = if i < fade { i as i64 } else if i >= out.len() - fade { (out.len() - 1 - i) as i64 } else { fade as i64 };
            let s = (out[i] as i64 * gn / gd * ramp / fade.max(1) as i64).clamp(-32768, 32767);
            out[i] = s as i16;
        }
        Some(out)
    }

    fn sfx_dir() -> Option<std::path::PathBuf> {
        Some(std::env::current_exe().ok()?.parent()?.join("sfx"))
    }

    fn try_load(lock: &OnceLock<Vec<i16>>, dir: &std::path::Path, name: &str) {
        if lock.get().is_none() {
            if let Some(b) = load_wav(&dir.join(name)) { let _ = lock.set(b); }
        }
    }

    /// Load a WAV and stretch it by `stretch` (>1 = slower/longer).
    /// Achieved by pretending the source sample rate is lower by that factor.
    fn try_load_stretched(lock: &OnceLock<Vec<i16>>, dir: &std::path::Path, name: &str, stretch: f32) {
        if lock.get().is_none() {
            let path = dir.join(name);
            let data = match std::fs::read(&path) { Ok(d) => d, Err(_) => return };
            if data.len() < 44 { return; }
            let mut pos = 12usize;
            let (mut fmt_off, mut dat_off, mut dat_sz) = (None, None, 0usize);
            while pos + 8 <= data.len() {
                let tag = &data[pos..pos+4];
                let sz  = u32::from_le_bytes(data[pos+4..pos+8].try_into().unwrap()) as usize;
                if tag == b"fmt " { fmt_off = Some(pos+8); }
                if tag == b"data" { dat_off = Some(pos+8); dat_sz = sz; }
                pos += 8 + sz + (sz & 1);
            }
            let (fmt, doff) = match (fmt_off, dat_off) { (Some(a), Some(b)) => (a, b), _ => return };
            if u16::from_le_bytes(data[fmt..fmt+2].try_into().unwrap()) != 1 { return; }
            let ch      = u16::from_le_bytes(data[fmt+2..fmt+4].try_into().unwrap()) as usize;
            let in_rate = u32::from_le_bytes(data[fmt+4..fmt+8].try_into().unwrap()) as usize;
            if u16::from_le_bytes(data[fmt+14..fmt+16].try_into().unwrap()) != 16 { return; }
            // Pretend the source rate is lower → more output frames → stretched playback
            let eff_rate = ((in_rate as f32 / stretch) as usize).max(1);
            let raw = &data[doff..(doff+dat_sz).min(data.len())];
            let in_frames  = raw.len() / ch.max(1) / 2;
            let out_frames = (in_frames as u64 * RATE as u64 / eff_rate as u64) as usize;
            let mut out = Vec::with_capacity(out_frames);
            for i in 0..out_frames {
                let src  = ((i as u64 * eff_rate as u64) / RATE as u64) as usize;
                let base = src * ch * 2;
                if base + ch*2 > raw.len() { break; }
                let sum: i32 = (0..ch).map(|c| {
                    i16::from_le_bytes(raw[base+c*2..base+c*2+2].try_into().unwrap_or([0;2])) as i32
                }).sum();
                out.push((sum / ch.max(1) as i32) as i16);
            }
            const HEADROOM: i64 = 32000;
            let peak = out.iter().map(|&s| (s as i64).abs()).max().unwrap_or(0).max(1);
            let (gn, gd): (i64, i64) =
                if GAIN_NUM as i64 * peak <= HEADROOM * GAIN_DEN as i64 { (GAIN_NUM as i64, GAIN_DEN as i64) }
                else { (HEADROOM, peak) };
            let fade = 480.min(out.len() / 4);
            for i in 0..out.len() {
                let ramp = if i < fade { i as i64 } else if i >= out.len() - fade { (out.len() - 1 - i) as i64 } else { fade as i64 };
                let s = (out[i] as i64 * gn / gd * ramp / fade.max(1) as i64).clamp(-32768, 32767);
                out[i] = s as i16;
            }
            let _ = lock.set(out);
        }
    }

    // True once every required SFX file has been loaded into its OnceLock.
    static ALL_LOADED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    fn ensure_loaded() {
        if ALL_LOADED.load(std::sync::atomic::Ordering::Relaxed) { return; }
        let Some(dir) = sfx_dir() else { return };
        try_load(&EXPLOSION, &dir, "bazooka_explosion.wav");
        try_load(&GRENADE,   &dir, "grenade.wav");
        try_load(&METEOR,    &dir, "meteor.wav");
        try_load(&MINE,      &dir, "mine.wav");
        try_load(&MINE_ARM,  &dir, "arm_beep.wav");
        try_load(&BARREL,    &dir, "barrell.wav");
        try_load(&REVOLVER,  &dir, "revolver_shot.wav");
        try_load(&TNT,       &dir, "tnt.wav");
        try_load(&SHOTGUN,   &dir, "shotgun.wav");
        try_load(&BAT,       &dir, "bat.wav");
        try_load(&WET,       &dir, "wet.wav");
        try_load(&WATER,     &dir, "water.wav");
        try_load(&HUM,       &dir, "hum.wav");
        try_load(&TORCH,     &dir, "torch.wav");
        try_load(&GARCIA,    &dir, "garcia.wav");
        try_load(&SMASH,       &dir, "smash.wav");
        try_load(&HALLELUJAH,  &dir, "hallelujah.wav");
        try_load(&MINIGUN,     &dir, "minigun.wav");
        try_load_stretched(&UZI, &dir, "mac10.wav", 1.26);
        if DEATHS.get().is_none() {
            let deaths: Vec<Vec<i16>> = std::fs::read_dir(dir.join("death"))
                .into_iter().flatten().flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("wav"))
                .filter(|e| e.file_name() != *"garcia.wav")
                .filter_map(|e| load_wav(&e.path()))
                .collect();
            if !deaths.is_empty() { let _ = DEATHS.set(deaths); }
        }
        // Mark fully loaded only when every required file is present.
        if EXPLOSION.get().is_some() && GRENADE.get().is_some() && METEOR.get().is_some()
            && MINE.get().is_some() && MINE_ARM.get().is_some() && BARREL.get().is_some()
            && REVOLVER.get().is_some() && TNT.get().is_some() && SHOTGUN.get().is_some()
            && BAT.get().is_some() && WET.get().is_some() && WATER.get().is_some()
            && HUM.get().is_some() && TORCH.get().is_some() && GARCIA.get().is_some()
            && SMASH.get().is_some() && HALLELUJAH.get().is_some() && MINIGUN.get().is_some()
            && UZI.get().is_some()
        {
            ALL_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    // ── ALSA playback ─────────────────────────────────────────────────────────

    fn play_samples(samples: Vec<i16>) {
        std::thread::spawn(move || {
            play_samples_inner(&samples, true);
        });
    }

    // Fire-and-forget: skip silently if hw:0,0 is busy. For rapid-fire sounds.
    fn play_samples_once(samples: Vec<i16>) {
        std::thread::spawn(move || {
            play_samples_inner(&samples, false);
        });
    }

    fn log(msg: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open("/tmp/arty_alsa.log") {
            let _ = writeln!(f, "{}", msg);
        }
    }

    fn play_samples_inner(samples: &[i16], retry: bool) {
        let candidates: &[&[u8]] = &[
            b"/customer/lib/libasound.so.2\0",
            b"/usr/lib/libasound.so.2\0",
            b"libasound.so.2\0",
        ];
        let lib = candidates.iter().find_map(|p| {
            let h = unsafe { libc::dlopen(p.as_ptr() as *const libc::c_char, libc::RTLD_NOW) };
            if h.is_null() { None } else { Some(h) }
        });
        let Some(lib) = lib else { log("FAIL: dlopen"); return };

        macro_rules! sym {
            ($name:literal, $T:ty) => {{
                let s = unsafe {
                    libc::dlsym(lib, concat!($name, "\0").as_ptr() as *const libc::c_char)
                };
                if s.is_null() { unsafe { libc::dlclose(lib); } return; }
                unsafe { std::mem::transmute::<_, $T>(s) }
            }};
        }

        type PcmT  = *mut libc::c_void;
        let pcm_open: unsafe extern "C" fn(*mut PcmT, *const libc::c_char, i32, i32) -> i32
            = sym!("snd_pcm_open",    _);
        let pcm_set:  unsafe extern "C" fn(PcmT, u32, u32, u32, u32, i32, u32) -> i32
            = sym!("snd_pcm_set_params", _);
        let pcm_write: unsafe extern "C" fn(PcmT, *const i16, u32) -> i32
            = sym!("snd_pcm_writei",  _);
        let pcm_recover: unsafe extern "C" fn(PcmT, i32, i32) -> i32
            = sym!("snd_pcm_recover", _);
        let pcm_drain: unsafe extern "C" fn(PcmT) -> i32
            = sym!("snd_pcm_drain",   _);
        let pcm_close: unsafe extern "C" fn(PcmT) -> i32
            = sym!("snd_pcm_close",   _);

        log(&format!("samples={}", samples.len()));
        let dev = b"hw:0,0\0";
        let mut pcm: PcmT = std::ptr::null_mut();
        let mut open_r = -1i32;
        let attempts = if retry { 30 } else { 1 };
        for _ in 0..attempts {
            open_r = unsafe { pcm_open(&mut pcm, dev.as_ptr() as *const libc::c_char, 0, 0) };
            if open_r == 0 { break; }
            if retry { std::thread::sleep(std::time::Duration::from_millis(100)); }
        }
        log(&format!("pcm_open={}", open_r));
        if open_r < 0 || pcm.is_null() { unsafe { libc::dlclose(lib); } return; }

        let r = unsafe { pcm_set(pcm, 2/*S16_LE*/, 3/*RW_INTERLEAVED*/, 1/*MONO*/, RATE, 0, 100_000) };
        log(&format!("pcm_set={}", r));
        if r < 0 { unsafe { pcm_close(pcm); libc::dlclose(lib); } return; }

        let start = std::time::Instant::now();
        let mut pos = 0usize;
        while pos < samples.len() {
            let end = (pos + PERIOD).min(samples.len());
            let chunk = &samples[pos..end];
            let buf: Vec<i16> = if chunk.len() < PERIOD {
                let mut v = chunk.to_vec();
                v.resize(PERIOD, 0);
                v
            } else {
                chunk.to_vec()
            };
            let r = unsafe { pcm_write(pcm, buf.as_ptr(), PERIOD as u32) };
            if r < 0 {
                log(&format!("write_err={}", r));
                unsafe { pcm_recover(pcm, r, 1); }
            } else if r > 0 {
                pos += r as usize;
            } else {
                break;
            }
        }

        // Write one period of silence to push any remaining audio fully out of the
        // hardware FIFO before drain — the stub driver's pcm_drain returns immediately
        // without waiting, so without this the tail gets cut and produces a pop.
        let silence = vec![0i16; PERIOD];
        unsafe { pcm_write(pcm, silence.as_ptr(), PERIOD as u32); }
        unsafe { pcm_drain(pcm); }
        let expected = std::time::Duration::from_millis(
            (samples.len() as u64 * 1000) / RATE as u64 + 50
        );
        if let Some(remaining) = expected.checked_sub(start.elapsed()) {
            std::thread::sleep(remaining);
        }
        log("done");
        unsafe { pcm_close(pcm); libc::dlclose(lib); }
    }

    use std::sync::atomic::{AtomicBool, Ordering};
    static TORCH_ACTIVE: AtomicBool = AtomicBool::new(false);

    /// Begin the looping plasma-torch burn sound (no-op if already playing or muted).
    pub fn start_torch() {
        if super::MUTED.load(Ordering::Relaxed) { return; }
        if TORCH_ACTIVE.swap(true, Ordering::Relaxed) { return; } // already burning
        std::thread::spawn(|| { play_torch_inner(); });
    }

    /// Stop the looping plasma-torch burn sound.
    pub fn stop_torch() { TORCH_ACTIVE.store(false, Ordering::Relaxed); }

    /// Hold hw:0,0 open and loop torch.wav while TORCH_ACTIVE; exits within one
    /// PERIOD (~21 ms) of stop_torch() so the sound ends with the torch.
    fn play_torch_inner() {
        ensure_loaded();
        let samples = match TORCH.get() { Some(s) => s.clone(), None => { TORCH_ACTIVE.store(false, Ordering::Relaxed); return; } };
        if samples.is_empty() { TORCH_ACTIVE.store(false, Ordering::Relaxed); return; }

        let candidates: &[&[u8]] = &[
            b"/customer/lib/libasound.so.2\0",
            b"/usr/lib/libasound.so.2\0",
            b"libasound.so.2\0",
        ];
        let lib = candidates.iter().find_map(|p| {
            let h = unsafe { libc::dlopen(p.as_ptr() as *const libc::c_char, libc::RTLD_NOW) };
            if h.is_null() { None } else { Some(h) }
        });
        let Some(lib) = lib else { TORCH_ACTIVE.store(false, Ordering::Relaxed); return };

        macro_rules! sym {
            ($name:literal, $T:ty) => {{
                let s = unsafe { libc::dlsym(lib, concat!($name, "\0").as_ptr() as *const libc::c_char) };
                if s.is_null() { unsafe { libc::dlclose(lib); } TORCH_ACTIVE.store(false, Ordering::Relaxed); return; }
                unsafe { std::mem::transmute::<_, $T>(s) }
            }};
        }
        type PcmT = *mut libc::c_void;
        let pcm_open:  unsafe extern "C" fn(*mut PcmT, *const libc::c_char, i32, i32) -> i32 = sym!("snd_pcm_open", _);
        let pcm_set:   unsafe extern "C" fn(PcmT, u32, u32, u32, u32, i32, u32) -> i32 = sym!("snd_pcm_set_params", _);
        let pcm_write: unsafe extern "C" fn(PcmT, *const i16, u32) -> i32 = sym!("snd_pcm_writei", _);
        let pcm_recover: unsafe extern "C" fn(PcmT, i32, i32) -> i32 = sym!("snd_pcm_recover", _);
        let pcm_drain: unsafe extern "C" fn(PcmT) -> i32 = sym!("snd_pcm_drain", _);
        let pcm_close: unsafe extern "C" fn(PcmT) -> i32 = sym!("snd_pcm_close", _);

        let dev = b"hw:0,0\0";
        let mut pcm: PcmT = std::ptr::null_mut();
        let mut open_r = -1i32;
        for _ in 0..10 {
            open_r = unsafe { pcm_open(&mut pcm, dev.as_ptr() as *const libc::c_char, 0, 0) };
            if open_r == 0 { break; }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if open_r < 0 || pcm.is_null() { unsafe { libc::dlclose(lib); } TORCH_ACTIVE.store(false, Ordering::Relaxed); return; }
        let r = unsafe { pcm_set(pcm, 2, 3, 1, RATE, 0, 100_000) };
        if r < 0 { unsafe { pcm_close(pcm); libc::dlclose(lib); } TORCH_ACTIVE.store(false, Ordering::Relaxed); return; }

        let mut pos = 0usize;
        while TORCH_ACTIVE.load(Ordering::Relaxed) && !super::MUTED.load(Ordering::Relaxed) {
            let end = (pos + PERIOD).min(samples.len());
            let chunk = &samples[pos..end];
            let buf: Vec<i16> = if chunk.len() < PERIOD {
                let mut v = chunk.to_vec(); v.resize(PERIOD, 0); v
            } else { chunk.to_vec() };
            let w = unsafe { pcm_write(pcm, buf.as_ptr(), PERIOD as u32) };
            if w < 0 { unsafe { pcm_recover(pcm, w, 1); } }
            else if w > 0 { pos += w as usize; }
            if pos >= samples.len() { pos = 0; } // loop the burn while still active
        }
        let silence = vec![0i16; PERIOD];
        unsafe { pcm_write(pcm, silence.as_ptr(), PERIOD as u32); }
        unsafe { pcm_drain(pcm); }
        std::thread::sleep(std::time::Duration::from_millis(50));
        unsafe { pcm_close(pcm); libc::dlclose(lib); }
        TORCH_ACTIVE.store(false, Ordering::Relaxed);
    }

    pub fn preload() { ensure_loaded(); }

    fn samples_for(name: &str) -> Option<Vec<i16>> {
        match name {
            "bazooka_explosion.wav" => EXPLOSION.get().cloned(),
            "grenade.wav"           => GRENADE.get().cloned(),
            "meteor.wav"            => METEOR.get().cloned(),
            "mine.wav"              => MINE.get().cloned(),
            "arm_beep.wav"          => MINE_ARM.get().cloned(),
            "barrell.wav"           => BARREL.get().cloned(),
            "tnt.wav"               => TNT.get().cloned(),
            "shotgun.wav"           => SHOTGUN.get().cloned(),
            "bat.wav"               => BAT.get().cloned(),
            "wet.wav"               => WET.get().cloned(),
            "water.wav"             => WATER.get().cloned(),
            "hum.wav"               => HUM.get().cloned(),
            "torch.wav"             => TORCH.get().cloned(),
            "garcia.wav"            => GARCIA.get().cloned(),
            "smash.wav"             => SMASH.get().cloned(),
            "hallelujah.wav"        => HALLELUJAH.get().cloned(),
            "minigun.wav"           => MINIGUN.get().cloned(),
            "mac10.wav"             => UZI.get().or_else(|| MINIGUN.get()).cloned(),
            _ => None,
        }
    }

    pub fn play(name: &str) {
        ensure_loaded();
        if let Some(s) = samples_for(name) { play_samples(s); }
    }

    pub fn play_once(name: &str) {
        ensure_loaded();
        if let Some(s) = samples_for(name) { play_samples_once(s); }
    }

    pub fn play_revolver_shot() {
        ensure_loaded();
        if let Some(s) = REVOLVER.get().cloned() { play_samples(s); }
    }

    pub fn play_death() {
        ensure_loaded();
        let Some(deaths) = DEATHS.get() else { return };
        if deaths.is_empty() { return; }
        let idx = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().subsec_nanos() as usize) % deaths.len();
        play_samples(deaths[idx].clone());
    }

    pub fn play_death_water() {
        ensure_loaded();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().subsec_nanos() as usize;
        // 50/50: water.wav or wet.wav
        if nanos % 2 == 0 {
            if let Some(s) = WATER.get().cloned() { play_samples(s); }
        } else {
            if let Some(s) = WET.get().cloned() { play_samples(s); }
        }
    }
}

// ── Desktop (rodio) implementation ───────────────────────────────────────────

#[cfg(feature = "desktop")]
mod imp_desktop {
    use std::sync::OnceLock;
    use rodio::{OutputStream, OutputStreamHandle, Decoder, Sink};
    use std::io::BufReader;

    static HANDLE: OnceLock<OutputStreamHandle> = OnceLock::new();

    pub fn init() {
        match OutputStream::try_default() {
            Ok((stream, handle)) => {
                std::mem::forget(stream); // keep alive for process lifetime
                let _ = HANDLE.set(handle);
            }
            Err(e) => eprintln!("audio init failed: {e}"),
        }
    }

    fn sfx_dir() -> Option<std::path::PathBuf> {
        Some(std::env::current_exe().ok()?.parent()?.join("sfx"))
    }

    pub fn play(name: &str) {
        let Some(handle) = HANDLE.get() else { return };
        let Some(dir) = sfx_dir() else { return };
        let path = dir.join(name);
        let Ok(file) = std::fs::File::open(&path) else { return };
        let Ok(decoder) = Decoder::new(BufReader::new(file)) else { return };
        match Sink::try_new(handle) {
            Ok(sink) => { sink.append(decoder); sink.detach(); }
            Err(_) => {}
        }
    }

    pub fn play_death() {
        let Some(dir) = sfx_dir() else { return };
        let death_dir = dir.join("death");
        let Ok(entries) = std::fs::read_dir(&death_dir) else {
            play("death1.wav");
            return;
        };
        let files: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("wav"))
            .filter(|e| e.file_name() != *"garcia.wav")
            .collect();
        if files.is_empty() { return; }
        let idx = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0) % files.len();
        let Some(handle) = HANDLE.get() else { return };
        let Ok(file) = std::fs::File::open(files[idx].path()) else { return };
        let Ok(decoder) = Decoder::new(BufReader::new(file)) else { return };
        match Sink::try_new(handle) {
            Ok(sink) => { sink.append(decoder); sink.detach(); }
            Err(_) => {}
        }
    }

    pub fn play_death_water() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);
        if nanos % 2 == 0 { play("water.wav"); } else { play("wet.wav"); }
    }

    pub fn preload() {
        // Trigger Sink creation (warms up rodio output stream) and try opening
        // each SFX file so first-play latency is low.
        let Some(dir) = sfx_dir() else { return };
        let files = &[
            "bazooka_explosion.wav","grenade.wav","meteor.wav","mine.wav","arm_beep.wav",
            "barrell.wav","tnt.wav","shotgun.wav","bat.wav","hum.wav","torch.wav",
            "garcia.wav","smash.wav","hallelujah.wav","minigun.wav","mac10.wav",
            "revolver_shot.wav","wet.wav","water.wav",
        ];
        for f in files {
            let path = dir.join(f);
            if let Ok(file) = std::fs::File::open(&path) {
                let _ = rodio::Decoder::new(std::io::BufReader::new(file));
            }
        }
    }
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

// Miyoo arm dispatch
#[cfg(all(target_arch = "arm", not(feature = "desktop")))]
fn _play(n: &str)      { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp::play(n); } }
#[cfg(all(target_arch = "arm", not(feature = "desktop")))]
fn _play_once(n: &str) { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp::play_once(n); } }
#[cfg(all(target_arch = "arm", not(feature = "desktop")))]
fn _play_revolver()    { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp::play_revolver_shot(); } }
#[cfg(all(target_arch = "arm", not(feature = "desktop")))]
fn _play_death()       { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp::play_death(); } }
#[cfg(all(target_arch = "arm", not(feature = "desktop")))]
fn _play_death_water() { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp::play_death_water(); } }

// Desktop rodio dispatch
#[cfg(feature = "desktop")]
fn _play(n: &str)      { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp_desktop::play(n); } }
#[cfg(feature = "desktop")]
fn _play_once(n: &str) { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp_desktop::play(n); } }
#[cfg(feature = "desktop")]
fn _play_revolver()    { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp_desktop::play("revolver_shot.wav"); } }
#[cfg(feature = "desktop")]
fn _play_death()       { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp_desktop::play_death(); } }
#[cfg(feature = "desktop")]
fn _play_death_water() { if !MUTED.load(std::sync::atomic::Ordering::Relaxed) { imp_desktop::play_death_water(); } }

// Non-arm, non-desktop: silence
#[cfg(all(not(target_arch = "arm"), not(feature = "desktop")))]
fn _play(_: &str)        {}
#[cfg(all(not(target_arch = "arm"), not(feature = "desktop")))]
fn _play_once(_: &str)   {}
#[cfg(all(not(target_arch = "arm"), not(feature = "desktop")))]
fn _play_revolver()      {}
#[cfg(all(not(target_arch = "arm"), not(feature = "desktop")))]
fn _play_death()         {}
#[cfg(all(not(target_arch = "arm"), not(feature = "desktop")))]
fn _play_death_water()   {}
