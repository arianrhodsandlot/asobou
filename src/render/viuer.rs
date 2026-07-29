use super::Frame;
use super::Renderer;
use base64::prelude::{BASE64_STANDARD, Engine as _};
use image::{DynamicImage, RgbImage};
use std::ffi::OsStr;
use std::io::{self, IsTerminal, Write};

const ENTER_SCREEN: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H";
const LEAVE_SCREEN: &[u8] = b"\x1b[?25h\x1b[?1049l";
const KITTY_IMAGE_ID: u32 = 0xa50b_0001;
const KITTY_PLACEMENT_ID: u32 = 1;
const KITTY_CHUNK_SIZE: usize = 4096;

pub struct ViuRenderer {
    config: viuer::Config,
    use_alternate_screen: bool,
    kitty_streaming: bool,
    screen_active: bool,
}

impl ViuRenderer {
    pub fn new(keep_scrollback: bool) -> Self {
        Self {
            config: viuer::Config {
                absolute_offset: !keep_scrollback,
                use_kitty: false,
                use_iterm: false,
                ..Default::default()
            },
            use_alternate_screen: !keep_scrollback,
            kitty_streaming: false,
            screen_active: false,
        }
    }

    pub fn halfblock(keep_scrollback: bool) -> Self {
        Self {
            config: viuer::Config {
                absolute_offset: !keep_scrollback,
                use_kitty: false,
                use_iterm: false,
                ..Default::default()
            },
            use_alternate_screen: !keep_scrollback,
            kitty_streaming: false,
            screen_active: false,
        }
    }

    fn frame_to_image(frame: &Frame) -> DynamicImage {
        RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
            .map(DynamicImage::ImageRgb8)
            .unwrap_or_else(|| DynamicImage::ImageRgb8(RgbImage::new(frame.width, frame.height)))
    }

    fn enter_screen(&mut self) {
        if !self.use_alternate_screen || self.screen_active || !io::stdout().is_terminal() {
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

    fn detect_kitty(&mut self) {
        self.kitty_streaming = self.use_alternate_screen
            && io::stdout().is_terminal()
            && kitty_streaming_supported(
                std::env::var_os("KITTY_WINDOW_ID").as_deref(),
                std::env::var_os("TERM").as_deref(),
                std::env::var_os("TERM_PROGRAM").as_deref(),
                std::env::var_os("TERM_PROGRAM_VERSION").as_deref(),
            );
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
        if self.kitty_streaming {
            let _ = write!(stdout, "\x1b_Ga=d,d=I,i={},q=2;\x1b\\", KITTY_IMAGE_ID);
        }
        let _ = stdout.write_all(LEAVE_SCREEN).and_then(|_| stdout.flush());
        self.kitty_streaming = false;
        self.screen_active = false;
    }
}

fn kitty_streaming_supported(
    kitty_window_id: Option<&OsStr>,
    term: Option<&OsStr>,
    term_program: Option<&OsStr>,
    term_program_version: Option<&OsStr>,
) -> bool {
    if kitty_window_id.is_some_and(|value| !value.is_empty()) {
        return true;
    }

    if term
        .and_then(OsStr::to_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("xterm-kitty"))
    {
        return true;
    }

    let Some(term_program) = term_program.and_then(OsStr::to_str) else {
        return false;
    };
    match term_program.to_ascii_lowercase().as_str() {
        "ghostty" | "wezterm" | "zed" => true,
        "iterm.app" | "iterm2" => term_program_version
            .and_then(OsStr::to_str)
            .is_some_and(iterm_supports_kitty_graphics),
        _ => false,
    }
}

fn iterm_supports_kitty_graphics(version: &str) -> bool {
    let mut components = version.split('.').map(|component| {
        component
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or_default()
    });
    (
        components.next().unwrap_or_default(),
        components.next().unwrap_or_default(),
        components.next().unwrap_or_default(),
    ) >= (3, 5, 5)
}

impl Renderer for ViuRenderer {
    fn setup(&mut self, _src_width: u32, _src_height: u32) {
        self.detect_kitty();
        self.enter_screen();
    }

    fn render(&mut self, frame: &Frame, out: &mut dyn io::Write) -> io::Result<()> {
        let img = Self::frame_to_image(frame);
        if self.kitty_streaming {
            self.render_kitty(&img, out)
        } else {
            if self.config.absolute_offset {
                if let Ok((tw, th)) = crossterm::terminal::size() {
                    let resized = viuer::resize(&img, self.config.width, self.config.height);
                    let rows = resized.height().div_ceil(2);
                    self.config.x = tw.saturating_sub(resized.width() as u16) / 2;
                    self.config.y = (th.saturating_sub(rows as u16) / 2) as i16;
                }
            }
            let _ = viuer::print(&img, &self.config);
            Ok(())
        }
    }

    fn cleanup(&mut self) {
        self.leave_screen();
    }
}

impl Drop for ViuRenderer {
    fn drop(&mut self) {
        self.leave_screen();
    }
}

#[cfg(test)]
mod tests {
    use super::{KITTY_IMAGE_ID, KITTY_PLACEMENT_ID, ViuRenderer, kitty_streaming_supported};
    use image::{DynamicImage, Rgb, RgbImage};
    use std::ffi::OsStr;

    #[test]
    fn kitty_stream_reuses_image_and_placement_ids() {
        let mut renderer = ViuRenderer::new(false);
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
        let renderer = ViuRenderer::new(false);
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
            |chunk| chunk.starts_with("\x1b_Gm=1,q=2;")
                || chunk.starts_with("\x1b_Gm=0,q=2;")
        ));
    }

    #[test]
    fn scrollback_mode_disables_streaming_protocols() {
        let renderer = ViuRenderer::new(true);

        assert!(!renderer.use_alternate_screen);
        assert!(!renderer.kitty_streaming);
        assert!(!renderer.config.absolute_offset);
    }

    #[test]
    fn automatic_renderer_disables_viuer_protocol_backends() {
        let renderer = ViuRenderer::new(false);

        assert!(!renderer.config.use_kitty && !renderer.config.use_iterm);
    }

    #[test]
    fn kitty_window_id_enables_streaming_without_a_probe() {
        let supported = kitty_streaming_supported(
            Some(OsStr::new("1")),
            Some(OsStr::new("xterm-256color")),
            None,
            None,
        );

        assert!(supported);
    }

    #[test]
    fn generic_terminal_does_not_enable_kitty_streaming() {
        let supported =
            kitty_streaming_supported(None, Some(OsStr::new("xterm-256color")), None, None);

        assert!(!supported);
    }

    #[test]
    fn zed_enables_kitty_streaming_without_a_probe() {
        let supported = kitty_streaming_supported(
            None,
            Some(OsStr::new("xterm-256color")),
            Some(OsStr::new("zed")),
            None,
        );

        assert!(supported);
    }

    #[test]
    fn recent_iterm_enables_kitty_streaming_without_a_probe() {
        let supported = kitty_streaming_supported(
            None,
            Some(OsStr::new("xterm-256color")),
            Some(OsStr::new("iTerm.app")),
            Some(OsStr::new("3.6.5")),
        );

        assert!(supported);
    }

    #[test]
    fn older_iterm_falls_back_without_requesting_file_display() {
        let supported = kitty_streaming_supported(
            None,
            Some(OsStr::new("xterm-256color")),
            Some(OsStr::new("iTerm.app")),
            Some(OsStr::new("3.4.23")),
        );

        assert!(!supported);
    }
}
