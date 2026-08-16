use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::{Duration, Instant};

use super::bindings::{InputBindings, InputKey};
use super::joypad::*;

pub(super) const REPEAT_TIMEOUT: Duration = Duration::from_millis(140);
pub(super) const INITIAL_HOLD_GRACE: Duration = Duration::from_millis(250);
pub(super) const RELEASE_EVENT_FAILSAFE: Duration = Duration::from_millis(650);
// Rio on Windows delivers keyboard input through ConPTY and synthesizes a
// key-up in the same millisecond as every key-down (and never repeats while a
// key is held), whereas healthy terminals report the true physical key-up.
// Trusting rio's instant release collapses a press into zero frames of input.
// The workaround is restricted to that identifiable mode (see
// `synthetic_releases`): a release arriving within this window of its press
// is treated as spurious and the key is kept pressed until the release
// failsafe expires instead.
pub(super) const SPURIOUS_RELEASE_WINDOW: Duration = Duration::from_millis(50);

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

pub(super) struct InputState {
    bindings: InputBindings,
    buttons: [bool; JOYPAD_BUTTON_COUNT],
    keys: Vec<KeyState>,
    quit_requested: bool,
    release_events_supported: bool,
    // Set for terminals whose releases cannot be trusted (rio on Windows
    // through ConPTY): releases arrive within milliseconds of their press,
    // so releases are debounced and every pressed key needs the failsafe.
    synthetic_releases: bool,
    failsafe_key: Option<usize>,
    rewind_key: KeyState,
    rewind_failsafe: bool,
    save_state: OneShotKey,
    load_state: OneShotKey,
    gamepad_buttons: [bool; JOYPAD_BUTTON_COUNT],
    previous_gamepad_buttons: [bool; JOYPAD_BUTTON_COUNT],
    gamepad_rewind: bool,
    gamepad_save_pending: bool,
    gamepad_load_pending: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self::with_bindings(InputBindings::default(), false, false)
    }
}

impl InputState {
    #[cfg(test)]
    pub(super) fn with_release_events_supported(release_events_supported: bool) -> Self {
        Self::with_bindings(InputBindings::default(), release_events_supported, false)
    }

    #[cfg(test)]
    pub(super) fn with_synthetic_releases(release_events_supported: bool) -> Self {
        Self::with_bindings(InputBindings::default(), release_events_supported, true)
    }

    pub fn with_bindings(
        bindings: InputBindings,
        release_events_supported: bool,
        synthetic_releases: bool,
    ) -> Self {
        let keys = vec![KeyState::new(); bindings.gamepad.len()];
        Self {
            bindings,
            buttons: [false; JOYPAD_BUTTON_COUNT],
            keys,
            quit_requested: false,
            release_events_supported,
            synthetic_releases,
            failsafe_key: None,
            rewind_key: KeyState::new(),
            rewind_failsafe: false,
            save_state: OneShotKey::default(),
            load_state: OneShotKey::default(),
            gamepad_buttons: [false; JOYPAD_BUTTON_COUNT],
            previous_gamepad_buttons: [false; JOYPAD_BUTTON_COUNT],
            gamepad_rewind: false,
            gamepad_save_pending: false,
            gamepad_load_pending: false,
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
                    if !self.is_spurious_release(self.rewind_key.last_seen, now) {
                        self.rewind_key.pressed = false;
                        self.rewind_key.last_seen = None;
                        self.rewind_key.repeat_seen = false;
                        self.rewind_failsafe = false;
                        self.release_events_supported = true;
                    }
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
                let spurious = self.is_spurious_release(self.keys[key_id].last_seen, now);
                if !spurious {
                    let key = &mut self.keys[key_id];
                    key.pressed = false;
                    key.last_seen = None;
                    key.repeat_seen = false;
                    self.release_events_supported = true;
                    if self.failsafe_key == Some(key_id) {
                        self.failsafe_key = None;
                    }
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

    // While Select is held it doubles as a hotkey modifier: Start quits, L1
    // holds rewind, R1 saves and R2 loads. The action buttons are withheld
    // from the core during the combo; Select itself still passes through.
    #[cfg(test)]
    pub fn update_gamepad(&mut self, buttons: [bool; JOYPAD_BUTTON_COUNT]) {
        self.update_gamepad_with_edges(buttons, true);
    }

    pub(super) fn update_gamepad_with_edges(
        &mut self,
        buttons: [bool; JOYPAD_BUTTON_COUNT],
        detect_edges: bool,
    ) {
        let previous = self.previous_gamepad_buttons;
        let select = buttons[BUTTON_SELECT];

        if detect_edges
            && select
            && buttons[BUTTON_START]
            && !(previous[BUTTON_SELECT] && previous[BUTTON_START])
        {
            self.quit_requested = true;
        }
        if detect_edges
            && select
            && buttons[BUTTON_R]
            && !(previous[BUTTON_SELECT] && previous[BUTTON_R])
        {
            self.gamepad_save_pending = true;
        }
        if detect_edges
            && select
            && buttons[BUTTON_R2]
            && !(previous[BUTTON_SELECT] && previous[BUTTON_R2])
        {
            self.gamepad_load_pending = true;
        }
        self.gamepad_rewind = self.bindings.rewind_enabled && select && buttons[BUTTON_L];

        let mut effective = buttons;
        if select {
            effective[BUTTON_START] = false;
            effective[BUTTON_R] = false;
            effective[BUTTON_R2] = false;
            if self.bindings.rewind_enabled {
                effective[BUTTON_L] = false;
            }
        }
        self.gamepad_buttons = effective;
        self.previous_gamepad_buttons = buttons;
        self.rebuild_buttons();
    }

    pub fn clear_gamepad(&mut self) {
        self.gamepad_buttons.fill(false);
        self.previous_gamepad_buttons.fill(false);
        self.gamepad_rewind = false;
        self.gamepad_save_pending = false;
        self.gamepad_load_pending = false;
        self.rebuild_buttons();
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
            if self.synthetic_releases {
                // Every release from this terminal is synthesized right after
                // its press, so any pressed key whose events have stopped
                // needs the failsafe to avoid being stuck forever.
                for key in &mut self.keys {
                    expire_held_key(key, now, RELEASE_EVENT_FAILSAFE);
                }
                expire_held_key(&mut self.rewind_key, now, RELEASE_EVENT_FAILSAFE);
            } else {
                // Healthy terminals report real releases. Only the most
                // recently pressed key needs a failsafe (a release can still
                // be lost); keys held before it must stay pressed until
                // their own release arrives, even when repeats have stopped
                // because another key was pressed.
                if let Some(key_id) = self.failsafe_key
                    && expire_held_key(&mut self.keys[key_id], now, RELEASE_EVENT_FAILSAFE)
                {
                    self.failsafe_key = None;
                }
                if self.rewind_failsafe
                    && expire_held_key(&mut self.rewind_key, now, RELEASE_EVENT_FAILSAFE)
                {
                    self.rewind_failsafe = false;
                }
            }
            Self::expire_one_shot(&mut self.save_state, now, self.release_events_supported);
            Self::expire_one_shot(&mut self.load_state, now, self.release_events_supported);
            self.rebuild_buttons();
            return;
        }

        for key in &mut self.keys {
            expire_held_key(key, now, INITIAL_HOLD_GRACE);
        }
        expire_held_key(&mut self.rewind_key, now, INITIAL_HOLD_GRACE);
        Self::expire_one_shot(&mut self.save_state, now, self.release_events_supported);
        Self::expire_one_shot(&mut self.load_state, now, self.release_events_supported);
        self.rebuild_buttons();
    }

    pub fn clear(&mut self) {
        self.clear_gamepad();
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

    pub(super) fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    pub fn rewind_pressed(&self) -> bool {
        self.rewind_key.pressed || self.gamepad_rewind
    }

    pub fn take_save(&mut self) -> bool {
        let pending = self.save_state.pending || self.gamepad_save_pending;
        self.save_state.pending = false;
        self.gamepad_save_pending = false;
        pending
    }

    pub fn take_load(&mut self) -> bool {
        let pending = self.load_state.pending || self.gamepad_load_pending;
        self.load_state.pending = false;
        self.gamepad_load_pending = false;
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
        for (button, pressed) in self.gamepad_buttons.iter().enumerate() {
            if *pressed {
                self.buttons[button] = true;
            }
        }
    }

    fn is_spurious_release(&self, last_seen: Option<Instant>, now: Instant) -> bool {
        self.synthetic_releases
            && last_seen.is_some_and(|press| now.saturating_duration_since(press) < SPURIOUS_RELEASE_WINDOW)
    }

    fn is_quit_key(&self, event: KeyEvent) -> bool {
        is_ctrl_c(event) || InputKey::from_event(event) == self.bindings.quit
    }
}

fn is_ctrl_c(event: KeyEvent) -> bool {
    matches!(event.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'c'))
        && event.modifiers.contains(KeyModifiers::CONTROL)
}

fn expire_held_key(key: &mut KeyState, now: Instant, no_repeat_timeout: Duration) -> bool {
    if !key.pressed {
        return false;
    }
    let timeout = if key.repeat_seen {
        REPEAT_TIMEOUT
    } else {
        no_repeat_timeout
    };
    if key
        .last_seen
        .is_some_and(|last_seen| now.saturating_duration_since(last_seen) >= timeout)
    {
        key.pressed = false;
        key.last_seen = None;
        key.repeat_seen = false;
        return true;
    }
    false
}
