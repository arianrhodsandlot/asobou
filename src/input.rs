use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode};
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

const MAPPED_KEY_COUNT: usize = 11;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Binding {
    key: usize,
    button: usize,
}

pub struct InputState {
    buttons: [bool; JOYPAD_BUTTON_COUNT],
    keys: [KeyState; MAPPED_KEY_COUNT],
    quit_requested: bool,
    release_events_supported: bool,
    failsafe_key: Option<usize>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            buttons: [false; JOYPAD_BUTTON_COUNT],
            keys: [KeyState::new(); MAPPED_KEY_COUNT],
            quit_requested: false,
            release_events_supported: false,
            failsafe_key: None,
        }
    }
}

impl InputState {
    pub fn with_release_events_supported(release_events_supported: bool) -> Self {
        Self {
            release_events_supported,
            ..Self::default()
        }
    }

    pub fn handle_key(&mut self, event: KeyEvent, now: Instant) {
        if event.kind != KeyEventKind::Release && is_quit_key(event) {
            self.quit_requested = true;
            return;
        }

        if event.kind != KeyEventKind::Release && is_reset_key(event) {
            self.clear();
            return;
        }

        let Some(binding) = binding_for(event.code) else {
            return;
        };
        let key = &mut self.keys[binding.key];

        match event.kind {
            KeyEventKind::Press => {
                key.repeat_seen |= key.pressed;
                key.pressed = true;
                key.last_seen = Some(now);
                self.failsafe_key = Some(binding.key);
            }
            KeyEventKind::Repeat => {
                key.pressed = true;
                key.repeat_seen = true;
                key.last_seen = Some(now);
                self.failsafe_key = Some(binding.key);
            }
            KeyEventKind::Release => {
                key.pressed = false;
                key.last_seen = None;
                key.repeat_seen = false;
                self.release_events_supported = true;
                if self.failsafe_key == Some(binding.key) {
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
            if key.pressed
                && let Some(binding) = binding_for_key_id(key_id)
            {
                self.buttons[binding.button] = true;
            }
        }
    }
}

fn binding_for(code: KeyCode) -> Option<Binding> {
    let binding = match code {
        KeyCode::Left => Binding {
            key: 0,
            button: BUTTON_LEFT,
        },
        KeyCode::Right => Binding {
            key: 1,
            button: BUTTON_RIGHT,
        },
        KeyCode::Up => Binding {
            key: 2,
            button: BUTTON_UP,
        },
        KeyCode::Down => Binding {
            key: 3,
            button: BUTTON_DOWN,
        },
        KeyCode::Char(character) if character.eq_ignore_ascii_case(&'z') => Binding {
            key: 4,
            button: BUTTON_B,
        },
        KeyCode::Char(character) if character.eq_ignore_ascii_case(&'x') => Binding {
            key: 5,
            button: BUTTON_A,
        },
        KeyCode::Char(character) if character.eq_ignore_ascii_case(&'a') => Binding {
            key: 6,
            button: BUTTON_Y,
        },
        KeyCode::Char(character) if character.eq_ignore_ascii_case(&'s') => Binding {
            key: 7,
            button: BUTTON_X,
        },
        KeyCode::Enter => Binding {
            key: 8,
            button: BUTTON_START,
        },
        KeyCode::Modifier(ModifierKeyCode::RightShift) => Binding {
            key: 9,
            button: BUTTON_SELECT,
        },
        KeyCode::Backspace => Binding {
            key: 10,
            button: BUTTON_SELECT,
        },
        _ => return None,
    };
    Some(binding)
}

fn binding_for_key_id(key: usize) -> Option<Binding> {
    [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Char('z'),
        KeyCode::Char('x'),
        KeyCode::Char('a'),
        KeyCode::Char('s'),
        KeyCode::Enter,
        KeyCode::Modifier(ModifierKeyCode::RightShift),
        KeyCode::Backspace,
    ]
    .get(key)
    .copied()
    .and_then(binding_for)
}

fn is_quit_key(event: KeyEvent) -> bool {
    matches!(event.code, KeyCode::Esc)
        || matches!(event.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'q'))
        || matches!(event.code, KeyCode::Char('c'))
            && event.modifiers.contains(KeyModifiers::CONTROL)
}

fn is_reset_key(event: KeyEvent) -> bool {
    matches!(event.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'r'))
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
    fn maps_keys_to_standard_libretro_buttons() {
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
            (
                KeyCode::Modifier(ModifierKeyCode::RightShift),
                BUTTON_SELECT,
            ),
        ];

        for (code, expected_button) in cases {
            assert_eq!(binding_for(code).unwrap().button, expected_button);
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
    fn multiple_buttons_and_aliases_remain_independent() {
        let now = Instant::now();
        let mut state = InputState::default();
        for code in [KeyCode::Right, KeyCode::Char('z'), KeyCode::Char('x')] {
            state.handle_key(key(code, KeyEventKind::Press), now);
        }
        state.handle_key(key(KeyCode::Backspace, KeyEventKind::Press), now);
        state.handle_key(
            key(
                KeyCode::Modifier(ModifierKeyCode::RightShift),
                KeyEventKind::Press,
            ),
            now,
        );
        state.handle_key(
            key(KeyCode::Backspace, KeyEventKind::Release),
            now + Duration::from_millis(1),
        );

        assert!(pressed(&state, BUTTON_RIGHT));
        assert!(pressed(&state, BUTTON_B));
        assert!(pressed(&state, BUTTON_A));
        assert!(pressed(&state, BUTTON_SELECT));
    }

    #[test]
    fn quit_keys_request_shutdown() {
        for code in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('Q')] {
            let mut state = InputState::default();
            state.handle_key(key(code, KeyEventKind::Press), Instant::now());
            assert!(state.quit_requested());
        }
    }
}
