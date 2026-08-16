use std::ffi::OsStr;
use std::fmt;
use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event, KeyCode, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};

use crate::renderer::{Frame, Renderer, RendererMode, Viewport};

const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const KITTY_QUERY: &[u8] = b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\";
const ENTER_SCREEN: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H";
const LEAVE_SCREEN: &[u8] = b"\x1b[?25h\x1b[?1049l";
const TERMINAL_INPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(50);
const TERMINAL_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(5);
const STATUS_MESSAGE_DURATION: Duration = Duration::from_secs(2);
const RESET_STYLE: &str = "\x1b[0m";
const DIM_STYLE: &str = "\x1b[2m";

pub struct Settings {
    pub renderer: RendererMode,
    pub primary_screen: bool,
    pub status_lines: Vec<String>,
}

#[derive(Clone, Copy)]
pub struct InputCapabilities {
    pub release_events_supported: bool,
    pub ghostty: bool,
    pub synthetic_releases: bool,
}

struct LatestFrameMailbox {
    state: Mutex<LatestFrameState>,
    ready: Condvar,
}

#[derive(Default)]
struct LatestFrameState {
    frame: Option<Arc<Frame>>,
    closed: bool,
    waiting: bool,
}

impl LatestFrameMailbox {
    fn new() -> Self {
        Self {
            state: Mutex::new(LatestFrameState {
                waiting: true,
                ..LatestFrameState::default()
            }),
            ready: Condvar::new(),
        }
    }

    fn publish(&self, frame: Arc<Frame>) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return false;
        }
        state.frame = Some(frame);
        state.waiting = false;
        self.ready.notify_one();
        true
    }

    fn receive(&self) -> Option<Arc<Frame>> {
        let mut state = self.state.lock().unwrap();
        while state.frame.is_none() && !state.closed {
            state.waiting = true;
            state = self.ready.wait(state).unwrap();
        }
        if state.closed {
            None
        } else {
            state.frame.take()
        }
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        state.frame = None;
        state.waiting = false;
        self.ready.notify_all();
    }

    fn wants_frame(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.waiting && !state.closed
    }
}

struct Lifecycle {
    raw_mode: bool,
    focus: bool,
    keyboard_flags: bool,
    alternate_screen: bool,
    graphic: bool,
    preserve_content: bool,
}

impl Lifecycle {
    fn restore(&mut self) -> io::Result<()> {
        let mut first_error = None;
        let mut stdout = io::stdout();
        let drain_input = self.keyboard_flags;
        if self.graphic && !self.preserve_content {
            self.graphic = false;
            record_error(
                &mut first_error,
                write!(
                    stdout,
                    "\x1b_Ga=d,d=I,i={},q=2;\x1b\\",
                    crate::renderer::graphic::KITTY_IMAGE_ID
                ),
            );
        }
        if self.alternate_screen {
            self.alternate_screen = false;
            record_error(&mut first_error, stdout.write_all(LEAVE_SCREEN));
        }
        if self.keyboard_flags {
            self.keyboard_flags = false;
            record_error(
                &mut first_error,
                crossterm::execute!(stdout, PopKeyboardEnhancementFlags),
            );
        }
        if self.focus {
            self.focus = false;
            record_error(
                &mut first_error,
                crossterm::execute!(stdout, DisableFocusChange),
            );
        }
        record_error(&mut first_error, stdout.flush());
        if drain_input {
            drain_pending_terminal_events();
        }
        if self.raw_mode {
            self.raw_mode = false;
            record_error(&mut first_error, crossterm::terminal::disable_raw_mode());
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn record_error(first: &mut Option<io::Error>, result: io::Result<()>) {
    if let Err(error) = result
        && first.is_none()
    {
        *first = Some(error);
    }
}

type RenderThread = thread::JoinHandle<io::Result<()>>;

pub struct TerminalSession {
    mailbox: Arc<LatestFrameMailbox>,
    status_messages: Arc<Mutex<Option<(String, Instant)>>>,
    thread: Option<RenderThread>,
    lifecycle: Lifecycle,
    release_events_supported: bool,
    ghostty: bool,
    synthetic_releases: bool,
}

impl TerminalSession {
    pub fn start(settings: Settings) -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut lifecycle = Lifecycle {
            raw_mode: true,
            focus: false,
            keyboard_flags: false,
            alternate_screen: false,
            graphic: false,
            preserve_content: settings.primary_screen,
        };
        match Self::start_active(settings, &mut lifecycle) {
            Ok(session) => Ok(session),
            Err(error) => {
                let _ = lifecycle.restore();
                Err(error)
            }
        }
    }

    fn start_active(settings: Settings, lifecycle: &mut Lifecycle) -> io::Result<Self> {
        let stdout_is_terminal = io::stdout().is_terminal();
        let selected = select_renderer(
            settings.renderer,
            settings.primary_screen,
            stdout_is_terminal,
        );
        let ghostty = is_ghostty();
        let renderer: Box<dyn Renderer> = match selected {
            RendererMode::Graphic => {
                Box::new(crate::renderer::graphic::GraphicRenderer::new(!ghostty))
            }
            RendererMode::Block => Box::new(crate::renderer::block::BlockRenderer),
            RendererMode::Ascii => Box::new(crate::renderer::ascii::AsciiRenderer),
            RendererMode::Auto => unreachable!(),
        };

        let mut stdout = io::stdout();
        lifecycle.focus = crossterm::execute!(stdout, EnableFocusChange).is_ok();
        let keyboard_enhancement_supported =
            ghostty || crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        lifecycle.keyboard_flags = keyboard_enhancement_supported
            && crossterm::execute!(stdout, PushKeyboardEnhancementFlags(flags)).is_ok();
        if !settings.primary_screen && stdout_is_terminal {
            stdout.write_all(ENTER_SCREEN)?;
            stdout.flush()?;
            lifecycle.alternate_screen = true;
        }
        lifecycle.graphic = selected == RendererMode::Graphic && stdout_is_terminal;

        let mailbox = Arc::new(LatestFrameMailbox::new());
        let render_mailbox = Arc::clone(&mailbox);
        let status_messages = Arc::new(Mutex::new(None::<(String, Instant)>));
        let render_messages = Arc::clone(&status_messages);
        let status_lines = settings.status_lines;
        let reserved_rows = status_lines.len();
        let thread = thread::Builder::new()
            .name("asobou-render".into())
            .spawn(move || {
                render_frames(
                    renderer,
                    render_mailbox,
                    render_messages,
                    status_lines,
                    reserved_rows,
                )
            })?;
        Ok(Self {
            mailbox,
            status_messages,
            thread: Some(thread),
            release_events_supported: cfg!(windows) || lifecycle.keyboard_flags,
            ghostty,
            synthetic_releases: cfg!(windows) && is_rio(),
            lifecycle: Lifecycle {
                raw_mode: lifecycle.raw_mode,
                focus: lifecycle.focus,
                keyboard_flags: lifecycle.keyboard_flags,
                alternate_screen: lifecycle.alternate_screen,
                graphic: lifecycle.graphic,
                preserve_content: lifecycle.preserve_content,
            },
        })
    }

    pub fn input_capabilities(&self) -> InputCapabilities {
        InputCapabilities {
            release_events_supported: self.release_events_supported,
            ghostty: self.ghostty,
            synthetic_releases: self.synthetic_releases,
        }
    }

    pub fn next_event(&self) -> io::Result<Option<Event>> {
        if event::poll(Duration::ZERO)? {
            event::read().map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn wants_frame(&self) -> bool {
        self.mailbox.wants_frame()
    }

    pub fn present(&self, frame: Arc<Frame>) -> bool {
        self.mailbox.publish(frame)
    }

    pub fn show_message(&self, text: &str) {
        if let Ok(mut message) = self.status_messages.lock() {
            *message = Some((text.to_owned(), Instant::now()));
        }
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.finish_inner()
    }

    fn finish_inner(&mut self) -> io::Result<()> {
        self.mailbox.close();
        let render_result = match self.thread.take() {
            Some(thread) => match thread.join() {
                Ok(result) => result,
                Err(_) => Err(io::Error::other("renderer thread panicked")),
            },
            None => Ok(()),
        };
        let cleanup_result = self.lifecycle.restore();
        render_result.and(cleanup_result)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.finish_inner();
    }
}

fn select_renderer(
    mode: RendererMode,
    primary_screen: bool,
    stdout_is_terminal: bool,
) -> RendererMode {
    match mode {
        RendererMode::Auto if primary_screen || !stdout_is_terminal => RendererMode::Block,
        RendererMode::Auto => {
            let mut stdout = io::stdout();
            if probe_kitty_support(&mut stdout) {
                RendererMode::Graphic
            } else {
                RendererMode::Block
            }
        }
        explicit => explicit,
    }
}

fn render_frames(
    mut renderer: Box<dyn Renderer>,
    mailbox: Arc<LatestFrameMailbox>,
    status_messages: Arc<Mutex<Option<(String, Instant)>>>,
    status_lines: Vec<String>,
    reserved_rows: usize,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let result = (|| {
        while let Some(frame) = mailbox.receive() {
            let (columns, rows) = crossterm::terminal::size()?;
            let viewport = Viewport {
                columns,
                rows: rows.saturating_sub(u16::try_from(reserved_rows).unwrap_or(u16::MAX)),
            };
            renderer.render(&frame, viewport, &mut stdout)?;
            let message = current_message(&status_messages);
            write_status_lines(
                &mut stdout,
                &status_lines,
                message.as_deref(),
                columns,
                rows,
            )?;
            stdout.flush()?;
        }
        Ok(())
    })();
    mailbox.close();
    result
}

fn current_message(messages: &Mutex<Option<(String, Instant)>>) -> Option<String> {
    let mut message = messages.lock().unwrap();
    match &*message {
        Some((text, at)) if at.elapsed() < STATUS_MESSAGE_DURATION => Some(text.clone()),
        _ => {
            *message = None;
            None
        }
    }
}

fn probe_kitty_support(stdout: &mut dyn Write) -> bool {
    if stdout
        .write_all(KITTY_QUERY)
        .and_then(|_| stdout.flush())
        .is_err()
    {
        return false;
    }
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut scan = ResponseScan::new();
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        if remaining.is_zero() || !event::poll(remaining).unwrap_or(false) {
            return false;
        }
        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };
        if scan.feed(key.code) {
            return true;
        }
    }
}

struct ResponseScan {
    previous: Option<char>,
    saw_ok: bool,
}

impl ResponseScan {
    fn new() -> Self {
        Self {
            previous: None,
            saw_ok: false,
        }
    }

    fn feed(&mut self, code: KeyCode) -> bool {
        if self.saw_ok {
            return code == KeyCode::Char('\\');
        }
        self.saw_ok = watch_response(&mut self.previous, code);
        false
    }
}

fn watch_response(previous: &mut Option<char>, code: KeyCode) -> bool {
    match code {
        KeyCode::Char(ch) => {
            let ok = *previous == Some('O') && ch == 'K';
            *previous = Some(ch);
            ok
        }
        _ => {
            *previous = None;
            false
        }
    }
}

fn drain_pending_terminal_events() {
    let deadline = Instant::now() + TERMINAL_INPUT_DRAIN_TIMEOUT;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match event::poll(remaining.min(TERMINAL_INPUT_POLL_INTERVAL)) {
            Ok(true) => {
                if event::read().is_err() {
                    break;
                }
            }
            Ok(false) => {}
            Err(_) => break,
        }
    }
}

fn format_status_line(status: &str, width: usize) -> String {
    let clipped: String = status.chars().take(width).collect();
    let padding = width.saturating_sub(clipped.chars().count()) / 2;
    let mut output = format!("{:padding$}", "");
    output.push_str(DIM_STYLE);
    output.push_str(&clipped);
    output.push_str(RESET_STYLE);
    output
}

fn write_status_lines(
    out: &mut dyn Write,
    status_lines: &[String],
    message: Option<&str>,
    columns: u16,
    rows: u16,
) -> io::Result<()> {
    if columns == 0 || rows == 0 || status_lines.is_empty() && message.is_none() {
        return Ok(());
    }
    let available = usize::from(rows);
    let first = status_lines.len().saturating_sub(available);
    let visible_lines = &status_lines[first..];
    let line_count = visible_lines.len().max(usize::from(message.is_some()));
    let first_row = usize::from(rows) - line_count + 1;
    for (index, status) in visible_lines.iter().enumerate() {
        let line = format_status_line(status, usize::from(columns));
        write!(
            out,
            "\x1b[{};1H{RESET_STYLE}\x1b[K{line}",
            first_row + index
        )?;
    }
    if let Some(message) = message {
        let message: String = message.chars().take(usize::from(columns)).collect();
        write!(out, "\x1b[{rows};1H{RESET_STYLE}{message}{RESET_STYLE}")?;
    }
    Ok(())
}

pub fn is_ghostty() -> bool {
    let term_program = std::env::var_os("TERM_PROGRAM");
    let term = std::env::var_os("TERM");
    is_ghostty_values(term_program.as_deref(), term.as_deref())
}

fn is_ghostty_values(term_program: Option<&OsStr>, term: Option<&OsStr>) -> bool {
    term_program.is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case("ghostty"))
        || term.is_some_and(|value| {
            value
                .to_string_lossy()
                .eq_ignore_ascii_case("xterm-ghostty")
        })
}

pub fn is_rio() -> bool {
    let term_program = std::env::var_os("TERM_PROGRAM");
    let term = std::env::var_os("TERM");
    is_rio_values(term_program.as_deref(), term.as_deref())
}

fn is_rio_values(term_program: Option<&OsStr>, term: Option<&OsStr>) -> bool {
    term_program.is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case("rio"))
        || term.is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case("rio"))
}

impl fmt::Debug for TerminalSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerminalSession")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_term_program() {
        assert!(is_ghostty_values(Some("Ghostty".as_ref()), None));
    }

    #[test]
    fn detects_rio_term_program() {
        assert!(is_rio_values(Some("rio".as_ref()), None));
    }

    #[test]
    fn detects_rio_term() {
        assert!(is_rio_values(None, Some("rio".as_ref())));
    }

    #[test]
    fn detects_term() {
        assert!(is_ghostty_values(None, Some("xterm-ghostty".as_ref())));
    }

    #[test]
    fn rejects_other_terminals() {
        assert!(!is_ghostty_values(
            Some("iTerm.app".as_ref()),
            Some("xterm-256color".as_ref())
        ));
    }

    #[test]
    fn kitty_response_completes_after_terminator() {
        let mut scan = ResponseScan::new();
        for code in ['_', 'G', ';', 'O', 'K'] {
            assert!(!scan.feed(KeyCode::Char(code)));
        }
        assert!(scan.feed(KeyCode::Char('\\')));
    }

    #[test]
    fn primary_screen_auto_selects_block_without_probing() {
        assert_eq!(
            select_renderer(RendererMode::Auto, true, true),
            RendererMode::Block
        );
    }

    #[test]
    fn explicit_graphic_remains_selected_on_primary_screen() {
        assert_eq!(
            select_renderer(RendererMode::Graphic, true, true),
            RendererMode::Graphic
        );
    }

    #[test]
    fn mailbox_replaces_an_unconsumed_frame_with_the_latest() {
        let mailbox = LatestFrameMailbox::new();
        let frame = |value| {
            Arc::new(Frame {
                data: vec![value; 3],
                width: 1,
                height: 1,
            })
        };
        mailbox.publish(frame(1));
        mailbox.publish(frame(2));
        assert_eq!(mailbox.receive().unwrap().data, vec![2; 3]);
    }

    #[test]
    fn closed_mailbox_rejects_frames() {
        let mailbox = LatestFrameMailbox::new();
        mailbox.close();
        assert!(!mailbox.publish(Arc::new(Frame {
            data: vec![1; 3],
            width: 1,
            height: 1,
        })));
    }

    #[test]
    fn status_message_uses_last_terminal_row() {
        let mut output = Vec::new();
        write_status_lines(&mut output, &[], Some("Saved"), 10, 2).unwrap();
        assert_eq!(output, b"\x1b[2;1H\x1b[0mSaved\x1b[0m");
    }

    #[test]
    fn status_lines_keep_only_rows_that_fit() {
        let mut output = Vec::new();
        let lines = vec!["first".to_owned(), "second".to_owned()];
        write_status_lines(&mut output, &lines, None, 20, 1).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("second"));
    }
}
