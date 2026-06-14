/// How long each turn lasts in physics ticks (20 Hz).
/// 45 seconds × 20 = 900 ticks.
pub const TURN_TICKS: u32 = 1350;

/// How long the retreat phase lasts after firing (~3.5 seconds at 30 Hz).
pub const RETREAT_TICKS: u32 = 105;

/// What phase the current turn is in.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnPhase {
    /// Player is moving/aiming — full control.
    Acting,
    /// Projectile(s) in flight — no control, camera follows shot.
    Watching,
    /// Projectile resolved, player walks to safety before next turn.
    Retreating { ticks_left: u32 },
    /// Turn is over, advancing to next player.
    Ending,
}

/// Manages turn order across teams and soldiers.
///
/// Turn order: round-robin by team, cycling soldiers within each team.
///   Team 0 Soldier 0 → Team 1 Soldier 0 → Team 0 Soldier 1 → Team 1 Soldier 1 ...
#[derive(Debug, Clone)]
pub struct TurnManager {
    /// Number of active (non-eliminated) teams.
    team_count:     usize,
    /// Current team index in the rotation (0..team_count).
    pub current_team:   usize,
    /// Current turn phase.
    pub phase:      TurnPhase,
    /// Ticks remaining in this turn's acting phase.
    pub ticks_left: u32,
    /// Total turns elapsed.
    pub turn_number: u32,
}

impl TurnManager {
    pub fn new(team_count: usize) -> Self {
        Self {
            team_count,
            current_team: 0,
            phase: TurnPhase::Acting,
            ticks_left: TURN_TICKS,
            turn_number: 0,
        }
    }

    /// Which team slot is currently acting.
    pub fn current_team(&self) -> usize {
        self.current_team
    }

    /// Seconds remaining in this turn (for HUD display).
    pub fn secs_remaining(&self) -> u32 {
        (self.ticks_left + 29) / 30 // round up
    }

    /// Call each physics tick. Returns true if the turn timer just expired.
    pub fn tick(&mut self) -> bool {
        match &mut self.phase {
            TurnPhase::Acting => {
                if self.ticks_left == 0 {
                    self.phase = TurnPhase::Ending;
                    return true;
                }
                self.ticks_left -= 1;
                false
            }
            TurnPhase::Retreating { ticks_left } => {
                if *ticks_left == 0 {
                    self.phase = TurnPhase::Ending;
                } else {
                    *ticks_left -= 1;
                }
                false
            }
            _ => false,
        }
    }

    /// Call when a projectile is fired — enters Watching phase.
    pub fn on_fired(&mut self) {
        self.phase = TurnPhase::Watching;
    }

    /// Call when all projectiles have resolved — enters Retreating phase.
    pub fn on_projectiles_resolved(&mut self) {
        self.phase = TurnPhase::Retreating { ticks_left: RETREAT_TICKS };
    }

    /// Active worm took damage — skip retreat, end turn immediately.
    pub fn skip_retreat(&mut self) {
        self.phase = TurnPhase::Ending;
    }

    /// Advance to the next team's turn. Returns the new current team index.
    /// `alive_teams` is a bitmask of which team slots still have living soldiers.
    pub fn advance(&mut self, alive_teams: &[bool]) -> usize {
        self.turn_number += 1;
        // Find next alive team in round-robin order
        for _ in 0..alive_teams.len() {
            self.current_team = (self.current_team + 1) % self.team_count;
            if self.current_team < alive_teams.len()
                && alive_teams[self.current_team]
            {
                break;
            }
        }
        self.phase = TurnPhase::Acting;
        self.ticks_left = TURN_TICKS;
        self.current_team
    }

    pub fn is_acting(&self) -> bool {
        matches!(self.phase, TurnPhase::Acting)
    }

    pub fn is_watching(&self) -> bool {
        matches!(self.phase, TurnPhase::Watching)
    }

    pub fn is_retreating(&self) -> bool {
        matches!(self.phase, TurnPhase::Retreating { .. })
    }

    pub fn is_ending(&self) -> bool {
        matches!(self.phase, TurnPhase::Ending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr(teams: usize) -> TurnManager { TurnManager::new(teams) }

    // ── Initial state ─────────────────────────────────────────────────────────

    #[test]
    fn starts_at_team_zero() {
        assert_eq!(mgr(2).current_team(), 0);
    }

    #[test]
    fn starts_in_acting_phase() {
        assert!(mgr(2).is_acting());
    }

    #[test]
    fn starts_with_full_timer() {
        assert_eq!(mgr(2).ticks_left, TURN_TICKS);
    }

    // ── secs_remaining ────────────────────────────────────────────────────────

    #[test]
    fn secs_remaining_at_full_timer() {
        let m = mgr(2);
        assert_eq!(m.secs_remaining(), 45);
    }

    #[test]
    fn secs_remaining_rounds_up() {
        let mut m = mgr(2);
        m.ticks_left = 1;
        assert_eq!(m.secs_remaining(), 1);
    }

    #[test]
    fn secs_remaining_at_zero() {
        let mut m = mgr(2);
        m.ticks_left = 0;
        assert_eq!(m.secs_remaining(), 0);
    }

    // ── tick ─────────────────────────────────────────────────────────────────

    #[test]
    fn tick_decrements_timer() {
        let mut m = mgr(2);
        m.tick();
        assert_eq!(m.ticks_left, TURN_TICKS - 1);
    }

    #[test]
    fn tick_returns_true_when_timer_expires() {
        let mut m = mgr(2);
        m.ticks_left = 0;
        let expired = m.tick();
        assert!(expired);
        assert!(m.is_ending());
    }

    #[test]
    fn tick_returns_false_while_time_remains() {
        let mut m = mgr(2);
        let expired = m.tick();
        assert!(!expired);
    }

    #[test]
    fn tick_does_nothing_in_watching_phase() {
        let mut m = mgr(2);
        m.on_fired();
        let ticks_before = m.ticks_left;
        m.tick();
        assert_eq!(m.ticks_left, ticks_before, "timer should not tick during watching");
    }

    // ── Phase transitions ─────────────────────────────────────────────────────

    #[test]
    fn on_fired_enters_watching() {
        let mut m = mgr(2);
        m.on_fired();
        assert!(m.is_watching());
    }

    #[test]
    fn on_projectiles_resolved_enters_retreating() {
        let mut m = mgr(2);
        m.on_fired();
        m.on_projectiles_resolved();
        assert!(m.is_retreating());
    }

    #[test]
    fn retreating_counts_down_to_ending() {
        let mut m = mgr(2);
        m.on_fired();
        m.on_projectiles_resolved();
        // RETREAT_TICKS counts down to 0, then one more tick flips to Ending
        for _ in 0..=RETREAT_TICKS {
            m.tick();
        }
        assert!(m.is_ending());
    }

    // ── advance ───────────────────────────────────────────────────────────────

    #[test]
    fn advance_moves_to_next_team() {
        let mut m = mgr(2);
        let alive = vec![true, true];
        let next = m.advance(&alive);
        assert_eq!(next, 1);
        assert_eq!(m.current_team(), 1);
    }

    #[test]
    fn advance_wraps_back_to_team_zero() {
        let mut m = mgr(2);
        let alive = vec![true, true];
        m.advance(&alive);
        m.advance(&alive);
        assert_eq!(m.current_team(), 0);
    }

    #[test]
    fn advance_skips_eliminated_teams() {
        let mut m = mgr(3);
        // Team 1 is eliminated
        let alive = vec![true, false, true];
        m.advance(&alive);
        assert_eq!(m.current_team(), 2, "should skip eliminated team 1");
    }

    #[test]
    fn advance_resets_timer() {
        let mut m = mgr(2);
        m.ticks_left = 10;
        m.advance(&vec![true, true]);
        assert_eq!(m.ticks_left, TURN_TICKS);
    }

    #[test]
    fn advance_enters_acting_phase() {
        let mut m = mgr(2);
        m.on_fired();
        m.advance(&vec![true, true]);
        assert!(m.is_acting());
    }

    #[test]
    fn advance_increments_turn_number() {
        let mut m = mgr(2);
        assert_eq!(m.turn_number, 0);
        m.advance(&vec![true, true]);
        assert_eq!(m.turn_number, 1);
        m.advance(&vec![true, true]);
        assert_eq!(m.turn_number, 2);
    }

    #[test]
    fn turn_ticks_is_900() {
        assert_eq!(TURN_TICKS, 900);
    }

    #[test]
    fn retreat_ticks_is_105() {
        assert_eq!(RETREAT_TICKS, 105);
    }
}
