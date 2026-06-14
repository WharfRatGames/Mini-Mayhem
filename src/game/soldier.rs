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
            self.state = SoldierState::Dead;
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

    #[test]
    fn base_loadout_has_correct_weapons() {
        let s = soldier();
        assert_eq!(s.weapons[0].0, WeaponKind::Bazooka);
        assert_eq!(s.weapons[1].0, WeaponKind::Grenade);
        assert_eq!(s.weapons[2].0, WeaponKind::Shotgun);
        assert_eq!(s.weapons.len(), 3);
    }

    #[test]
    fn infinite_weapons_have_none_ammo() {
        let s = soldier();
        assert_eq!(s.weapons[0].1, None); // Bazooka
        assert_eq!(s.weapons[1].1, None); // Grenade
    }

    #[test]
    fn limited_weapons_have_correct_ammo() {
        let s = soldier();
        assert_eq!(s.weapons[4].1, Some(2)); // Landmine
        assert_eq!(s.weapons[5].1, Some(2)); // TNT
        assert_eq!(s.weapons[6].1, Some(3)); // Rope
    }

    #[test]
    fn next_weapon_cycles_forward() {
        let mut s = soldier();
        assert_eq!(s.selected_weapon, 0);
        s.next_weapon();
        assert_eq!(s.selected_weapon, 1);
    }

    #[test]
    fn next_weapon_wraps_around() {
        let mut s = soldier();
        let total = s.weapons.len();
        for _ in 0..total { s.next_weapon(); }
        assert_eq!(s.selected_weapon, 0);
    }

    #[test]
    fn prev_weapon_cycles_backward() {
        let mut s = soldier();
        s.next_weapon();
        s.next_weapon();
        s.prev_weapon();
        assert_eq!(s.selected_weapon, 1);
    }

    #[test]
    fn prev_weapon_wraps_around() {
        let mut s = soldier();
        s.prev_weapon();
        assert_eq!(s.selected_weapon, s.weapons.len() - 1);
    }

    #[test]
    fn consume_infinite_weapon_always_succeeds() {
        let mut s = soldier();
        s.selected_weapon = 0; // Bazooka, infinite
        for _ in 0..100 {
            assert!(s.consume_weapon());
        }
    }

    #[test]
    fn consume_limited_weapon_decrements() {
        let mut s = soldier();
        s.selected_weapon = 4; // Landmine x2
        assert!(s.consume_weapon());
        assert_eq!(s.weapons[4].1, Some(1));
        assert!(s.consume_weapon());
        assert_eq!(s.weapons[4].1, Some(0));
        assert!(!s.consume_weapon()); // out of ammo
    }

    #[test]
    fn add_weapon_new_kind_appends() {
        let mut s = soldier();
        let before = s.weapons.len();
        s.add_weapon(WeaponKind::AirStrike, Some(1));
        assert_eq!(s.weapons.len(), before + 1);
    }

    #[test]
    fn add_weapon_existing_tops_up_ammo() {
        let mut s = soldier();
        s.selected_weapon = 4; // Landmine x2
        s.add_weapon(WeaponKind::Landmine, Some(3));
        assert_eq!(s.weapons[4].1, Some(5));
    }

    #[test]
    fn add_weapon_upgrades_to_infinite() {
        let mut s = soldier();
        s.add_weapon(WeaponKind::Landmine, None);
        assert_eq!(s.weapons[4].1, None);
    }

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
