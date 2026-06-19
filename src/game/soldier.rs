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
    /// Ticks remaining of "on fire" squirm animation. Visual-only.
    pub on_fire_ticks: u32,
    /// How the soldier died — set just before the fatal take_damage call.
    pub death_cause: DeathCause,
    /// Which weapon last dealt damage to this soldier — used to credit kill weapon.
    /// Updated on every damaging hit; survives into water/fall deaths so the weapon
    /// that knocked them off the map gets the kill credit.
    pub kill_weapon: Option<crate::physics::WeaponKind>,

    // ── Per-soldier cosmetics (0 = default for all) ──────────────────────────
    pub hat_id:           u8,
    pub uniform_color_id: u8,
    pub boot_color_id:    u8,
    pub gun_style_id:     u8,
}

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
            walk_ticks:   0,
            name: format!("Soldier {}", index + 1),
            has_fired: false,
            has_grave: false,
            death_explosion_pending: false,
            hp_display_ticks: 0,
            displayed_hp: 100,
            on_fire_ticks: 0,
            death_cause: DeathCause::Generic,
            kill_weapon: None,
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
