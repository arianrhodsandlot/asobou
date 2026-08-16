use super::{Frame, Renderer, Viewport};
use base64::prelude::{BASE64_STANDARD, Engine as _};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::borrow::Cow;
use std::io::{self, Write};

pub(crate) const KITTY_IMAGE_ID: u32 = 0xa50b_0001;
const KITTY_PLACEMENT_ID: u32 = 1;
const KITTY_CHUNK_SIZE: usize = 4096;

pub struct GraphicRenderer {
    width: Option<u32>,
    height: Option<u32>,
    compression_enabled: bool,
}

/// Expand 3-byte RGB pixels to 4-byte RGBA with opaque alpha. Rio's kitty
/// graphics implementation fails to render RGB (`f=24`) payloads on Windows,
/// so frames are always transmitted as RGBA (`f=32`).
fn rgb_to_rgba(data: &[u8]) -> Vec<u8> {
    debug_assert_eq!(data.len() % 3, 0, "frame data must be RGB triples");
    let mut rgba = Vec::with_capacity(data.len() / 3 * 4);
    for pixel in data.chunks(3) {
        rgba.extend_from_slice(pixel);
        rgba.push(0xff);
    }
    rgba
}

/// Size an image in terminal cells for kitty placement.
///
/// A terminal cell is twice as tall as it is wide, so `cell_height` rows hold
/// `2 * cell_height` pixels. With `cell_width` set the exact cell size is used;
/// otherwise the image keeps its aspect ratio and only the height is bounded.
fn fit_cell_size(
    img_width: u32,
    img_height: u32,
    cell_width: Option<u32>,
    cell_height: u32,
) -> (u32, u32) {
    match cell_width {
        Some(width) => (width, cell_height),
        None if img_height <= 2 * cell_height => (img_width, img_height.div_ceil(2).max(1)),
        None => (img_width * 2 * cell_height / img_height, cell_height),
    }
}

impl GraphicRenderer {
    pub fn new(compression_enabled: bool) -> Self {
        Self {
            width: None,
            height: None,
            compression_enabled,
        }
    }

    fn render_kitty_at(
        &self,
        frame: &Frame,
        out: &mut dyn io::Write,
        terminal_columns: u16,
        available_rows: u16,
    ) -> io::Result<()> {
        if terminal_columns == 0 || available_rows == 0 {
            return Ok(());
        }
        let available_rows = usize::from(available_rows);
        let height = self.height.map_or(available_rows, |height| {
            usize::try_from(height)
                .unwrap_or(usize::MAX)
                .min(available_rows)
        });
        let (columns, rows) = fit_cell_size(frame.width, frame.height, self.width, height as u32);
        let rgba = rgb_to_rgba(&frame.data);
        let payload = if self.compression_enabled {
            let mut encoder = ZlibEncoder::new(
                Vec::with_capacity(rgba.len() / 2),
                Compression::fast(),
            );
            encoder.write_all(&rgba)?;
            Cow::Owned(encoder.finish()?)
        } else {
            Cow::Owned(rgba)
        };
        let encoded = BASE64_STANDARD.encode(payload);
        let chunks = encoded.as_bytes().chunks(KITTY_CHUNK_SIZE);
        let chunk_count = chunks.len();
        let mut command = Vec::with_capacity(encoded.len() + chunk_count * 32 + 256);

        command.extend_from_slice(b"\x1b[?2026h");
        for (index, chunk) in chunks.enumerate() {
            let more = u8::from(index + 1 < chunk_count);
            if index == 0 {
                let compression = if self.compression_enabled { ",o=z" } else { "" };
                write!(
                    command,
                    "\x1b_Ga=t,f=32,t=d{compression},s={},v={},i={},q=2,N=1,m={};",
                    frame.width, frame.height, KITTY_IMAGE_ID, more
                )?;
            } else {
                write!(command, "\x1b_Gm={more},q=2;")?;
            }
            command.extend_from_slice(chunk);
            command.extend_from_slice(b"\x1b\\");
        }
        let col = u32::from(terminal_columns).saturating_sub(columns) / 2 + 1;
        let row = (available_rows as u32).saturating_sub(rows) / 2 + 1;
        write!(
            command,
            "\x1b[{row};{col}H\x1b_Ga=p,i={},p={},c={},r={},C=1,q=2;\x1b\\\x1b[?2026l",
            KITTY_IMAGE_ID, KITTY_PLACEMENT_ID, columns, rows
        )?;

        out.write_all(&command)
    }
}

impl Renderer for GraphicRenderer {
    fn render(
        &mut self,
        frame: &Frame,
        viewport: Viewport,
        out: &mut dyn io::Write,
    ) -> io::Result<()> {
        self.render_kitty_at(frame, out, viewport.columns, viewport.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphicRenderer, KITTY_IMAGE_ID, KITTY_PLACEMENT_ID, fit_cell_size};
    use crate::renderer::Frame;
    use base64::prelude::{BASE64_STANDARD, Engine as _};
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    #[test]
    fn kitty_stream_reuses_image_and_placement_ids() {
        let mut renderer = GraphicRenderer::new(true);
        renderer.width = Some(1);
        renderer.height = Some(1);
        let frame = Frame {
            data: vec![1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3],
            width: 2,
            height: 2,
        };
        let mut output = Vec::new();

        renderer
            .render_kitty_at(&frame, &mut output, 80, 24)
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(&format!("a=t,f=32,t=d,o=z,s=2,v=2,i={KITTY_IMAGE_ID}")));
        assert!(output.contains(&format!(
            "a=p,i={KITTY_IMAGE_ID},p={KITTY_PLACEMENT_ID},c=1,r=1"
        )));
        assert!(!output.contains("a=T"));
        assert!(output.starts_with("\x1b[?2026h"));
        assert!(output.ends_with("\x1b[?2026l"));
    }

    #[test]
    fn kitty_stream_compresses_rgba_payload_with_zlib() {
        let renderer = GraphicRenderer::new(true);
        let frame = test_frame(4, 4);
        let mut expected = Vec::with_capacity(frame.data.len() / 3 * 4);
        for pixel in frame.data.chunks(3) {
            expected.extend_from_slice(pixel);
            expected.push(0xff);
        }
        let mut output = Vec::new();

        renderer
            .render_kitty_at(&frame, &mut output, 80, 24)
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        let payload = output
            .split_once("\x1b_G")
            .unwrap()
            .1
            .split_once(';')
            .unwrap()
            .1
            .split_once("\x1b\\")
            .unwrap()
            .0;
        let compressed = BASE64_STANDARD.decode(payload).unwrap();
        let mut decoded = Vec::new();
        ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut decoded)
            .unwrap();

        assert_eq!(decoded, expected);
    }

    #[test]
    fn kitty_stream_sends_uncompressed_rgba_when_compression_is_disabled() {
        let renderer = GraphicRenderer::new(false);
        let frame = test_frame(4, 4);
        let mut expected = Vec::with_capacity(frame.data.len() / 3 * 4);
        for pixel in frame.data.chunks(3) {
            expected.extend_from_slice(pixel);
            expected.push(0xff);
        }
        let mut output = Vec::new();

        renderer
            .render_kitty_at(&frame, &mut output, 80, 24)
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        let command = output.split_once("\x1b_G").unwrap().1;
        let (control, payload) = command.split_once(';').unwrap();
        let payload = payload.split_once("\x1b\\").unwrap().0;
        let decoded = BASE64_STANDARD.decode(payload).unwrap();

        assert_eq!((control.contains("o=z"), decoded), (false, expected));
    }

    #[test]
    fn kitty_stream_silences_every_image_chunk() {
        let renderer = GraphicRenderer::new(true);
        let frame = test_frame(64, 64);
        let mut output = Vec::new();

        renderer
            .render_kitty_at(&frame, &mut output, 80, 24)
            .unwrap();

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

    #[test]
    fn fit_cell_size_bounds_and_rounds_correctly() {
        assert_eq!(fit_cell_size(200, 200, None, 22), (44, 22));
        assert_eq!(fit_cell_size(200, 100, None, 22), (88, 22));
        assert_eq!(fit_cell_size(80, 40, None, 22), (80, 20));
        assert_eq!(fit_cell_size(80, 39, None, 22), (80, 20));
        assert_eq!(fit_cell_size(200, 200, Some(1), 1), (1, 1));
    }

    #[test]
    fn kitty_stream_reserves_status_rows_below_the_image() {
        let renderer = GraphicRenderer::new(true);
        let frame = Frame {
            data: vec![1; 200 * 200 * 3],
            width: 200,
            height: 200,
        };
        let mut output = Vec::new();

        renderer
            .render_kitty_at(&frame, &mut output, 80, 22)
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\x1b[1;19H"));
        assert!(output.contains("c=44,r=22"));
    }

    fn test_frame(width: u32, height: u32) -> Frame {
        let data = (0..height)
            .flat_map(|y| (0..width).flat_map(move |x| [x as u8, y as u8, (x ^ y) as u8]))
            .collect();
        Frame {
            data,
            width,
            height,
        }
    }
}
