pub mod auto;
pub mod ascii;
pub mod block;
pub mod debug;
pub mod graphic;

use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum RendererMode {
    Auto,
    Graphic,
    Block,
    Ascii,
    Debug,
}

pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub trait Renderer: Send {
    fn setup(&mut self, src_width: u32, src_height: u32);
    fn render(&mut self, frame: &Frame, out: &mut dyn io::Write) -> io::Result<()>;
    fn cleanup(&mut self);
}

pub fn create(
    mode: RendererMode,
    rom_path: &std::path::Path,
    keep_scrollback: bool,
) -> io::Result<Box<dyn Renderer>> {
    match resolve_mode(mode, graphic::supported(keep_scrollback))? {
        RendererMode::Debug => {
            let stem = rom_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            Ok(Box::new(debug::DebugRenderer::new(stem)))
        }
        RendererMode::Graphic => Ok(Box::new(graphic::GraphicRenderer::new())),
        RendererMode::Block => Ok(Box::new(block::BlockRenderer::new(keep_scrollback))),
        RendererMode::Ascii => Ok(Box::new(ascii::AsciiRenderer::new(keep_scrollback))),
        RendererMode::Auto => unreachable!(),
    }
}

fn resolve_mode(mode: RendererMode, graphics_supported: bool) -> io::Result<RendererMode> {
    match (mode, graphics_supported) {
        (RendererMode::Auto, supported) => Ok(auto::select(supported)),
        (RendererMode::Graphic, false) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "graphic renderer is not supported by the current terminal",
        )),
        (mode, _) => Ok(mode),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{RendererMode, resolve_mode};

    #[test]
    fn explicit_graphic_rejects_unsupported_terminal() {
        let error = resolve_mode(RendererMode::Graphic, false).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }
}
