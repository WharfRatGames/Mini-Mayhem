//! Replay harness (experimental plan, step 1; see docs/EXPERIMENTAL_PLAN.md).
//!
//! A `Replay` is everything needed to reproduce a match segment exactly: the map seed, the team count and the held-button bits for
//! every tick. `run()` feeds them through the shared `simulate()` and returns a state hash at every checkpoint, so a bug becomes a
//! recorded file plus a failing comparison instead of a log hunt, and behaviour changes show up as the first tick that diverges.
//!
//! Internal tooling: nothing here is advertised to players and nothing here touches the network or save data.

use crate::game::loop_runner::server_tick;
use crate::game::state::GameState;
use crate::input::{Button, InputState};

/// Ticks between state-hash checkpoints.
pub const CHECKPOINT_STRIDE: usize = 30;
const FORMAT_VERSION: u32 = 2;

/// Everything the live server hands `server_tick` for one tick: the active player's input frame plus the client's authoritative aim
/// angle, rendered muzzle position and selected weapon kind (`InputMsg`). Scripted replays leave the optional parts empty.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TickInput {
    pub held: u16,
    pub pressed: u16,
    pub released: u16,
    pub aim: Option<f32>,
    pub muzzle: Option<(f32, f32)>,
    /// `WeaponKind` net id the client had selected (the server selects by kind, not index).
    pub weapon: Option<u8>,
}

impl TickInput {
    fn to_token(&self) -> String {
        let f = |v: Option<f32>| v.map_or("-".to_string(), |x| format!("{:08x}", x.to_bits()));
        let m = self.muzzle.map_or("-,-".to_string(), |(x, y)| format!("{:08x},{:08x}", x.to_bits(), y.to_bits()));
        format!("{:x},{:x},{:x},{},{},{}", self.held, self.pressed, self.released, f(self.aim), m, self.weapon.map_or("-".to_string(), |w| w.to_string()))
    }

    fn from_token(t: &str) -> Result<Self, String> {
        let p: Vec<&str> = t.split(',').collect();
        if p.len() != 7 { return Err(format!("bad tick record '{t}'")); }
        let hex = |s: &str| u16::from_str_radix(s, 16).map_err(|e| format!("{e} in '{t}'"));
        let fl = |s: &str| -> Result<Option<f32>, String> {
            if s == "-" { Ok(None) } else { u32::from_str_radix(s, 16).map(|b| Some(f32::from_bits(b))).map_err(|e| format!("{e} in '{t}'")) }
        };
        let muzzle = match (fl(p[4])?, fl(p[5])?) { (Some(x), Some(y)) => Some((x, y)), _ => None };
        let weapon = if p[6] == "-" { None } else { Some(p[6].parse::<u8>().map_err(|e| format!("{e} in '{t}'"))?) };
        Ok(Self { held: hex(p[0])?, pressed: hex(p[1])?, released: hex(p[2])?, aim: fl(p[3])?, muzzle, weapon })
    }

    /// Bit pattern equality (so NaN aim angles round-trip and compare equal).
    fn same(&self, o: &Self) -> bool { self.to_token() == o.to_token() }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub seed: u64,
    /// Team colour ids as the server was given them (`setup::build_game_n(seed, &colors)`); the length is the team count.
    pub colors: Vec<u8>,
    pub frames: Vec<TickInput>,
}

/// What a run produced.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// (tick, state hash) every `CHECKPOINT_STRIDE` ticks, plus the final tick.
    pub checkpoints: Vec<(usize, u64)>,
    /// Total and worst single-tick time spent inside `server_tick()`, in microseconds (host speed, for relative comparison only).
    pub sim_us_total: u64,
    pub sim_us_max: u64,
}

impl Replay {
    /// A scripted replay for `teams` teams with the default colours 0..teams.
    pub fn new(seed: u64, teams: usize) -> Self { Self { seed, colors: (0..teams.clamp(2, 4) as u8).collect(), frames: Vec::new() } }
    pub fn teams(&self) -> usize { self.colors.len() }

    /// Serialise to a small versioned text file: header lines, then run-length encoded `count*record` tokens.
    pub fn to_text(&self) -> String {
        let colors: Vec<String> = self.colors.iter().map(|c| c.to_string()).collect();
        let mut s = format!("arty-replay {}\nseed {}\ncolors {}\nframes {}\n", FORMAT_VERSION, self.seed, colors.join(","), self.frames.len());
        let mut i = 0;
        let mut toks: Vec<String> = Vec::new();
        while i < self.frames.len() {
            let mut n = 1;
            while i + n < self.frames.len() && self.frames[i + n].same(&self.frames[i]) { n += 1; }
            toks.push(format!("{}*{}", n, self.frames[i].to_token()));
            i += n;
        }
        s.push_str(&toks.join(" "));
        s.push('\n');
        s
    }

    pub fn from_text(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let hdr = lines.next().ok_or("empty replay")?;
        if hdr != format!("arty-replay {}", FORMAT_VERSION) { return Err(format!("unsupported replay header: {hdr}")); }
        let mut field = |name: &str| -> Result<String, String> {
            let l = lines.next().ok_or(format!("missing {name}"))?;
            l.strip_prefix(&format!("{name} ")).map(|v| v.to_string()).ok_or(format!("expected '{name} ...', got '{l}'"))
        };
        let seed: u64 = field("seed")?.parse().map_err(|e| format!("seed: {e}"))?;
        let colors: Vec<u8> = field("colors")?.split(',').map(|c| c.parse::<u8>().map_err(|e| format!("colors: {e}"))).collect::<Result<_, _>>()?;
        let nframes: usize = field("frames")?.parse().map_err(|e| format!("frames: {e}"))?;
        let mut frames = Vec::with_capacity(nframes);
        for tok in lines.next().unwrap_or("").split_whitespace() {
            let (n, rec) = tok.split_once('*').ok_or(format!("bad token {tok}"))?;
            let n: usize = n.parse().map_err(|e| format!("count: {e}"))?;
            let f = TickInput::from_token(rec)?;
            frames.extend(std::iter::repeat(f).take(n));
        }
        if frames.len() != nframes { return Err(format!("frame count {} != header {}", frames.len(), nframes)); }
        Ok(Self { seed, colors, frames })
    }

    /// Replay the inputs through `server_tick()` (the live-server path) from a freshly built game and hash the state at each
    /// checkpoint.
    pub fn run(&self) -> RunResult { self.run_game().0 }

    /// Like `run`, but also returns the final game state (for assertions about what the scenario actually did).
    pub fn run_game(&self) -> (RunResult, GameState) { self.run_observed(|_, _| {}) }

    /// Like `run_game`, calling `observe(game, tick)` after every simulated tick (for invariant checks).
    pub fn run_observed(&self, mut observe: impl FnMut(&GameState, usize)) -> (RunResult, GameState) {
        let mut game = new_game(self.seed, &self.colors);
        let mut checkpoints = Vec::new();
        let (mut total, mut max) = (0u64, 0u64);
        for (i, f) in self.frames.iter().enumerate() {
            // Same per-tick preparation as the live server loop: select the weapon by kind, then tick.
            if let Some(w) = f.weapon {
                let kind = crate::physics::projectile::WeaponKind::from_net_u8(w);
                let ti = game.active_team();
                if let Some(idx) = game.teams[ti].weapons.iter().position(|(k, _)| *k == kind) { game.teams[ti].selected_weapon = idx; }
            }
            let input = InputState::from_sets(f.held, f.pressed, f.released);
            let aim = f.aim.filter(|a| a.is_finite());
            let t = std::time::Instant::now();
            server_tick(&mut game, &input, f.muzzle, aim);
            let us = t.elapsed().as_micros() as u64;
            total += us;
            max = max.max(us);
            observe(&game, i);
            if (i + 1) % CHECKPOINT_STRIDE == 0 || i + 1 == self.frames.len() {
                checkpoints.push((i + 1, state_hash(&game)));
            }
        }
        (RunResult { checkpoints, sim_us_total: total, sim_us_max: max }, game)
    }
}

/// Collects a live match into a `Replay`. Call `push` once per `server_tick` with exactly what was passed to it.
pub struct Recorder { replay: Replay }

impl Recorder {
    pub fn new(seed: u64, colors: &[u8]) -> Self { Self { replay: Replay { seed, colors: colors.to_vec(), frames: Vec::new() } } }

    /// `weapon` is the `InputMsg::selected_weapon_kind` of the frame, if one was received this tick.
    pub fn push(&mut self, input: &InputState, aim: Option<f32>, muzzle: Option<(f32, f32)>, weapon: Option<u8>) {
        self.replay.frames.push(TickInput { held: input.to_bits(), pressed: input.pressed_bits(), released: input.released_bits(), aim, muzzle, weapon });
    }
    pub fn finish(self) -> Replay { self.replay }
}

/// Server-side match recorder, enabled by the `ARTY_RECORD_DIR` environment variable (internal tooling: unset on the production
/// server, nothing is advertised to players, and it never touches game state). Writes `<dir>/<name>.replay` via `save`; every failure is swallowed so a full disk can never affect a match.
pub struct MatchRecorder { rec: Recorder, dir: std::path::PathBuf }

impl MatchRecorder {
    pub fn from_env(seed: u64, colors: &[u8]) -> Option<Self> {
        let dir = std::env::var_os("ARTY_RECORD_DIR")?;
        Some(Self { rec: Recorder::new(seed, colors), dir: dir.into() })
    }
    pub fn push(&mut self, input: &InputState, aim: Option<f32>, muzzle: Option<(f32, f32)>, weapon: Option<u8>) {
        if self.rec.replay.frames.len() < 4 * 60 * 30 * 60 { self.rec.push(input, aim, muzzle, weapon); } // cap: 4 hours
    }
    pub fn save(self, name: &str) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.dir.join(format!("{name}.replay")), self.rec.finish().to_text());
    }
}

/// Deterministic fresh game built exactly as the live server builds one (`setup::build_game_n`, including mines and barrels).
pub fn new_game(seed: u64, colors: &[u8]) -> GameState { crate::game::setup::build_game_n(seed, colors) }

struct Fnv(u64);
impl Fnv {
    fn new() -> Self { Self(0xcbf29ce484222325) }
    fn bytes(&mut self, b: &[u8]) { for &x in b { self.0 ^= x as u64; self.0 = self.0.wrapping_mul(0x100000001b3); } }
    fn u(&mut self, v: u64) { self.bytes(&v.to_le_bytes()); }
    fn f(&mut self, v: f32) { self.bytes(&v.to_bits().to_le_bytes()); }
    fn s(&mut self, v: &str) { self.bytes(v.as_bytes()); self.bytes(&[0xff]); }
}

/// Hash of everything that decides what a player sees or the outcome: soldiers, projectiles, objects, turn, wind, craters.
/// Floats are hashed by bit pattern, so any change of arithmetic (even 1 ulp) diverges; that is the point.
pub fn state_hash(g: &GameState) -> u64 {
    let mut h = Fnv::new();
    h.u(g.tick as u64);
    h.u(g.turn.turn_number as u64);
    h.u(g.active_team() as u64);
    h.f(g.wind.value());
    for t in &g.teams {
        h.u(t.active as u64);
        for s in &t.soldiers {
            h.f(s.pos.x); h.f(s.pos.y);
            h.u(s.hp as u64); h.u(s.facing as i64 as u64);
            h.s(&format!("{:?}", s.state));
        }
    }
    for p in &g.projectiles {
        h.f(p.pos.x); h.f(p.pos.y); h.f(p.vel.x); h.f(p.vel.y);
        h.s(&format!("{:?}", p.kind)); h.u(p.age_ticks as u64);
    }
    h.s(&format!("{:?}", g.crates));
    h.s(&format!("{:?}", g.mines));
    h.s(&format!("{:?}", g.barrels));
    h.s(&format!("{:?}", g.fire_patches));
    h.s(&format!("{:?}", g.black_holes));
    h.u(g.crater_log.len() as u64);
    for c in &g.crater_log { h.f(c.0); h.f(c.1); h.f(c.2); }
    h.0
}

/// Build a scripted input list from `(button list, ticks)` segments. Press and release events are derived from the held changes
/// (as `InputState::from_bits` does); no aim, muzzle or weapon is supplied, so the sim uses its own button-driven aiming.
pub fn script(segments: &[(&[Button], usize)]) -> Vec<TickInput> {
    let mut out = Vec::new();
    let mut prev = 0u16;
    for (btns, n) in segments {
        let mut bits = 0u16;
        for (i, b) in Button::ALL.iter().enumerate() { if btns.contains(b) { bits |= 1 << i; } }
        for _ in 0..*n {
            out.push(TickInput { held: bits, pressed: bits & !prev, released: prev & !bits, ..Default::default() });
            prev = bits;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip() {
        let mut r = Replay::new(42, 2);
        r.frames = script(&[(&[], 10), (&[Button::Right], 5), (&[Button::Right, Button::B], 1), (&[], 3)]);
        r.frames[2].aim = Some(1.25);
        r.frames[3].muzzle = Some((10.5, -3.0));
        r.frames[4].weapon = Some(7);
        r.frames[5].aim = Some(f32::NAN);
        assert_eq!(Replay::from_text(&r.to_text()).unwrap().to_text(), r.to_text());
        let back = Replay::from_text(&r.to_text()).unwrap();
        assert_eq!(back.frames.len(), r.frames.len());
        assert_eq!(back.frames[3].muzzle, Some((10.5, -3.0)));
        assert_eq!(back.frames[4].weapon, Some(7));
        assert!(back.frames[5].aim.unwrap().is_nan());
    }
}
