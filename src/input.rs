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

#[derive(Clone, Copy, Default)]
struct OneShotKey {
    pressed: bool,
    last_seen: Option<Instant>,
    repeat_seen: bool,
    pending: bool,
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

        if is_keypad_name(&name) {
            let code = parse_numpad_key(&name).ok_or_else(|| format!("unknown key: \"{name}\""))?;
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
    rewind: InputKey,
    rewind_name: String,
    rewind_enabled: bool,
    save_state: InputKey,
    save_state_name: String,
    load_state: InputKey,
    load_state_name: String,
}

impl InputBindings {
    pub fn new(
        gamepad: &[(&str, usize, &str)],
        quit: &str,
        rewind: &str,
        save_state: &str,
        load_state: &str,
        rewind_enabled: bool,
    ) -> Result<Self, String> {
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
        assigned.insert(quit_key, "quit");

        let (rewind_key, rewind_name) = if rewind_enabled {
            let rewind_key = InputKey::parse(rewind)
                .map_err(|error| format!("invalid key for input \"rewind\": {error}"))?;
            if let Some(previous) = assigned.get(&rewind_key) {
                return Err(format!(
                    "key \"{rewind}\" for input \"rewind\" is already bound to \"{previous}\""
                ));
            }
            assigned.insert(rewind_key, "rewind");
            (rewind_key, rewind.to_ascii_lowercase())
        } else {
            (
                InputKey {
                    code: KeyCode::Char('r'),
                    keypad: false,
                },
                "r".into(),
            )
        };

        let save_key = InputKey::parse(save_state)
            .map_err(|error| format!("invalid key for input \"save_state\": {error}"))?;
        if let Some(previous) = assigned.get(&save_key) {
            return Err(format!(
                "key \"{save_state}\" for input \"save_state\" is already bound to \"{previous}\""
            ));
        }
        assigned.insert(save_key, "save_state");

        let load_key = InputKey::parse(load_state)
            .map_err(|error| format!("invalid key for input \"load_state\": {error}"))?;
        if let Some(previous) = assigned.get(&load_key) {
            return Err(format!(
                "key \"{load_state}\" for input \"load_state\" is already bound to \"{previous}\""
            ));
        }

        Ok(Self {
            gamepad: bindings,
            quit: quit_key,
            quit_name: quit.to_ascii_lowercase(),
            rewind: rewind_key,
            rewind_name,
            rewind_enabled,
            save_state: save_key,
            save_state_name: save_state.to_ascii_lowercase(),
            load_state: load_key,
            load_state_name: load_state.to_ascii_lowercase(),
        })
    }

    #[cfg(test)]
    pub fn quit_name(&self) -> &str {
        &self.quit_name
    }

    #[cfg(test)]
    pub fn rewind_name(&self) -> &str {
        &self.rewind_name
    }

    #[cfg(test)]
    pub fn rewind_enabled(&self) -> bool {
        self.rewind_enabled
    }

    pub fn gamepad_status_line(&self) -> String {
        let mut items = Vec::with_capacity(16);
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
        items.join(" ")
    }

    pub fn controls_status_line(&self) -> String {
        let mut items = Vec::with_capacity(4);
        items.push(format!("Save-{}", self.save_state_name));
        items.push(format!("Load-{}", self.load_state_name));
        if self.rewind_enabled {
            items.push(format!("Rewind-{}", self.rewind_name));
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
                        code: KeyCode::Modifier(ModifierKeyCode::RightShift),
                        keypad: false,
                    },
                    key_name: "rshift".into(),
                    button: BUTTON_SELECT,
                },
            ],
            quit: InputKey {
                code: KeyCode::Esc,
                keypad: false,
            },
            quit_name: "escape".into(),
            rewind: InputKey {
                code: KeyCode::Char('r'),
                keypad: false,
            },
            rewind_name: "r".into(),
            rewind_enabled: true,
            save_state: InputKey {
                code: KeyCode::F(2),
                keypad: false,
            },
            save_state_name: "f2".into(),
            load_state: InputKey {
                code: KeyCode::F(4),
                keypad: false,
            },
            load_state_name: "f4".into(),
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
    rewind_key: KeyState,
    rewind_failsafe: bool,
    save_state: OneShotKey,
    load_state: OneShotKey,
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
            rewind_key: KeyState::new(),
            rewind_failsafe: false,
            save_state: OneShotKey::default(),
            load_state: OneShotKey::default(),
        }
    }

    pub fn handle_key(&mut self, event: KeyEvent, now: Instant) {
        if event.kind != KeyEventKind::Release && self.is_quit_key(event) {
            self.quit_requested = true;
            return;
        }

        let event_key = InputKey::from_event(event);
        if self.bindings.rewind_enabled && event_key == self.bindings.rewind {
            match event.kind {
                KeyEventKind::Press => {
                    self.rewind_key.repeat_seen |= self.rewind_key.pressed;
                    self.rewind_key.pressed = true;
                    self.rewind_key.last_seen = Some(now);
                    self.rewind_failsafe = true;
                }
                KeyEventKind::Repeat => {
                    self.rewind_key.pressed = true;
                    self.rewind_key.repeat_seen = true;
                    self.rewind_key.last_seen = Some(now);
                    self.rewind_failsafe = true;
                }
                KeyEventKind::Release => {
                    self.rewind_key.pressed = false;
                    self.rewind_key.last_seen = None;
                    self.rewind_key.repeat_seen = false;
                    self.rewind_failsafe = false;
                    self.release_events_supported = true;
                }
            }
            return;
        }

        if event_key == self.bindings.save_state {
            Self::handle_one_shot(&mut self.save_state, event.kind, now);
            return;
        }
        if event_key == self.bindings.load_state {
            Self::handle_one_shot(&mut self.load_state, event.kind, now);
            return;
        }

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

    fn handle_one_shot(key: &mut OneShotKey, kind: KeyEventKind, now: Instant) {
        match kind {
            KeyEventKind::Press => {
                key.pending |= !key.pressed;
                key.repeat_seen |= key.pressed;
                key.pressed = true;
                key.last_seen = Some(now);
            }
            KeyEventKind::Repeat => {
                key.pressed = true;
                key.repeat_seen = true;
                key.last_seen = Some(now);
            }
            KeyEventKind::Release => {
                key.pressed = false;
                key.last_seen = None;
                key.repeat_seen = false;
            }
        }
    }

    fn expire_one_shot(key: &mut OneShotKey, now: Instant, release_events_supported: bool) {
        let Some(last_seen) = key.last_seen else {
            return;
        };
        let timeout = if release_events_supported {
            if key.repeat_seen {
                REPEAT_TIMEOUT
            } else {
                RELEASE_EVENT_FAILSAFE
            }
        } else if key.repeat_seen {
            REPEAT_TIMEOUT
        } else {
            INITIAL_HOLD_GRACE
        };
        if now.saturating_duration_since(last_seen) >= timeout {
            key.pressed = false;
            key.last_seen = None;
            key.repeat_seen = false;
        }
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
            if self.rewind_failsafe {
                let key = &mut self.rewind_key;
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
                    self.rewind_failsafe = false;
                }
            }
            Self::expire_one_shot(&mut self.save_state, now, self.release_events_supported);
            Self::expire_one_shot(&mut self.load_state, now, self.release_events_supported);
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
        if self.rewind_key.pressed {
            let timeout = if self.rewind_key.repeat_seen {
                REPEAT_TIMEOUT
            } else {
                INITIAL_HOLD_GRACE
            };
            if self
                .rewind_key
                .last_seen
                .is_some_and(|last_seen| now.saturating_duration_since(last_seen) >= timeout)
            {
                self.rewind_key.pressed = false;
                self.rewind_key.last_seen = None;
                self.rewind_key.repeat_seen = false;
            }
        }
        Self::expire_one_shot(&mut self.save_state, now, self.release_events_supported);
        Self::expire_one_shot(&mut self.load_state, now, self.release_events_supported);
        self.rebuild_buttons();
    }

    pub fn clear(&mut self) {
        for key in &mut self.keys {
            key.pressed = false;
            key.last_seen = None;
            key.repeat_seen = false;
        }
        self.failsafe_key = None;
        self.rewind_key = KeyState::new();
        self.rewind_failsafe = false;
        self.save_state = OneShotKey::default();
        self.load_state = OneShotKey::default();
        self.buttons.fill(false);
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn rewind_pressed(&self) -> bool {
        self.rewind_key.pressed
    }

    pub fn take_save(&mut self) -> bool {
        let pending = self.save_state.pending;
        self.save_state.pending = false;
        pending
    }

    pub fn take_load(&mut self) -> bool {
        let pending = self.load_state.pending;
        self.load_state.pending = false;
        pending
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
        "pageup" | "page-up" => KeyCode::PageUp,
        "pagedown" | "page-down" => KeyCode::PageDown,
        "tab" => KeyCode::Tab,
        "del" | "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "null" => KeyCode::Null,
        "escape" | "esc" => KeyCode::Esc,
        "capslock" | "caps-lock" => KeyCode::CapsLock,
        "scroll_lock" | "scroll-lock" => KeyCode::ScrollLock,
        "numlock" | "num-lock" => KeyCode::NumLock,
        "print_screen" | "print-screen" => KeyCode::PrintScreen,
        "pause" => KeyCode::Pause,
        "menu" => KeyCode::Menu,
        "space" => KeyCode::Char(' '),
        "shift" | "left-shift" => KeyCode::Modifier(ModifierKeyCode::LeftShift),
        "rshift" | "right-shift" => KeyCode::Modifier(ModifierKeyCode::RightShift),
        "ctrl" | "left-control" | "left-ctrl" => KeyCode::Modifier(ModifierKeyCode::LeftControl),
        "rctrl" | "right-control" | "right-ctrl" => {
            KeyCode::Modifier(ModifierKeyCode::RightControl)
        }
        "alt" | "left-alt" => KeyCode::Modifier(ModifierKeyCode::LeftAlt),
        "ralt" | "right-alt" => KeyCode::Modifier(ModifierKeyCode::RightAlt),
        "left-super" => KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        "left-hyper" => KeyCode::Modifier(ModifierKeyCode::LeftHyper),
        "left-meta" => KeyCode::Modifier(ModifierKeyCode::LeftMeta),
        "right-super" => KeyCode::Modifier(ModifierKeyCode::RightSuper),
        "right-hyper" => KeyCode::Modifier(ModifierKeyCode::RightHyper),
        "right-meta" => KeyCode::Modifier(ModifierKeyCode::RightMeta),
        "iso-level-3-shift" => KeyCode::Modifier(ModifierKeyCode::IsoLevel3Shift),
        "iso-level-5-shift" => KeyCode::Modifier(ModifierKeyCode::IsoLevel5Shift),
        "num0" => KeyCode::Char('0'),
        "num1" => KeyCode::Char('1'),
        "num2" => KeyCode::Char('2'),
        "num3" => KeyCode::Char('3'),
        "num4" => KeyCode::Char('4'),
        "num5" => KeyCode::Char('5'),
        "num6" => KeyCode::Char('6'),
        "num7" => KeyCode::Char('7'),
        "num8" => KeyCode::Char('8'),
        "num9" => KeyCode::Char('9'),
        "period" => KeyCode::Char('.'),
        "comma" => KeyCode::Char(','),
        "slash" => KeyCode::Char('/'),
        "minus" | "subtract" => KeyCode::Char('-'),
        "equals" => KeyCode::Char('='),
        "leftbracket" => KeyCode::Char('['),
        "backslash" => KeyCode::Char('\\'),
        "rightbracket" => KeyCode::Char(']'),
        "backquote" => KeyCode::Char('`'),
        "quote" => KeyCode::Char('\''),
        "semicolon" => KeyCode::Char(';'),
        "tilde" => KeyCode::Char('~'),
        "add" => KeyCode::Char('+'),
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

fn is_keypad_name(name: &str) -> bool {
    matches!(
        name,
        "keypad0"
            | "keypad1"
            | "keypad2"
            | "keypad3"
            | "keypad4"
            | "keypad5"
            | "keypad6"
            | "keypad7"
            | "keypad8"
            | "keypad9"
            | "kp_period"
            | "kp_equals"
            | "kp_enter"
            | "kp_plus"
            | "kp_minus"
            | "multiply"
            | "divide"
    )
}

fn parse_numpad_key(name: &str) -> Option<KeyCode> {
    let code = match name {
        "0" | "keypad0" => KeyCode::Char('0'),
        "1" | "keypad1" => KeyCode::Char('1'),
        "2" | "keypad2" => KeyCode::Char('2'),
        "3" | "keypad3" => KeyCode::Char('3'),
        "4" | "keypad4" => KeyCode::Char('4'),
        "5" | "keypad5" => KeyCode::Char('5'),
        "6" | "keypad6" => KeyCode::Char('6'),
        "7" | "keypad7" => KeyCode::Char('7'),
        "8" | "keypad8" => KeyCode::Char('8'),
        "9" | "keypad9" => KeyCode::Char('9'),
        "decimal" | "kp_period" => KeyCode::Char('.'),
        "divide" => KeyCode::Char('/'),
        "multiply" => KeyCode::Char('*'),
        "subtract" | "kp_minus" => KeyCode::Char('-'),
        "add" | "kp_plus" => KeyCode::Char('+'),
        "enter" | "kp_enter" => KeyCode::Enter,
        "equal" | "kp_equals" => KeyCode::Char('='),
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

    fn bindings(
        gamepad: &[(&str, usize, &str)],
        quit: &str,
        rewind: &str,
        rewind_enabled: bool,
    ) -> InputBindings {
        InputBindings::new(gamepad, quit, rewind, "f2", "f4", rewind_enabled).unwrap()
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
            (
                KeyCode::Modifier(ModifierKeyCode::RightShift),
                BUTTON_SELECT,
            ),
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
        let bindings = bindings(&[("a", BUTTON_A, "x")], "q", "r", true);
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
        let bindings = bindings(&[("a", BUTTON_A, "numpad-1")], "esc", "r", true);
        let mut state = InputState::with_bindings(bindings, true);

        state.handle_key(key(KeyCode::Char('1'), KeyEventKind::Press), Instant::now());

        assert!(!pressed(&state, BUTTON_A));
    }

    #[test]
    fn numpad_binding_matches_keypad_events() {
        let bindings = bindings(&[("a", BUTTON_A, "numpad-1")], "esc", "r", true);
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
        let bindings = bindings(&[("a", BUTTON_A, "numpad-1")], "esc", "r", true);
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
    fn retroarch_key_names_map_to_expected_codes() {
        use ModifierKeyCode::*;

        let cases = [
            ("del", KeyCode::Delete, false),
            ("pageup", KeyCode::PageUp, false),
            ("pagedown", KeyCode::PageDown, false),
            ("capslock", KeyCode::CapsLock, false),
            ("numlock", KeyCode::NumLock, false),
            ("print_screen", KeyCode::PrintScreen, false),
            ("scroll_lock", KeyCode::ScrollLock, false),
            ("escape", KeyCode::Esc, false),
            ("shift", KeyCode::Modifier(LeftShift), false),
            ("rshift", KeyCode::Modifier(RightShift), false),
            ("ctrl", KeyCode::Modifier(LeftControl), false),
            ("rctrl", KeyCode::Modifier(RightControl), false),
            ("alt", KeyCode::Modifier(LeftAlt), false),
            ("ralt", KeyCode::Modifier(RightAlt), false),
            ("num0", KeyCode::Char('0'), false),
            ("num9", KeyCode::Char('9'), false),
            ("add", KeyCode::Char('+'), false),
            ("subtract", KeyCode::Char('-'), false),
            ("period", KeyCode::Char('.'), false),
            ("comma", KeyCode::Char(','), false),
            ("slash", KeyCode::Char('/'), false),
            ("minus", KeyCode::Char('-'), false),
            ("equals", KeyCode::Char('='), false),
            ("leftbracket", KeyCode::Char('['), false),
            ("backslash", KeyCode::Char('\\'), false),
            ("rightbracket", KeyCode::Char(']'), false),
            ("backquote", KeyCode::Char('`'), false),
            ("quote", KeyCode::Char('\''), false),
            ("semicolon", KeyCode::Char(';'), false),
            ("tilde", KeyCode::Char('~'), false),
            ("keypad0", KeyCode::Char('0'), true),
            ("keypad9", KeyCode::Char('9'), true),
            ("kp_period", KeyCode::Char('.'), true),
            ("kp_equals", KeyCode::Char('='), true),
            ("kp_enter", KeyCode::Enter, true),
            ("kp_plus", KeyCode::Char('+'), true),
            ("kp_minus", KeyCode::Char('-'), true),
            ("multiply", KeyCode::Char('*'), true),
            ("divide", KeyCode::Char('/'), true),
        ];

        for (name, code, keypad) in cases {
            let key = InputKey::parse(name).unwrap();
            assert_eq!(key, InputKey { code, keypad }, "name: {name}");
        }
    }

    #[test]
    fn printable_plus_is_a_valid_binding() {
        let bindings = bindings(&[("a", BUTTON_A, "+")], "esc", "r", true);
        let mut state = InputState::with_bindings(bindings, false);

        state.handle_key(key(KeyCode::Char('+'), KeyEventKind::Press), Instant::now());

        assert!(pressed(&state, BUTTON_A));
    }

    #[test]
    fn gamepad_status_line_shows_default_keybindings() {
        let status = InputBindings::default().gamepad_status_line();

        assert_eq!(
            status,
            "↑-up ↓-down ←-left →-right Select-rshift Start-enter X-s Y-a A-x B-z"
        );
    }

    #[test]
    fn gamepad_status_line_shows_only_bound_optional_buttons() {
        let bindings = InputBindings::new(
            &[
                ("l", BUTTON_L, "q"),
                ("l2", BUTTON_L2, "w"),
                ("r", BUTTON_R, "e"),
                ("r2", BUTTON_R2, "u"),
                ("l3", BUTTON_L3, "t"),
                ("r3", BUTTON_R3, "y"),
            ],
            "esc",
            "p",
            "f2",
            "f4",
            true,
        )
        .unwrap();

        assert_eq!(
            bindings.gamepad_status_line(),
            "L1-q L2-w R1-e R2-u L3-t R3-y"
        );
    }

    #[test]
    fn controls_status_line_uses_action_order() {
        let bindings = bindings(&[], "esc", "r", true);

        assert_eq!(
            bindings.controls_status_line(),
            "Save-f2 Load-f4 Rewind-r Exit-esc"
        );
    }

    #[test]
    fn rewind_key_stays_pressed_until_release() {
        let now = Instant::now();
        let mut state = InputState::with_release_events_supported(true);

        state.handle_key(key(KeyCode::Char('r'), KeyEventKind::Press), now);
        assert!(state.rewind_pressed());
        assert_eq!(state.button_mask(), 0);

        state.handle_key(
            key(KeyCode::Char('r'), KeyEventKind::Release),
            now + Duration::from_millis(1),
        );
        assert!(!state.rewind_pressed());
    }

    #[test]
    fn rewind_key_uses_the_configured_binding() {
        let bindings = bindings(&[], "esc", "y", true);
        let mut state = InputState::with_bindings(bindings, true);

        state.handle_key(key(KeyCode::Char('r'), KeyEventKind::Press), Instant::now());
        assert!(!state.rewind_pressed());

        state.handle_key(key(KeyCode::Char('y'), KeyEventKind::Press), Instant::now());
        assert!(state.rewind_pressed());
    }

    #[test]
    fn rewind_key_recovers_from_a_missing_release() {
        let now = Instant::now();
        let mut state = InputState::with_release_events_supported(true);

        state.handle_key(key(KeyCode::Char('r'), KeyEventKind::Press), now);
        state.expire(now + RELEASE_EVENT_FAILSAFE);

        assert!(!state.rewind_pressed());
    }

    #[test]
    fn rewind_key_conflicts_with_gamepad_and_quit_bindings() {
        let error =
            InputBindings::new(&[("a", BUTTON_A, "r")], "esc", "r", "f2", "f4", true).unwrap_err();
        assert!(error.contains("already bound"));

        let error = InputBindings::new(&[], "r", "r", "f2", "f4", true).unwrap_err();
        assert!(error.contains("already bound"));
    }

    #[test]
    fn disabled_rewind_frees_the_key_for_gamepad() {
        let bindings = bindings(&[("a", BUTTON_A, "r")], "esc", "r", false);
        let mut state = InputState::with_bindings(bindings, true);

        state.handle_key(key(KeyCode::Char('r'), KeyEventKind::Press), Instant::now());

        assert!(pressed(&state, BUTTON_A));
        assert!(!state.rewind_pressed());
    }

    #[test]
    fn disabled_rewind_hides_the_hotkey() {
        let bindings = bindings(&[], "esc", "r", false);

        assert_eq!(bindings.controls_status_line(), "Save-f2 Load-f4 Exit-esc");
    }

    #[test]
    fn clear_resets_the_rewind_key() {
        let now = Instant::now();
        let mut state = InputState::default();
        state.handle_key(key(KeyCode::Char('r'), KeyEventKind::Press), now);

        state.clear();

        assert!(!state.rewind_pressed());
    }

    #[test]
    fn save_and_load_fire_once_per_press() {
        let now = Instant::now();
        let mut state = InputState::default();

        state.handle_key(key(KeyCode::F(2), KeyEventKind::Press), now);
        assert!(state.take_save());
        assert!(!state.take_save());

        state.handle_key(key(KeyCode::F(4), KeyEventKind::Press), now);
        assert!(state.take_load());
        assert!(!state.take_load());

        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Release),
            now + Duration::from_millis(50),
        );
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + Duration::from_millis(100),
        );
        assert!(state.take_save());
    }

    #[test]
    fn repeated_keys_do_not_retrigger_save_or_load() {
        let now = Instant::now();
        let mut state = InputState::default();

        state.handle_key(key(KeyCode::F(2), KeyEventKind::Press), now);
        assert!(state.take_save());
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Repeat),
            now + Duration::from_millis(50),
        );
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Repeat),
            now + Duration::from_millis(100),
        );
        assert!(!state.take_save());

        state.handle_key(key(KeyCode::F(4), KeyEventKind::Press), now);
        assert!(state.take_load());
        state.handle_key(
            key(KeyCode::F(4), KeyEventKind::Repeat),
            now + Duration::from_millis(50),
        );
        assert!(!state.take_load());

        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Release),
            now + Duration::from_millis(150),
        );
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + Duration::from_millis(200),
        );
        assert!(state.take_save());
    }

    #[test]
    fn legacy_auto_repeat_does_not_retrigger_save() {
        let now = Instant::now();
        let mut state = InputState::default();

        state.handle_key(key(KeyCode::F(2), KeyEventKind::Press), now);
        assert!(state.take_save());
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + Duration::from_millis(30),
        );
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + Duration::from_millis(60),
        );
        assert!(!state.take_save());

        state.expire(now + Duration::from_millis(500));
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + Duration::from_millis(600),
        );
        assert!(state.take_save());
    }

    #[test]
    fn late_legacy_repeat_without_expiration_does_not_retrigger_save() {
        let now = Instant::now();
        let mut state = InputState::default();

        state.handle_key(key(KeyCode::F(2), KeyEventKind::Press), now);
        assert!(state.take_save());

        // A queued auto-repeat Press arrives 200 ms later with no
        // expiration in between; the key is still the same held press.
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + Duration::from_millis(200),
        );
        assert!(!state.take_save());

        state.expire(now + Duration::from_millis(400));
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + Duration::from_millis(500),
        );
        assert!(state.take_save());
    }

    #[test]
    fn quick_repeat_of_a_single_tap_is_ignored() {
        let now = Instant::now();
        let mut state = InputState::default();

        state.handle_key(key(KeyCode::F(2), KeyEventKind::Press), now);
        assert!(state.take_save());
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + Duration::from_millis(30),
        );
        assert!(!state.take_save());

        state.expire(now + INITIAL_HOLD_GRACE + Duration::from_millis(1));
        state.handle_key(
            key(KeyCode::F(2), KeyEventKind::Press),
            now + INITIAL_HOLD_GRACE + Duration::from_millis(2),
        );
        assert!(state.take_save());
    }

    #[test]
    fn clear_cancels_pending_save_and_load() {
        let now = Instant::now();
        let mut state = InputState::default();
        state.handle_key(key(KeyCode::F(2), KeyEventKind::Press), now);
        state.handle_key(key(KeyCode::F(4), KeyEventKind::Press), now);

        state.clear();

        assert!(!state.take_save());
        assert!(!state.take_load());
    }

    #[test]
    fn save_and_load_keys_conflict_with_other_bindings() {
        let error =
            InputBindings::new(&[("a", BUTTON_A, "f2")], "esc", "r", "f2", "f4", true).unwrap_err();
        assert!(error.contains("already bound"));

        let error = InputBindings::new(&[], "f4", "r", "f2", "f4", true).unwrap_err();
        assert!(error.contains("already bound"));

        let error = InputBindings::new(&[], "esc", "f2", "f2", "f4", true).unwrap_err();
        assert!(error.contains("already bound"));

        let error = InputBindings::new(&[], "esc", "r", "f2", "f2", true).unwrap_err();
        assert!(error.contains("already bound"));
    }

    #[test]
    fn save_and_load_use_the_configured_bindings() {
        let bindings = InputBindings::new(&[], "esc", "r", "f1", "f3", true).unwrap();
        let mut state = InputState::with_bindings(bindings, true);

        state.handle_key(key(KeyCode::F(2), KeyEventKind::Press), Instant::now());
        assert!(!state.take_save());
        state.handle_key(key(KeyCode::F(4), KeyEventKind::Press), Instant::now());
        assert!(!state.take_load());

        state.handle_key(key(KeyCode::F(1), KeyEventKind::Press), Instant::now());
        assert!(state.take_save());
        state.handle_key(key(KeyCode::F(3), KeyEventKind::Press), Instant::now());
        assert!(state.take_load());
    }
}
