use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, ModifierKeyCode,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const JOYPAD_BUTTON_COUNT: usize = 16;
pub const RETRO_DEVICE_JOYPAD: u32 = 1;
pub const REPEAT_TIMEOUT: Duration = Duration::from_millis(140);
pub const INITIAL_HOLD_GRACE: Duration = Duration::from_millis(250);
pub const RELEASE_EVENT_FAILSAFE: Duration = Duration::from_millis(650);

pub const BUTTON_B: usize = 0;
pub const BUTTON_Y: usize = 1;
pub const BUTTON_SELECT: usize = 2;
pub const BUTTON_START: usize = 3;
pub const BUTTON_UP: usize = 4;
pub const BUTTON_DOWN: usize = 5;
pub const BUTTON_LEFT: usize = 6;
pub const BUTTON_RIGHT: usize = 7;
pub const BUTTON_A: usize = 8;
pub const BUTTON_X: usize = 9;
pub const BUTTON_L: usize = 10;
pub const BUTTON_R: usize = 11;
pub const BUTTON_L2: usize = 12;
pub const BUTTON_R2: usize = 13;
pub const BUTTON_L3: usize = 14;
pub const BUTTON_R3: usize = 15;

#[derive(Clone, Copy)]
struct KeyState {
    pressed: bool,
    last_seen: Option<Instant>,
    repeat_seen: bool,
}

impl KeyState {
    const fn new() -> Self {
        Self {
            pressed: false,
            last_seen: None,
            repeat_seen: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct InputKey {
    code: KeyCode,
    keypad: bool,
}

impl InputKey {
    fn from_event(event: KeyEvent) -> Self {
        let keypad = event.state.contains(KeyEventState::KEYPAD);
        let code = normalize_code(event.code);
        Self {
            code: if keypad {
                normalize_keypad_code(code)
            } else {
                code
            },
            keypad,
        }
    }

    fn parse(name: &str) -> Result<Self, String> {
        if name != "+" && name.contains('+') {
            return Err(format!("key combinations are not supported: \"{name}\""));
        }

        let name = name.to_ascii_lowercase();
        if name.is_empty() {
            return Err("key name must not be empty".into());
        }

        if let Some(name) = name.strip_prefix("numpad-") {
            let code = parse_numpad_key(name)
                .ok_or_else(|| format!("unknown numeric keypad key: \"numpad-{name}\""))?;
            return Ok(Self {
                code: normalize_keypad_code(code),
                keypad: true,
            });
        }

        let code = parse_key_code(&name).ok_or_else(|| format!("unknown key: \"{name}\""))?;
        Ok(Self {
            code: normalize_code(code),
            keypad: false,
        })
    }
}

#[derive(Clone, Debug)]
struct Binding {
    key: InputKey,
    key_name: String,
    button: usize,
}

#[derive(Clone, Debug)]
pub struct InputBindings {
    gamepad: Vec<Binding>,
    quit: InputKey,
    quit_name: String,
}

impl InputBindings {
    pub fn new(gamepad: &[(&str, usize, &str)], quit: &str) -> Result<Self, String> {
        let mut assigned = HashMap::new();
        let mut bindings = Vec::with_capacity(gamepad.len());

        for &(input, button, key_name) in gamepad {
            let key = InputKey::parse(key_name)
                .map_err(|error| format!("invalid key for input \"{input}\": {error}"))?;
            if let Some(previous) = assigned.insert(key, input) {
                return Err(format!(
                    "key \"{key_name}\" for input \"{input}\" is already bound to \"{previous}\""
                ));
            }
            bindings.push(Binding {
                key,
                key_name: key_name.to_ascii_lowercase(),
                button,
            });
        }

        let quit_key = InputKey::parse(quit)
            .map_err(|error| format!("invalid key for input \"quit\": {error}"))?;
        if let Some(previous) = assigned.get(&quit_key) {
            return Err(format!(
                "key \"{quit}\" for input \"quit\" is already bound to \"{previous}\""
            ));
        }

        Ok(Self {
            gamepad: bindings,
            quit: quit_key,
            quit_name: quit.to_ascii_lowercase(),
        })
    }

    #[cfg(test)]
    pub fn quit_name(&self) -> &str {
        &self.quit_name
    }

    pub fn status_line(&self) -> String {
        let mut items = Vec::with_capacity(17);
        for (label, button) in [
            ("↑", BUTTON_UP),
            ("↓", BUTTON_DOWN),
            ("←", BUTTON_LEFT),
            ("→", BUTTON_RIGHT),
            ("Select", BUTTON_SELECT),
            ("Start", BUTTON_START),
            ("L1", BUTTON_L),
            ("L2", BUTTON_L2),
            ("R1", BUTTON_R),
            ("R2", BUTTON_R2),
            ("L3", BUTTON_L3),
            ("R3", BUTTON_R3),
            ("X", BUTTON_X),
            ("Y", BUTTON_Y),
            ("A", BUTTON_A),
            ("B", BUTTON_B),
        ] {
            if let Some(binding) = self.gamepad.iter().find(|binding| binding.button == button) {
                items.push(format!("{label}-{}", binding.key_name));
            }
        }
        items.push(format!("Exit-{}", self.quit_name));
        items.join(" ")
    }
}

impl Default for InputBindings {
    fn default() -> Self {
        Self {
            gamepad: vec![
                Binding {
                    key: InputKey {
                        code: KeyCode::Up,
                        keypad: false,
                    },
                    key_name: "up".into(),
                    button: BUTTON_UP,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Down,
                        keypad: false,
                    },
                    key_name: "down".into(),
                    button: BUTTON_DOWN,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Left,
                        keypad: false,
                    },
                    key_name: "left".into(),
                    button: BUTTON_LEFT,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Right,
                        keypad: false,
                    },
                    key_name: "right".into(),
                    button: BUTTON_RIGHT,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Char('x'),
                        keypad: false,
                    },
                    key_name: "x".into(),
                    button: BUTTON_A,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Char('z'),
                        keypad: false,
                    },
                    key_name: "z".into(),
                    button: BUTTON_B,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Char('s'),
                        keypad: false,
                    },
                    key_name: "s".into(),
                    button: BUTTON_X,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Char('a'),
                        keypad: false,
                    },
                    key_name: "a".into(),
                    button: BUTTON_Y,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Enter,
                        keypad: false,
                    },
                    key_name: "enter".into(),
                    button: BUTTON_START,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Backspace,
                        keypad: false,
                    },
                    key_name: "backspace".into(),
                    button: BUTTON_SELECT,
                },
            ],
            quit: InputKey {
                code: KeyCode::Esc,
                keypad: false,
            },
            quit_name: "esc".into(),
        }
    }
}

pub struct InputState {
    bindings: InputBindings,
    buttons: [bool; JOYPAD_BUTTON_COUNT],
    keys: Vec<KeyState>,
    quit_requested: bool,
    release_events_supported: bool,
    failsafe_key: Option<usize>,
}

impl Default for InputState {
    fn default() -> Self {
        Self::with_bindings(InputBindings::default(), false)
    }
}

impl InputState {
    #[cfg(test)]
    fn with_release_events_supported(release_events_supported: bool) -> Self {
        Self::with_bindings(InputBindings::default(), release_events_supported)
    }

    pub fn with_bindings(bindings: InputBindings, release_events_supported: bool) -> Self {
        let keys = vec![KeyState::new(); bindings.gamepad.len()];
        Self {
            bindings,
            buttons: [false; JOYPAD_BUTTON_COUNT],
            keys,
            quit_requested: false,
            release_events_supported,
            failsafe_key: None,
        }
    }

    pub fn handle_key(&mut self, event: KeyEvent, now: Instant) {
        if event.kind != KeyEventKind::Release && self.is_quit_key(event) {
            self.quit_requested = true;
            return;
        }

        let event_key = InputKey::from_event(event);
        let Some(key_id) = self
            .bindings
            .gamepad
            .iter()
            .position(|binding| binding.key == event_key)
        else {
            return;
        };
        let key = &mut self.keys[key_id];

        match event.kind {
            KeyEventKind::Press => {
                key.repeat_seen |= key.pressed;
                key.pressed = true;
                key.last_seen = Some(now);
                self.failsafe_key = Some(key_id);
            }
            KeyEventKind::Repeat => {
                key.pressed = true;
                key.repeat_seen = true;
                key.last_seen = Some(now);
                self.failsafe_key = Some(key_id);
            }
            KeyEventKind::Release => {
                key.pressed = false;
                key.last_seen = None;
                key.repeat_seen = false;
                self.release_events_supported = true;
                if self.failsafe_key == Some(key_id) {
                    self.failsafe_key = None;
                }
            }
        }

        self.rebuild_buttons();
    }

    pub fn expire(&mut self, now: Instant) {
        if self.release_events_supported {
            if let Some(key_id) = self.failsafe_key {
                let key = &mut self.keys[key_id];
                let timeout = if key.repeat_seen {
                    REPEAT_TIMEOUT
                } else {
                    RELEASE_EVENT_FAILSAFE
                };
                if key
                    .last_seen
                    .is_some_and(|last_seen| now.saturating_duration_since(last_seen) >= timeout)
                {
                    key.pressed = false;
                    key.last_seen = None;
                    key.repeat_seen = false;
                    self.failsafe_key = None;
                }
            }
            self.rebuild_buttons();
            return;
        }

        for key in &mut self.keys {
            if !key.pressed {
                continue;
            }
            let timeout = if key.repeat_seen {
                REPEAT_TIMEOUT
            } else {
                INITIAL_HOLD_GRACE
            };
            if key
                .last_seen
                .is_some_and(|last_seen| now.saturating_duration_since(last_seen) >= timeout)
            {
                key.pressed = false;
                key.last_seen = None;
                key.repeat_seen = false;
            }
        }
        self.rebuild_buttons();
    }

    pub fn clear(&mut self) {
        for key in &mut self.keys {
            key.pressed = false;
            key.last_seen = None;
            key.repeat_seen = false;
        }
        self.failsafe_key = None;
        self.buttons.fill(false);
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn button_mask(&self) -> u16 {
        self.buttons
            .iter()
            .enumerate()
            .fold(0, |mask, (id, pressed)| mask | (u16::from(*pressed) << id))
    }

    fn rebuild_buttons(&mut self) {
        self.buttons.fill(false);
        for (key_id, key) in self.keys.iter().enumerate() {
            if key.pressed {
                self.buttons[self.bindings.gamepad[key_id].button] = true;
            }
        }
    }

    fn is_quit_key(&self, event: KeyEvent) -> bool {
        is_ctrl_c(event) || InputKey::from_event(event) == self.bindings.quit
    }
}

fn normalize_code(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(character) => KeyCode::Char(character.to_ascii_lowercase()),
        code => code,
    }
}

fn normalize_keypad_code(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Insert => KeyCode::Char('0'),
        KeyCode::End => KeyCode::Char('1'),
        KeyCode::Down => KeyCode::Char('2'),
        KeyCode::PageDown => KeyCode::Char('3'),
        KeyCode::Left => KeyCode::Char('4'),
        KeyCode::KeypadBegin => KeyCode::Char('5'),
        KeyCode::Right => KeyCode::Char('6'),
        KeyCode::Home => KeyCode::Char('7'),
        KeyCode::Up => KeyCode::Char('8'),
        KeyCode::PageUp => KeyCode::Char('9'),
        KeyCode::Delete => KeyCode::Char('.'),
        code => code,
    }
}

fn parse_key_code(name: &str) -> Option<KeyCode> {
    let code = match name {
        "backspace" => KeyCode::Backspace,
        "enter" | "return" => KeyCode::Enter,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "page-up" => KeyCode::PageUp,
        "page-down" => KeyCode::PageDown,
        "tab" => KeyCode::Tab,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "null" => KeyCode::Null,
        "esc" | "escape" => KeyCode::Esc,
        "caps-lock" => KeyCode::CapsLock,
        "scroll-lock" => KeyCode::ScrollLock,
        "num-lock" => KeyCode::NumLock,
        "print-screen" => KeyCode::PrintScreen,
        "pause" => KeyCode::Pause,
        "menu" => KeyCode::Menu,
        "space" => KeyCode::Char(' '),
        "left-shift" => KeyCode::Modifier(ModifierKeyCode::LeftShift),
        "left-control" | "left-ctrl" => KeyCode::Modifier(ModifierKeyCode::LeftControl),
        "left-alt" => KeyCode::Modifier(ModifierKeyCode::LeftAlt),
        "left-super" => KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        "left-hyper" => KeyCode::Modifier(ModifierKeyCode::LeftHyper),
        "left-meta" => KeyCode::Modifier(ModifierKeyCode::LeftMeta),
        "right-shift" => KeyCode::Modifier(ModifierKeyCode::RightShift),
        "right-control" | "right-ctrl" => KeyCode::Modifier(ModifierKeyCode::RightControl),
        "right-alt" => KeyCode::Modifier(ModifierKeyCode::RightAlt),
        "right-super" => KeyCode::Modifier(ModifierKeyCode::RightSuper),
        "right-hyper" => KeyCode::Modifier(ModifierKeyCode::RightHyper),
        "right-meta" => KeyCode::Modifier(ModifierKeyCode::RightMeta),
        "iso-level-3-shift" => KeyCode::Modifier(ModifierKeyCode::IsoLevel3Shift),
        "iso-level-5-shift" => KeyCode::Modifier(ModifierKeyCode::IsoLevel5Shift),
        _ => {
            if let Some(number) = name.strip_prefix('f').and_then(|value| value.parse().ok())
                && (1..=24).contains(&number)
            {
                return Some(KeyCode::F(number));
            }
            let mut characters = name.chars();
            let character = characters.next()?;
            if characters.next().is_none() {
                return Some(KeyCode::Char(character));
            }
            return None;
        }
    };
    Some(code)
}

fn parse_numpad_key(name: &str) -> Option<KeyCode> {
    let code = match name {
        "0" => KeyCode::Char('0'),
        "1" => KeyCode::Char('1'),
        "2" => KeyCode::Char('2'),
        "3" => KeyCode::Char('3'),
        "4" => KeyCode::Char('4'),
        "5" => KeyCode::Char('5'),
        "6" => KeyCode::Char('6'),
        "7" => KeyCode::Char('7'),
        "8" => KeyCode::Char('8'),
        "9" => KeyCode::Char('9'),
        "decimal" => KeyCode::Char('.'),
        "divide" => KeyCode::Char('/'),
        "multiply" => KeyCode::Char('*'),
        "subtract" => KeyCode::Char('-'),
        "add" => KeyCode::Char('+'),
        "enter" => KeyCode::Enter,
        "equal" => KeyCode::Char('='),
        "comma" => KeyCode::Char(','),
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "page-up" => KeyCode::PageUp,
        "page-down" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "insert" => KeyCode::Insert,
        "delete" => KeyCode::Delete,
        "begin" => KeyCode::KeypadBegin,
        _ => return None,
    };
    Some(code)
}

fn is_ctrl_c(event: KeyEvent) -> bool {
    matches!(event.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'c'))
        && event.modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind)
    }

    fn pressed(state: &InputState, button: usize) -> bool {
        state.button_mask() & (1 << button) != 0
    }

    #[test]
    fn default_keys_map_to_standard_libretro_buttons() {
        let cases = [
            (KeyCode::Left, BUTTON_LEFT),
            (KeyCode::Right, BUTTON_RIGHT),
            (KeyCode::Up, BUTTON_UP),
            (KeyCode::Down, BUTTON_DOWN),
            (KeyCode::Char('z'), BUTTON_B),
            (KeyCode::Char('Z'), BUTTON_B),
            (KeyCode::Char('x'), BUTTON_A),
            (KeyCode::Char('a'), BUTTON_Y),
            (KeyCode::Char('s'), BUTTON_X),
            (KeyCode::Enter, BUTTON_START),
            (KeyCode::Backspace, BUTTON_SELECT),
        ];

        for (code, expected_button) in cases {
            let mut state = InputState::default();
            state.handle_key(key(code, KeyEventKind::Press), Instant::now());
            assert!(pressed(&state, expected_button));
        }
    }

    #[test]
    fn press_repeat_and_release_update_a_button() {
        let now = Instant::now();
        let mut state = InputState::default();

        state.handle_key(key(KeyCode::Right, KeyEventKind::Press), now);
        assert!(pressed(&state, BUTTON_RIGHT));

        state.handle_key(
            key(KeyCode::Right, KeyEventKind::Repeat),
            now + Duration::from_millis(100),
        );
        state.expire(now + Duration::from_millis(200));
        assert!(pressed(&state, BUTTON_RIGHT));

        state.handle_key(
            key(KeyCode::Right, KeyEventKind::Release),
            now + Duration::from_millis(201),
        );
        assert!(!pressed(&state, BUTTON_RIGHT));
    }

    #[test]
    fn timeout_clears_only_stale_legacy_keys() {
        let now = Instant::now();
        let mut state = InputState::default();
        state.handle_key(key(KeyCode::Right, KeyEventKind::Press), now);
        state.handle_key(key(KeyCode::Char('z'), KeyEventKind::Press), now);
        state.handle_key(
            key(KeyCode::Right, KeyEventKind::Repeat),
            now + Duration::from_millis(500),
        );
        state.handle_key(
            key(KeyCode::Char('z'), KeyEventKind::Repeat),
            now + Duration::from_millis(600),
        );

        state.expire(now + Duration::from_millis(650));

        assert!(!pressed(&state, BUTTON_RIGHT));
        assert!(pressed(&state, BUTTON_B));
    }

    #[test]
    fn initial_hold_grace_bridges_the_os_repeat_delay() {
        let now = Instant::now();
        let mut state = InputState::default();
        state.handle_key(key(KeyCode::Right, KeyEventKind::Press), now);

        state.expire(now + REPEAT_TIMEOUT);

        assert!(pressed(&state, BUTTON_RIGHT));
    }

    #[test]
    fn release_aware_terminal_keeps_a_button_held_until_release() {
        let now = Instant::now();
        let mut state = InputState::with_release_events_supported(true);

        state.handle_key(key(KeyCode::Char('x'), KeyEventKind::Press), now);
        state.expire(now + INITIAL_HOLD_GRACE + Duration::from_millis(1));
        assert!(pressed(&state, BUTTON_A));

        state.handle_key(
            key(KeyCode::Char('x'), KeyEventKind::Release),
            now + INITIAL_HOLD_GRACE + Duration::from_millis(2),
        );
        assert!(!pressed(&state, BUTTON_A));
    }

    #[test]
    fn release_aware_terminal_recovers_from_a_missing_release() {
        let now = Instant::now();
        let mut state = InputState::with_release_events_supported(true);

        state.handle_key(key(KeyCode::Char('x'), KeyEventKind::Press), now);
        state.expire(now + RELEASE_EVENT_FAILSAFE);

        assert!(!pressed(&state, BUTTON_A));
    }

    #[test]
    fn release_failsafe_preserves_other_held_keys() {
        let now = Instant::now();
        let mut state = InputState::with_release_events_supported(true);

        state.handle_key(key(KeyCode::Right, KeyEventKind::Press), now);
        state.handle_key(
            key(KeyCode::Right, KeyEventKind::Repeat),
            now + Duration::from_millis(100),
        );
        state.handle_key(
            key(KeyCode::Char('x'), KeyEventKind::Press),
            now + Duration::from_millis(200),
        );
        state.expire(now + Duration::from_millis(300));

        assert!(pressed(&state, BUTTON_RIGHT));
        assert!(pressed(&state, BUTTON_A));

        state.expire(now + Duration::from_millis(850));

        assert!(pressed(&state, BUTTON_RIGHT));
        assert!(!pressed(&state, BUTTON_A));
    }

    #[test]
    fn ctrl_c_always_requests_shutdown() {
        let mut state = InputState::default();
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        state.handle_key(event, Instant::now());

        assert!(state.quit_requested());
    }

    #[test]
    fn q_is_not_a_default_quit_key() {
        let mut state = InputState::default();

        state.handle_key(key(KeyCode::Char('q'), KeyEventKind::Press), Instant::now());

        assert!(!state.quit_requested());
    }

    #[test]
    fn configured_quit_key_requests_shutdown() {
        let bindings = InputBindings::new(&[("a", BUTTON_A, "x")], "q").unwrap();
        let mut state = InputState::with_bindings(bindings, false);

        state.handle_key(key(KeyCode::Char('q'), KeyEventKind::Press), Instant::now());

        assert!(state.quit_requested());
    }

    #[test]
    fn ctrl_r_does_not_clear_held_buttons() {
        let mut state = InputState::default();
        state.handle_key(key(KeyCode::Right, KeyEventKind::Press), Instant::now());
        let event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);

        state.handle_key(event, Instant::now());

        assert!(pressed(&state, BUTTON_RIGHT));
    }

    #[test]
    fn numpad_binding_does_not_match_the_number_row() {
        let bindings = InputBindings::new(&[("a", BUTTON_A, "numpad-1")], "esc").unwrap();
        let mut state = InputState::with_bindings(bindings, true);

        state.handle_key(key(KeyCode::Char('1'), KeyEventKind::Press), Instant::now());

        assert!(!pressed(&state, BUTTON_A));
    }

    #[test]
    fn numpad_binding_matches_keypad_events() {
        let bindings = InputBindings::new(&[("a", BUTTON_A, "numpad-1")], "esc").unwrap();
        let mut state = InputState::with_bindings(bindings, true);
        let event = KeyEvent::new_with_kind_and_state(
            KeyCode::Char('1'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
            KeyEventState::KEYPAD,
        );

        state.handle_key(event, Instant::now());

        assert!(pressed(&state, BUTTON_A));
    }

    #[test]
    fn numpad_binding_ignores_num_lock_state() {
        let bindings = InputBindings::new(&[("a", BUTTON_A, "numpad-1")], "esc").unwrap();
        let mut state = InputState::with_bindings(bindings, true);
        let event = KeyEvent::new_with_kind_and_state(
            KeyCode::End,
            KeyModifiers::NONE,
            KeyEventKind::Press,
            KeyEventState::KEYPAD,
        );

        state.handle_key(event, Instant::now());

        assert!(pressed(&state, BUTTON_A));
    }

    #[test]
    fn printable_plus_is_a_valid_binding() {
        let bindings = InputBindings::new(&[("a", BUTTON_A, "+")], "esc").unwrap();
        let mut state = InputState::with_bindings(bindings, false);

        state.handle_key(key(KeyCode::Char('+'), KeyEventKind::Press), Instant::now());

        assert!(pressed(&state, BUTTON_A));
    }

    #[test]
    fn status_line_shows_default_keybindings() {
        let status = InputBindings::default().status_line();

        assert_eq!(
            status,
            "↑-up ↓-down ←-left →-right Select-backspace Start-enter X-s Y-a A-x B-z Exit-esc"
        );
    }

    #[test]
    fn status_line_shows_only_bound_optional_buttons() {
        let bindings = InputBindings::new(
            &[
                ("l", BUTTON_L, "q"),
                ("l2", BUTTON_L2, "w"),
                ("r", BUTTON_R, "e"),
                ("r2", BUTTON_R2, "r"),
                ("l3", BUTTON_L3, "t"),
                ("r3", BUTTON_R3, "y"),
            ],
            "esc",
        )
        .unwrap();

        assert_eq!(
            bindings.status_line(),
            "L1-q L2-w R1-e R2-r L3-t R3-y Exit-esc"
        );
    }
}
