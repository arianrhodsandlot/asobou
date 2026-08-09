use crossterm::event::{KeyCode, KeyEvent, KeyEventState, ModifierKeyCode};
use std::collections::HashMap;

use super::joypad::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct InputKey {
    pub(super) code: KeyCode,
    pub(super) keypad: bool,
}

impl InputKey {
    pub(super) fn from_event(event: KeyEvent) -> Self {
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

    pub(super) fn parse(name: &str) -> Result<Self, String> {
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
pub(super) struct Binding {
    pub(super) key: InputKey,
    key_name: String,
    pub(super) button: usize,
}

#[derive(Clone, Debug)]

pub struct InputBindings {
    pub(super) gamepad: Vec<Binding>,
    pub(super) quit: InputKey,
    quit_name: String,
    pub(super) rewind: InputKey,
    rewind_name: String,
    pub(super) rewind_enabled: bool,
    pub(super) save_state: InputKey,
    save_state_name: String,
    pub(super) load_state: InputKey,
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
                Binding {
                    key: InputKey {
                        code: KeyCode::Char('q'),
                        keypad: false,
                    },
                    key_name: "q".into(),
                    button: BUTTON_L,
                },
                Binding {
                    key: InputKey {
                        code: KeyCode::Char('w'),
                        keypad: false,
                    },
                    key_name: "w".into(),
                    button: BUTTON_R,
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
