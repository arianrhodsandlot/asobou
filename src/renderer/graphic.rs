use super::{Frame, Renderer};
use base64::prelude::{BASE64_STANDARD, Engine as _};
use image::{DynamicImage, RgbImage};
use std::io::{self, IsTerminal, Write};

const ENTER_SCREEN: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H";
const LEAVE_SCREEN: &[u8] = b"\x1b[?25h\x1b[?1049l";
const KITTY_IMAGE_ID: u32 = 0xa50b_0001;
const KITTY_PLACEMENT_ID: u32 = 1;
const KITTY_CHUNK_SIZE: usize = 4096;

pub struct GraphicRenderer {
    config: viuer::Config,
    screen_active: bool,
}

impl GraphicRenderer {
    pub fn new() -> Self {
        Self {
            config: viuer::Config::default(),
            screen_active: false,
        }
    }

    fn frame_to_image(frame: &Frame) -> DynamicImage {
        RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
            .map(DynamicImage::ImageRgb8)
            .unwrap_or_else(|| DynamicImage::ImageRgb8(RgbImage::new(frame.width, frame.height)))
    }

    fn enter_screen(&mut self) {
        if self.screen_active || !io::stdout().is_terminal() {
            return;
        }

        let mut stdout = io::stdout().lock();
        if stdout
            .write_all(ENTER_SCREEN)
            .and_then(|_| stdout.flush())
            .is_ok()
        {
            self.screen_active = true;
        }
    }

    fn render_kitty(&self, img: &DynamicImage, out: &mut dyn io::Write) -> io::Result<()> {
        let display_size = viuer::resize(img, self.config.width, self.config.height);
        let columns = display_size.width();
        let rows = display_size.height().div_ceil(2);
        let pixels = img.to_rgb8();
        let encoded = BASE64_STANDARD.encode(pixels.as_raw());
        let chunks = encoded.as_bytes().chunks(KITTY_CHUNK_SIZE);
        let chunk_count = chunks.len();
        let mut command = Vec::with_capacity(encoded.len() + chunk_count * 32 + 256);

        command.extend_from_slice(b"\x1b[?2026h");
        for (index, chunk) in chunks.enumerate() {
            let more = u8::from(index + 1 < chunk_count);
            if index == 0 {
                write!(
                    command,
                    "\x1b_Ga=t,f=24,t=d,s={},v={},i={},q=2,N=1,m={};",
                    pixels.width(),
                    pixels.height(),
                    KITTY_IMAGE_ID,
                    more
                )?;
            } else {
                write!(command, "\x1b_Gm={more},q=2;")?;
            }
            command.extend_from_slice(chunk);
            command.extend_from_slice(b"\x1b\\");
        }
        let (tw, th) = crossterm::terminal::size().unwrap_or((80, 24));
        let col = (tw as u32).saturating_sub(columns) / 2 + 1;
        let row = (th as u32).saturating_sub(rows) / 2 + 1;
        write!(
            command,
            "\x1b[{row};{col}H\x1b_Ga=p,i={},p={},c={},r={},C=1,q=2;\x1b\\\x1b[?2026l",
            KITTY_IMAGE_ID, KITTY_PLACEMENT_ID, columns, rows
        )?;

        out.write_all(&command)
    }

    fn leave_screen(&mut self) {
        if !self.screen_active {
            return;
        }

        let mut stdout = io::stdout().lock();
        let _ = write!(stdout, "\x1b_Ga=d,d=I,i={},q=2;\x1b\\", KITTY_IMAGE_ID);
        let _ = stdout.write_all(LEAVE_SCREEN).and_then(|_| stdout.flush());
        self.screen_active = false;
    }
}

impl Renderer for GraphicRenderer {
    fn setup(&mut self, _src_width: u32, _src_height: u32) {
        self.enter_screen();
    }

    fn render(&mut self, frame: &Frame, out: &mut dyn io::Write) -> io::Result<()> {
        self.render_kitty(&Self::frame_to_image(frame), out)
    }

    fn cleanup(&mut self) {
        self.leave_screen();
    }
}

impl Drop for GraphicRenderer {
    fn drop(&mut self) {
        self.leave_screen();
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphicRenderer, KITTY_IMAGE_ID, KITTY_PLACEMENT_ID};
    use image::{DynamicImage, Rgb, RgbImage};

    #[test]
    fn kitty_stream_reuses_image_and_placement_ids() {
        let mut renderer = GraphicRenderer::new();
        renderer.config.width = Some(1);
        renderer.config.height = Some(1);
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 2, Rgb([1, 2, 3])));
        let mut output = Vec::new();

        renderer.render_kitty(&img, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(&format!("a=t,f=24,t=d,s=2,v=2,i={KITTY_IMAGE_ID}")));
        assert!(output.contains(&format!(
            "a=p,i={KITTY_IMAGE_ID},p={KITTY_PLACEMENT_ID},c=1,r=1"
        )));
        assert!(!output.contains("a=T"));
        assert!(output.starts_with("\x1b[?2026h"));
        assert!(output.ends_with("\x1b[?2026l"));
    }

    #[test]
    fn kitty_stream_silences_every_image_chunk() {
        let renderer = GraphicRenderer::new();
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |x, y| {
            Rgb([x as u8, y as u8, (x ^ y) as u8])
        }));
        let mut output = Vec::new();

        renderer.render_kitty(&img, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        let continuation_chunks = output
            .match_indices("\x1b_Gm=")
            .map(|(index, _)| &output[index..])
            .collect::<Vec<_>>();
        assert!(continuation_chunks.len() > 1);
        assert!(continuation_chunks.iter().all(
            |chunk| chunk.starts_with("\x1b_Gm=1,q=2;") || chunk.starts_with("\x1b_Gm=0,q=2;")
        ));
    }

}
