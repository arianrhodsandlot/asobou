mod bindings;
mod joypad;
mod physical;
mod state;

pub use bindings::InputBindings;
pub use joypad::*;
pub use physical::{InputSnapshot, PhysicalInput};
use state::InputState;

#[cfg(test)]
mod tests;
