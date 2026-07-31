use super::RendererMode;
use std::io::{self, IsTerminal};

pub fn select(no_alt_screen: bool) -> RendererMode {
    if graphics_supported(no_alt_screen) {
        RendererMode::Graphic
    } else {
        RendererMode::Block
    }
}

fn graphics_supported(no_alt_screen: bool) -> bool {
    !no_alt_screen
        && io::stdout().is_terminal()
        && viuer::get_kitty_support() != viuer::KittySupport::None
}

#[cfg(test)]
mod tests {
    use super::{RendererMode, select};

    #[test]
    fn selects_block_when_alt_screen_is_disabled() {
        assert_eq!(select(true), RendererMode::Block);
    }
}
