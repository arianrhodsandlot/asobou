pub mod ascii;
pub mod debug;
pub mod viuer;

use std::io;

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
    name: &str,
    rom_path: &std::path::Path,
    keep_scrollback: bool,
) -> Box<dyn Renderer> {
    match name {
        "debug" => {
            let stem = rom_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            Box::new(debug::DebugRenderer::new(stem))
        }
        "ascii" => Box::new(ascii::AsciiRenderer::new(keep_scrollback)),
        _ => Box::new(viuer::ViuRenderer::new(keep_scrollback)),
    }
}
