use super::viuer::ViuRenderer;
use super::{Frame, Renderer};
use std::io;

pub struct AsciiRenderer {
    inner: ViuRenderer,
}

impl AsciiRenderer {
    pub fn new(keep_scrollback: bool) -> Self {
        Self {
            inner: ViuRenderer::halfblock(keep_scrollback),
        }
    }
}

impl Renderer for AsciiRenderer {
    fn setup(&mut self, src_width: u32, src_height: u32) {
        self.inner.setup(src_width, src_height);
    }

    fn render(&mut self, frame: &Frame, out: &mut dyn io::Write) -> io::Result<()> {
        self.inner.render(frame, out)
    }

    fn cleanup(&mut self) {
        self.inner.cleanup();
    }
}
