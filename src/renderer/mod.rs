pub mod ascii;
pub mod block;
pub mod graphic;

use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum RendererMode {
    Auto,
    Graphic,
    Block,
    Ascii,
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
