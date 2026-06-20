use crate::world::{WorldPos, Vec2};

/// Every weapon that fires a projectile.
/// Utility items (rope, jetpack, girder) are not projectiles and live elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponKind {
    // ── Base kit ─────────────────────────────────────────────────────────────
    Bazooka,
    Grenade,
    Shotgun,
    ClusterBomb,
    Landmine,
    Tnt,

    /// Small explosion when a soldier dies — triggers chain reactions.
    DeathExplosion,

    // ── Crate only ───────────────────────────────────────────────────────────
    HolyHandGrenade,
    AirStrike,
    Minigun,
    BaseballBat,
    FreezeGrenade,
    Earthquake,
    Drill,
    HomingMissile,
    MineLayer,
    ConcreteDonkey,
    BananaBomb,
    SuperSheep,
    NinjaRope,
    Revolver,
    Blasthive,
    BlackHoleBomb,
    PlasmaTorch,
    Garcia,
    Uzi,
}

impl WeaponKind {
    /// Blast radius in pixels when this weapon explodes.
    pub fn blast_radius(self) -> f32 {
        match self {
            Self::Bazooka        => 45.0,
            Self::Grenade        => 30.0,
            Self::Shotgun        => 12.0,
            Self::ClusterBomb    => 20.0,
            Self::Landmine       => 43.0,
            Self::Tnt            => 75.0,   // 2.5× grenade
            Self::HolyHandGrenade => 80.0,
            Self::AirStrike      => 45.0,
            Self::Minigun        => 8.0,
            Self::Uzi            => 6.0,
            Self::BaseballBat    => 0.0,    // knockback only, no blast
            Self::FreezeGrenade  => 35.0,
            Self::Earthquake     => 0.0,    // world shake, no crater
            Self::Drill          => 10.0,
            Self::HomingMissile  => 30.0,
            Self::MineLayer      => 25.0,
            Self::ConcreteDonkey => 40.0,
            Self::DeathExplosion => 50.0,
            Self::BananaBomb     => 14.0,   // fragment blast radius
            Self::SuperSheep     => 40.0,
            Self::NinjaRope      => 0.0,
            Self::Revolver       => 0.0,    // hitscan, no blast
            Self::Blasthive        => 15.0,   // per bee sting (wider for reliable hits)
            Self::BlackHoleBomb    => 0.0,    // no crater; gravity well handles range
            Self::PlasmaTorch      => 0.0,    // tunnels terrain; no blast
            Self::Garcia           => 55.0,
        }
    }

    /// Maximum damage dealt at the centre of the blast.
    pub fn max_damage(self) -> u32 {
        match self {
            Self::Bazooka        => 50,
            Self::Grenade        => 45,
            Self::Shotgun        => 20,   // per pellet
            Self::ClusterBomb    => 30,   // per cluster
            Self::Landmine       => 35,
            Self::Tnt            => 75,
            Self::HolyHandGrenade => 100,
            Self::AirStrike      => 50,
            Self::Minigun        => 12,   // per bullet
            Self::Uzi            => 8,    // per bullet
            Self::BaseballBat    => 25,
            Self::FreezeGrenade  => 10,
            Self::Earthquake     => 20,
            Self::Drill          => 15,
            Self::HomingMissile  => 45,
            Self::MineLayer      => 50,
            Self::ConcreteDonkey => 100,
            Self::DeathExplosion => 40,
            Self::BananaBomb     => 18,   // fragment damage (direct hit max 38 — can't one-shot)
            Self::SuperSheep     => 60,
            Self::NinjaRope      => 0,
            Self::Revolver       => 15,
            Self::Blasthive        => 5,     // per bee sting
            Self::BlackHoleBomb    => 35,    // collapse burst damage
            Self::PlasmaTorch      => 0,     // no explosion
            Self::Garcia           => 45,
        }
    }

    /// Knockback impulse applied to worms in the blast radius.
    pub fn blast_force(self) -> f32 {
        match self {
            Self::Bazooka        => 7.5,
            Self::Grenade        => 8.0,
            Self::Shotgun        => 3.0,
            Self::ClusterBomb    => 6.0,
            Self::Landmine       => 9.0,
            Self::Tnt            => 18.0,
            Self::HolyHandGrenade => 10.0,
            Self::AirStrike      => 14.0,
            Self::Minigun        => 2.0,
            Self::Uzi            => 1.5,
            Self::BaseballBat    => 12.0,
            Self::HomingMissile  => 10.0,
            Self::ConcreteDonkey => 20.0,
            Self::DeathExplosion => 8.0,
            Self::BananaBomb     => 5.0,    // fragment knockback
            Self::Blasthive      => 3.0,    // low per-sting — up to 7 hits compound on direct
            Self::BlackHoleBomb  => 5.0,    // collapse burst knockback
            Self::PlasmaTorch    => 0.0,
            Self::Garcia         => 18.0,
            _                    => 6.0,
        }
    }

    /// Returns true if this weapon has a timed fuse.
    pub fn has_fuse(self) -> bool {
        matches!(
            self,
            Self::Grenade
                | Self::ClusterBomb
                | Self::Tnt
                | Self::HolyHandGrenade
        )
    }

    /// Returns true if the player can adjust the fuse before throwing (L1/R1).
    /// HHG has a fixed 3-second fuse; only Grenade/ClusterBomb/Tnt are adjustable.
    pub fn has_adjustable_fuse(self) -> bool {
        matches!(self, Self::Grenade | Self::ClusterBomb | Self::Tnt)
    }

    /// Default fuse duration in physics ticks (30 ticks = 1 second).
    /// Returns None for weapons with no fuse.
    pub fn default_fuse_ticks(self) -> Option<u32> {
        match self {
            Self::Grenade         => Some(60),  // 3 s
            Self::ClusterBomb     => Some(60),  // 3 s
            Self::Tnt             => None,      // fuse is random 4-5 s, set on spawn
            Self::HolyHandGrenade => Some(90),  // 3 s at 30 Hz
            Self::BananaBomb      => None,       // no fuse — explodes on terrain contact
            Self::BlackHoleBomb   => None,
            _                     => None,
        }
    }

    /// Sort key for the weapon menu: lower = earlier.
    /// Bazooka=0, Grenade=1, then Common=10s, Uncommon=20s, Rare=30s, Ultra Rare=40.
    pub fn menu_sort_key(self) -> u8 {
        match self {
            Self::Bazooka         => 0,
            Self::Grenade         => 1,
            // Common
            Self::Shotgun         => 10,
            Self::Tnt             => 11,
            Self::Landmine        => 12,
            Self::NinjaRope       => 13,
            Self::BaseballBat     => 14,
            Self::PlasmaTorch     => 15,
            Self::Uzi             => 16,
            // Uncommon
            Self::Blasthive       => 20,
            Self::BananaBomb      => 21,
            Self::AirStrike       => 22,
            Self::HomingMissile   => 23,
            // Rare
            Self::BlackHoleBomb   => 30,
            Self::Revolver        => 31,
            Self::Minigun         => 32,
            Self::HolyHandGrenade => 33,
            // Ultra Rare
            Self::Garcia          => 40,
            // Not in loadout / internal
            _                     => 99,
        }
    }

    /// Human-readable display name for messages and UI.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Bazooka        => "BAZOOKA",
            Self::Grenade        => "GRENADE",
            Self::Shotgun        => "SHOTGUN",
            Self::ClusterBomb    => "CLUSTER BOMB",
            Self::Landmine       => "MINE",
            Self::Tnt            => "TNT",
            Self::HolyHandGrenade => "SACRED ORDNANCE",
            Self::BananaBomb     => "METEOR BOMB",
            Self::AirStrike      => "AIR STRIKE",
            Self::HomingMissile  => "HOMING MISSILE",
            Self::Revolver       => "REVOLVER",
            Self::NinjaRope      => "GRAPPLE",
            Self::Blasthive        => "BLASTHIVE",
            Self::BlackHoleBomb   => "BLACK HOLE",
            Self::PlasmaTorch     => "PLASMA TORCH",
            Self::Garcia          => "HAND OF JERRY",
            Self::Uzi             => "MAC-10",
            _                     => "WEAPON",
        }
    }

    /// Network serialisation index — stable across versions.
    pub fn to_net_u8(self) -> u8 {
        // EXHAUSTIVE — no wildcard. Adding a WeaponKind variant breaks compilation
        // here, forcing you to assign it a stable wire ID before it can ship.
        match self {
            Self::Bazooka          => 0,
            Self::Grenade          => 1,
            Self::Shotgun          => 2,
            Self::ClusterBomb      => 3,
            Self::Landmine         => 4,
            Self::Tnt              => 5,
            Self::BananaBomb       => 6,
            Self::BaseballBat      => 7,
            Self::Revolver         => 8,
            Self::NinjaRope        => 9,
            Self::Blasthive        => 10,
            Self::BlackHoleBomb    => 11,
            Self::PlasmaTorch      => 12,
            Self::Garcia           => 13,
            Self::AirStrike        => 14,
            // Legacy/unimplemented variants — not in any loadout or crate table.
            // Assigned IDs so the match stays exhaustive; round-trip is untested.
            Self::DeathExplosion   => 15,
            Self::HolyHandGrenade  => 16,
            Self::Minigun          => 17,
            Self::FreezeGrenade    => 18,
            Self::Earthquake       => 19,
            Self::Drill            => 20,
            Self::HomingMissile    => 21,
            Self::MineLayer        => 22,
            Self::ConcreteDonkey   => 23,
            Self::SuperSheep       => 24,
            Self::Uzi              => 25,
        }
    }

    pub fn from_net_u8(v: u8) -> Self {
        match v {
            1  => Self::Grenade,
            2  => Self::Shotgun,
            3  => Self::ClusterBomb,
            4  => Self::Landmine,
            5  => Self::Tnt,
            6  => Self::BananaBomb,
            7  => Self::BaseballBat,
            8  => Self::Revolver,
            9  => Self::NinjaRope,
            10 => Self::Blasthive,
            11 => Self::BlackHoleBomb,
            12 => Self::PlasmaTorch,
            13 => Self::Garcia,
            14 => Self::AirStrike,
            15 => Self::DeathExplosion,
            16 => Self::HolyHandGrenade,
            17 => Self::Minigun,
            18 => Self::FreezeGrenade,
            19 => Self::Earthquake,
            20 => Self::Drill,
            21 => Self::HomingMissile,
            22 => Self::MineLayer,
            23 => Self::ConcreteDonkey,
            24 => Self::SuperSheep,
            25 => Self::Uzi,
            _  => Self::Bazooka,
        }
    }

    /// Compile-time forcing function: exhaustively matches every WeaponKind so
    /// adding a new variant breaks compilation here until you assign it a wire ID
    /// in both `to_net_u8` AND `from_net_u8`. Mirrors the GameState parity
    /// checklists in `src/game/net_sync.rs`.
    #[allow(dead_code)]
    fn _net_coverage_checklist(k: WeaponKind) -> u8 {
        match k {
            WeaponKind::Bazooka | WeaponKind::Grenade | WeaponKind::Shotgun |
            WeaponKind::ClusterBomb | WeaponKind::Landmine | WeaponKind::Tnt |
            WeaponKind::BananaBomb | WeaponKind::BaseballBat | WeaponKind::Revolver |
            WeaponKind::NinjaRope | WeaponKind::Blasthive | WeaponKind::BlackHoleBomb |
            WeaponKind::PlasmaTorch | WeaponKind::Garcia | WeaponKind::AirStrike |
            WeaponKind::DeathExplosion | WeaponKind::HolyHandGrenade | WeaponKind::Minigun |
            WeaponKind::FreezeGrenade | WeaponKind::Earthquake | WeaponKind::Drill |
            WeaponKind::HomingMissile | WeaponKind::MineLayer | WeaponKind::ConcreteDonkey |
            WeaponKind::SuperSheep | WeaponKind::Uzi => k.to_net_u8()
        }
    }

    /// Returns true if this weapon is affected by wind.
    /// Grenades, Meteor Bomb, Blasthive, and other thrown weapons are NOT
    /// affected by wind — they fly ballistically like in Worms Armageddon.
    pub fn affected_by_wind(self) -> bool {
        matches!(
            self,
            Self::Bazooka
                | Self::SuperSheep
        )
    }
}

/// State of a projectile's fuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuseState {
    /// No fuse — detonates on terrain impact.
    None,
    /// Counting down. Value is ticks remaining.
    Burning(u32),
    /// Fuse has reached zero — detonate this tick.
    Expired,
    /// HHG only: fuse gone, waiting for projectile to stop moving.
    Armed,
    /// HHG only: stopped, counting down to detonation.
    Detonating(u32),
}

impl FuseState {
    /// Advance the fuse by one tick. Returns the new state.
    pub fn tick(self) -> Self {
        match self {
            Self::None              => Self::None,
            Self::Burning(0)        => Self::Expired,
            Self::Burning(n)        => Self::Burning(n - 1),
            Self::Expired           => Self::Expired,
            Self::Armed             => Self::Armed,
            Self::Detonating(0)     => Self::Expired,
            Self::Detonating(n)     => Self::Detonating(n - 1),
        }
    }

    pub fn is_expired(self) -> bool {
        matches!(self, Self::Expired)
    }

    pub fn is_burning(self) -> bool {
        matches!(self, Self::Burning(_))
    }
}

/// A projectile in flight.
#[derive(Debug, Clone)]
pub struct Projectile {
    /// Current world-space position.
    pub pos: WorldPos,
    /// Current velocity in pixels per tick.
    pub vel: Vec2,
    /// What kind of weapon fired this.
    pub kind: WeaponKind,
    /// How many physics ticks this projectile has been alive.
    pub age_ticks: u32,
    /// Fuse state for timed weapons.
    pub fuse: FuseState,
    /// True for BananaBomb sub-munitions — fragments don't spawn more fragments.
    pub is_fragment: bool,
    /// Fixed world-space target for homing missiles (set at fire time, never changes).
    pub homing_target: Option<(f32, f32)>,
}

impl Projectile {
    /// Create a new projectile at a spawn position with an initial velocity.
    pub fn new(pos: WorldPos, vel: Vec2, kind: WeaponKind) -> Self {
        let fuse = match kind.default_fuse_ticks() {
            Some(ticks) => FuseState::Burning(ticks),
            None        => FuseState::None,
        };
        Self {
            pos,
            vel,
            kind,
            age_ticks: 0,
            fuse,
            is_fragment: false,
            homing_target: None,
        }
    }

    /// Create a Meteor Bomb fragment (BananaBomb sub-munition) — explodes on terrain contact.
    pub fn new_meteor_fragment(pos: WorldPos, vel: Vec2) -> Self {
        Self {
            pos,
            vel,
            kind: WeaponKind::BananaBomb,
            age_ticks: 0,
            fuse: FuseState::None,
            is_fragment: true,
            homing_target: None,
        }
    }

    /// Create a homing bee (Blasthive sub-munition) with a 6-second lifetime fuse.
    pub fn new_bee(pos: WorldPos, vel: Vec2) -> Self {
        Self {
            pos,
            vel,
            kind: WeaponKind::Blasthive,
            age_ticks: 0,
            fuse: FuseState::Burning(180), // ~6 s to find a target
            is_fragment: true,
            homing_target: None,
        }
    }

    /// Create a TNT projectile with a random fuse between 80 and 100 ticks (4-5 s).
    pub fn new_tnt(pos: WorldPos, fuse_ticks: u32) -> Self {
        Self {
            pos,
            vel: Vec2::zero(),  // TNT is placed, not fired
            kind: WeaponKind::Tnt,
            age_ticks: 0,
            fuse: FuseState::Burning(fuse_ticks),
            is_fragment: false,
            homing_target: None,
        }
    }

    /// Maximum ticks a projectile can be alive before being despawned.
    /// Prevents runaway projectiles flying forever off the edge of the world.
    pub const MAX_AGE_TICKS: u32 = 600; // 30 seconds at 20 Hz

    /// Returns true if this projectile has exceeded its maximum lifetime.
    pub fn is_expired(&self) -> bool {
        self.age_ticks >= Self::MAX_AGE_TICKS || self.fuse.is_expired()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos() -> WorldPos { WorldPos::new(100.0, 100.0) }
    fn vel() -> Vec2     { Vec2::new(5.0, -3.0) }

    // ── WeaponKind ───────────────────────────────────────────────────────────

    #[test]
    fn tnt_blast_is_2_5x_grenade() {
        let grenade = WeaponKind::Grenade.blast_radius();
        let tnt     = WeaponKind::Tnt.blast_radius();
        let ratio = tnt / grenade;
        assert!(
            (ratio - 2.5).abs() < 0.01,
            "TNT blast radius should be 2.5x grenade, got {ratio:.2}x"
        );
    }

    #[test]
    fn tnt_damage_is_roughly_2_5x_grenade() {
        let grenade = WeaponKind::Grenade.max_damage() as f32;
        let tnt     = WeaponKind::Tnt.max_damage() as f32;
        let ratio = tnt / grenade;
        assert!(
            ratio >= 2.4 && ratio <= 2.6,
            "TNT damage should be ~2.5x grenade, got {ratio:.2}x"
        );
    }

    #[test]
    fn baseball_bat_has_no_blast_radius() {
        assert_eq!(WeaponKind::BaseballBat.blast_radius(), 0.0);
    }

    #[test]
    fn earthquake_has_no_blast_radius() {
        assert_eq!(WeaponKind::Earthquake.blast_radius(), 0.0);
    }

    #[test]
    fn weapons_with_fuse_report_has_fuse() {
        assert!(WeaponKind::Grenade.has_fuse());
        assert!(WeaponKind::Tnt.has_fuse());
        assert!(WeaponKind::HolyHandGrenade.has_fuse());
        assert!(WeaponKind::ClusterBomb.has_fuse());
        // BananaBomb (Meteor) no longer has a fuse — it explodes on terrain contact.
    }

    #[test]
    fn bazooka_has_no_fuse() {
        assert!(!WeaponKind::Bazooka.has_fuse());
        assert!(WeaponKind::Bazooka.default_fuse_ticks().is_none());
    }

    #[test]
    fn tnt_has_no_default_fuse_ticks() {
        // TNT fuse is randomised on spawn, not a fixed default
        assert!(WeaponKind::Tnt.default_fuse_ticks().is_none());
    }

    #[test]
    fn bazooka_affected_by_wind() {
        assert!(WeaponKind::Bazooka.affected_by_wind());
    }

    #[test]
    fn shotgun_not_affected_by_wind() {
        assert!(!WeaponKind::Shotgun.affected_by_wind());
    }

    #[test]
    fn landmine_not_affected_by_wind() {
        assert!(!WeaponKind::Landmine.affected_by_wind());
    }

    // ── FuseState ────────────────────────────────────────────────────────────

    #[test]
    fn fuse_counts_down() {
        let f = FuseState::Burning(3);
        let f = f.tick();
        assert_eq!(f, FuseState::Burning(2));
        let f = f.tick();
        assert_eq!(f, FuseState::Burning(1));
        let f = f.tick();
        assert_eq!(f, FuseState::Burning(0));
        let f = f.tick();
        assert_eq!(f, FuseState::Expired);
    }

    #[test]
    fn expired_fuse_stays_expired() {
        let f = FuseState::Expired;
        assert_eq!(f.tick(), FuseState::Expired);
    }

    #[test]
    fn none_fuse_stays_none() {
        let f = FuseState::None;
        assert_eq!(f.tick(), FuseState::None);
    }

    #[test]
    fn fuse_burning_at_zero_expires_next_tick() {
        let f = FuseState::Burning(0);
        assert!(!f.is_expired());
        assert!(f.tick().is_expired());
    }

    // ── Projectile ───────────────────────────────────────────────────────────

    #[test]
    fn bazooka_projectile_has_no_fuse() {
        let p = Projectile::new(pos(), vel(), WeaponKind::Bazooka);
        assert_eq!(p.fuse, FuseState::None);
        assert_eq!(p.age_ticks, 0);
    }

    #[test]
    fn grenade_projectile_has_burning_fuse() {
        let p = Projectile::new(pos(), vel(), WeaponKind::Grenade);
        assert!(p.fuse.is_burning());
        assert_eq!(p.fuse, FuseState::Burning(60));
    }

    #[test]
    fn tnt_projectile_uses_given_fuse() {
        let p = Projectile::new_tnt(pos(), 90);
        assert_eq!(p.fuse, FuseState::Burning(90));
        assert_eq!(p.vel.x, 0.0);
        assert_eq!(p.vel.y, 0.0);
    }

    #[test]
    fn tnt_fuse_range_is_80_to_100_ticks() {
        // 4 s = 80 ticks, 5 s = 100 ticks at 20 Hz
        let min_ticks = 80u32;
        let max_ticks = 100u32;
        // Simulate a few random values in range
        for ticks in [80, 85, 90, 95, 100] {
            let p = Projectile::new_tnt(pos(), ticks);
            match p.fuse {
                FuseState::Burning(t) => {
                    assert!(t >= min_ticks && t <= max_ticks,
                        "TNT fuse {t} out of range [{min_ticks},{max_ticks}]");
                }
                _ => panic!("TNT should have a burning fuse"),
            }
        }
    }

    #[test]
    fn projectile_not_expired_at_birth() {
        let p = Projectile::new(pos(), vel(), WeaponKind::Bazooka);
        assert!(!p.is_expired());
    }

    #[test]
    fn projectile_expired_at_max_age() {
        let mut p = Projectile::new(pos(), vel(), WeaponKind::Bazooka);
        p.age_ticks = Projectile::MAX_AGE_TICKS;
        assert!(p.is_expired());
    }

    #[test]
    fn projectile_stores_position_and_velocity() {
        let p = Projectile::new(
            WorldPos::new(200.0, 150.0),
            Vec2::new(3.5, -7.2),
            WeaponKind::HomingMissile,
        );
        assert_eq!(p.pos.x, 200.0);
        assert_eq!(p.pos.y, 150.0);
        assert_eq!(p.vel.x, 3.5);
        assert_eq!(p.vel.y, -7.2);
        assert_eq!(p.kind, WeaponKind::HomingMissile);
    }

    #[test]
    fn weapon_net_roundtrip() {
        // Every variant must survive to_net_u8 → from_net_u8 intact.
        // This test is exhaustive: adding a WeaponKind and forgetting
        // from_net_u8 will fail here even if to_net_u8 compiles.
        use WeaponKind::*;
        let all = [
            Bazooka, Grenade, Shotgun, ClusterBomb, Landmine, Tnt,
            BananaBomb, BaseballBat, Revolver, NinjaRope,
            Blasthive, BlackHoleBomb, PlasmaTorch, Garcia, AirStrike,
            DeathExplosion, HolyHandGrenade, Minigun, FreezeGrenade,
            Earthquake, Drill, HomingMissile, MineLayer, ConcreteDonkey, SuperSheep, Uzi,
        ];
        for kind in all {
            assert_eq!(WeaponKind::from_net_u8(kind.to_net_u8()), kind,
                "{kind:?} did not survive net round-trip");
        }
    }
}
