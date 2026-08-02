pub mod ascii;
pub mod auto;
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
    no_alt_screen: bool,
) -> io::Result<Box<dyn Renderer>> {
    let mode = match mode {
        RendererMode::Auto => auto::select(no_alt_screen),
        explicit => explicit,
    };
    match mode {
        RendererMode::Debug => {
            let stem = rom_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            Ok(Box::new(debug::DebugRenderer::new(stem)))
        }
        RendererMode::Graphic => Ok(Box::new(graphic::GraphicRenderer::new())),
        RendererMode::Block => Ok(Box::new(block::BlockRenderer::new(no_alt_screen))),
        RendererMode::Ascii => Ok(Box::new(ascii::AsciiRenderer::new(no_alt_screen))),
        RendererMode::Auto => unreachable!(),
    }
}
