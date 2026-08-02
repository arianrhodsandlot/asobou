use super::RendererMode;
use crossterm::event::{self, Event, KeyCode};
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const KITTY_QUERY: &[u8] = b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\";

pub fn select(no_alt_screen: bool) -> RendererMode {
    if graphics_supported(no_alt_screen) {
        RendererMode::Graphic
    } else {
        RendererMode::Block
    }
}

fn graphics_supported(no_alt_screen: bool) -> bool {
    if no_alt_screen || !io::stdout().is_terminal() {
        return false;
    }
    let mut stdout = io::stdout();
    let _ = stdout.flush();
    if crossterm::terminal::enable_raw_mode().is_err() {
        return false;
    }
    let supported = probe_kitty_support(&mut stdout);
    let _ = crossterm::terminal::disable_raw_mode();
    supported
}

// Sends the kitty graphics protocol query and watches for the "OK" response,
// bounded by a deadline. Terminals that ignore the query (or respond with
// anything else) fall back to the block renderer instead of hanging forever,
// which is what viuer's own support check does.
fn probe_kitty_support(stdout: &mut dyn io::Write) -> bool {
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

    // Returns true once the full `OK ESC \` response has been consumed, so
    // nothing of it is left queued for the emulator input loop. A leftover
    // ESC could otherwise be parsed as the quit key if the terminator arrives
    // fragmented. A partial response (OK without the terminator) is never
    // reported as support; the probe's deadline treats it as unsupported.
    fn feed(&mut self, code: KeyCode) -> bool {
        if self.saw_ok {
            if code == KeyCode::Char('\\') {
                return true;
            }
            return false;
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

#[cfg(test)]
mod tests {
    use super::{RendererMode, ResponseScan, select, watch_response};
    use crossterm::event::KeyCode;

    #[test]
    fn selects_block_when_alt_screen_is_disabled() {
        assert_eq!(select(true), RendererMode::Block);
    }

    #[test]
    fn detects_kitty_ok_across_chars() {
        let codes = [
            KeyCode::Char('_'),
            KeyCode::Char('G'),
            KeyCode::Char(';'),
            KeyCode::Char('O'),
            KeyCode::Char('K'),
            KeyCode::Char('\\'),
        ];
        let mut previous = None;
        assert!(
            codes
                .iter()
                .any(|&code| watch_response(&mut previous, code))
        );
    }

    #[test]
    fn ignores_non_ok_responses() {
        let codes = [
            KeyCode::Char('E'),
            KeyCode::Char('R'),
            KeyCode::Char('R'),
            KeyCode::Char('O'),
            KeyCode::Char('R'),
        ];
        let mut previous = None;
        assert!(
            !codes
                .iter()
                .any(|&code| watch_response(&mut previous, code))
        );
    }

    #[test]
    fn non_char_events_reset_the_sequence() {
        let codes = [KeyCode::Char('O'), KeyCode::Char('x'), KeyCode::Char('K')];
        let mut previous = None;
        assert!(
            !codes
                .iter()
                .any(|&code| watch_response(&mut previous, code))
        );
    }

    #[test]
    fn completes_only_after_the_response_terminator() {
        let mut scan = ResponseScan::new();
        assert!(!scan.feed(KeyCode::Char('_')));
        assert!(!scan.feed(KeyCode::Char('G')));
        assert!(!scan.feed(KeyCode::Char(';')));
        assert!(!scan.feed(KeyCode::Char('O')));
        assert!(!scan.feed(KeyCode::Char('K')));
        assert!(scan.feed(KeyCode::Char('\\')));
    }

    #[test]
    fn consumes_fragmented_terminator_with_standalone_esc() {
        let mut scan = ResponseScan::new();
        assert!(!scan.feed(KeyCode::Char('O')));
        assert!(!scan.feed(KeyCode::Char('K')));
        // The response's ESC arrived alone and was parsed as a key event;
        // the scan must not treat it as the terminator or give up.
        assert!(!scan.feed(KeyCode::Esc));
        assert!(scan.feed(KeyCode::Char('\\')));
    }

    #[test]
    fn ok_without_terminator_never_completes() {
        let mut scan = ResponseScan::new();
        assert!(!scan.feed(KeyCode::Char('O')));
        assert!(!scan.feed(KeyCode::Char('K')));
        // Without the terminator, feeding more chars never reports completion,
        // so the probe's deadline path fails closed.
        assert!(!scan.feed(KeyCode::Char('x')));
        assert!(!scan.feed(KeyCode::Char('y')));
    }
}
