use crate::world::WorldPos;
use crate::physics::WeaponKind;
use super::soldier::Soldier;

/// Maximum soldiers per team.
pub const SOLDIERS_PER_TEAM: usize = 4;

/// One team of up to 4 soldiers.
#[derive(Debug, Clone)]
pub struct Team {
    /// Compact team index 0..team_count. Used for turn rotation and as the
    /// authoritative team identity on the wire (NetSoldier.team). NOT the colour.
    pub slot: usize,
    /// Colour identity 0-3 (Red/Blue/Green/Yellow) used by the renderer. Defaults
    /// to `slot` but in casual lobbies each player picks their colour, so it is
    /// decoupled from the compact index. See `set_color`.
    pub color_id: u8,
    /// Display name — set from the player's active roster.
    pub name: String,
    /// Avatar index 0–3 (wraps from the roster). u8::MAX = default.
    pub avatar_id: u8,
    /// ELO rating to display during ranked matches. 0 = not shown.
    pub elo: u32,
    /// Whether this slot is controlled by a human or CPU.
    pub is_cpu: bool,
    /// CPU difficulty (ignored for human teams).
    pub difficulty: Difficulty,
    /// The soldiers in this team.
    pub soldiers: Vec<Soldier>,
    /// Which soldier index is currently active (0-3).
    pub active: usize,
    /// Shared weapon inventory for all soldiers on this team: (kind, ammo). None = infinite.
    pub weapons: Vec<(WeaponKind, Option<u32>)>,
    /// Index into weapons vec of the currently selected weapon.
    pub selected_weapon: usize,
    /// Which headstone design this team uses (0–5).
    pub headstone_id: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty { Easy, Medium, Hard }

impl Team {
    /// Create a new team at spawn positions.
    pub fn new(slot: usize, is_cpu: bool, difficulty: Difficulty, spawns: &[WorldPos]) -> Self {
        let soldiers = spawns.iter().enumerate()
            .map(|(i, &pos)| Soldier::new(pos, slot, i))
            .collect();
        let name = ["Red", "Blue", "Green", "Yellow"][slot.min(3)].to_string();
        let avatar_id = slot as u8 % crate::renderer::avatar::AVATAR_COUNT as u8;
        Self { slot, color_id: slot.min(3) as u8, name, avatar_id, elo: 0, is_cpu, difficulty, soldiers, active: 0,
               weapons: team_loadout(), selected_weapon: 0,
               headstone_id: slot as u8 % crate::renderer::draw_sprites::HEADSTONE_COUNT }
    }

    /// Override this team's colour identity (casual lobby colour pick). Also
    /// resets the default display name to match the colour; callers that have a
    /// real roster name should set `name` afterwards.
    pub fn set_color(&mut self, color_id: u8) {
        let c = color_id.min(3);
        self.color_id = c;
        self.name = ["Red", "Blue", "Green", "Yellow"][c as usize].to_string();
    }

    /// Currently selected weapon kind.
    pub fn current_weapon(&self) -> WeaponKind {
        self.weapons[self.selected_weapon].0
    }

    /// Consume one use of the selected weapon. Returns false if out of ammo.
    pub fn consume_weapon(&mut self) -> bool {
        let ammo = &mut self.weapons[self.selected_weapon].1;
        match ammo {
            None => true,
            Some(0) => false,
            Some(n) => { *n -= 1; true }
        }
    }

    /// Remove weapons with zero ammo and keep selected_weapon in bounds.
    pub fn prune_empty_weapons(&mut self) {
        self.weapons.retain(|(_, ammo)| ammo.map_or(true, |n| n > 0));
        if !self.weapons.is_empty() {
            self.selected_weapon = self.selected_weapon.min(self.weapons.len() - 1);
        }
    }

    /// Add a weapon from a crate. Tops up ammo if already owned.
    pub fn add_weapon(&mut self, kind: WeaponKind, ammo: Option<u32>) {
        for (k, a) in &mut self.weapons {
            if *k == kind {
                match (a, ammo) {
                    (Some(existing), Some(extra)) => *existing += extra,
                    (slot, None) => *slot = None,
                    _ => {}
                }
                return;
            }
        }
        self.weapons.push((kind, ammo));
        self.weapons.sort_by_key(|(k, _)| k.menu_sort_key());
    }

    /// How many soldiers are still alive.
    pub fn alive_count(&self) -> u32 {
        self.soldiers.iter().filter(|s| s.is_alive()).count() as u32
    }

    /// Total HP across all living soldiers.
    pub fn total_hp(&self) -> u32 {
        self.soldiers.iter().filter(|s| s.is_alive()).map(|s| s.hp as u32).sum()
    }

    /// True if all soldiers are dead.
    pub fn is_eliminated(&self) -> bool {
        self.soldiers.iter().all(|s| s.is_dead())
    }

    /// The currently active soldier (may be dead — caller should check).
    pub fn active_soldier(&self) -> &Soldier {
        &self.soldiers[self.active]
    }

    pub fn active_soldier_mut(&mut self) -> &mut Soldier {
        &mut self.soldiers[self.active]
    }

    /// Advance to the next living soldier in this team.
    /// Cycles through all slots, skipping dead ones.
    /// Returns false if no living soldiers remain.
    pub fn advance_active(&mut self) -> bool {
        if self.is_eliminated() { return false; }
        let total = self.soldiers.len();
        for _ in 0..total {
            self.active = (self.active + 1) % total;
            if self.soldiers[self.active].is_alive() {
                return true;
            }
        }
        false
    }
}

/// Shared weapon loadout all teams start with.
fn team_loadout() -> Vec<(WeaponKind, Option<u32>)> {
    let mut v = vec![
        (WeaponKind::Bazooka,  None),    // infinite
        (WeaponKind::Grenade,  None),    // infinite
        (WeaponKind::Shotgun,  Some(2)), // 2 shots
        (WeaponKind::NinjaRope, Some(3)), // 3 uses; utility tool, doesn't end turn
        (WeaponKind::Tnt,          Some(1)), // 1 use; locked until 5 rotations
        (WeaponKind::Landmine,     Some(2)), // 2 uses
        (WeaponKind::BaseballBat,  Some(1)), // 1 use; locked until 3 full cycles
        (WeaponKind::PlasmaTorch,  Some(3)), // 3 uses; terrain tunneling tool
        (WeaponKind::HomingMissile, Some(1)), // 1 use; locked until 2 full cycles
    ];
    v.sort_by_key(|(k, _)| k.menu_sort_key());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: f32) -> WorldPos { WorldPos::new(x, 200.0) }

    fn spawns() -> Vec<WorldPos> {
        vec![pos(100.0), pos(150.0), pos(200.0), pos(250.0)]
    }

    fn team() -> Team {
        Team::new(0, false, Difficulty::Medium, &spawns())
    }

    #[test]
    fn team_starts_with_four_soldiers() {
        let t = team();
        assert_eq!(t.soldiers.len(), SOLDIERS_PER_TEAM);
    }

    #[test]
    fn all_soldiers_start_alive() {
        let t = team();
        assert_eq!(t.alive_count(), 4);
        assert!(!t.is_eliminated());
    }

    #[test]
    fn total_hp_is_400_at_start() {
        let t = team();
        assert_eq!(t.total_hp(), 400);
    }

    #[test]
    fn alive_count_decreases_when_soldier_dies() {
        let mut t = team();
        t.soldiers[0].take_damage(100);
        assert_eq!(t.alive_count(), 3);
    }

    #[test]
    fn eliminated_when_all_dead() {
        let mut t = team();
        for s in &mut t.soldiers { s.take_damage(100); }
        assert!(t.is_eliminated());
        assert_eq!(t.alive_count(), 0);
    }

    #[test]
    fn total_hp_excludes_dead_soldiers() {
        let mut t = team();
        t.soldiers[0].take_damage(100);
        assert_eq!(t.total_hp(), 300);
    }

    #[test]
    fn advance_active_cycles_to_next_alive() {
        let mut t = team();
        assert_eq!(t.active, 0);
        t.advance_active();
        assert_eq!(t.active, 1);
    }

    #[test]
    fn advance_active_skips_dead_soldiers() {
        let mut t = team();
        t.soldiers[1].take_damage(100); // kill slot 1
        t.advance_active();
        assert_eq!(t.active, 2, "should skip dead slot 1");
    }

    #[test]
    fn advance_active_wraps_around() {
        let mut t = team();
        t.active = 3;
        t.advance_active();
        assert_eq!(t.active, 0);
    }

    #[test]
    fn advance_active_returns_false_when_all_dead() {
        let mut t = team();
        for s in &mut t.soldiers { s.take_damage(100); }
        assert!(!t.advance_active());
    }

    #[test]
    fn active_soldier_returns_correct_slot() {
        let t = team();
        assert_eq!(t.active_soldier().index, 0);
    }

    #[test]
    fn team_slot_stored_correctly() {
        let t = Team::new(2, true, Difficulty::Hard, &spawns());
        assert_eq!(t.slot, 2);
        assert!(t.is_cpu);
        assert_eq!(t.difficulty, Difficulty::Hard);
    }

    // ── Weapons (moved here from soldier.rs when the loadout became per-team) ──

    #[test]
    fn default_loadout_matches_team_loadout() {
        let t = team();
        assert_eq!(t.weapons[0], (WeaponKind::Bazooka, None));   // infinite
        assert_eq!(t.weapons[1], (WeaponKind::Grenade, None));   // infinite
        assert_eq!(t.weapons[2], (WeaponKind::Shotgun, Some(2)));
        assert_eq!(t.weapons.len(), 8);
        assert_eq!(t.selected_weapon, 0);
        assert_eq!(t.current_weapon(), WeaponKind::Bazooka);
    }

    #[test]
    fn consume_infinite_weapon_always_succeeds() {
        let mut t = team();
        t.selected_weapon = 0; // Bazooka, infinite
        for _ in 0..100 { assert!(t.consume_weapon()); }
    }

    #[test]
    fn consume_limited_weapon_decrements_then_fails() {
        let mut t = team();
        t.selected_weapon = 2; // Shotgun x2
        assert!(t.consume_weapon());
        assert_eq!(t.weapons[2].1, Some(1));
        assert!(t.consume_weapon());
        assert_eq!(t.weapons[2].1, Some(0));
        assert!(!t.consume_weapon()); // out of ammo
    }

    #[test]
    fn add_weapon_new_kind_appends() {
        let mut t = team();
        let before = t.weapons.len();
        t.add_weapon(WeaponKind::Revolver, Some(1));
        assert_eq!(t.weapons.len(), before + 1);
    }

    #[test]
    fn add_weapon_existing_tops_up_ammo() {
        let mut t = team();
        t.add_weapon(WeaponKind::Shotgun, Some(3)); // starts at 2
        assert_eq!(t.weapons[2].1, Some(5));
    }

    #[test]
    fn add_weapon_upgrades_to_infinite() {
        let mut t = team();
        t.add_weapon(WeaponKind::Shotgun, None);
        assert_eq!(t.weapons[2].1, None);
    }

    #[test]
    fn prune_empty_weapons_drops_zero_ammo() {
        let mut t = team();
        t.selected_weapon = 2; // Shotgun x2
        t.consume_weapon();
        t.consume_weapon(); // now Some(0)
        let before = t.weapons.len();
        t.prune_empty_weapons();
        assert_eq!(t.weapons.len(), before - 1);
        assert!(t.selected_weapon < t.weapons.len());
    }
}
