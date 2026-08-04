use std::io::{self, Write};
use std::path::Path;

pub fn list(
    rom_filter: Option<&str>,
    core_filter: Option<&str>,
    states_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let list = crate::emulation::state::list_states(states_dir)?;
    let entries: Vec<_> = list
        .entries
        .into_iter()
        .filter(|entry| rom_filter.is_none_or(|rom| entry.game == rom))
        .filter(|entry| core_filter.is_none_or(|core| entry.core == core))
        .collect();

    let mut stdout = io::stdout().lock();
    if entries.is_empty() {
        writeln!(stdout, "No save states found.")?;
    } else {
        writeln!(stdout, "{:<24} {:<12} {:<25} PATH", "GAME", "CORE", "SAVED")?;
        for entry in entries {
            writeln!(
                stdout,
                "{:<24} {:<12} {:<25} {}",
                entry.game,
                entry.core,
                entry.timestamp.human(),
                display_path(&entry.path)
            )?;
        }
    }
    for malformed in list.malformed {
        eprintln!(
            "Warning: skipping malformed state file {}: {}",
            malformed.path.display(),
            malformed.reason
        );
    }
    Ok(())
}

fn display_path(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if cfg!(windows) {
        return absolute.display().to_string();
    }
    let Some(home) = dirs::home_dir() else {
        return absolute.display().to_string();
    };
    shorten_home(&absolute, &home)
}

fn shorten_home(path: &Path, home: &Path) -> String {
    if home == Path::new("/") {
        return path.display().to_string();
    }
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::shorten_home;
    use std::path::Path;

    #[test]
    fn paths_under_home_are_shortened() {
        let home = Path::new("/home/user");

        assert_eq!(
            shorten_home(Path::new("/home/user/games/s.state"), home),
            "~/games/s.state"
        );
        assert_eq!(shorten_home(Path::new("/home/user"), home), "~");
    }

    #[test]
    fn paths_outside_home_stay_absolute() {
        let home = Path::new("/home/user");

        assert_eq!(
            shorten_home(Path::new("/opt/asoby/s.state"), home),
            "/opt/asoby/s.state"
        );
    }

    #[test]
    fn root_home_is_not_shortened() {
        assert_eq!(
            shorten_home(Path::new("/etc/passwd"), Path::new("/")),
            "/etc/passwd"
        );
    }
}
