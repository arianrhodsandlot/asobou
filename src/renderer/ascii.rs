use super::{Frame, Renderer};
use std::io::{self, IsTerminal, Write};

const RAMP: &[u8] = b" .:-=+*#%@";
const RAMP_LEN: f32 = (RAMP.len() - 1) as f32;

pub struct AsciiRenderer {
    use_alternate_screen: bool,
    screen_active: bool,
}

impl AsciiRenderer {
    pub fn new(no_alt_screen: bool) -> Self {
        Self {
            use_alternate_screen: !no_alt_screen,
            screen_active: false,
        }
    }

    fn enter_screen(&mut self) {
        if !self.use_alternate_screen || self.screen_active || !io::stdout().is_terminal() {
            return;
        }
        let mut stdout = io::stdout().lock();
        if stdout
            .write_all(b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H")
            .and_then(|_| stdout.flush())
            .is_ok()
        {
            self.screen_active = true;
        }
    }

    fn leave_screen(&mut self) {
        if !self.screen_active {
            return;
        }
        let mut stdout = io::stdout().lock();
        let _ = stdout
            .write_all(b"\x1b[?25h\x1b[?1049l")
            .and_then(|_| stdout.flush());
        self.screen_active = false;
    }
}

impl Renderer for AsciiRenderer {
    fn setup(&mut self, _src_width: u32, _src_height: u32) {
        self.enter_screen();
    }

    fn render(&mut self, frame: &Frame, out: &mut dyn io::Write) -> io::Result<()> {
        let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let tw = term_cols as u32;
        let th = term_rows as u32;

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
                for _ in 0..tw as usize {
                    buf.push(b' ');
                }
            } else {
                let src_row = row - pad_top;
                for _ in 0..pad_left {
                    buf.push(b' ');
                }
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
                for _ in 0..pad_right {
                    buf.push(b' ');
                }
            }
        }

        out.write_all(&buf)?;
        out.flush()
    }

    fn cleanup(&mut self) {
        self.leave_screen();
    }
}

impl Drop for AsciiRenderer {
    fn drop(&mut self) {
        self.leave_screen();
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
