//! Input state tracker.
//!
//! `InputState` maintains which buttons are currently held,
//! which were just pressed this frame, and which were just released.
//!
//! Call `poll()` once per frame to drain events from /dev/input/event0.
//! Then query `held()`, `just_pressed()`, `just_released()` freely.
//!
//! On non-Miyoo targets (dev machine) `poll()` is a no-op —
//! tests drive state directly via `inject_press/release`.

use std::collections::HashSet;
use super::buttons::{Button, EV_KEY, KEY_PRESSED, KEY_RELEASED, KEY_REPEAT};

/// Raw evdev input_event struct from <linux/input.h>.
/// On 32-bit ARM: tv_sec and tv_usec are i32 (not i64).
#[repr(C)]
struct InputEvent {
    tv_sec:  i32,
    tv_usec: i32,
    typ:     u16,
    code:    u16,
    value:   i32,
}

/// Tracks button state across frames.
pub struct InputState {
    /// Buttons currently held down.
    held:          HashSet<Button>,
    /// Buttons first pressed this frame.
    just_pressed:  Vec<Button>,
    /// Buttons released this frame.
    just_released: Vec<Button>,
    /// File descriptor for /dev/input/event0. -1 if not open.
    fd: i32,
}

impl InputState {
    /// Create a new input state. Does not open the device yet.
    pub fn new() -> Self {
        Self {
            held:          HashSet::new(),
            just_pressed:  Vec::new(),
            just_released: Vec::new(),
            fd: -1,
        }
    }

    /// Open /dev/input/event0 in non-blocking mode.
    /// Only call this on the actual Miyoo hardware.
    pub fn open(&mut self) -> Result<(), String> {
        use libc::{open, O_RDONLY, O_NONBLOCK};
        let path = b"/dev/input/event0\0";
        let fd = unsafe {
            open(path.as_ptr() as *const libc::c_char, O_RDONLY | O_NONBLOCK)
        };
        if fd < 0 {
            return Err(format!(
                "open /dev/input/event0 failed: errno {}",
                unsafe { *libc::__errno_location() }
            ));
        }
        self.fd = fd;
        Ok(())
    }

    /// Drain all pending evdev events and update state.
    /// Call once per frame before querying buttons.
    /// No-op if the device is not open (fd == -1).
    pub fn poll(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();

        if self.fd < 0 { return; }

        let ev_size = std::mem::size_of::<InputEvent>(); // 16 bytes on armv7
        loop {
            let mut ev = std::mem::MaybeUninit::<InputEvent>::uninit();
            let n = unsafe {
                libc::read(self.fd, ev.as_mut_ptr() as *mut libc::c_void, ev_size)
            };

            if n < ev_size as isize { break; } // EAGAIN or short read

            let ev = unsafe { ev.assume_init() };
            if ev.typ != EV_KEY { continue; }

            let Some(btn) = Button::from_key_code(ev.code) else { continue };

            match ev.value {
                KEY_PRESSED => {
                    self.held.insert(btn);
                    self.just_pressed.push(btn);
                }
                KEY_RELEASED => {
                    self.held.remove(&btn);
                    self.just_released.push(btn);
                }
                KEY_REPEAT => {
                    // Already in held — no state change needed
                }
                _ => {}
            }
        }
    }

    /// Returns true if the button is currently held down.
    pub fn held(&self, btn: Button) -> bool {
        self.held.contains(&btn)
    }

    /// Returns true if the button was pressed this frame (edge detect).
    pub fn just_pressed(&self, btn: Button) -> bool {
        self.just_pressed.contains(&btn)
    }

    /// Returns true if the button was released this frame.
    pub fn just_released(&self, btn: Button) -> bool {
        self.just_released.contains(&btn)
    }

    /// Returns true if any button was pressed this frame.
    pub fn any_just_pressed(&self) -> bool {
        !self.just_pressed.is_empty()
    }

    /// Quit combo: Start + Select held simultaneously.
    pub fn quit_combo(&self) -> bool {
        self.held(Button::Start) && self.held(Button::Select)
    }

    /// Pan mode: R1 held.
    pub fn panning(&self) -> bool {
        self.held(Button::R1) && !self.held(Button::L1)
    }

    /// Fast pan mode: R1 + L1 held.
    pub fn fast_panning(&self) -> bool {
        self.held(Button::R1) && self.held(Button::L1)
    }

    /// Charging a shot: A held (not just tapped).
    pub fn charging(&self) -> bool {
        self.held(Button::A)
    }

    /// Fire released: A was held and just released.
    pub fn fired(&self) -> bool {
        self.just_released(Button::A)
    }

    /// Weapon menu: A tapped (just pressed, not a hold).
    /// The game loop distinguishes tap vs hold by tracking charge time.
    pub fn weapon_menu_pressed(&self) -> bool {
        self.just_pressed(Button::A)
    }

    /// Cycle weapon forward: R1 just pressed (when not panning).
    pub fn cycle_weapon_next(&self) -> bool {
        !self.held(Button::R1) && self.just_pressed(Button::R1)
    }

    /// Cycle weapon back: L1 just pressed (when not in jump combo).
    pub fn cycle_weapon_prev(&self) -> bool {
        !self.held(Button::B) && self.just_pressed(Button::L1)
    }

    /// Jump: B just pressed.
    pub fn jump(&self) -> bool {
        self.just_pressed(Button::B)
    }

    /// Jump up: B + L1 held.
    pub fn jump_up(&self) -> bool {
        self.just_pressed(Button::B) && self.held(Button::L1)
    }

    /// Backflip: X just pressed.
    pub fn backflip(&self) -> bool {
        self.just_pressed(Button::X)
    }

    /// Move left: D-pad left held.
    pub fn move_left(&self) -> bool {
        self.held(Button::Left)
    }

    /// Move right: D-pad right held.
    pub fn move_right(&self) -> bool {
        self.held(Button::Right)
    }

    /// Pause: Start just pressed.
    pub fn pause(&self) -> bool {
        self.just_pressed(Button::Start)
    }

    // ── Test helpers — inject events without hardware ─────────────────────────

    /// Remove a button from held and just_pressed (used on the server to strip
    /// aim buttons when the client sends aim_angle directly).
    pub fn clear_button(&mut self, btn: Button) {
        self.held.remove(&btn);
        self.just_pressed.retain(|&b| b != btn);
    }

    /// Simulate a button press for testing.
    /// Adds to held and just_pressed.
    pub fn inject_held(&mut self, btn: Button) {
        self.held.insert(btn);
    }
    /// Serialize held buttons to a u16 bitmask (Button::ALL order).
    pub fn to_bits(&self) -> u16 {
        use super::buttons::Button;
        let mut bits = 0u16;
        for (i, &btn) in Button::ALL.iter().enumerate() {
            if self.held(btn) { bits |= 1 << i; }
        }
        bits
    }

    /// Reconstruct an InputState from two consecutive bitmasks.
    /// `prev` is the previous tick, `curr` is this tick.
    pub fn from_bits(prev: u16, curr: u16) -> Self {
        use super::buttons::Button;
        let mut state = Self::new();
        for (i, &btn) in Button::ALL.iter().enumerate() {
            let held = (curr >> i) & 1 == 1;
            let was_held = (prev >> i) & 1 == 1;
            if held {
                state.held.insert(btn);
                if !was_held { state.just_pressed.push(btn); }
            } else {
                if was_held { state.just_released.push(btn); }
            }
        }
        state
    }

    pub fn inject_press(&mut self, btn: Button) {
        self.held.insert(btn);
        if !self.just_pressed.contains(&btn) {
            self.just_pressed.push(btn);
        }
    }

    /// Simulate a button release for testing.
    pub fn inject_release(&mut self, btn: Button) {
        self.held.remove(&btn);
        if !self.just_released.contains(&btn) {
            self.just_released.push(btn);
        }
    }

    /// Clear just_pressed and just_released — simulates advancing a frame.
    pub fn advance_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
    }
}

impl Default for InputState {
    fn default() -> Self { Self::new() }
}

impl Drop for InputState {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe { libc::close(self.fd); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inp() -> InputState { InputState::new() }

    // ── Initial state ─────────────────────────────────────────────────────────

    #[test]
    fn new_state_has_nothing_held() {
        let s = inp();
        for &btn in Button::ALL {
            assert!(!s.held(btn), "{btn:?} should not be held initially");
        }
    }

    #[test]
    fn new_state_has_no_just_pressed() {
        let s = inp();
        assert!(!s.any_just_pressed());
        for &btn in Button::ALL {
            assert!(!s.just_pressed(btn));
        }
    }

    // ── inject_press ──────────────────────────────────────────────────────────

    #[test]
    fn inject_press_sets_held() {
        let mut s = inp();
        s.inject_press(Button::A);
        assert!(s.held(Button::A));
    }

    #[test]
    fn inject_press_sets_just_pressed() {
        let mut s = inp();
        s.inject_press(Button::A);
        assert!(s.just_pressed(Button::A));
    }

    #[test]
    fn inject_press_does_not_affect_other_buttons() {
        let mut s = inp();
        s.inject_press(Button::A);
        assert!(!s.held(Button::B));
        assert!(!s.just_pressed(Button::B));
    }

    #[test]
    fn inject_press_twice_does_not_duplicate_just_pressed() {
        let mut s = inp();
        s.inject_press(Button::A);
        s.inject_press(Button::A);
        let count = s.just_pressed.iter().filter(|&&b| b == Button::A).count();
        assert_eq!(count, 1, "just_pressed should not duplicate");
    }

    // ── inject_release ────────────────────────────────────────────────────────

    #[test]
    fn inject_release_clears_held() {
        let mut s = inp();
        s.inject_press(Button::B);
        s.inject_release(Button::B);
        assert!(!s.held(Button::B));
    }

    #[test]
    fn inject_release_sets_just_released() {
        let mut s = inp();
        s.inject_press(Button::B);
        s.inject_release(Button::B);
        assert!(s.just_released(Button::B));
    }

    // ── advance_frame ─────────────────────────────────────────────────────────

    #[test]
    fn advance_frame_clears_just_pressed() {
        let mut s = inp();
        s.inject_press(Button::A);
        assert!(s.just_pressed(Button::A));
        s.advance_frame();
        assert!(!s.just_pressed(Button::A));
    }

    #[test]
    fn advance_frame_clears_just_released() {
        let mut s = inp();
        s.inject_press(Button::A);
        s.inject_release(Button::A);
        s.advance_frame();
        assert!(!s.just_released(Button::A));
    }

    #[test]
    fn advance_frame_preserves_held() {
        let mut s = inp();
        s.inject_press(Button::L1);
        s.advance_frame();
        assert!(s.held(Button::L1), "held should persist across frames");
    }

    // ── Multi-button ──────────────────────────────────────────────────────────

    #[test]
    fn multiple_buttons_can_be_held_simultaneously() {
        let mut s = inp();
        s.inject_press(Button::Left);
        s.inject_press(Button::A);
        assert!(s.held(Button::Left));
        assert!(s.held(Button::A));
    }

    #[test]
    fn releasing_one_button_does_not_affect_others() {
        let mut s = inp();
        s.inject_press(Button::Left);
        s.inject_press(Button::A);
        s.inject_release(Button::Left);
        assert!(!s.held(Button::Left));
        assert!(s.held(Button::A));
    }

    // ── Quit combo ────────────────────────────────────────────────────────────

    #[test]
    fn quit_combo_requires_both_start_and_select() {
        let mut s = inp();
        s.inject_press(Button::Start);
        assert!(!s.quit_combo(), "Start alone should not trigger quit");
        s.inject_press(Button::Select);
        assert!(s.quit_combo(), "Start + Select should trigger quit");
    }

    #[test]
    fn quit_combo_false_with_neither() {
        let s = inp();
        assert!(!s.quit_combo());
    }

    #[test]
    fn quit_combo_false_with_select_only() {
        let mut s = inp();
        s.inject_press(Button::Select);
        assert!(!s.quit_combo());
    }

    // ── Pan mode ──────────────────────────────────────────────────────────────

    #[test]
    fn panning_when_r1_held() {
        let mut s = inp();
        assert!(!s.panning());
        s.inject_press(Button::R1);
        assert!(s.panning());
    }

    #[test]
    fn panning_false_when_r1_released() {
        let mut s = inp();
        s.inject_press(Button::R1);
        s.inject_release(Button::R1);
        assert!(!s.panning());
    }

    // ── any_just_pressed ──────────────────────────────────────────────────────

    #[test]
    fn any_just_pressed_true_after_press() {
        let mut s = inp();
        s.inject_press(Button::X);
        assert!(s.any_just_pressed());
    }

    #[test]
    fn any_just_pressed_false_after_advance() {
        let mut s = inp();
        s.inject_press(Button::X);
        s.advance_frame();
        assert!(!s.any_just_pressed());
    }

    // ── Frame simulation ──────────────────────────────────────────────────────

    #[test]
    fn held_while_button_stays_down_across_frames() {
        let mut s = inp();
        s.inject_press(Button::Right);
        s.advance_frame();
        s.advance_frame();
        s.advance_frame();
        assert!(s.held(Button::Right));
        assert!(!s.just_pressed(Button::Right),
            "just_pressed should be false after the first frame");
    }

    #[test]
    fn full_press_release_cycle() {
        let mut s = inp();

        // Frame 1: press A
        s.inject_press(Button::A);
        assert!(s.held(Button::A));
        assert!(s.just_pressed(Button::A));
        assert!(!s.just_released(Button::A));

        // Frame 2: still held
        s.advance_frame();
        assert!(s.held(Button::A));
        assert!(!s.just_pressed(Button::A));
        assert!(!s.just_released(Button::A));

        // Frame 3: release
        s.inject_release(Button::A);
        assert!(!s.held(Button::A));
        assert!(s.just_released(Button::A));

        // Frame 4: gone
        s.advance_frame();
        assert!(!s.held(Button::A));
        assert!(!s.just_released(Button::A));
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;

    fn inp() -> InputState { InputState::new() }

    #[test]
    fn panning_requires_r1_without_l1() {
        let mut s = inp();
        s.inject_press(Button::R1);
        assert!(s.panning());
        s.inject_press(Button::L1);
        assert!(!s.panning(), "R1+L1 is fast pan, not normal pan");
    }

    #[test]
    fn fast_panning_requires_both_r1_and_l1() {
        let mut s = inp();
        s.inject_press(Button::R1);
        assert!(!s.fast_panning());
        s.inject_press(Button::L1);
        assert!(s.fast_panning());
    }

    #[test]
    fn charging_when_a_held() {
        let mut s = inp();
        assert!(!s.charging());
        s.inject_press(Button::A);
        assert!(s.charging());
    }

    #[test]
    fn fired_when_a_released() {
        let mut s = inp();
        s.inject_press(Button::A);
        s.advance_frame();
        assert!(!s.fired());
        s.inject_release(Button::A);
        assert!(s.fired());
    }

    #[test]
    fn jump_on_b_press() {
        let mut s = inp();
        assert!(!s.jump());
        s.inject_press(Button::B);
        assert!(s.jump());
    }

    #[test]
    fn jump_up_requires_b_and_l1() {
        let mut s = inp();
        s.inject_press(Button::L1);
        s.inject_press(Button::B);
        assert!(s.jump_up());
    }

    #[test]
    fn jump_up_false_without_l1() {
        let mut s = inp();
        s.inject_press(Button::B);
        assert!(!s.jump_up());
    }

    #[test]
    fn backflip_on_x_press() {
        let mut s = inp();
        s.inject_press(Button::X);
        assert!(s.backflip());
    }

    #[test]
    fn move_left_when_left_held() {
        let mut s = inp();
        s.inject_press(Button::Left);
        assert!(s.move_left());
        assert!(!s.move_right());
    }

    #[test]
    fn move_right_when_right_held() {
        let mut s = inp();
        s.inject_press(Button::Right);
        assert!(s.move_right());
        assert!(!s.move_left());
    }

    #[test]
    fn pause_on_start_press() {
        let mut s = inp();
        assert!(!s.pause());
        s.inject_press(Button::Start);
        assert!(s.pause());
    }

    #[test]
    fn pause_only_on_first_frame() {
        let mut s = inp();
        s.inject_press(Button::Start);
        assert!(s.pause());
        s.advance_frame();
        assert!(!s.pause(), "pause should only fire on press frame");
    }
}
