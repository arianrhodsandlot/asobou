use gilrs::Button;

pub const JOYPAD_BUTTON_COUNT: usize = 16;
pub const RETRO_DEVICE_JOYPAD: u32 = 1;

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

pub const GAMEPAD_STICK_DEADZONE: f32 = 0.5;

// Positional default mapping: the face buttons line up with the libretro
// B/A/Y/X diamond (South->B, East->A, West->Y, North->X), matching the
// RetroArch defaults, and the triggers map to the shoulder pairs.
pub fn default_gamepad_button(button: Button) -> Option<usize> {
    Some(match button {
        Button::South => BUTTON_B,
        Button::East => BUTTON_A,
        Button::West => BUTTON_Y,
        Button::North => BUTTON_X,
        Button::LeftTrigger => BUTTON_L,
        Button::RightTrigger => BUTTON_R,
        Button::LeftTrigger2 => BUTTON_L2,
        Button::RightTrigger2 => BUTTON_R2,
        Button::LeftThumb => BUTTON_L3,
        Button::RightThumb => BUTTON_R3,
        Button::Select => BUTTON_SELECT,
        Button::Start => BUTTON_START,
        Button::DPadUp => BUTTON_UP,
        Button::DPadDown => BUTTON_DOWN,
        Button::DPadLeft => BUTTON_LEFT,
        Button::DPadRight => BUTTON_RIGHT,
        Button::Mode | Button::C | Button::Z | Button::Unknown => return None,
    })
}

pub fn apply_left_stick(buttons: &mut [bool; JOYPAD_BUTTON_COUNT], x: f32, y: f32) {
    if x > GAMEPAD_STICK_DEADZONE {
        buttons[BUTTON_RIGHT] = true;
    }
    if x < -GAMEPAD_STICK_DEADZONE {
        buttons[BUTTON_LEFT] = true;
    }
    if y > GAMEPAD_STICK_DEADZONE {
        buttons[BUTTON_UP] = true;
    }
    if y < -GAMEPAD_STICK_DEADZONE {
        buttons[BUTTON_DOWN] = true;
    }
}
