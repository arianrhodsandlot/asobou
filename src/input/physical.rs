use std::io;
use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode};
use gilrs::{Axis, Button, EventType};

use super::joypad::{JOYPAD_BUTTON_COUNT, apply_left_stick, default_gamepad_button};
use super::{InputBindings, InputState};

const GAMEPAD_BUTTONS: [Button; 16] = [
    Button::South,
    Button::East,
    Button::North,
    Button::West,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputSnapshot {
    pub joypad_mask: u16,
    pub quit_requested: bool,
    pub rewind_held: bool,
    pub save_requested: bool,
    pub load_requested: bool,
}

pub struct PhysicalInput {
    state: InputState,
    gamepads: Option<gilrs::Gilrs>,
    active_gamepad: Option<usize>,
    focused: bool,
    baseline_gamepad: bool,
    ghostty_ctrl_c: Option<GhosttyCtrlCWorkaround>,
}

impl PhysicalInput {
    pub fn new(
        bindings: InputBindings,
        capabilities: crate::terminal::InputCapabilities,
    ) -> (Self, Option<String>) {
        let (gamepads, warning) = match gilrs::Gilrs::new() {
            Ok(gamepads) => (Some(gamepads), None),
            Err(error) => (None, Some(error.to_string())),
        };
        (
            Self::from_parts(
                bindings,
                capabilities.release_events_supported,
                capabilities.ghostty,
                gamepads,
            ),
            warning,
        )
    }

    fn from_parts(
        bindings: InputBindings,
        release_events_supported: bool,
        ghostty: bool,
        gamepads: Option<gilrs::Gilrs>,
    ) -> Self {
        Self {
            state: InputState::with_bindings(bindings, release_events_supported),
            gamepads,
            active_gamepad: None,
            focused: true,
            baseline_gamepad: true,
            ghostty_ctrl_c: ghostty.then(GhosttyCtrlCWorkaround::default),
        }
    }

    #[cfg(test)]
    fn without_gamepad(bindings: InputBindings, release_events_supported: bool) -> Self {
        Self::from_parts(bindings, release_events_supported, false, None)
    }

    pub fn poll(
        &mut self,
        terminal: &crate::terminal::TerminalSession,
        now: Instant,
    ) -> io::Result<InputSnapshot> {
        let mut terminal = TerminalSessionSource(terminal);
        drain_terminal(
            &mut self.state,
            &mut self.focused,
            &mut self.baseline_gamepad,
            &mut self.ghostty_ctrl_c,
            &mut terminal,
            now,
        )?;
        if let Some(gamepads) = self.gamepads.as_mut() {
            let mut gamepads = GilrsSource(gamepads);
            poll_gamepads(
                &mut self.state,
                &mut self.active_gamepad,
                self.focused,
                &mut self.baseline_gamepad,
                &mut gamepads,
            );
        }
        self.state.expire(now);
        Ok(self.snapshot())
    }

    pub fn clear_effective_state(&mut self) {
        self.state.clear();
        if let Some(workaround) = self.ghostty_ctrl_c.as_mut() {
            workaround.clear();
        }
        self.baseline_gamepad = true;
    }

    fn snapshot(&mut self) -> InputSnapshot {
        InputSnapshot {
            joypad_mask: self.state.button_mask(),
            quit_requested: self.state.quit_requested(),
            rewind_held: self.state.rewind_pressed(),
            save_requested: self.state.take_save(),
            load_requested: self.state.take_load(),
        }
    }

    #[cfg(test)]
    fn poll_sources<T: TerminalSource, G: GamepadSource>(
        &mut self,
        terminal: &mut T,
        gamepads: &mut G,
        now: Instant,
    ) -> io::Result<InputSnapshot> {
        if let Err(error) = drain_terminal(
            &mut self.state,
            &mut self.focused,
            &mut self.baseline_gamepad,
            &mut self.ghostty_ctrl_c,
            terminal,
            now,
        ) {
            self.clear_effective_state();
            return Err(error);
        }
        poll_gamepads(
            &mut self.state,
            &mut self.active_gamepad,
            self.focused,
            &mut self.baseline_gamepad,
            gamepads,
        );
        self.state.expire(now);
        Ok(self.snapshot())
    }
}

#[derive(Clone, Copy)]
enum TerminalObservation {
    Key(KeyEvent),
    FocusLost,
    FocusGained,
    Other,
}

trait TerminalSource {
    fn next_observation(&mut self) -> io::Result<Option<TerminalObservation>>;
}

struct TerminalSessionSource<'a>(&'a crate::terminal::TerminalSession);

impl TerminalSource for TerminalSessionSource<'_> {
    fn next_observation(&mut self) -> io::Result<Option<TerminalObservation>> {
        self.0.next_event().map(|event| {
            event.map(|event| match event {
                Event::Key(key) => TerminalObservation::Key(key),
                Event::FocusLost => TerminalObservation::FocusLost,
                Event::FocusGained => TerminalObservation::FocusGained,
                _ => TerminalObservation::Other,
            })
        })
    }
}

fn drain_terminal<T: TerminalSource>(
    state: &mut InputState,
    focused: &mut bool,
    baseline_gamepad: &mut bool,
    ghostty_ctrl_c: &mut Option<GhosttyCtrlCWorkaround>,
    terminal: &mut T,
    now: Instant,
) -> io::Result<()> {
    loop {
        match terminal.next_observation() {
            Ok(Some(TerminalObservation::Key(key))) => {
                if ghostty_ctrl_c
                    .as_mut()
                    .is_some_and(|workaround| workaround.handle_key(key))
                {
                    state.request_quit();
                }
                if *focused || is_ctrl_c(key) {
                    state.handle_key(key, now);
                }
            }
            Ok(Some(TerminalObservation::FocusLost)) => {
                *focused = false;
                state.clear();
                if let Some(workaround) = ghostty_ctrl_c.as_mut() {
                    workaround.clear();
                }
            }
            Ok(Some(TerminalObservation::FocusGained)) => {
                *focused = true;
                *baseline_gamepad = true;
            }
            Ok(Some(TerminalObservation::Other)) => {}
            Ok(None) => return Ok(()),
            Err(error) => {
                state.clear();
                if let Some(workaround) = ghostty_ctrl_c.as_mut() {
                    workaround.clear();
                }
                return Err(error);
            }
        }
    }
}

fn is_ctrl_c(event: KeyEvent) -> bool {
    matches!(event.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'c'))
        && event.modifiers.contains(KeyModifiers::CONTROL)
}

#[derive(Clone, Copy)]
enum GamepadObservation {
    Connected(usize),
    Disconnected(usize),
    MappedButtonPressed(usize),
    Other,
}

trait GamepadSource {
    fn next_observation(&mut self) -> Option<GamepadObservation>;
    fn advance(&mut self);
    fn first_connected(&self) -> Option<usize>;
    fn sample(&self, id: usize) -> Option<[bool; JOYPAD_BUTTON_COUNT]>;
}

struct GilrsSource<'a>(&'a mut gilrs::Gilrs);

impl GamepadSource for GilrsSource<'_> {
    fn next_observation(&mut self) -> Option<GamepadObservation> {
        self.0.next_event().map(|event| match event.event {
            EventType::Connected => GamepadObservation::Connected(event.id.into()),
            EventType::Disconnected => GamepadObservation::Disconnected(event.id.into()),
            EventType::ButtonPressed(button, _) if default_gamepad_button(button).is_some() => {
                GamepadObservation::MappedButtonPressed(event.id.into())
            }
            _ => GamepadObservation::Other,
        })
    }

    fn advance(&mut self) {
        self.0.inc();
    }

    fn first_connected(&self) -> Option<usize> {
        self.0.gamepads().next().map(|(id, _)| id.into())
    }

    fn sample(&self, id: usize) -> Option<[bool; JOYPAD_BUTTON_COUNT]> {
        let (_, gamepad) = self
            .0
            .gamepads()
            .find(|(gamepad_id, _)| usize::from(*gamepad_id) == id)?;
        let mut buttons = [false; JOYPAD_BUTTON_COUNT];
        for button in GAMEPAD_BUTTONS {
            if gamepad.is_pressed(button)
                && let Some(index) = default_gamepad_button(button)
            {
                buttons[index] = true;
            }
        }
        apply_left_stick(
            &mut buttons,
            gamepad.value(Axis::LeftStickX),
            gamepad.value(Axis::LeftStickY),
        );
        Some(buttons)
    }
}

fn poll_gamepads<G: GamepadSource>(
    state: &mut InputState,
    active_gamepad: &mut Option<usize>,
    focused: bool,
    baseline_gamepad: &mut bool,
    gamepads: &mut G,
) {
    while let Some(event) = gamepads.next_observation() {
        match event {
            GamepadObservation::Connected(id) if active_gamepad.is_none() => {
                *active_gamepad = Some(id);
                *baseline_gamepad = true;
            }
            GamepadObservation::Disconnected(id) if *active_gamepad == Some(id) => {
                *active_gamepad = None;
                state.clear_gamepad();
                *baseline_gamepad = true;
            }
            GamepadObservation::MappedButtonPressed(id) if *active_gamepad != Some(id) => {
                *active_gamepad = Some(id);
                *baseline_gamepad = true;
            }
            _ => {}
        }
    }
    gamepads.advance();
    let Some(id) = active_gamepad.or_else(|| gamepads.first_connected()) else {
        state.clear_gamepad();
        return;
    };
    if *active_gamepad != Some(id) {
        *active_gamepad = Some(id);
        *baseline_gamepad = true;
    }
    let Some(buttons) = gamepads.sample(id) else {
        state.clear_gamepad();
        return;
    };
    if !focused {
        state.clear_gamepad();
        return;
    }
    state.update_gamepad_with_edges(buttons, !std::mem::take(baseline_gamepad));
}

#[derive(Default)]
struct GhosttyCtrlCWorkaround {
    control_pressed_without_key: bool,
}

impl GhosttyCtrlCWorkaround {
    fn handle_key(&mut self, event: KeyEvent) -> bool {
        let control = matches!(
            event.code,
            KeyCode::Modifier(ModifierKeyCode::LeftControl | ModifierKeyCode::RightControl)
        );
        match (control, event.kind) {
            (true, KeyEventKind::Press) => {
                self.control_pressed_without_key = true;
                false
            }
            (true, KeyEventKind::Release) => std::mem::take(&mut self.control_pressed_without_key),
            (true, KeyEventKind::Repeat) => false,
            (false, _) => {
                self.control_pressed_without_key = false;
                false
            }
        }
    }

    fn clear(&mut self) {
        self.control_pressed_without_key = false;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::time::Instant;

    use super::*;
    use crate::input::{BUTTON_A, BUTTON_B, BUTTON_R, BUTTON_SELECT, InputBindings};

    struct ScriptedTerminal {
        observations: VecDeque<io::Result<TerminalObservation>>,
    }

    impl ScriptedTerminal {
        fn new(observations: impl IntoIterator<Item = TerminalObservation>) -> Self {
            Self {
                observations: observations.into_iter().map(Ok).collect(),
            }
        }

        fn failing(error: io::Error) -> Self {
            Self {
                observations: [Err(error)].into_iter().collect(),
            }
        }
    }

    impl TerminalSource for ScriptedTerminal {
        fn next_observation(&mut self) -> io::Result<Option<TerminalObservation>> {
            self.observations.pop_front().transpose()
        }
    }

    struct ScriptedGamepads {
        events: VecDeque<GamepadObservation>,
        devices: BTreeMap<usize, [bool; JOYPAD_BUTTON_COUNT]>,
    }

    impl ScriptedGamepads {
        fn connected(id: usize, pressed: impl IntoIterator<Item = usize>) -> Self {
            let mut buttons = [false; JOYPAD_BUTTON_COUNT];
            for button in pressed {
                buttons[button] = true;
            }
            Self {
                events: VecDeque::new(),
                devices: [(id, buttons)].into_iter().collect(),
            }
        }

        fn with_devices(
            devices: impl IntoIterator<Item = (usize, [bool; JOYPAD_BUTTON_COUNT])>,
        ) -> Self {
            Self {
                events: VecDeque::new(),
                devices: devices.into_iter().collect(),
            }
        }
    }

    impl GamepadSource for ScriptedGamepads {
        fn next_observation(&mut self) -> Option<GamepadObservation> {
            self.events.pop_front()
        }

        fn advance(&mut self) {}

        fn first_connected(&self) -> Option<usize> {
            self.devices.keys().next().copied()
        }

        fn sample(&self, id: usize) -> Option<[bool; JOYPAD_BUTTON_COUNT]> {
            self.devices.get(&id).copied()
        }
    }

    #[test]
    fn focus_loss_suppresses_gamepad_input_until_focus_returns() {
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut terminal = ScriptedTerminal::new([TerminalObservation::FocusLost]);
        let mut gamepads = ScriptedGamepads::connected(1, [BUTTON_A]);

        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, Instant::now())
            .unwrap();

        assert_eq!(snapshot.joypad_mask, 0);
    }

    #[test]
    fn focus_gain_restores_gamepad_state_without_firing_held_one_shots() {
        let now = Instant::now();
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut gamepads = ScriptedGamepads::connected(1, [BUTTON_SELECT, BUTTON_R]);
        let mut terminal = ScriptedTerminal::new([TerminalObservation::FocusLost]);
        input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();
        let mut terminal = ScriptedTerminal::new([TerminalObservation::FocusGained]);

        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();

        assert!(!snapshot.save_requested);
        assert_ne!(snapshot.joypad_mask & (1 << BUTTON_SELECT), 0);
        assert_eq!(snapshot.joypad_mask & (1 << BUTTON_R), 0);
    }

    #[test]
    fn fallback_controller_uses_held_buttons_as_its_hotkey_edge_baseline() {
        let now = Instant::now();
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut first_buttons = [false; JOYPAD_BUTTON_COUNT];
        first_buttons[BUTTON_A] = true;
        let mut fallback_buttons = [false; JOYPAD_BUTTON_COUNT];
        fallback_buttons[BUTTON_SELECT] = true;
        fallback_buttons[BUTTON_R] = true;
        let mut gamepads =
            ScriptedGamepads::with_devices([(1, first_buttons), (2, fallback_buttons)]);
        let mut terminal = ScriptedTerminal::new([]);
        input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();
        gamepads
            .events
            .push_back(GamepadObservation::Disconnected(1));
        gamepads.devices.remove(&1);

        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();

        assert!(!snapshot.save_requested);
        assert_ne!(snapshot.joypad_mask & (1 << BUTTON_SELECT), 0);
    }

    #[test]
    fn terminal_error_clears_effective_input_before_returning() {
        let now = Instant::now();
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut terminal = ScriptedTerminal::new([TerminalObservation::Key(KeyEvent::new(
            KeyCode::Char('x'),
            crossterm::event::KeyModifiers::NONE,
        ))]);
        let mut gamepads = ScriptedGamepads::with_devices([]);
        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();
        assert_ne!(snapshot.joypad_mask, 0);
        let mut terminal = ScriptedTerminal::failing(io::Error::other("terminal failed"));

        assert!(
            input
                .poll_sources(&mut terminal, &mut gamepads, now)
                .is_err()
        );

        assert_eq!(input.snapshot().joypad_mask, 0);
    }

    #[test]
    fn ghostty_unreported_control_sequence_requests_quit() {
        let now = Instant::now();
        let mut input = PhysicalInput::from_parts(InputBindings::default(), true, true, None);
        let control = KeyCode::Modifier(ModifierKeyCode::LeftControl);
        let mut terminal = ScriptedTerminal::new([
            TerminalObservation::Key(KeyEvent::new_with_kind(
                control,
                crossterm::event::KeyModifiers::CONTROL,
                KeyEventKind::Press,
            )),
            TerminalObservation::Key(KeyEvent::new_with_kind(
                control,
                crossterm::event::KeyModifiers::CONTROL,
                KeyEventKind::Release,
            )),
        ]);
        let mut gamepads = ScriptedGamepads::with_devices([]);

        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();

        assert!(snapshot.quit_requested);
    }

    #[test]
    fn ctrl_c_remains_available_while_unfocused() {
        let now = Instant::now();
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut terminal = ScriptedTerminal::new([
            TerminalObservation::FocusLost,
            TerminalObservation::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        ]);
        let mut gamepads = ScriptedGamepads::with_devices([]);

        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();

        assert!(snapshot.quit_requested);
    }

    #[test]
    fn mapped_button_press_transfers_the_active_controller() {
        let now = Instant::now();
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut first = [false; JOYPAD_BUTTON_COUNT];
        first[BUTTON_A] = true;
        let mut second = [false; JOYPAD_BUTTON_COUNT];
        second[BUTTON_B] = true;
        let mut gamepads = ScriptedGamepads::with_devices([(1, first), (2, second)]);
        let mut terminal = ScriptedTerminal::new([]);
        input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();
        gamepads
            .events
            .push_back(GamepadObservation::MappedButtonPressed(2));

        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();

        assert_eq!(snapshot.joypad_mask & (1 << BUTTON_A), 0);
        assert_ne!(snapshot.joypad_mask & (1 << BUTTON_B), 0);
    }

    #[test]
    fn connection_noise_does_not_transfer_the_active_controller() {
        let now = Instant::now();
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut first = [false; JOYPAD_BUTTON_COUNT];
        first[BUTTON_A] = true;
        let mut second = [false; JOYPAD_BUTTON_COUNT];
        second[BUTTON_B] = true;
        let mut gamepads = ScriptedGamepads::with_devices([(1, first), (2, second)]);
        let mut terminal = ScriptedTerminal::new([]);
        input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();
        gamepads.events.push_back(GamepadObservation::Other);

        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();

        assert_ne!(snapshot.joypad_mask & (1 << BUTTON_A), 0);
        assert_eq!(snapshot.joypad_mask & (1 << BUTTON_B), 0);
    }

    #[test]
    fn keyboard_and_gamepad_intent_are_merged_in_one_snapshot() {
        let now = Instant::now();
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut terminal = ScriptedTerminal::new([TerminalObservation::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))]);
        let mut gamepads = ScriptedGamepads::connected(1, [BUTTON_B]);

        let snapshot = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();

        assert_ne!(snapshot.joypad_mask & (1 << BUTTON_A), 0);
        assert_ne!(snapshot.joypad_mask & (1 << BUTTON_B), 0);
    }

    #[test]
    fn producing_a_snapshot_consumes_one_shot_actions() {
        let now = Instant::now();
        let mut input = PhysicalInput::without_gamepad(InputBindings::default(), true);
        let mut terminal = ScriptedTerminal::new([TerminalObservation::Key(KeyEvent::new(
            KeyCode::F(2),
            KeyModifiers::NONE,
        ))]);
        let mut gamepads = ScriptedGamepads::with_devices([]);

        let first = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();
        let second = input
            .poll_sources(&mut terminal, &mut gamepads, now)
            .unwrap();

        assert!(first.save_requested);
        assert!(!second.save_requested);
    }
}
