use serde::Deserialize;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use toml_edit::{DocumentMut, Item, Table};

#[derive(Clone, Default, Deserialize)]
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

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PathsConfig {
    data_dir: Option<String>,
    cache_dir: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
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

#[derive(Clone, Copy, Default, Deserialize)]
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

#[derive(Clone, Default, Deserialize)]
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

#[derive(Clone, Copy, Deserialize)]
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

#[derive(Clone, Deserialize)]
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

#[derive(Clone, Deserialize)]
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
            l: None,
            r: None,
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
    UnknownKey {
        key: String,
        suggestion: Option<&'static str>,
    },
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
            Self::UnknownKey { key, suggestion } => {
                write!(formatter, "unknown config key \"{key}\"")?;
                if let Some(suggestion) = suggestion {
                    write!(formatter, "\nDid you mean \"{suggestion}\"?")?;
                }
                Ok(())
            }
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

enum ConfigValue {
    String(String),
    Boolean(bool),
    Integer(i64),
}

impl ConfigValue {
    fn plain(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Boolean(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
        }
    }

    fn into_item(self) -> Item {
        match self {
            Self::String(value) => toml_edit::value(value),
            Self::Boolean(value) => toml_edit::value(value),
            Self::Integer(value) => toml_edit::value(value),
        }
    }
}

#[derive(Clone, Copy)]
enum ConfigValueKind {
    String,
    Boolean,
    Integer,
}

struct ConfigKey {
    name: &'static str,
    kind: ConfigValueKind,
    value: fn(&Config) -> Option<ConfigValue>,
}

const CONFIG_KEYS: &[ConfigKey] = &[
    ConfigKey {
        name: "input.up",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.up.clone())),
    },
    ConfigKey {
        name: "input.down",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.down.clone())),
    },
    ConfigKey {
        name: "input.left",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.left.clone())),
    },
    ConfigKey {
        name: "input.right",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.right.clone())),
    },
    ConfigKey {
        name: "input.a",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.a.clone())),
    },
    ConfigKey {
        name: "input.b",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.b.clone())),
    },
    ConfigKey {
        name: "input.x",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.x.clone())),
    },
    ConfigKey {
        name: "input.y",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.y.clone())),
    },
    ConfigKey {
        name: "input.start",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.start.clone())),
    },
    ConfigKey {
        name: "input.select",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.select.clone())),
    },
    ConfigKey {
        name: "input.l",
        kind: ConfigValueKind::String,
        value: |config| config.input.l.clone().map(ConfigValue::String),
    },
    ConfigKey {
        name: "input.r",
        kind: ConfigValueKind::String,
        value: |config| config.input.r.clone().map(ConfigValue::String),
    },
    ConfigKey {
        name: "input.l2",
        kind: ConfigValueKind::String,
        value: |config| config.input.l2.clone().map(ConfigValue::String),
    },
    ConfigKey {
        name: "input.r2",
        kind: ConfigValueKind::String,
        value: |config| config.input.r2.clone().map(ConfigValue::String),
    },
    ConfigKey {
        name: "input.l3",
        kind: ConfigValueKind::String,
        value: |config| config.input.l3.clone().map(ConfigValue::String),
    },
    ConfigKey {
        name: "input.r3",
        kind: ConfigValueKind::String,
        value: |config| config.input.r3.clone().map(ConfigValue::String),
    },
    ConfigKey {
        name: "input.quit",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.quit.clone())),
    },
    ConfigKey {
        name: "input.rewind",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.rewind.clone())),
    },
    ConfigKey {
        name: "input.save_state",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.save_state.clone())),
    },
    ConfigKey {
        name: "input.load_state",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.input.load_state.clone())),
    },
    ConfigKey {
        name: "display.renderer",
        kind: ConfigValueKind::String,
        value: |config| Some(ConfigValue::String(config.display.renderer.as_str().into())),
    },
    ConfigKey {
        name: "display.fps",
        kind: ConfigValueKind::Integer,
        value: |config| Some(ConfigValue::Integer(i64::from(config.display.fps))),
    },
    ConfigKey {
        name: "display.primary_screen",
        kind: ConfigValueKind::Boolean,
        value: |config| Some(ConfigValue::Boolean(config.display.primary_screen)),
    },
    ConfigKey {
        name: "audio.muted",
        kind: ConfigValueKind::Boolean,
        value: |config| Some(ConfigValue::Boolean(config.audio.muted)),
    },
    ConfigKey {
        name: "rewind.enabled",
        kind: ConfigValueKind::Boolean,
        value: |config| Some(ConfigValue::Boolean(config.rewind.enabled)),
    },
    ConfigKey {
        name: "rewind.granularity",
        kind: ConfigValueKind::Integer,
        value: |config| {
            Some(ConfigValue::Integer(
                i64::try_from(config.rewind.granularity).unwrap_or(i64::MAX),
            ))
        },
    },
    ConfigKey {
        name: "rewind.buffer_size_mb",
        kind: ConfigValueKind::Integer,
        value: |config| {
            Some(ConfigValue::Integer(
                i64::try_from(config.rewind.buffer_size_mb).unwrap_or(i64::MAX),
            ))
        },
    },
    ConfigKey {
        name: "state.save_on_exit",
        kind: ConfigValueKind::Boolean,
        value: |config| Some(ConfigValue::Boolean(config.state.save_on_exit)),
    },
    ConfigKey {
        name: "state.resume",
        kind: ConfigValueKind::Boolean,
        value: |config| Some(ConfigValue::Boolean(config.state.resume)),
    },
    ConfigKey {
        name: "status.enabled",
        kind: ConfigValueKind::Boolean,
        value: |config| Some(ConfigValue::Boolean(config.status.enabled)),
    },
    ConfigKey {
        name: "status.gamepad",
        kind: ConfigValueKind::Boolean,
        value: |config| Some(ConfigValue::Boolean(config.status.gamepad)),
    },
    ConfigKey {
        name: "status.controls",
        kind: ConfigValueKind::Boolean,
        value: |config| Some(ConfigValue::Boolean(config.status.controls)),
    },
    ConfigKey {
        name: "paths.data_dir",
        kind: ConfigValueKind::String,
        value: |config| {
            Some(ConfigValue::String(
                crate::paths::data_base(config.paths.data_dir.as_deref())
                    .to_string_lossy()
                    .into_owned(),
            ))
        },
    },
    ConfigKey {
        name: "paths.cache_dir",
        kind: ConfigValueKind::String,
        value: |config| {
            Some(ConfigValue::String(
                crate::paths::cache_base(config.paths.cache_dir.as_deref())
                    .to_string_lossy()
                    .into_owned(),
            ))
        },
    },
];

fn edit_distance(left: &str, right: &str) -> usize {
    let mut previous: Vec<usize> = (0..=right.chars().count()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_char) in right.chars().enumerate() {
            current.push(std::cmp::min(
                std::cmp::min(current[right_index] + 1, previous[right_index + 1] + 1),
                previous[right_index] + usize::from(left_char != right_char),
            ));
        }
        previous = current;
    }
    previous.last().copied().unwrap_or_default()
}

fn unknown_key(key: &str) -> ConfigError {
    let max_distance = if key.len() < 8 { 2 } else { 3 };
    let suggestion = CONFIG_KEYS
        .iter()
        .map(|candidate| (candidate.name, edit_distance(key, candidate.name)))
        .filter(|(_, distance)| *distance <= max_distance)
        .min_by_key(|(_, distance)| *distance)
        .map(|(name, _)| name);
    ConfigError::UnknownKey {
        key: key.into(),
        suggestion,
    }
}

fn config_key(key: &str) -> Result<&'static ConfigKey, ConfigError> {
    CONFIG_KEYS
        .iter()
        .find(|candidate| candidate.name == key)
        .ok_or_else(|| unknown_key(key))
}

fn parse_value(key: &ConfigKey, value: &str) -> Result<ConfigValue, ConfigError> {
    match key.kind {
        ConfigValueKind::String => Ok(ConfigValue::String(value.into())),
        ConfigValueKind::Boolean => {
            value
                .parse()
                .map(ConfigValue::Boolean)
                .map_err(|_| ConfigError::InvalidValue {
                    key: key.name.into(),
                    value: value.into(),
                    expected: "true or false",
                })
        }
        ConfigValueKind::Integer => {
            value
                .parse()
                .map(ConfigValue::Integer)
                .map_err(|_| ConfigError::InvalidValue {
                    key: key.name.into(),
                    value: value.into(),
                    expected: "a decimal integer",
                })
        }
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

fn key_parts(key: &ConfigKey) -> (&str, &str) {
    key.name.split_once('.').unwrap()
}

fn key_is_stored(document: &DocumentMut, key: &ConfigKey) -> bool {
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
    Ok(CONFIG_KEYS
        .iter()
        .map(|key| ConfigEntry {
            key: key.name,
            value: (key.value)(&config)
                .map(|value| value.plain())
                .unwrap_or_else(|| "<unset>".into()),
            configured: key_is_stored(&document, key),
        })
        .collect())
}

pub fn get(key: &str) -> Result<String, ConfigError> {
    let key = config_key(key)?;
    let path = config_path()?;
    let (_, config) = read_config(&path)?;
    (key.value)(&config)
        .map(|value| value.plain())
        .ok_or_else(|| ConfigError::UnsetKey(key.name.into()))
}

pub fn set(key: &str, value: &str) -> Result<String, ConfigError> {
    let key = config_key(key)?;
    let value = parse_value(key, value)?;
    let path = config_path()?;
    let (contents, _) = read_config(&path)?;
    let mut document = parse_document(contents.as_deref().unwrap_or(""), &path)?;
    let (section, name) = key_parts(key);
    if !document.contains_key(section) {
        document[section] = Item::Table(Table::new());
    }
    document[section][name] = value.into_item();
    let config = validated_document(&document, &path)?;
    let effective = (key.value)(&config).unwrap().plain();
    write_document(&path, &document)?;
    Ok(effective)
}

pub fn unset(key: &str) -> Result<Option<String>, ConfigError> {
    let key = config_key(key)?;
    let path = config_path()?;
    let (contents, config) = read_config(&path)?;
    let (section, name) = key_parts(key);
    let Some(contents) = contents else {
        return Ok((key.value)(&config).map(|value| value.plain()));
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
    Ok((key.value)(&config).map(|value| value.plain()))
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
