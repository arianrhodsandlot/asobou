use std::ffi::OsStr;

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

#[cfg(test)]
mod tests {
    use super::is_ghostty_values;

    #[test]
    fn detects_term_program() {
        assert!(is_ghostty_values(Some("Ghostty".as_ref()), None));
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
}
