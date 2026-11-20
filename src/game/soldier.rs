use crate::world::{WorldPos, Vec2};
use crate::physics::FallTracker;

/// What a soldier is currently doing.
#[derive(Debug, Clone, PartialEq)]
pub enum SoldierState {
    /// Standing on solid ground, waiting for input.
    Idle,
    /// Walking left or right.
    Walking { dir: f32 },
    /// In the air — falling or jumping.
    /// `spinning` is true for a backflip (Worms-style constant rotation).
    Airborne { vel: Vec2, spinning: bool },
    /// Dead.
    Dead,
}

/// How the soldier died — used to pick a contextual death message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathCause {
    Generic,
    Explosion,
    Fall,
    Water,
}

/// WA worm actions that take fixed time (50 fps frames; Arty ticks run 5 frames per 3 ticks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaAct {
    /// Crouch before takeoff (state 0x77, 20 frames); `backflip` selects the impulse.
    /// `tilt` = WA's 2nd-press tweak of a backflip: 0 straight up (90 deg), 1 Backspace twice (86.4 deg), 2 Enter twice (80 deg).
    Windup { backflip: bool, tilt: u8, age: u32 },
    /// Landing recovery (state 0x7a, 12 frames).
    Recover { age: u32 },
    /// Turn-around before a walk in the new direction (wa_rig walk_right vs walk_slope: first step lands 14 frames after the +0.5 px nudge).
    Turn { age: u32 },
}
/// Crouch before a jump/backflip, in 50 Hz frames. WA's is 20 (0.4 s); Arty shortens it to 8 (0.16 s) now that the crouch is animated
/// (`SoldierAnim::Crouch`): long enough to read as a wind-up, short enough not to feel like input lag. Deliberate deviation.
pub const WA_WINDUP_FRAMES: u32 = 8;
pub const WA_RECOVER_FRAMES: u32 = 12;
pub const WA_TURN_FRAMES: u32 = 14;

/// One soldier in the game.
#[derive(Debug, Clone)]
pub struct Soldier {
    /// World-space foot position.
    pub pos:     WorldPos,
    /// +1 = facing right, -1 = facing left.
    pub facing:  i8,
    /// Current HP. 0 = dead.
    pub hp:      u8,
    /// Which team this soldier belongs to (0-3).
    pub team:    usize,
    /// Index within the team (0-3).
    pub index:   usize,
    /// Current behavioural state.
    pub state:   SoldierState,
    /// Tracks fall distance for damage.
    pub fall:    FallTracker,
    /// True if this soldier has moved this turn.
    pub has_moved:    bool,
    pub airtime:      u32,
    /// Fixed-point WA worm body while airborne (+ the f32 vel it was last synced with). None = (re)initialise from pos/vel.
    pub wa: Option<(crate::physics::wa_worm::WormBody, crate::world::Vec2)>,
    /// WA walk-cycle state (anchor/phase) while a walk is in progress.
    pub wa_walk: Option<crate::physics::wa_worm::WalkState>,
    /// WA jump windup / landing recovery in progress (input locked for walking and jumping meanwhile).
    pub wa_act: Option<WaAct>,
    /// Display smoothing for WA's 50 fps motion on the 30 Hz tick: `(prev_x, prev_y, frac)` = foot position one WA frame
    /// back and how far towards `pos` to draw. Set by the walk/flight steppers, cleared at the start of every tick. Visual only.
    pub wa_vis: Option<(f32, f32, f32)>,
    /// Accumulated ticks spent walking this frame (used for walk cycle animation).
    pub walk_ticks:   u32,
    /// True if this soldier has fired this turn.
    pub has_fired: bool,
    /// Display name — set from the player's active roster.
    pub name: String,
    /// True once a grave has been recorded for this soldier (set on first death).
    pub has_grave: bool,
    /// True while waiting for the 1-second death explosion timer.
    pub death_explosion_pending: bool,
    /// Ticks remaining to display the HP box after taking damage. Visual-only.
    pub hp_display_ticks: u32,
    /// Displayed HP — animates toward actual hp at 3/tick. Visual-only.
    pub displayed_hp: u8,
    /// Sub-HP shadow of `displayed_hp` for WA's exponential countdown. Visual-only.
    pub displayed_hp_f: f32,
    /// Damage taken but not yet shown as a popup — tallies hits landing close
    /// together (e.g. 5 pistol shots) into one number. Flushed by the sim once
    /// `damage_settle` runs out.
    pub pending_damage: u32,
    /// Ticks until `pending_damage` is considered settled; reset on every hit.
    pub damage_settle: u32,
    /// Ticks the HP countdown holds after the damage popup appears (synced so
    /// the live client's displayed_hp waits identically). Visual-only timing.
    pub hp_countdown_delay: u32,
    /// Ticks remaining of "on fire" squirm animation. Visual-only.
    pub on_fire_ticks: u32,
    /// How the soldier died — set just before the fatal take_damage call.
    pub death_cause: DeathCause,
    /// Which weapon last dealt damage to this soldier — used to credit kill weapon.
    /// Updated on every damaging hit; survives into water/fall deaths so the weapon
    /// that knocked them off the map gets the kill credit.
    pub kill_weapon: Option<crate::physics::WeaponKind>,
    /// Baseball Bat damage held until this soldier lands (WA-style: the knockback
    /// plays out first, hit only registers on impact). Applied at either landing
    /// site in loop_runner.rs alongside fall damage, then cleared.
    pub pending_bat_damage: u32,

    // ── Per-soldier cosmetics (0 = default for all) ──────────────────────────
    pub hat_id:           u8,
    pub uniform_color_id: u8,
    pub boot_color_id:    u8,
    pub gun_style_id:     u8,
}

/// Ticks with no new damage before the pending tally pops up (game runs 30 Hz).
pub const DAMAGE_SETTLE_TICKS: u32 = 20;
/// Popup-to-countdown pause: the HP box holds its old value ~2 s after the
/// damage number appears, then ticks down.
/// WA (wa_rig, 2 runs): the real HP (worm +0x17c) only changes ~148-151 frames (~3 s, the post-shot retreat timer) after
/// the shot is fired, i.e. ~142-147 frames after the explosion; the displayed HP (+0x178) starts easing that same frame.
/// Arty: 20-tick tally pause + this hold = ~86 ticks (~144 frames) after the explosion.
pub const HP_COUNTDOWN_DELAY_TICKS: u32 = 66;
/// WA closes 10% of the gap per 50 Hz frame; one 30 Hz tick = 5/3 frames.
const HP_EASE_PER_TICK: f32 = 0.16093; // 1 - 0.9^(5/3)

impl Soldier {
    /// Create a new soldier at a world position.
    pub fn new(pos: WorldPos, team: usize, index: usize) -> Self {
        Self {
            pos,
            facing: if team == 0 { 1 } else { -1 },
            hp: 100,
            team,
            index,
            state: SoldierState::Idle,
            fall: FallTracker::new(),
            has_moved:    false,
            airtime:      0,
            wa: None,
            wa_walk: None,
            wa_act: None,
            wa_vis: None,
            walk_ticks:   0,
            name: format!("Soldier {}", index + 1),
            has_fired: false,
            has_grave: false,
            death_explosion_pending: false,
            hp_display_ticks: 0,
            displayed_hp: 100,
            displayed_hp_f: 100.0,
            pending_damage: 0,
            damage_settle: 0,
            hp_countdown_delay: 0,
            on_fire_ticks: 0,
            death_cause: DeathCause::Generic,
            kill_weapon: None,
            pending_bat_damage: 0,
            hat_id:           0,
            uniform_color_id: 0,
            boot_color_id:    0,
            gun_style_id:     0,
        }
    }

    pub fn is_alive(&self) -> bool { self.hp > 0 }
    pub fn is_dead(&self)  -> bool { self.hp == 0 }

    /// Apply damage. Clamps to 0, sets state to Dead if hp reaches 0.
    pub fn take_damage(&mut self, dmg: u32) {
        if dmg > 0 {
            self.hp_display_ticks = 150; // show HP box for ~5 s after being hit
            // Tally into the pending popup instead of showing per-hit numbers;
            // the settle window restarts on every hit so rapid multi-hit damage
            // (pistol/uzi bursts, multi-pellet shotgun) reads as one total.
            self.pending_damage += dmg;
            self.damage_settle = DAMAGE_SETTLE_TICKS;
        }
        self.hp = self.hp.saturating_sub(dmg as u8);
        if self.hp == 0 {
            // Airborne soldiers keep their velocity so they fall before the death explosion.
            // Grounded soldiers go Dead immediately.
            if !matches!(self.state, SoldierState::Airborne { .. }) {
                self.state = SoldierState::Dead;
            }
        }
    }

    /// One tick of the HP-box display: visibility timer, popup-to-countdown
    /// hold, then drain displayed_hp toward hp at 1/tick. Heals snap up
    /// immediately. Single source for all three display-stepping contexts
    /// (crate-watch, main sim, live-client update_visuals).
    pub fn step_hp_display(&mut self) {
        if self.hp_display_ticks > 0 { self.hp_display_ticks -= 1; }
        if self.displayed_hp_f.floor() as u8 != self.displayed_hp {
            self.displayed_hp_f = self.displayed_hp as f32; // set externally
        }
        if self.displayed_hp < self.hp {
            self.displayed_hp = self.hp;
            self.displayed_hp_f = self.hp as f32;
        } else if self.displayed_hp > self.hp {
            // Keep the box on screen while a countdown is pending or running.
            self.hp_display_ticks = self.hp_display_ticks.max(30);
            if self.pending_damage > 0 {
                // Damage still tallying — hold until the popup has appeared.
            } else if self.hp_countdown_delay > 0 {
                self.hp_countdown_delay -= 1;
            } else {
                let gap = self.displayed_hp_f - self.hp as f32;
                self.displayed_hp_f = if gap <= 1.0 { self.hp as f32 }
                    else { self.displayed_hp_f - gap * HP_EASE_PER_TICK };
                self.displayed_hp = (self.displayed_hp_f.floor() as u8).max(self.hp);
                // The integer box has caught up: land the float exactly on the real HP (WA's shown HP ends on it).
                if self.displayed_hp == self.hp { self.displayed_hp_f = self.hp as f32; }
            }
        } else if self.hp_countdown_delay > 0 {
            // displayed_hp already caught up to hp (e.g. a damage flush armed
            // the countdown in the same tick displayed_hp got reconciled) —
            // nothing left to animate, so drop the stale countdown. Otherwise
            // it never decrements (the branch above is unreachable once
            // displayed_hp == hp) and any_soldier_tallying() blocks
            // TurnPhase::Ending forever, soft-locking the match.
            self.hp_countdown_delay = 0;
        }
    }

    /// Heal (no cap — overhealing is allowed).
    pub fn heal(&mut self, amount: u32) {
        self.hp = self.hp.saturating_add(amount as u8);
        self.hp_display_ticks = 150; // show HP box on crate pickup
    }

    /// Reset per-turn flags.
    pub fn begin_turn(&mut self) {
        self.has_moved = false;
        self.has_fired = false;
        self.hp_display_ticks = 150; // show HP at start of turn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos() -> WorldPos { WorldPos::new(100.0, 200.0) }
    fn soldier() -> Soldier { Soldier::new(pos(), 0, 0) }

    #[test]
    fn new_soldier_has_100hp() {
        assert_eq!(soldier().hp, 100);
    }

    #[test]
    fn new_soldier_is_alive() {
        assert!(soldier().is_alive());
        assert!(!soldier().is_dead());
    }

    #[test]
    fn take_damage_reduces_hp() {
        let mut s = soldier();
        s.take_damage(30);
        assert_eq!(s.hp, 70);
    }

    #[test]
    fn take_damage_does_not_underflow() {
        let mut s = soldier();
        s.take_damage(999);
        assert_eq!(s.hp, 0);
    }

    #[test]
    fn death_sets_state_to_dead() {
        let mut s = soldier();
        s.take_damage(100);
        assert_eq!(s.state, SoldierState::Dead);
        assert!(s.is_dead());
    }

    #[test]
    fn partial_damage_does_not_kill() {
        let mut s = soldier();
        s.take_damage(99);
        assert_eq!(s.hp, 1);
        assert!(s.is_alive());
        assert_ne!(s.state, SoldierState::Dead);
    }

    #[test]
    fn heal_increases_hp() {
        let mut s = soldier();
        s.take_damage(50);
        s.heal(25);
        assert_eq!(s.hp, 75);
    }

    #[test]
    fn overheal_allowed() {
        let mut s = soldier();
        s.heal(50);
        assert_eq!(s.hp, 150);
    }

    #[test]
    fn heal_saturates_at_u8_max() {
        let mut s = soldier();
        s.heal(200);
        assert_eq!(s.hp, u8::MAX);
    }

    // Weapon tests moved to team.rs — the loadout is per-Team now, not per-Soldier.

    #[test]
    fn begin_turn_clears_flags() {
        let mut s = soldier();
        s.has_moved = true;
        s.has_fired = true;
        s.begin_turn();
        assert!(!s.has_moved);
        assert!(!s.has_fired);
    }
}
