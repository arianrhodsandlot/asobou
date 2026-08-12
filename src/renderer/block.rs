use std::io::{self, Write};

use image::{RgbImage, imageops::FilterType};

use super::{Frame, Renderer, Viewport};

pub struct BlockRenderer;

impl BlockRenderer {
    fn render_frame(
        frame: &Frame,
        term_cols: u32,
        term_rows: u32,
        out: &mut dyn io::Write,
    ) -> io::Result<()> {
        if frame.width == 0 || frame.height == 0 || term_cols == 0 || term_rows == 0 {
            return Ok(());
        }

        let frame_width = frame.width as f32;
        let frame_height = frame.height as f32;
        let source_ratio = frame_width / frame_height;
        let terminal_ratio = term_cols as f32 / (term_rows as f32 * 2.0);

        let (output_width, output_height) = if source_ratio > terminal_ratio {
            let width = term_cols;
            let height = ((term_cols as f32 / source_ratio) / 2.0).round() as u32;
            (width, height.max(1))
        } else {
            let height = term_rows;
            let width = (term_rows as f32 * 2.0 * source_ratio).round() as u32;
            (width.max(1), height)
        };

        let pad_left = (term_cols.saturating_sub(output_width) / 2) as usize;
        let pad_top = (term_rows.saturating_sub(output_height) / 2) as usize;
        let pad_right = term_cols as usize - pad_left - output_width as usize;
        let image = RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
            .unwrap_or_else(|| RgbImage::new(frame.width, frame.height));
        let resized = image::imageops::resize(
            &image,
            output_width,
            output_height * 2,
            FilterType::CatmullRom,
        );

        let mut buffer = Vec::with_capacity((term_cols * term_rows * 40) as usize);
        write!(buffer, "\x1b[H\x1b[0m")?;

        for row in 0..term_rows as usize {
            if row < pad_top || row >= pad_top + output_height as usize {
                buffer.extend(std::iter::repeat_n(b' ', term_cols as usize));
                continue;
            }

            buffer.extend(std::iter::repeat_n(b' ', pad_left));
            let image_row = row - pad_top;
            for column in 0..output_width as usize {
                let [top_red, top_green, top_blue] =
                    resized.get_pixel(column as u32, image_row as u32 * 2).0;
                let [bottom_red, bottom_green, bottom_blue] =
                    resized.get_pixel(column as u32, image_row as u32 * 2 + 1).0;
                write!(
                    buffer,
                    "\x1b[38;2;{top_red};{top_green};{top_blue}m\
                     \x1b[48;2;{bottom_red};{bottom_green};{bottom_blue}m▀"
                )?;
            }
            write!(buffer, "\x1b[0m")?;
            buffer.extend(std::iter::repeat_n(b' ', pad_right));
        }

        out.write_all(&buffer)
    }
}

impl Renderer for BlockRenderer {
    fn render(
        &mut self,
        frame: &Frame,
        viewport: Viewport,
        out: &mut dyn io::Write,
    ) -> io::Result<()> {
        Self::render_frame(
            frame,
            u32::from(viewport.columns),
            u32::from(viewport.rows),
            out,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockRenderer, Frame};

    #[test]
    fn frame_repaint_starts_at_terminal_home() {
        let frame = Frame {
            data: vec![255; 12],
            width: 2,
            height: 2,
        };
        let mut output = Vec::new();

        BlockRenderer::render_frame(&frame, 2, 2, &mut output).unwrap();

        assert!(output.starts_with(b"\x1b[H"));
    }

    #[test]
    fn frame_repaint_does_not_emit_newlines() {
        let frame = Frame {
            data: vec![255; 12],
            width: 2,
            height: 2,
        };
        let mut output = Vec::new();

        BlockRenderer::render_frame(&frame, 2, 2, &mut output).unwrap();

        assert!(!output.contains(&b'\n'));
    }
}
