use super::RendererMode;

pub fn select(graphics_supported: bool) -> RendererMode {
    if graphics_supported {
        RendererMode::Graphic
    } else {
        RendererMode::Block
    }
}

#[cfg(test)]
mod tests {
    use super::{RendererMode, select};

    #[test]
    fn selects_graphic_when_terminal_supports_graphics() {
        let mode = select(true);

        assert_eq!(mode, RendererMode::Graphic);
    }

    #[test]
    fn selects_block_when_terminal_does_not_support_graphics() {
        let mode = select(false);

        assert_eq!(mode, RendererMode::Block);
    }
}
