//! TEST mode pause menu: RESUME / PAUSE GAME / EXIT MATCH. PAUSE GAME freezes the sim AND the turn timer; normal mode has no such item.
use arty::game::loop_runner::{tick, LoopState};
use arty::input::{Button, InputState};
use arty::renderer::{buffer::WorldBuffer, camera::Camera};

fn press(b: Button) -> InputState {
    let i = Button::ALL.iter().position(|&x| x == b).unwrap();
    InputState::from_sets(1 << i, 1 << i, 0)
}

#[test]
fn pause_game_freezes_the_turn_timer_in_test_mode() {
    let mut g = arty::replay::new_game(5, &[0, 1]);
    g.is_test = true;
    let mut ls = LoopState::new(); let mut buf = WorldBuffer::new();
    let mut cam = Camera::new(g.teams[0].soldiers[0].pos.x, g.teams[0].soldiers[0].pos.y);
    let idle = InputState::default();
    for _ in 0..12 { tick(&mut g, &idle, &mut buf, &mut cam, &mut ls, None); }
    tick(&mut g, &press(Button::Start), &mut buf, &mut cam, &mut ls, None);
    assert!(ls.paused, "Start opens the pause menu");
    tick(&mut g, &press(Button::Down), &mut buf, &mut cam, &mut ls, None);
    assert_eq!(ls.pause_cursor, 1);
    tick(&mut g, &press(Button::A), &mut buf, &mut cam, &mut ls, None);
    assert!(ls.frozen, "PAUSE GAME freezes");
    let t0 = g.turn.secs_remaining();
    for _ in 0..200 { tick(&mut g, &idle, &mut buf, &mut cam, &mut ls, None); }
    assert_eq!(g.turn.secs_remaining(), t0, "turn timer must not run while the game is paused");
    tick(&mut g, &press(Button::A), &mut buf, &mut cam, &mut ls, None);
    assert!(!ls.frozen && ls.paused, "A returns to the pause menu");
}
