use super::bindings::InputKey;
use super::state::{INITIAL_HOLD_GRACE, RELEASE_EVENT_FAILSAFE, REPEAT_TIMEOUT};
use super::*;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, ModifierKeyCode,
};
use gilrs::Button;
use std::time::{Duration, Instant};

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
        (KeyCode::Char('q'), BUTTON_L),
        (KeyCode::Char('w'), BUTTON_R),
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
        "↑-up ↓-down ←-left →-right Select-rshift Start-enter L1-q R1-w X-s Y-a A-x B-z"
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

fn gamepad_buttons(pressed: &[usize]) -> [bool; JOYPAD_BUTTON_COUNT] {
    let mut buttons = [false; JOYPAD_BUTTON_COUNT];
    for button in pressed {
        buttons[*button] = true;
    }
    buttons
}

#[test]
fn gamepad_buttons_map_to_positional_libretro_buttons() {
    let cases = [
        (Button::South, BUTTON_B),
        (Button::East, BUTTON_A),
        (Button::West, BUTTON_Y),
        (Button::North, BUTTON_X),
        (Button::LeftTrigger, BUTTON_L),
        (Button::RightTrigger, BUTTON_R),
        (Button::LeftTrigger2, BUTTON_L2),
        (Button::RightTrigger2, BUTTON_R2),
        (Button::LeftThumb, BUTTON_L3),
        (Button::RightThumb, BUTTON_R3),
        (Button::Select, BUTTON_SELECT),
        (Button::Start, BUTTON_START),
        (Button::DPadUp, BUTTON_UP),
        (Button::DPadDown, BUTTON_DOWN),
        (Button::DPadLeft, BUTTON_LEFT),
        (Button::DPadRight, BUTTON_RIGHT),
    ];

    for (button, expected) in cases {
        assert_eq!(default_gamepad_button(button), Some(expected));
    }
    assert_eq!(default_gamepad_button(Button::Mode), None);
    assert_eq!(default_gamepad_button(Button::C), None);
    assert_eq!(default_gamepad_button(Button::Z), None);
    assert_eq!(default_gamepad_button(Button::Unknown), None);
}

#[test]
fn gamepad_buttons_reach_the_core_mask() {
    let mut state = InputState::default();

    state.update_gamepad(gamepad_buttons(&[BUTTON_A, BUTTON_UP, BUTTON_START]));

    assert!(pressed(&state, BUTTON_A));
    assert!(pressed(&state, BUTTON_UP));
    assert!(pressed(&state, BUTTON_START));
    assert!(!pressed(&state, BUTTON_B));
}

#[test]
fn gamepad_buttons_blend_with_keyboard_buttons() {
    let mut state = InputState::default();
    state.handle_key(key(KeyCode::Char('x'), KeyEventKind::Press), Instant::now());

    state.update_gamepad(gamepad_buttons(&[BUTTON_B]));

    assert!(pressed(&state, BUTTON_A));
    assert!(pressed(&state, BUTTON_B));
}

#[test]
fn select_and_start_quits() {
    let mut state = InputState::default();

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT]));
    assert!(!state.quit_requested());

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_START]));
    assert!(state.quit_requested());
}

#[test]
fn select_and_start_quits_in_either_press_order() {
    let mut state = InputState::default();

    state.update_gamepad(gamepad_buttons(&[BUTTON_START]));
    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_START]));

    assert!(state.quit_requested());
}

#[test]
fn select_hotkeys_fire_once_per_combo() {
    let mut state = InputState::default();

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_R]));
    assert!(state.take_save());
    assert!(!state.take_save());

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT]));
    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_R]));
    assert!(state.take_save());

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_R2]));
    assert!(state.take_load());
}

#[test]
fn select_hotkeys_do_not_fire_while_the_combo_is_held() {
    let mut state = InputState::default();

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_R]));
    assert!(state.take_save());

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_R]));
    assert!(!state.take_save());
}

#[test]
fn select_and_l1_holds_rewind() {
    let mut state = InputState::default();

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT]));
    assert!(!state.rewind_pressed());

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_L]));
    assert!(state.rewind_pressed());

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT]));
    assert!(!state.rewind_pressed());
}

#[test]
fn disabled_rewind_disables_the_gamepad_hotkey() {
    let bindings = bindings(&[], "esc", "r", false);
    let mut state = InputState::with_bindings(bindings, true);

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_L]));

    assert!(!state.rewind_pressed());
    assert!(pressed(&state, BUTTON_L));
}

#[test]
fn select_hotkeys_are_withheld_from_the_core() {
    let mut state = InputState::default();

    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_L, BUTTON_R]));

    assert!(pressed(&state, BUTTON_SELECT));
    assert!(!pressed(&state, BUTTON_L));
    assert!(!pressed(&state, BUTTON_R));

    state.update_gamepad(gamepad_buttons(&[BUTTON_L, BUTTON_R]));

    assert!(!pressed(&state, BUTTON_SELECT));
    assert!(pressed(&state, BUTTON_L));
    assert!(pressed(&state, BUTTON_R));
}

#[test]
fn left_stick_crosses_the_deadzone_into_dpad_directions() {
    let mut buttons = [false; JOYPAD_BUTTON_COUNT];

    apply_left_stick(&mut buttons, 0.0, 0.0);
    assert_eq!(buttons.iter().filter(|pressed| **pressed).count(), 0);

    apply_left_stick(&mut buttons, 0.4, 0.4);
    assert_eq!(buttons.iter().filter(|pressed| **pressed).count(), 0);

    apply_left_stick(&mut buttons, 0.6, 0.0);
    assert!(buttons[BUTTON_RIGHT]);

    apply_left_stick(&mut buttons, -0.6, 0.0);
    assert!(buttons[BUTTON_LEFT]);

    apply_left_stick(&mut buttons, 0.0, 0.6);
    assert!(buttons[BUTTON_UP]);

    apply_left_stick(&mut buttons, 0.0, -0.6);
    assert!(buttons[BUTTON_DOWN]);

    apply_left_stick(&mut buttons, -0.6, 0.6);
    assert!(buttons[BUTTON_LEFT] && buttons[BUTTON_UP]);
}

#[test]
fn clear_gamepad_releases_held_buttons_and_hotkeys() {
    let mut state = InputState::default();
    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_L, BUTTON_A]));
    assert!(state.rewind_pressed());

    state.clear_gamepad();

    assert!(!state.rewind_pressed());
    assert_eq!(state.button_mask(), 0);
}

#[test]
fn clear_resets_gamepad_edges() {
    let mut state = InputState::default();
    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_R]));
    assert!(state.take_save());

    state.clear();
    state.update_gamepad(gamepad_buttons(&[BUTTON_SELECT, BUTTON_R]));

    assert!(state.take_save());
}
