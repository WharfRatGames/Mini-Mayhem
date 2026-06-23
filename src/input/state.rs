//! Input state tracker.
//!
//! `InputState` maintains which buttons are currently held,
//! which were just pressed this frame, and which were just released.
//!
//! Call `poll()` once per frame. Then query `held()`, `just_pressed()`,
//! `just_released()` freely.
//!
//! Desktop: reads minifb keyboard + gilrs gamepad.
//! Miyoo:   reads /dev/input/event0 evdev.
//! Tests:   drive state directly via `inject_press/release`.

use std::collections::HashSet;
use super::buttons::Button;

// ── Miyoo-only imports ────────────────────────────────────────────────────────
#[cfg(not(feature = "desktop"))]
use super::buttons::{EV_KEY, KEY_PRESSED, KEY_RELEASED, KEY_REPEAT};

// ── Miyoo evdev event struct ──────────────────────────────────────────────────
#[cfg(not(feature = "desktop"))]
#[repr(C)]
struct InputEvent {
    tv_sec:  i32,
    tv_usec: i32,
    typ:     u16,
    code:    u16,
    value:   i32,
}

// ── Struct definition — desktop ───────────────────────────────────────────────

#[cfg(feature = "desktop")]
pub struct InputState {
    held:          HashSet<Button>,
    just_pressed:  Vec<Button>,
    just_released: Vec<Button>,
    #[cfg(feature = "gilrs")]
    gilrs: Option<gilrs::Gilrs>,
}

// ── Struct definition — Miyoo ─────────────────────────────────────────────────

#[cfg(not(feature = "desktop"))]
pub struct InputState {
    held:          HashSet<Button>,
    just_pressed:  Vec<Button>,
    just_released: Vec<Button>,
    fd: i32,
}

// ── Shared constructor ────────────────────────────────────────────────────────

#[cfg(feature = "desktop")]
impl InputState {
    pub fn new() -> Self {
        Self {
            held:          HashSet::new(),
            just_pressed:  Vec::new(),
            just_released: Vec::new(),
        }
    }

    /// No-op on desktop — no device to open.
    pub fn open(&mut self) -> Result<(), String> { Ok(()) }

    /// Poll keyboard (via minifb) and gamepad (via gilrs).
    pub fn poll(&mut self) {
        // Pump the window event queue so the OS doesn't think we're frozen.
        crate::renderer::fb::pump_events();

        self.just_pressed.clear();
        self.just_released.clear();

        // ── minifb keyboard ───────────────────────────────────────────────────
        let pressed  = crate::renderer::fb::desktop_keys_pressed();
        let released = crate::renderer::fb::desktop_keys_released();
        let held_now = crate::renderer::fb::desktop_keys_held();

        for key in pressed {
            if let Some(btn) = minifb_key_to_button(key) {
                self.held.insert(btn);
                if !self.just_pressed.contains(&btn) {
                    self.just_pressed.push(btn);
                }
            }
        }
        for key in released {
            if let Some(btn) = minifb_key_to_button(key) {
                self.held.remove(&btn);
                if !self.just_released.contains(&btn) {
                    self.just_released.push(btn);
                }
            }
        }
        // Reconcile: if a key is held in minifb but not in our set, add it.
        // (Covers the case where poll() missed an edge due to re-present.)
        let _ = held_now; // used for reconciliation if needed — currently edge-based is sufficient

        // ── gilrs gamepad ─────────────────────────────────────────────────────
        use gilrs::{Gilrs, EventType, Button as GB, Axis};
        // Lazy-init gilrs — may fail if no gamepad subsystem is available.
        // We store it in a thread-local to avoid putting it in the struct
        // (gilrs::Gilrs is large and not always needed).
        thread_local! {
            static GILRS: std::cell::RefCell<Option<Gilrs>> = std::cell::RefCell::new(
                Gilrs::new().ok()
            );
        }
        GILRS.with(|g| {
            if let Some(ref mut gilrs) = *g.borrow_mut() {
                while let Some(gilrs::Event { event, .. }) = gilrs.next_event() {
                    match event {
                        EventType::ButtonPressed(b, _) => {
                            if let Some(btn) = gilrs_button_to_button(b) {
                                self.held.insert(btn);
                                if !self.just_pressed.contains(&btn) {
                                    self.just_pressed.push(btn);
                                }
                            }
                        }
                        EventType::ButtonReleased(b, _) => {
                            if let Some(btn) = gilrs_button_to_button(b) {
                                self.held.remove(&btn);
                                if !self.just_released.contains(&btn) {
                                    self.just_released.push(btn);
                                }
                            }
                        }
                        EventType::AxisChanged(axis, value, _) => {
                            // Map d-pad axes to Up/Down/Left/Right
                            match axis {
                                Axis::DPadX => {
                                    if value > 0.5 {
                                        self.held.insert(Button::Right);
                                        if !self.just_pressed.contains(&Button::Right) {
                                            self.just_pressed.push(Button::Right);
                                        }
                                        self.held.remove(&Button::Left);
                                    } else if value < -0.5 {
                                        self.held.insert(Button::Left);
                                        if !self.just_pressed.contains(&Button::Left) {
                                            self.just_pressed.push(Button::Left);
                                        }
                                        self.held.remove(&Button::Right);
                                    } else {
                                        self.held.remove(&Button::Left);
                                        self.held.remove(&Button::Right);
                                    }
                                }
                                Axis::DPadY => {
                                    if value > 0.5 {
                                        self.held.insert(Button::Up);
                                        if !self.just_pressed.contains(&Button::Up) {
                                            self.just_pressed.push(Button::Up);
                                        }
                                        self.held.remove(&Button::Down);
                                    } else if value < -0.5 {
                                        self.held.insert(Button::Down);
                                        if !self.just_pressed.contains(&Button::Down) {
                                            self.just_pressed.push(Button::Down);
                                        }
                                        self.held.remove(&Button::Up);
                                    } else {
                                        self.held.remove(&Button::Up);
                                        self.held.remove(&Button::Down);
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}

#[cfg(feature = "desktop")]
fn minifb_key_to_button(key: minifb::Key) -> Option<Button> {
    use minifb::Key::*;
    Some(match key {
        Up              => Button::Up,
        Down            => Button::Down,
        Left            => Button::Left,
        Right           => Button::Right,
        A | Space       => Button::A,
        B | Z           => Button::B,
        X               => Button::X,
        Y               => Button::Y,
        Q               => Button::L1,
        E               => Button::R1,
        LeftBracket     => Button::L2,
        RightBracket    => Button::R2,
        Enter           => Button::Start,
        Tab             => Button::Select,
        Escape          => Button::Start,  // ESC also pauses
        _               => return None,
    })
}

#[cfg(feature = "desktop")]
fn gilrs_button_to_button(b: gilrs::Button) -> Option<Button> {
    use gilrs::Button::*;
    Some(match b {
        DPadUp    => Button::Up,
        DPadDown  => Button::Down,
        DPadLeft  => Button::Left,
        DPadRight => Button::Right,
        South     => Button::B,
        East      => Button::A,
        West      => Button::Y,
        North     => Button::X,
        LeftTrigger  => Button::L1,
        RightTrigger => Button::R1,
        LeftTrigger2  => Button::L2,
        RightTrigger2 => Button::R2,
        Start     => Button::Start,
        Select    => Button::Select,
        Mode      => Button::Select,
        _         => return None,
    })
}

// ── Miyoo impl ────────────────────────────────────────────────────────────────

#[cfg(not(feature = "desktop"))]
impl InputState {
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
    pub fn poll(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();

        if self.fd < 0 { return; }

        let ev_size = std::mem::size_of::<InputEvent>();
        loop {
            let mut ev = std::mem::MaybeUninit::<InputEvent>::uninit();
            let n = unsafe {
                libc::read(self.fd, ev.as_mut_ptr() as *mut libc::c_void, ev_size)
            };

            if n < ev_size as isize { break; }

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
                KEY_REPEAT => {}
                _ => {}
            }
        }
    }
}

#[cfg(not(feature = "desktop"))]
impl Drop for InputState {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe { libc::close(self.fd); }
        }
    }
}

// ── Shared query methods ──────────────────────────────────────────────────────

impl InputState {
    pub fn held(&self, btn: Button) -> bool {
        self.held.contains(&btn)
    }

    pub fn just_pressed(&self, btn: Button) -> bool {
        self.just_pressed.contains(&btn)
    }

    pub fn just_released(&self, btn: Button) -> bool {
        self.just_released.contains(&btn)
    }

    pub fn any_just_pressed(&self) -> bool {
        !self.just_pressed.is_empty()
    }

    pub fn quit_combo(&self) -> bool {
        self.held(Button::Start) && self.held(Button::Select)
    }

    pub fn panning(&self) -> bool {
        self.held(Button::R1) && !self.held(Button::L1)
    }

    pub fn fast_panning(&self) -> bool {
        self.held(Button::R1) && self.held(Button::L1)
    }

    pub fn charging(&self) -> bool {
        self.held(Button::A)
    }

    pub fn fired(&self) -> bool {
        self.just_released(Button::A)
    }

    pub fn weapon_menu_pressed(&self) -> bool {
        self.just_pressed(Button::A)
    }

    pub fn cycle_weapon_next(&self) -> bool {
        !self.held(Button::R1) && self.just_pressed(Button::R1)
    }

    pub fn cycle_weapon_prev(&self) -> bool {
        !self.held(Button::B) && self.just_pressed(Button::L1)
    }

    pub fn jump(&self) -> bool {
        self.just_pressed(Button::B)
    }

    pub fn jump_up(&self) -> bool {
        self.just_pressed(Button::B) && self.held(Button::L1)
    }

    pub fn backflip(&self) -> bool {
        self.just_pressed(Button::X)
    }

    pub fn move_left(&self) -> bool {
        self.held(Button::Left)
    }

    pub fn move_right(&self) -> bool {
        self.held(Button::Right)
    }

    pub fn pause(&self) -> bool {
        self.just_pressed(Button::Start)
    }

    pub fn clear_button(&mut self, btn: Button) {
        self.held.remove(&btn);
        self.just_pressed.retain(|&b| b != btn);
    }

    pub fn inject_held(&mut self, btn: Button) {
        self.held.insert(btn);
    }

    pub fn to_bits(&self) -> u16 {
        let mut bits = 0u16;
        for (i, &btn) in Button::ALL.iter().enumerate() {
            if self.held(btn) { bits |= 1 << i; }
        }
        bits
    }

    pub fn from_bits(prev: u16, curr: u16) -> Self {
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

    pub fn inject_release(&mut self, btn: Button) {
        self.held.remove(&btn);
        if !self.just_released.contains(&btn) {
            self.just_released.push(btn);
        }
    }

    pub fn advance_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
    }
}

impl Default for InputState {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inp() -> InputState { InputState::new() }

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
    fn inject_release_clears_held() {
        let mut s = inp();
        s.inject_press(Button::A);
        s.inject_release(Button::A);
        assert!(!s.held(Button::A));
    }

    #[test]
    fn inject_release_sets_just_released() {
        let mut s = inp();
        s.inject_press(Button::A);
        s.inject_release(Button::A);
        assert!(s.just_released(Button::A));
    }

    #[test]
    fn advance_frame_clears_edges() {
        let mut s = inp();
        s.inject_press(Button::A);
        s.advance_frame();
        assert!(!s.just_pressed(Button::A));
        assert!(!s.just_released(Button::A));
    }

    #[test]
    fn from_bits_round_trips() {
        let mut orig = inp();
        orig.inject_held(Button::Up);
        orig.inject_held(Button::A);
        let bits = orig.to_bits();
        let recovered = InputState::from_bits(0, bits);
        assert!(recovered.held(Button::Up));
        assert!(recovered.held(Button::A));
        assert!(!recovered.held(Button::B));
    }
}
