use super::{Frame, Renderer};
use image::{DynamicImage, RgbImage};
use std::io::{self, IsTerminal, Write};

pub struct BlockRenderer {
    config: viuer::Config,
    use_alternate_screen: bool,
    screen_active: bool,
}

impl BlockRenderer {
    pub fn new(keep_scrollback: bool) -> Self {
        Self {
            config: viuer::Config {
                absolute_offset: !keep_scrollback,
                use_kitty: false,
                use_iterm: false,
                ..Default::default()
            },
            use_alternate_screen: !keep_scrollback,
            screen_active: false,
        }
    }

    fn frame_to_image(frame: &Frame) -> DynamicImage {
        RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
            .map(DynamicImage::ImageRgb8)
            .unwrap_or_else(|| DynamicImage::ImageRgb8(RgbImage::new(frame.width, frame.height)))
    }

    fn enter_screen(&mut self) {
        if !self.use_alternate_screen || self.screen_active || !io::stdout().is_terminal() {
            return;
        }

        let mut stdout = io::stdout().lock();
        if stdout
            .write_all(b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H")
            .and_then(|_| stdout.flush())
            .is_ok()
        {
            self.screen_active = true;
        }
    }

    fn leave_screen(&mut self) {
        if !self.screen_active {
            return;
        }

        let mut stdout = io::stdout().lock();
        let _ = stdout
            .write_all(b"\x1b[?25h\x1b[?1049l")
            .and_then(|_| stdout.flush());
        self.screen_active = false;
    }
}

impl Renderer for BlockRenderer {
    fn setup(&mut self, _src_width: u32, _src_height: u32) {
        self.enter_screen();
    }

    fn render(&mut self, frame: &Frame, _out: &mut dyn io::Write) -> io::Result<()> {
        let img = Self::frame_to_image(frame);
        if self.config.absolute_offset
            && let Ok((tw, th)) = crossterm::terminal::size()
        {
            let resized = viuer::resize(&img, self.config.width, self.config.height);
            let rows = resized.height().div_ceil(2);
            self.config.x = tw.saturating_sub(resized.width() as u16) / 2;
            self.config.y = (th.saturating_sub(rows as u16) / 2) as i16;
        }
        let _ = viuer::print(&img, &self.config);
        Ok(())
    }

    fn cleanup(&mut self) {
        self.leave_screen();
    }
}

impl Drop for BlockRenderer {
    fn drop(&mut self) {
        self.leave_screen();
    }
}

#[cfg(test)]
mod tests {
    use super::BlockRenderer;

    #[test]
    fn scrollback_mode_disables_alternate_screen() {
        let renderer = BlockRenderer::new(true);

        assert!(!renderer.use_alternate_screen);
    }

    #[test]
    fn block_renderer_disables_graphics_protocols() {
        let renderer = BlockRenderer::new(false);

        assert!(!renderer.config.use_kitty && !renderer.config.use_iterm);
    }
}
