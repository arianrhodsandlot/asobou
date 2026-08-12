pub mod ascii;
pub mod block;
pub mod graphic;

use serde::Deserialize;
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum RendererMode {
    Auto,
    Graphic,
    Block,
    Ascii,
}

impl RendererMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Graphic => "graphic",
            Self::Block => "block",
            Self::Ascii => "ascii",
        }
    }
}

pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Viewport {
    pub columns: u16,
    pub rows: u16,
}

pub trait Renderer: Send {
    fn render(
        &mut self,
        frame: &Frame,
        viewport: Viewport,
        out: &mut dyn io::Write,
    ) -> io::Result<()>;
}
