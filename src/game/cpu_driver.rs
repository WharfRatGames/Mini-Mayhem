/// Sim-only N-team CPU turn driver, used exclusively by the `match-sim`
/// binary. Generalizes the single-CPU-seat state machine inlined in
/// `src/main.rs` (VS-CPU mode) to drive any number of CPU-controlled team
/// slots headlessly. Does not touch or replace that inline logic — the
/// shipped desktop VS-CPU experience is unaffected by this file.
use crate::input::{Button, InputState};
use crate::physics::projectile::WeaponKind;
use super::cpu::CpuState;
use super::state::GameState;
use super::loop_runner::fire_bazooka_tat;

pub struct CpuDriver {
    states: Vec<CpuState>,
}

impl CpuDriver {
    pub fn new(team_count: usize) -> Self {
        Self { states: (0..team_count).map(|_| CpuState::undecided()).collect() }
    }

    /// Advances one tick of CPU behavior for the currently active team.
    /// Returns the synthetic input to feed into `server_tick`, plus the
    /// `WeaponKind` fired this tick, if any.
    pub fn drive_turn(&mut self, game: &mut GameState, noise_seed: u32) -> (InputState, Option<WeaponKind>) {
        let active = game.active_team();
        let mut input = InputState::new();
        let mut fired = None;

        let has_fired = game.teams[active].soldiers[game.teams[active].active].has_fired;
        if game.turn.is_retreating() {
            game.turn.skip_retreat();
        }
        if has_fired {
            self.states[active] = CpuState::undecided();
            return (input, fired);
        }

        let state = &mut self.states[active];
        if !state.decided {
            let difficulty = game.teams[active].difficulty;
            *state = CpuState::decide_with_difficulty(game, active, noise_seed, difficulty);
        }

        if self.states[active].walk_ticks > 0 {
            let state = &mut self.states[active];
            state.walk_ticks -= 1;
            match state.walk_dir {
                1  => input.inject_press(Button::Right),
                -1 => input.inject_press(Button::Left),
                _  => {}
            }
        } else if self.states[active].thinking > 0 {
            self.states[active].thinking -= 1;
        } else {
            let state = &self.states[active];
            game.aim.angle = state.angle;
            game.aim.power = state.power;
            game.teams[active].selected_weapon = state.weapon_idx;
            fired = game.teams[active].weapons.get(state.weapon_idx).map(|(kind, _)| *kind);
            fire_bazooka_tat(game);
            self.states[active] = CpuState::undecided();
        }

        (input, fired)
    }
}
