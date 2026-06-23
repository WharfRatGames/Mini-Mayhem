//! Miyoo Mini Plus button definitions.
//!
//! Key codes come directly from the dev guide:
//!   D-pad: Up=103  Down=108  Left=105  Right=106
//!   A=57   B=29    X=42      Y=56
//!   L1=18  R1=20   L2=15     R2=14
//!   Start=28   Select=97
//!
//! The MENU button (centre) sends KEY_ESC (1) when keymon is disabled via
//! /tmp/disable_menu_button. We define it here for bug-reporter use.
//!
//! Control scheme:
//!   Move worm:        D-pad left/right
//!   Jump forward:     B
//!   Jump backward:    B + B (double tap)
//!   Jump up:          B + L1
//!   Backflip:         X
//!   Rotate aim:       D-pad left/right (weapon selected)
//!   Charge/fire:      Hold A, release to fire
//!   Open weapon menu: A (tap, before charging)
//!   Cycle weapons:    L1 / R1
//!   Fuse length:      L1 / R1 (grenade/fuse weapons)
//!   Fire ninja rope:  A
//!   Steer jetpack:    D-pad
//!   Scroll view:      R1 + D-pad
//!   Fast scroll:      R1 + L1 + D-pad
//!   Pause menu:       Start
//!   Quit:             Start + Select (MENU is taken by OS)

/// Every button available to the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L1,
    R1,
    L2,
    R2,
    Start,
    Select,
    /// Miyoo MENU button — only fires when /tmp/disable_menu_button exists.
    Menu,
}

impl Button {
    /// Linux key code for this button, from /dev/input/event0.
    /// Returns None for buttons we don't map (shouldn't happen).
    pub fn key_code(self) -> u16 {
        match self {
            Self::Up     => 103,
            Self::Down   => 108,
            Self::Left   => 105,
            Self::Right  => 106,
            Self::A      => 57,
            Self::B      => 29,
            Self::X      => 42,
            Self::Y      => 56,
            Self::L1     => 18,
            Self::R1     => 20,
            Self::L2     => 15,
            Self::R2     => 14,
            Self::Start  => 28,
            Self::Select => 97,
            Self::Menu   => 1,   // KEY_ESC — active only when disable_menu_button flag set
        }
    }

    /// Reverse map: Linux key code → Button.
    /// Returns None for unknown codes (other keys, EV_SYN, etc).
    pub fn from_key_code(code: u16) -> Option<Self> {
        Some(match code {
            103 => Self::Up,
            108 => Self::Down,
            105 => Self::Left,
            106 => Self::Right,
            57  => Self::A,
            29  => Self::B,
            42  => Self::X,
            56  => Self::Y,
            18  => Self::L1,
            20  => Self::R1,
            15  => Self::L2,
            14  => Self::R2,
            28  => Self::Start,
            97  => Self::Select,
            1   => Self::Menu,
            _   => return None,
        })
    }

    /// All buttons in a fixed order — useful for iterating.
    pub const ALL: &'static [Button] = &[
        Button::Up, Button::Down, Button::Left, Button::Right,
        Button::A,  Button::B,   Button::X,    Button::Y,
        Button::L1, Button::R1,  Button::L2,   Button::R2,
        Button::Start, Button::Select, Button::Menu,
    ];
}

/// evdev event types we care about.
pub const EV_KEY:     u16 = 1;
pub const KEY_RELEASED: i32 = 0;
pub const KEY_PRESSED:  i32 = 1;
pub const KEY_REPEAT:   i32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_buttons_have_unique_key_codes() {
        let mut codes: Vec<u16> = Button::ALL.iter().map(|b| b.key_code()).collect();
        let len_before = codes.len();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), len_before, "all buttons should have unique key codes");
    }

    #[test]
    fn from_key_code_round_trips_all_buttons() {
        for &btn in Button::ALL {
            let code = btn.key_code();
            let recovered = Button::from_key_code(code);
            assert_eq!(recovered, Some(btn),
                "{btn:?} key_code={code} did not round-trip");
        }
    }

    #[test]
    fn unknown_key_code_returns_none() {
        assert_eq!(Button::from_key_code(0),   None);
        assert_eq!(Button::from_key_code(1),   None);
        assert_eq!(Button::from_key_code(999), None);
        assert_eq!(Button::from_key_code(255), None);
    }

    #[test]
    fn exact_key_codes_match_dev_guide() {
        // Hardcoded from the Miyoo Mini Plus dev guide — if these fail
        // the hardware mapping is wrong.
        assert_eq!(Button::Up.key_code(),     103);
        assert_eq!(Button::Down.key_code(),   108);
        assert_eq!(Button::Left.key_code(),   105);
        assert_eq!(Button::Right.key_code(),  106);
        assert_eq!(Button::A.key_code(),       57);
        assert_eq!(Button::B.key_code(),       29);
        assert_eq!(Button::X.key_code(),       42);
        assert_eq!(Button::Y.key_code(),       56);
        assert_eq!(Button::L1.key_code(),      18);
        assert_eq!(Button::R1.key_code(),      20);
        assert_eq!(Button::L2.key_code(),      15);
        assert_eq!(Button::R2.key_code(),      14);
        assert_eq!(Button::Start.key_code(),   28);
        assert_eq!(Button::Select.key_code(),  97);
    }

    #[test]
    fn all_slice_contains_all_variants() {
        assert_eq!(Button::ALL.len(), 14);
        // Every variant appears exactly once
        for &btn in Button::ALL {
            let count = Button::ALL.iter().filter(|&&b| b == btn).count();
            assert_eq!(count, 1, "{btn:?} appears {count} times in ALL");
        }
    }

    #[test]
    fn dpad_codes_are_distinct_from_face_buttons() {
        let dpad  = [Button::Up, Button::Down, Button::Left, Button::Right];
        let face  = [Button::A, Button::B, Button::X, Button::Y];
        for d in dpad {
            for f in face {
                assert_ne!(d.key_code(), f.key_code(),
                    "{d:?} and {f:?} should have different key codes");
            }
        }
    }

    #[test]
    fn shoulder_buttons_have_distinct_codes() {
        let shoulders = [Button::L1, Button::R1, Button::L2, Button::R2];
        for i in 0..shoulders.len() {
            for j in 0..shoulders.len() {
                if i != j {
                    assert_ne!(shoulders[i].key_code(), shoulders[j].key_code());
                }
            }
        }
    }
}
