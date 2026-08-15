use serde::{Deserialize, Serialize};
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use toml_edit::{DocumentMut, Item, Table};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    input: InputConfig,
    display: DisplayConfig,
    audio: AudioConfig,
    rewind: RewindConfig,
    state: StateConfig,
    status: StatusConfig,
    paths: PathsConfig,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct PathsConfig {
    data_dir: Option<String>,
    cache_dir: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct DisplayConfig {
    renderer: crate::renderer::RendererMode,
    fps: u32,
    primary_screen: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            renderer: crate::renderer::RendererMode::Auto,
            fps: 60,
            primary_screen: false,
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct AudioConfig {
    muted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplaySettings {
    pub renderer: crate::renderer::RendererMode,
    pub fps: u32,
    pub primary_screen: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioSettings {
    pub muted: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct StateConfig {
    save_on_exit: bool,
    resume: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct StateSettings {
    pub save_on_exit: bool,
    pub resume: bool,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct StatusConfig {
    enabled: bool,
    gamepad: bool,
    controls: bool,
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            gamepad: true,
            controls: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatusSettings {
    pub enabled: bool,
    pub gamepad: bool,
    pub controls: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct RewindConfig {
    enabled: bool,
    granularity: u64,
    buffer_size_mb: usize,
}

impl Default for RewindConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            granularity: 2,
            buffer_size_mb: 20,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RewindSettings {
    pub enabled: bool,
    pub granularity: u64,
    pub buffer_size: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathSettings {
    pub data_dir: Option<String>,
    pub cache_dir: Option<String>,
}

#[derive(Debug)]
pub struct Settings {
    pub input_bindings: crate::input::InputBindings,
    pub display: DisplaySettings,
    pub audio: AudioSettings,
    pub rewind: RewindSettings,
    pub state: StateSettings,
    pub status: StatusSettings,
    pub paths: PathSettings,
}

#[derive(Clone, Deserialize, Serialize)]
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
    rewind: String,
    save_state: String,
    load_state: String,
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
            select: "rshift".into(),
            l: Some("q".into()),
            r: Some("w".into()),
            l2: None,
            r2: None,
            l3: None,
            r3: None,
            quit: "escape".into(),
            rewind: "r".into(),
            save_state: "f2".into(),
            load_state: "f4".into(),
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
    Write {
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
    UnknownKey(String),
    UnsetKey(String),
    InvalidValue {
        key: String,
        value: String,
        expected: &'static str,
    },
    EditorUnavailable,
    InvalidEditor(String),
    EditorLaunch {
        editor: String,
        source: io::Error,
    },
    EditorFailed(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathUnavailable => write!(formatter, "could not determine the config directory"),
            Self::EmptyOverride => write!(formatter, "ASOBOU_CONFIG must not be empty"),
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read config {}: {source}",
                    path.display()
                )
            }
            Self::Write { path, source } => {
                write!(
                    formatter,
                    "failed to write config {}: {source}",
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
            Self::UnknownKey(key) => write!(formatter, "unknown config key \"{key}\""),
            Self::UnsetKey(key) => write!(formatter, "config key \"{key}\" is unset"),
            Self::InvalidValue {
                key,
                value,
                expected,
            } => write!(
                formatter,
                "invalid value \"{value}\" for config key \"{key}\": expected {expected}"
            ),
            Self::EditorUnavailable => {
                write!(formatter, "VISUAL and EDITOR are not set")
            }
            Self::InvalidEditor(editor) => {
                write!(formatter, "invalid editor command \"{editor}\"")
            }
            Self::EditorLaunch { editor, source } => {
                write!(formatter, "failed to launch editor \"{editor}\": {source}")
            }
            Self::EditorFailed(editor) => {
                write!(formatter, "editor \"{editor}\" exited unsuccessfully")
            }
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. }
            | Self::Write { source, .. }
            | Self::EditorLaunch { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn load_settings() -> Result<Settings, ConfigError> {
    let path = config_path()?;
    load_settings_from(&path)
}

fn config_path() -> Result<PathBuf, ConfigError> {
    resolve_config_path(
        std::env::var_os("ASOBOU_CONFIG").as_deref(),
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        dirs::home_dir().as_deref(),
        dirs::config_dir().as_deref(),
    )
}

fn resolve_config_path(
    asobou_config: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
    _home: Option<&Path>,
    _os_config_dir: Option<&Path>,
) -> Result<PathBuf, ConfigError> {
    if let Some(path) = asobou_config {
        if path.is_empty() {
            return Err(ConfigError::EmptyOverride);
        }
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = xdg_config_home
        && !path.is_empty()
    {
        return Ok(PathBuf::from(path).join("asobou").join("config.toml"));
    }

    #[cfg(windows)]
    let base = _os_config_dir.ok_or(ConfigError::PathUnavailable)?;
    #[cfg(not(windows))]
    let base = _home
        .map(|path| path.join(".config"))
        .ok_or(ConfigError::PathUnavailable)?;

    Ok(base.join("asobou").join("config.toml"))
}

fn load_settings_from(path: &Path) -> Result<Settings, ConfigError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return settings_from_config(Config::default(), path);
        }
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    parse_settings(&contents, path)
}

fn parse_settings(contents: &str, path: &Path) -> Result<Settings, ConfigError> {
    let config = parse_config(contents, path)?;
    settings_from_config(config, path)
}

fn parse_config(contents: &str, path: &Path) -> Result<Config, ConfigError> {
    toml::from_str(contents).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn settings_from_config(config: Config, path: &Path) -> Result<Settings, ConfigError> {
    validate_display(&config.display, path)?;
    validate_rewind(&config.rewind, path)?;
    validate_paths(&config.paths, path)?;
    let display = DisplaySettings {
        renderer: config.display.renderer,
        fps: config.display.fps,
        primary_screen: config.display.primary_screen,
    };
    let audio = AudioSettings {
        muted: config.audio.muted,
    };
    let rewind = RewindSettings {
        enabled: config.rewind.enabled,
        granularity: config.rewind.granularity,
        buffer_size: config
            .rewind
            .buffer_size_mb
            .checked_mul(1024 * 1024)
            .ok_or_else(|| ConfigError::Invalid {
                path: path.to_path_buf(),
                reason: "[rewind] buffer_size_mb is too large".into(),
            })?,
    };
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

    let input_bindings = crate::input::InputBindings::new(
        &gamepad,
        &input.quit,
        &input.rewind,
        &input.save_state,
        &input.load_state,
        rewind.enabled,
    )
    .map_err(|reason| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason,
    })?;
    Ok(Settings {
        input_bindings,
        display,
        audio,
        rewind,
        state: StateSettings {
            save_on_exit: config.state.save_on_exit,
            resume: config.state.resume,
        },
        status: StatusSettings {
            enabled: config.status.enabled,
            gamepad: config.status.gamepad,
            controls: config.status.controls,
        },
        paths: PathSettings {
            data_dir: config.paths.data_dir,
            cache_dir: config.paths.cache_dir,
        },
    })
}

const CONFIG_KEYS: &[&str] = &[
    "input.up",
    "input.down",
    "input.left",
    "input.right",
    "input.a",
    "input.b",
    "input.x",
    "input.y",
    "input.start",
    "input.select",
    "input.l",
    "input.r",
    "input.l2",
    "input.r2",
    "input.l3",
    "input.r3",
    "input.quit",
    "input.rewind",
    "input.save_state",
    "input.load_state",
    "display.renderer",
    "display.fps",
    "display.primary_screen",
    "audio.muted",
    "rewind.enabled",
    "rewind.granularity",
    "rewind.buffer_size_mb",
    "state.save_on_exit",
    "state.resume",
    "status.enabled",
    "status.gamepad",
    "status.controls",
    "paths.data_dir",
    "paths.cache_dir",
];

fn config_key(key: &str) -> Result<&'static str, ConfigError> {
    CONFIG_KEYS
        .iter()
        .copied()
        .find(|candidate| *candidate == key)
        .ok_or_else(|| ConfigError::UnknownKey(key.into()))
}

fn parse_value(key: &str, current: Option<&toml::Value>, value: &str) -> Result<Item, ConfigError> {
    let invalid = |expected| ConfigError::InvalidValue {
        key: key.into(),
        value: value.into(),
        expected,
    };
    match current {
        Some(toml::Value::Boolean(_)) => value
            .parse::<bool>()
            .map(toml_edit::value)
            .map_err(|_| invalid("true or false")),
        Some(toml::Value::Integer(_)) => value
            .parse::<i64>()
            .map(toml_edit::value)
            .map_err(|_| invalid("a decimal integer")),
        _ => Ok(toml_edit::value(value)),
    }
}

fn effective_config(config: &Config, path: &Path) -> Result<toml::Value, ConfigError> {
    let mut value = toml::Value::try_from(config).map_err(|error| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    let Some(root) = value.as_table_mut() else {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: "configuration did not serialize as a table".into(),
        });
    };
    let paths = root
        .entry("paths")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let Some(paths) = paths.as_table_mut() else {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: "[paths] did not serialize as a table".into(),
        });
    };
    paths.insert(
        "data_dir".into(),
        toml::Value::String(
            crate::paths::data_base(config.paths.data_dir.as_deref())
                .to_string_lossy()
                .into_owned(),
        ),
    );
    paths.insert(
        "cache_dir".into(),
        toml::Value::String(
            crate::paths::cache_base(config.paths.cache_dir.as_deref())
                .to_string_lossy()
                .into_owned(),
        ),
    );
    Ok(value)
}

fn effective_value<'a>(config: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let (section, name) = key_parts(key);
    config.get(section)?.get(name)
}

fn plain_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

fn read_config(path: &Path) -> Result<(Option<String>, Config), ConfigError> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let config = parse_config(&contents, path)?;
            settings_from_config(config.clone(), path)?;
            Ok((Some(contents), config))
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok((None, Config::default())),
        Err(source) => Err(ConfigError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn parse_document(contents: &str, path: &Path) -> Result<DocumentMut, ConfigError> {
    contents.parse().map_err(|error| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: format!("could not edit TOML document: {error}"),
    })
}

fn validated_document(document: &DocumentMut, path: &Path) -> Result<Config, ConfigError> {
    let config = parse_config(&document.to_string(), path)?;
    settings_from_config(config.clone(), path)?;
    Ok(config)
}

fn key_parts(key: &str) -> (&str, &str) {
    key.split_once('.').unwrap()
}

fn key_is_stored(document: &DocumentMut, key: &str) -> bool {
    let (section, name) = key_parts(key);
    document
        .get(section)
        .and_then(Item::as_table_like)
        .is_some_and(|table| table.contains_key(name))
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn write_document(path: &Path, document: &DocumentMut) -> Result<(), ConfigError> {
    let parent = parent_directory(path);
    fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .write_all(document.to_string().as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .map_err(|source| ConfigError::Write {
                path: path.to_path_buf(),
                source,
            })?;
    }
    temporary
        .persist(path)
        .map_err(|error| ConfigError::Write {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    Ok(())
}

pub struct ConfigEntry {
    pub key: &'static str,
    pub value: String,
    pub configured: bool,
}

pub fn list() -> Result<Vec<ConfigEntry>, ConfigError> {
    let path = config_path()?;
    let (contents, config) = read_config(&path)?;
    let document = parse_document(contents.as_deref().unwrap_or(""), &path)?;
    let effective = effective_config(&config, &path)?;
    Ok(CONFIG_KEYS
        .iter()
        .map(|&key| ConfigEntry {
            key,
            value: effective_value(&effective, key)
                .map(plain_value)
                .unwrap_or_else(|| "<unset>".into()),
            configured: key_is_stored(&document, key),
        })
        .collect())
}

pub fn get(key: &str) -> Result<String, ConfigError> {
    let key = config_key(key)?;
    let path = config_path()?;
    let (_, config) = read_config(&path)?;
    effective_value(&effective_config(&config, &path)?, key)
        .map(plain_value)
        .ok_or_else(|| ConfigError::UnsetKey(key.into()))
}

pub fn set(key: &str, value: &str) -> Result<String, ConfigError> {
    let key = config_key(key)?;
    let path = config_path()?;
    let (contents, current) = read_config(&path)?;
    let effective = effective_config(&current, &path)?;
    let value = parse_value(key, effective_value(&effective, key), value)?;
    let mut document = parse_document(contents.as_deref().unwrap_or(""), &path)?;
    let (section, name) = key_parts(key);
    if !document.contains_key(section) {
        document[section] = Item::Table(Table::new());
    }
    document[section][name] = value;
    let config = validated_document(&document, &path)?;
    let effective = effective_value(&effective_config(&config, &path)?, key)
        .map(plain_value)
        .ok_or_else(|| ConfigError::UnsetKey(key.into()))?;
    write_document(&path, &document)?;
    Ok(effective)
}

pub fn unset(key: &str) -> Result<Option<String>, ConfigError> {
    let key = config_key(key)?;
    let path = config_path()?;
    let (contents, config) = read_config(&path)?;
    let (section, name) = key_parts(key);
    let Some(contents) = contents else {
        return Ok(effective_value(&effective_config(&config, &path)?, key).map(plain_value));
    };
    let mut document = parse_document(&contents, &path)?;
    let removed = document
        .get_mut(section)
        .and_then(Item::as_table_like_mut)
        .and_then(|table| table.remove(name))
        .is_some();
    let config = validated_document(&document, &path)?;
    if removed {
        write_document(&path, &document)?;
    }
    Ok(effective_value(&effective_config(&config, &path)?, key).map(plain_value))
}

pub fn edit() -> Result<(), ConfigError> {
    let path = config_path()?;
    fs::create_dir_all(parent_directory(&path)).map_err(|source| ConfigError::Write {
        path: path.clone(),
        source,
    })?;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| ConfigError::Write {
            path: path.clone(),
            source,
        })?;
    let editor = ["VISUAL", "EDITOR"]
        .into_iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .ok_or(ConfigError::EditorUnavailable)?;
    let mut words = shell_words::split(&editor)
        .map_err(|_| ConfigError::InvalidEditor(editor.clone()))?
        .into_iter();
    let program = words
        .next()
        .ok_or_else(|| ConfigError::InvalidEditor(editor.clone()))?;
    let status = Command::new(&program)
        .args(words)
        .arg(&path)
        .status()
        .map_err(|source| ConfigError::EditorLaunch {
            editor: editor.clone(),
            source,
        })?;
    if !status.success() {
        return Err(ConfigError::EditorFailed(editor));
    }
    load_settings_from(&path)?;
    Ok(())
}

fn validate_display(display: &DisplayConfig, path: &Path) -> Result<(), ConfigError> {
    if !(1..=240).contains(&display.fps) {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: "[display] fps must be between 1 and 240".into(),
        });
    }
    Ok(())
}

fn is_absolute_or_tilde(value: &str) -> bool {
    value == "~" || value.starts_with("~/") || Path::new(value).is_absolute()
}

fn validate_paths(paths: &PathsConfig, path: &Path) -> Result<(), ConfigError> {
    for (name, value) in [
        ("data_dir", paths.data_dir.as_deref()),
        ("cache_dir", paths.cache_dir.as_deref()),
    ] {
        let Some(value) = value else {
            continue;
        };
        if value.is_empty() {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                reason: format!("[paths] {name} must not be empty"),
            });
        }
        if !is_absolute_or_tilde(value) {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                reason: format!("[paths] {name} must be an absolute path or start with \"~/\""),
            });
        }
    }
    Ok(())
}

fn validate_rewind(rewind: &RewindConfig, path: &Path) -> Result<(), ConfigError> {
    if rewind.granularity == 0 {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: "[rewind] granularity must be at least 1".into(),
        });
    }
    if rewind.buffer_size_mb == 0 {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: "[rewind] buffer_size_mb must be at least 1".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asobou_config_has_highest_priority() {
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

        assert_eq!(path, PathBuf::from("/xdg/asobou/config.toml"));
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

        assert_eq!(path, PathBuf::from("/home/user/.config/asobou/config.toml"));
    }

    #[test]
    fn missing_config_uses_default_bindings() {
        let directory = tempfile::tempdir().unwrap();
        let settings = load_settings_from(&directory.path().join("missing.toml")).unwrap();

        assert_eq!(settings.input_bindings.quit_name(), "escape");
    }

    #[test]
    fn partial_config_keeps_unspecified_defaults() {
        let settings = parse_settings("[input]\na = \"v\"\n", Path::new("config.toml")).unwrap();

        assert_eq!(settings.input_bindings.quit_name(), "escape");
        assert_eq!(settings.input_bindings.rewind_name(), "r");
        assert!(settings.input_bindings.rewind_enabled());
        assert_eq!(
            settings.display,
            DisplaySettings {
                renderer: crate::renderer::RendererMode::Auto,
                fps: 60,
                primary_screen: false,
            }
        );
        assert_eq!(settings.audio, AudioSettings { muted: false });
        assert!(settings.rewind.enabled);
        assert_eq!(settings.rewind.granularity, 2);
        assert_eq!(settings.rewind.buffer_size, 20 * 1024 * 1024);
    }

    #[test]
    fn display_and_audio_settings_are_configurable() {
        let settings = parse_settings(
            "[display]\nrenderer = \"ascii\"\nfps = 30\nprimary_screen = true\n\n[audio]\nmuted = true\n",
            Path::new("config.toml"),
        )
        .unwrap();

        assert_eq!(
            settings.display,
            DisplaySettings {
                renderer: crate::renderer::RendererMode::Ascii,
                fps: 30,
                primary_screen: true,
            }
        );
        assert_eq!(settings.audio, AudioSettings { muted: true });
    }

    #[test]
    fn display_fps_must_be_in_range() {
        for fps in [0, 241] {
            let error = parse_settings(
                &format!("[display]\nfps = {fps}\n"),
                Path::new("config.toml"),
            )
            .unwrap_err();

            assert!(error.to_string().contains("fps"));
        }
    }

    #[test]
    fn status_defaults_to_both_groups_visible() {
        let settings = parse_settings("", Path::new("config.toml")).unwrap();

        assert_eq!(
            settings.status,
            StatusSettings {
                enabled: true,
                gamepad: true,
                controls: true,
            }
        );
    }

    #[test]
    fn status_groups_are_independently_configurable() {
        let settings = parse_settings(
            "[status]\nenabled = true\ngamepad = false\ncontrols = true\n",
            Path::new("config.toml"),
        )
        .unwrap();

        assert_eq!(
            settings.status,
            StatusSettings {
                enabled: true,
                gamepad: false,
                controls: true,
            }
        );
    }

    #[test]
    fn unknown_status_config_fields_are_rejected() {
        let error =
            parse_settings("[status]\nsession = true\n", Path::new("config.toml")).unwrap_err();

        assert!(error.to_string().contains("session"));
    }

    #[test]
    fn rewind_can_be_disabled() {
        let settings =
            parse_settings("[rewind]\nenabled = false\n", Path::new("config.toml")).unwrap();

        assert!(!settings.rewind.enabled);
        assert!(!settings.input_bindings.rewind_enabled());
    }

    #[test]
    fn rewind_settings_are_tunable() {
        let settings = parse_settings(
            "[rewind]\ngranularity = 5\nbuffer_size_mb = 64\n",
            Path::new("config.toml"),
        )
        .unwrap();

        assert!(settings.rewind.enabled);
        assert_eq!(settings.rewind.granularity, 5);
        assert_eq!(settings.rewind.buffer_size, 64 * 1024 * 1024);
    }

    #[test]
    fn zero_granularity_is_rejected() {
        let error =
            parse_settings("[rewind]\ngranularity = 0\n", Path::new("config.toml")).unwrap_err();

        assert!(error.to_string().contains("granularity"));
    }

    #[test]
    fn zero_buffer_size_is_rejected() {
        let error =
            parse_settings("[rewind]\nbuffer_size_mb = 0\n", Path::new("config.toml")).unwrap_err();

        assert!(error.to_string().contains("buffer_size_mb"));
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let error = parse_settings("[input]\na = \"z\"\n", Path::new("config.toml")).unwrap_err();

        assert!(error.to_string().contains("already bound"));
    }

    #[test]
    fn combinations_are_rejected() {
        let error =
            parse_settings("[input]\na = \"ctrl+x\"\n", Path::new("config.toml")).unwrap_err();

        assert!(error.to_string().contains("key combinations"));
    }

    #[test]
    fn save_on_exit_defaults_to_false() {
        let settings =
            parse_settings("[rewind]\nenabled = false\n", Path::new("config.toml")).unwrap();

        assert!(!settings.state.save_on_exit);
    }

    #[test]
    fn save_on_exit_parses_when_enabled() {
        let settings =
            parse_settings("[state]\nsave_on_exit = true\n", Path::new("config.toml")).unwrap();

        assert!(settings.state.save_on_exit);
    }

    #[test]
    fn resume_defaults_to_false() {
        let settings =
            parse_settings("[rewind]\nenabled = false\n", Path::new("config.toml")).unwrap();

        assert!(!settings.state.resume);
    }

    #[test]
    fn resume_parses_when_enabled() {
        let settings =
            parse_settings("[state]\nresume = true\n", Path::new("config.toml")).unwrap();

        assert!(settings.state.resume);
    }

    #[test]
    fn save_and_load_bindings_are_configurable() {
        let settings = parse_settings(
            "[input]\nsave_state = \"f1\"\nload_state = \"f3\"\n",
            Path::new("config.toml"),
        )
        .unwrap();

        assert!(
            settings
                .input_bindings
                .controls_status_line()
                .contains("Save-f1 Load-f3")
        );
    }

    #[test]
    fn unknown_state_config_fields_are_rejected() {
        let error =
            parse_settings("[state]\nprune = true\n", Path::new("config.toml")).unwrap_err();

        assert!(error.to_string().contains("prune"));
    }

    #[test]
    fn paths_settings_default_to_unset() {
        let settings = parse_settings("", Path::new("config.toml")).unwrap();

        assert_eq!(
            settings.paths,
            PathSettings {
                data_dir: None,
                cache_dir: None
            }
        );
    }

    #[cfg(windows)]
    const ABSOLUTE_DATA_DIR: &str = r"C:\custom\data";
    #[cfg(not(windows))]
    const ABSOLUTE_DATA_DIR: &str = "/custom/data";

    #[test]
    fn paths_settings_carry_raw_configured_values() {
        let settings = parse_settings(
            &format!("[paths]\ndata_dir = '{ABSOLUTE_DATA_DIR}'\ncache_dir = '~/cache'\n"),
            Path::new("config.toml"),
        )
        .unwrap();

        assert_eq!(
            settings.paths,
            PathSettings {
                data_dir: Some(ABSOLUTE_DATA_DIR.into()),
                cache_dir: Some("~/cache".into()),
            }
        );
    }

    #[test]
    fn relative_paths_values_are_rejected() {
        for value in ["relative", "~user/data", "./data"] {
            let error = parse_settings(
                &format!("[paths]\ndata_dir = \"{value}\"\n"),
                Path::new("config.toml"),
            )
            .unwrap_err();

            assert!(error.to_string().contains("absolute"), "{value}: {error}");
        }
    }

    #[test]
    fn empty_path_values_are_rejected() {
        let error =
            parse_settings("[paths]\ncache_dir = \"\"\n", Path::new("config.toml")).unwrap_err();

        assert!(error.to_string().contains("empty"));
    }

    #[test]
    fn absolute_and_tilde_path_values_are_accepted() {
        for value in [ABSOLUTE_DATA_DIR, "~", "~/games"] {
            let settings = parse_settings(
                &format!("[paths]\ndata_dir = '{value}'\n"),
                Path::new("config.toml"),
            )
            .unwrap();

            assert_eq!(settings.paths.data_dir.as_deref(), Some(value));
        }
    }

    #[test]
    fn unknown_paths_config_fields_are_rejected() {
        let error = parse_settings("[paths]\nsaves_dir = \"/tmp\"\n", Path::new("config.toml"))
            .unwrap_err();

        assert!(error.to_string().contains("saves_dir"));
    }
}
