use super::{Frame, Renderer, Viewport};
use std::io::{self, Write};

const RAMP: &[u8] = b" .:-=+*#%@";
const RAMP_LEN: f32 = (RAMP.len() - 1) as f32;

pub struct AsciiRenderer;

impl AsciiRenderer {
    fn render_at(
        &mut self,
        frame: &Frame,
        out: &mut dyn io::Write,
        term_cols: u16,
        term_rows: u16,
    ) -> io::Result<()> {
        let tw = term_cols as u32;
        let th = term_rows as u32;
        if frame.width == 0 || frame.height == 0 || tw == 0 || th == 0 {
            return Ok(());
        }

        let fw = frame.width as f32;
        let fh = frame.height as f32;
        let src_ratio = fw / fh;
        let term_ratio = tw as f32 / (th as f32 * 2.0);

        let (out_w, out_h) = if src_ratio > term_ratio {
            let w = tw;
            let h = ((tw as f32 / src_ratio) / 2.0).round() as u32;
            (w, h.max(1))
        } else {
            let h = th;
            let w = (th as f32 * 2.0 * src_ratio).round() as u32;
            (w.max(1), h)
        };

        let cell_w = fw / out_w as f32;
        let cell_h = fh / out_h as f32;
        let pad_left = (tw.saturating_sub(out_w) / 2) as usize;
        let pad_top = (th.saturating_sub(out_h) / 2) as usize;
        let pad_right = tw as usize - pad_left - out_w as usize;

        let mut buf = Vec::with_capacity((tw * th * 24) as usize);
        write!(buf, "\x1b[H")?;

        for row in 0..th as usize {
            if row < pad_top || row >= pad_top + out_h as usize {
                buf.resize(buf.len() + tw as usize, b' ');
            } else {
                let src_row = row - pad_top;
                buf.resize(buf.len() + pad_left, b' ');
                for col in 0..out_w as usize {
                    let sx = (col as f32 * cell_w) as u32;
                    let sy = (src_row as f32 * cell_h) as u32;
                    let ex = ((col as f32 + 1.0) * cell_w).ceil() as u32;
                    let ey = ((src_row as f32 + 1.0) * cell_h).ceil() as u32;
                    let ex = ex.min(frame.width);
                    let ey = ey.min(frame.height);

                    let (r, g, b, count) = average_region(frame, sx, sy, ex, ey);

                    if count == 0 {
                        buf.push(b' ');
                    } else {
                        let lum = 0.299f32 * r as f32 + 0.587f32 * g as f32 + 0.114f32 * b as f32;
                        let idx = ((lum / 255.0f32) * RAMP_LEN).round() as usize;
                        write!(buf, "\x1b[38;2;{r};{g};{b}m")?;
                        buf.push(RAMP[idx.min(RAMP.len() - 1)]);
                        write!(buf, "\x1b[0m")?;
                    }
                }
                buf.resize(buf.len() + pad_right, b' ');
            }
        }

        out.write_all(&buf)
    }
}

impl Renderer for AsciiRenderer {
    fn render(
        &mut self,
        frame: &Frame,
        viewport: Viewport,
        out: &mut dyn io::Write,
    ) -> io::Result<()> {
        self.render_at(frame, out, viewport.columns, viewport.rows)
    }
}

fn average_region(frame: &Frame, sx: u32, sy: u32, ex: u32, ey: u32) -> (u32, u32, u32, u32) {
    let mut r = 0u64;
    let mut g = 0u64;
    let mut b = 0u64;
    let mut count = 0u64;

    for y in sy..ey {
        for x in sx..ex {
            let i = ((y * frame.width + x) * 3) as usize;
            if i + 2 < frame.data.len() {
                r += frame.data[i] as u64;
                g += frame.data[i + 1] as u64;
                b += frame.data[i + 2] as u64;
                count += 1;
            }
        }
    }

    if count == 0 {
        return (0, 0, 0, 0);
    }

    (
        (r / count) as u32,
        (g / count) as u32,
        (b / count) as u32,
        count as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_of(width: u32, height: u32, color: (u8, u8, u8)) -> Frame {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..width * height {
            data.extend_from_slice(&[color.0, color.1, color.2]);
        }
        Frame {
            data,
            width,
            height,
        }
    }

    fn render_ascii(frame: &Frame, tw: u16, th: u16) -> Vec<u8> {
        let mut renderer = AsciiRenderer;
        let mut buf = Vec::new();
        renderer.render_at(frame, &mut buf, tw, th).unwrap();
        buf
    }

    fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    }

    // Count non-escape bytes; each cell contributes exactly one printable char.
    fn printable_count(bytes: &[u8]) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0x1b {
                i += 1;
                if i < bytes.len() && bytes[i] == b'[' {
                    i += 1;
                    // CSI sequences end at the first final byte (0x40..=0x7e).
                    while i < bytes.len() && bytes[i] < 0x40 {
                        i += 1;
                    }
                    i += 1;
                } else {
                    i += 1;
                }
            } else {
                count += 1;
                i += 1;
            }
        }
        count
    }

    #[test]
    fn average_region_averages_all_pixels() {
        let mut data = vec![0u8; 2 * 2 * 3];
        data[0..3].copy_from_slice(&[255, 0, 0]);
        data[3..6].copy_from_slice(&[0, 255, 0]);
        data[6..9].copy_from_slice(&[0, 0, 255]);
        data[9..12].copy_from_slice(&[255, 255, 255]);
        let frame = Frame {
            data,
            width: 2,
            height: 2,
        };

        let (r, g, b, count) = average_region(&frame, 0, 0, 2, 2);
        assert_eq!((r, g, b), (127, 127, 127));
        assert_eq!(count, 4);
    }

    #[test]
    fn average_region_ignores_pixels_outside_the_frame_data() {
        let frame = Frame {
            data: vec![10, 20, 30, 40, 50, 60, 70, 80, 90],
            width: 3,
            height: 1,
        };
        let (r, g, b, count) = average_region(&frame, 0, 0, 5, 1);
        assert_eq!((r, g, b), (40, 50, 60));
        assert_eq!(count, 3);
    }

    #[test]
    fn average_region_empty_region_returns_zero() {
        let frame = Frame {
            data: vec![0, 0, 0],
            width: 1,
            height: 1,
        };
        let (r, g, b, count) = average_region(&frame, 1, 1, 1, 1);
        assert_eq!((r, g, b, count), (0, 0, 0, 0));
    }

    #[test]
    fn single_white_pixel_renders_brightest_ramp_char() {
        let out = render_ascii(&frame_of(1, 1, (255, 255, 255)), 1, 1);
        assert_eq!(out, b"\x1b[H\x1b[38;2;255;255;255m@\x1b[0m");
    }

    #[test]
    fn single_black_pixel_renders_darkest_ramp_char() {
        let out = render_ascii(&frame_of(1, 1, (0, 0, 0)), 1, 1);
        assert_eq!(out, b"\x1b[H\x1b[38;2;0;0;0m \x1b[0m");
    }

    #[test]
    fn brighter_frames_use_brighter_ramp_chars() {
        let dark = render_ascii(&frame_of(1, 1, (0, 0, 0)), 1, 1);
        let mid = render_ascii(&frame_of(1, 1, (128, 128, 128)), 1, 1);
        let bright = render_ascii(&frame_of(1, 1, (255, 255, 255)), 1, 1);
        assert_eq!(&dark[..], b"\x1b[H\x1b[38;2;0;0;0m \x1b[0m");
        assert_eq!(&mid[..], b"\x1b[H\x1b[38;2;128;128;128m+\x1b[0m");
        assert_eq!(&bright[..], b"\x1b[H\x1b[38;2;255;255;255m@\x1b[0m");
    }

    #[test]
    fn render_fills_terminal_when_ratios_match() {
        // 20:12 frame in an 80x24 terminal maps to exactly 80x24 cells.
        let out = render_ascii(&frame_of(20, 12, (10, 20, 30)), 80, 24);
        assert!(out.starts_with(b"\x1b[H"));
        assert_eq!(count_occurrences(&out, b"\x1b[38;2;"), 80 * 24);
        assert_eq!(count_occurrences(&out, b"\x1b[0m"), 80 * 24);
        assert_eq!(printable_count(&out), 80 * 24);
    }

    #[test]
    fn render_pads_small_frame_equally_on_both_sides() {
        // 1:1 frame fits 48x24 cells in an 80x24 terminal.
        let out = render_ascii(&frame_of(2, 2, (0, 0, 0)), 80, 24);
        assert_eq!(count_occurrences(&out, b"\x1b[38;2;"), 48 * 24);
        assert_eq!(count_occurrences(&out, b"\x1b[0m"), 48 * 24);
        assert_eq!(printable_count(&out), 80 * 24);
    }

    #[test]
    fn render_letterboxes_wide_frame_top_and_bottom() {
        // 2:1 frame in an 80x24 terminal maps to 80x20 cells, 2 pad rows per side.
        let out = render_ascii(&frame_of(20, 10, (0, 0, 0)), 80, 24);
        assert_eq!(count_occurrences(&out, b"\x1b[38;2;"), 80 * 20);
        assert_eq!(count_occurrences(&out, b"\x1b[0m"), 80 * 20);
        assert_eq!(printable_count(&out), 80 * 24);
    }
}
