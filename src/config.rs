use serde::Deserialize;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    input: InputConfig,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct InputConfig {
    up: String,
    down: String,
    left: String,
    right: String,
    a: String,
    b: String,
    x: String,
    y: String,
    start: String,
    select: String,
    l: Option<String>,
    r: Option<String>,
    l2: Option<String>,
    r2: Option<String>,
    l3: Option<String>,
    r3: Option<String>,
    quit: String,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            up: "up".into(),
            down: "down".into(),
            left: "left".into(),
            right: "right".into(),
            a: "x".into(),
            b: "z".into(),
            x: "s".into(),
            y: "a".into(),
            start: "enter".into(),
            select: "backspace".into(),
            l: None,
            r: None,
            l2: None,
            r2: None,
            l3: None,
            r3: None,
            quit: "esc".into(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    PathUnavailable,
    EmptyOverride,
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Invalid {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathUnavailable => write!(formatter, "could not determine the config directory"),
            Self::EmptyOverride => write!(formatter, "ASOBY_CONFIG must not be empty"),
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read config {}: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(
                    formatter,
                    "failed to parse config {}: {source}",
                    path.display()
                )
            }
            Self::Invalid { path, reason } => {
                write!(formatter, "invalid config {}: {reason}", path.display())
            }
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn load_input_bindings() -> Result<crate::input::InputBindings, ConfigError> {
    let path = config_path()?;
    load_input_bindings_from(&path)
}

fn config_path() -> Result<PathBuf, ConfigError> {
    resolve_config_path(
        std::env::var_os("ASOBY_CONFIG").as_deref(),
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        dirs::home_dir().as_deref(),
        dirs::config_dir().as_deref(),
    )
}

fn resolve_config_path(
    asoby_config: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
    _home: Option<&Path>,
    _os_config_dir: Option<&Path>,
) -> Result<PathBuf, ConfigError> {
    if let Some(path) = asoby_config {
        if path.is_empty() {
            return Err(ConfigError::EmptyOverride);
        }
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = xdg_config_home
        && !path.is_empty()
    {
        return Ok(PathBuf::from(path).join("asoby").join("config.toml"));
    }

    #[cfg(windows)]
    let base = _os_config_dir.ok_or(ConfigError::PathUnavailable)?;
    #[cfg(not(windows))]
    let base = _home
        .map(|path| path.join(".config"))
        .ok_or(ConfigError::PathUnavailable)?;

    Ok(base.join("asoby").join("config.toml"))
}

fn load_input_bindings_from(path: &Path) -> Result<crate::input::InputBindings, ConfigError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(crate::input::InputBindings::default());
        }
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    parse_input_bindings(&contents, path)
}

fn parse_input_bindings(
    contents: &str,
    path: &Path,
) -> Result<crate::input::InputBindings, ConfigError> {
    let config: Config = toml::from_str(contents).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let input = config.input;
    let mut gamepad = vec![
        ("up", crate::input::BUTTON_UP, input.up.as_str()),
        ("down", crate::input::BUTTON_DOWN, input.down.as_str()),
        ("left", crate::input::BUTTON_LEFT, input.left.as_str()),
        ("right", crate::input::BUTTON_RIGHT, input.right.as_str()),
        ("a", crate::input::BUTTON_A, input.a.as_str()),
        ("b", crate::input::BUTTON_B, input.b.as_str()),
        ("x", crate::input::BUTTON_X, input.x.as_str()),
        ("y", crate::input::BUTTON_Y, input.y.as_str()),
        ("start", crate::input::BUTTON_START, input.start.as_str()),
        ("select", crate::input::BUTTON_SELECT, input.select.as_str()),
    ];
    for (name, button, key) in [
        ("l", crate::input::BUTTON_L, input.l.as_deref()),
        ("r", crate::input::BUTTON_R, input.r.as_deref()),
        ("l2", crate::input::BUTTON_L2, input.l2.as_deref()),
        ("r2", crate::input::BUTTON_R2, input.r2.as_deref()),
        ("l3", crate::input::BUTTON_L3, input.l3.as_deref()),
        ("r3", crate::input::BUTTON_R3, input.r3.as_deref()),
    ] {
        if let Some(key) = key {
            gamepad.push((name, button, key));
        }
    }

    crate::input::InputBindings::new(&gamepad, &input.quit).map_err(|reason| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asoby_config_has_highest_priority() {
        let path = resolve_config_path(
            Some(OsStr::new("custom.toml")),
            Some(OsStr::new("/xdg")),
            Some(Path::new("/home/user")),
            Some(Path::new("/native")),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("custom.toml"));
    }

    #[test]
    fn xdg_config_home_is_used_when_override_is_absent() {
        let path = resolve_config_path(
            None,
            Some(OsStr::new("/xdg")),
            Some(Path::new("/home/user")),
            Some(Path::new("/native")),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/xdg/asoby/config.toml"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_fallback_uses_dot_config() {
        let path = resolve_config_path(
            None,
            None,
            Some(Path::new("/home/user")),
            Some(Path::new("/native")),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/home/user/.config/asoby/config.toml"));
    }

    #[test]
    fn missing_config_uses_default_bindings() {
        let directory = tempfile::tempdir().unwrap();
        let bindings = load_input_bindings_from(&directory.path().join("missing.toml")).unwrap();

        assert_eq!(bindings.quit_name(), "esc");
    }

    #[test]
    fn partial_config_keeps_unspecified_defaults() {
        let bindings =
            parse_input_bindings("[input]\na = \"v\"\n", Path::new("config.toml")).unwrap();

        assert_eq!(bindings.quit_name(), "esc");
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let error =
            parse_input_bindings("[input]\na = \"z\"\n", Path::new("config.toml")).unwrap_err();

        assert!(error.to_string().contains("already bound"));
    }

    #[test]
    fn combinations_are_rejected() {
        let error = parse_input_bindings("[input]\na = \"ctrl+x\"\n", Path::new("config.toml"))
            .unwrap_err();

        assert!(error.to_string().contains("key combinations"));
    }
}
