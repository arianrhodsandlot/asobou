use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8] = b"ASOBYST";
const FORMAT_VERSION: u8 = 1;
const TIMESTAMP_LEN: usize = 24;
const HEADER_FIXED: usize = MAGIC.len() + 1 + 4 + 4 + 8 + 4 + 8;
const TEMP_PREFIX: &str = ".asoby-state-";
const CAPACITY_HINT_CAP: usize = 1 << 26;
const MAX_STATE_FILE_SIZE: usize = 256 * 1024 * 1024;

pub trait StateBackend {
    fn serialize(&self, data: &mut [u8]) -> bool;
    fn unserialize(&self, data: &[u8]) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp(chrono::DateTime<chrono::FixedOffset>);

impl Timestamp {
    pub fn now() -> Self {
        Self(chrono::Local::now().fixed_offset())
    }

    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != TIMESTAMP_LEN || bytes.get(15) != Some(&b'.') {
            return None;
        }
        chrono::DateTime::parse_from_str(text, "%Y%m%dT%H%M%S%.3f%z")
            .ok()
            .map(Self)
    }

    pub fn filename(&self) -> String {
        self.0.format("%Y%m%dT%H%M%S%.3f%z").to_string()
    }

    pub fn human(&self) -> String {
        self.0.format("%Y-%m-%d %H:%M:%S %:z").to_string()
    }

    fn unix_millis(&self) -> i64 {
        self.0.timestamp_millis()
    }

    fn offset_minutes(&self) -> i32 {
        self.0.offset().local_minus_utc() / 60
    }

    fn from_millis_offset(millis: i64, offset_minutes: i32) -> Option<Self> {
        let seconds = i64::from(offset_minutes).checked_mul(60)?;
        let offset = chrono::FixedOffset::east_opt(i32::try_from(seconds).ok()?)?;
        let datetime = chrono::DateTime::from_timestamp_millis(millis)?.with_timezone(&offset);
        Some(Self(datetime))
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.filename())
    }
}

#[derive(Debug)]
pub enum StateError {
    Io(io::Error),
    ReadDir {
        path: PathBuf,
        source: io::Error,
    },
    NotAState,
    UnsupportedVersion(u8),
    Truncated,
    CoreMismatch {
        expected: String,
        found: String,
    },
    RomMismatch {
        expected: String,
        found: String,
    },
    SizeMismatch {
        expected: usize,
        found: usize,
    },
    TooLarge {
        size: usize,
        max: usize,
    },
    InvalidTimestamp,
    TimestampMismatch {
        filename: Timestamp,
        recorded: Timestamp,
    },
    Decompression(io::Error),
    SerializationFailed,
    UnserializationFailed,
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(source) => write!(formatter, "I/O error: {source}"),
            Self::ReadDir { path, source } => {
                write!(
                    formatter,
                    "failed to read directory {}: {source}",
                    path.display()
                )
            }
            Self::NotAState => write!(formatter, "not an Asoby state file"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported state format version {version}")
            }
            Self::Truncated => write!(formatter, "truncated state file"),
            Self::CoreMismatch { expected, found } => write!(
                formatter,
                "state was saved by core \"{found}\", expected \"{expected}\""
            ),
            Self::RomMismatch { expected, found } => write!(
                formatter,
                "state was saved for ROM \"{found}\", expected \"{expected}\""
            ),
            Self::SizeMismatch { expected, found } => write!(
                formatter,
                "state size mismatch: file holds {found} bytes, expected {expected}"
            ),
            Self::TooLarge { size, max } => {
                write!(formatter, "state is {size} bytes, maximum is {max} bytes")
            }
            Self::InvalidTimestamp => write!(formatter, "invalid timestamp in state file"),
            Self::TimestampMismatch { filename, recorded } => write!(
                formatter,
                "filename timestamp {filename} does not match the recorded timestamp {recorded}"
            ),
            Self::Decompression(source) => {
                write!(formatter, "failed to decompress state: {source}")
            }
            Self::SerializationFailed => {
                write!(formatter, "core failed to serialize its state")
            }
            Self::UnserializationFailed => {
                write!(formatter, "core failed to restore the state")
            }
        }
    }
}

impl From<io::Error> for StateError {
    fn from(source: io::Error) -> Self {
        Self::Io(source)
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(source) | Self::Decompression(source) | Self::ReadDir { source, .. } => {
                Some(source)
            }
            _ => None,
        }
    }
}

pub fn states_dir() -> PathBuf {
    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::data_local_dir().unwrap());
    data_home.join("asoby").join("states")
}

pub fn core_name_from_path(path: &Path) -> String {
    path.file_name()
        .map(|name| core_name_from_file_name(&name.to_string_lossy()))
        .unwrap_or_else(|| "core".into())
}

fn core_name_from_file_name(name: &str) -> String {
    let without_ext = [".dylib", ".so", ".dll"]
        .iter()
        .find_map(|ext| name.strip_suffix(ext))
        .unwrap_or(name);
    without_ext
        .strip_suffix("_libretro")
        .unwrap_or(without_ext)
        .to_string()
}

pub fn save_state(
    backend: &dyn StateBackend,
    state_size: usize,
    core: &str,
    game: &str,
) -> Result<PathBuf, StateError> {
    save_state_in(
        &states_dir(),
        backend,
        state_size,
        core,
        game,
        Timestamp::now(),
    )
}

fn save_state_in(
    root: &Path,
    backend: &dyn StateBackend,
    state_size: usize,
    core: &str,
    game: &str,
    timestamp: Timestamp,
) -> Result<PathBuf, StateError> {
    let mut scratch = vec![0u8; state_size];
    if !backend.serialize(&mut scratch) {
        return Err(StateError::SerializationFailed);
    }
    let container = encode(core, game, timestamp, &scratch)?;
    let dir = root.join(core).join(game);
    fs::create_dir_all(&dir)?;
    let mut temp = tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .tempfile_in(&dir)?;
    temp.write_all(&container)?;
    temp.flush()?;
    let base = format!("{game}.{}", timestamp.filename());
    let mut counter = 1u32;
    loop {
        let name = if counter == 1 {
            format!("{base}.state")
        } else {
            format!("{base}-{counter}.state")
        };
        let path = dir.join(name);
        match temp.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                temp = error.file;
                counter += 1;
            }
            Err(error) => return Err(error.error.into()),
        }
    }
}

fn read_state_file(path: &Path, cap: Option<usize>) -> Result<Vec<u8>, StateError> {
    let len = fs::metadata(path).map_err(StateError::Io)?.len();
    if let Some(cap) = cap
        && len > cap as u64
    {
        return Err(StateError::TooLarge {
            size: usize::try_from(len).unwrap_or(usize::MAX),
            max: cap,
        });
    }
    fs::read(path).map_err(StateError::Io)
}

fn load_file_cap(state_size: usize) -> usize {
    state_size.saturating_mul(2).saturating_add(1 << 16)
}

pub fn load_newest(
    backend: &dyn StateBackend,
    core: &str,
    game: &str,
    state_size: usize,
) -> Result<Option<PathBuf>, StateError> {
    load_newest_in(&states_dir(), backend, core, game, state_size)
}

fn load_newest_in(
    root: &Path,
    backend: &dyn StateBackend,
    core: &str,
    game: &str,
    state_size: usize,
) -> Result<Option<PathBuf>, StateError> {
    let mut candidates = scan_candidates(root, core, game)?;
    candidates.sort_by_key(|candidate| std::cmp::Reverse((candidate.millis, candidate.counter)));
    let cap = load_file_cap(state_size);
    let mut first_invalid = None;
    for candidate in candidates {
        let bytes = match read_state_file(&candidate.path, Some(cap)) {
            Ok(bytes) => bytes,
            Err(error) => {
                first_invalid.get_or_insert(error);
                continue;
            }
        };
        let (payload, recorded) = match decode(&bytes, core, game, Some(state_size)) {
            Ok(decoded) => decoded,
            Err(error) => {
                first_invalid.get_or_insert(error);
                continue;
            }
        };
        if !timestamps_match(candidate.timestamp, recorded) {
            first_invalid.get_or_insert(StateError::TimestampMismatch {
                filename: candidate.timestamp,
                recorded,
            });
            continue;
        }
        if !backend.unserialize(&payload) {
            return Err(StateError::UnserializationFailed);
        }
        return Ok(Some(candidate.path));
    }
    match first_invalid {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

pub fn load_from_path(
    backend: &dyn StateBackend,
    path: &Path,
    core: &str,
    game: &str,
    state_size: usize,
) -> Result<(), StateError> {
    let bytes = read_state_file(path, Some(load_file_cap(state_size)))?;
    let (payload, _timestamp) = decode(&bytes, core, game, Some(state_size))?;
    if !backend.unserialize(&payload) {
        return Err(StateError::UnserializationFailed);
    }
    Ok(())
}

fn timestamps_match(filename: Timestamp, recorded: Timestamp) -> bool {
    filename.unix_millis() == recorded.unix_millis()
        && filename.offset_minutes() == recorded.offset_minutes()
}

struct Candidate {
    millis: i64,
    counter: u32,
    path: PathBuf,
    timestamp: Timestamp,
}

fn scan_candidates(root: &Path, core: &str, game: &str) -> Result<Vec<Candidate>, StateError> {
    let dir = root.join(core).join(game);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(StateError::ReadDir { path: dir, source }),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| StateError::ReadDir {
            path: dir.clone(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Some((name_game, timestamp, counter)) = parse_state_file_name(&name) else {
            continue;
        };
        if name_game != game {
            continue;
        }
        candidates.push(Candidate {
            millis: timestamp.unix_millis(),
            counter,
            path: entry.path(),
            timestamp,
        });
    }
    Ok(candidates)
}

#[derive(Debug)]
pub struct StateEntry {
    pub core: String,
    pub game: String,
    pub timestamp: Timestamp,
    pub path: PathBuf,
    counter: u32,
}

#[derive(Debug)]
pub struct MalformedState {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct StateList {
    pub entries: Vec<StateEntry>,
    pub malformed: Vec<MalformedState>,
}

pub fn list_states() -> Result<StateList, StateError> {
    list_states_in(&states_dir())
}

fn list_states_in(root: &Path) -> Result<StateList, StateError> {
    let mut list = StateList::default();
    let core_entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(list),
        Err(source) => {
            return Err(StateError::ReadDir {
                path: root.to_path_buf(),
                source,
            });
        }
    };
    for core_entry in core_entries {
        let core_entry = core_entry.map_err(|source| StateError::ReadDir {
            path: root.to_path_buf(),
            source,
        })?;
        let core_path = core_entry.path();
        if !core_entry
            .file_type()
            .map_err(|source| StateError::ReadDir {
                path: core_path.clone(),
                source,
            })?
            .is_dir()
        {
            list.malformed.push(MalformedState {
                path: core_path,
                reason: "not a directory".into(),
            });
            continue;
        }
        let Some(core) = core_entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let game_entries = match fs::read_dir(&core_path) {
            Ok(entries) => entries,
            Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(StateError::ReadDir {
                    path: core_path,
                    source,
                });
            }
        };
        for game_entry in game_entries {
            let game_entry = game_entry.map_err(|source| StateError::ReadDir {
                path: core_path.clone(),
                source,
            })?;
            let game_path = game_entry.path();
            if !game_entry
                .file_type()
                .map_err(|source| StateError::ReadDir {
                    path: game_path.clone(),
                    source,
                })?
                .is_dir()
            {
                list.malformed.push(MalformedState {
                    path: game_path,
                    reason: "not a directory".into(),
                });
                continue;
            }
            let Some(game) = game_entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let file_entries = match fs::read_dir(&game_path) {
                Ok(entries) => entries,
                Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(StateError::ReadDir {
                        path: game_path,
                        source,
                    });
                }
            };
            for file_entry in file_entries {
                let file_entry = file_entry.map_err(|source| StateError::ReadDir {
                    path: game_path.clone(),
                    source,
                })?;
                let path = file_entry.path();
                let name = file_entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let Some((name_game, filename_timestamp, counter)) = parse_state_file_name(&name)
                else {
                    list.malformed.push(MalformedState {
                        path,
                        reason: "name does not match the state naming scheme".into(),
                    });
                    continue;
                };
                if name_game != game {
                    list.malformed.push(MalformedState {
                        path,
                        reason: format!(
                            "filename names game \"{name_game}\", directory is \"{game}\""
                        ),
                    });
                    continue;
                }
                let bytes = match read_state_file(&path, Some(MAX_STATE_FILE_SIZE)) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        list.malformed.push(MalformedState {
                            path,
                            reason: error.to_string(),
                        });
                        continue;
                    }
                };
                match decode(&bytes, &core, &game, None) {
                    Ok((_payload, recorded)) => {
                        if !timestamps_match(filename_timestamp, recorded) {
                            list.malformed.push(MalformedState {
                                path,
                                reason: format!(
                                    "filename timestamp {filename_timestamp} does not match the recorded timestamp {recorded}"
                                ),
                            });
                            continue;
                        }
                        list.entries.push(StateEntry {
                            core: core.clone(),
                            game: game.clone(),
                            timestamp: recorded,
                            path,
                            counter,
                        });
                    }
                    Err(error) => list.malformed.push(MalformedState {
                        path,
                        reason: error.to_string(),
                    }),
                }
            }
        }
    }
    list.entries.sort_by(|a, b| {
        (
            a.core.as_str(),
            a.game.as_str(),
            a.timestamp.unix_millis(),
            a.counter,
        )
            .cmp(&(
                b.core.as_str(),
                b.game.as_str(),
                b.timestamp.unix_millis(),
                b.counter,
            ))
    });
    Ok(list)
}

fn parse_state_file_name(name: &str) -> Option<(String, Timestamp, u32)> {
    let stem = name.strip_suffix(".state")?;
    if let Some((game, timestamp)) = parse_timestamp_suffix(stem) {
        return Some((game.to_string(), timestamp, 1));
    }
    let (base, counter) = split_counter(stem)?;
    let (game, timestamp) = parse_timestamp_suffix(base)?;
    Some((game.to_string(), timestamp, counter))
}

fn parse_timestamp_suffix(text: &str) -> Option<(&str, Timestamp)> {
    let start = text.len().checked_sub(TIMESTAMP_LEN)?;
    let game_end = start.checked_sub(1)?;
    if &text[game_end..start] != "." {
        return None;
    }
    let timestamp_text = text.get(start..)?;
    let game = &text[..game_end];
    Timestamp::parse(timestamp_text).map(|timestamp| (game, timestamp))
}

fn split_counter(text: &str) -> Option<(&str, u32)> {
    let (base, digits) = text.rsplit_once('-')?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((base, digits.parse().ok()?))
}

fn encode(
    core: &str,
    game: &str,
    timestamp: Timestamp,
    payload: &[u8],
) -> Result<Vec<u8>, StateError> {
    let mut out = Vec::with_capacity(HEADER_FIXED + core.len() + game.len() + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    write_string(&mut out, core)?;
    write_string(&mut out, game)?;
    out.extend_from_slice(&timestamp.unix_millis().to_le_bytes());
    out.extend_from_slice(&timestamp.offset_minutes().to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(payload)?;
    out.extend_from_slice(&encoder.finish()?);
    Ok(out)
}

fn decode(
    bytes: &[u8],
    expected_core: &str,
    expected_game: &str,
    expected_size: Option<usize>,
) -> Result<(Vec<u8>, Timestamp), StateError> {
    if bytes.len() < MAGIC.len() + 1 {
        return Err(StateError::Truncated);
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(StateError::NotAState);
    }
    let version = bytes[MAGIC.len()];
    if version != FORMAT_VERSION {
        return Err(StateError::UnsupportedVersion(version));
    }
    let mut pos = MAGIC.len() + 1;
    let core = read_string(bytes, &mut pos)?;
    let game = read_string(bytes, &mut pos)?;
    if pos + 8 + 4 + 8 > bytes.len() {
        return Err(StateError::Truncated);
    }
    let millis = i64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
    let offset = i32::from_le_bytes(bytes[pos + 8..pos + 12].try_into().unwrap());
    let size = u64::from_le_bytes(bytes[pos + 12..pos + 20].try_into().unwrap());
    pos += 20;
    let timestamp =
        Timestamp::from_millis_offset(millis, offset).ok_or(StateError::InvalidTimestamp)?;
    if core != expected_core {
        return Err(StateError::CoreMismatch {
            expected: expected_core.into(),
            found: core,
        });
    }
    if game != expected_game {
        return Err(StateError::RomMismatch {
            expected: expected_game.into(),
            found: game,
        });
    }
    if let Some(expected) = expected_size
        && size != expected as u64
    {
        return Err(StateError::SizeMismatch {
            expected,
            found: size as usize,
        });
    }
    let declared = usize::try_from(size).map_err(|_| StateError::Truncated)?;
    if expected_size.is_none() && declared > MAX_STATE_FILE_SIZE {
        return Err(StateError::TooLarge {
            size: declared,
            max: MAX_STATE_FILE_SIZE,
        });
    }
    let payload = &bytes[pos..];
    let mut decoder = ZlibDecoder::new(payload);
    let mut output = Vec::with_capacity(declared.min(CAPACITY_HINT_CAP));
    let limit = declared.saturating_add(1) as u64;
    let read = decoder
        .by_ref()
        .take(limit)
        .read_to_end(&mut output)
        .map_err(StateError::Decompression)?;
    if read != declared {
        return Err(StateError::SizeMismatch {
            expected: declared,
            found: read,
        });
    }
    if decoder.total_in() != payload.len() as u64 {
        return Err(StateError::Decompression(io::Error::new(
            io::ErrorKind::InvalidData,
            "trailing data after compressed payload",
        )));
    }
    Ok((output, timestamp))
}

fn read_string(bytes: &[u8], pos: &mut usize) -> Result<String, StateError> {
    if *pos + 4 > bytes.len() {
        return Err(StateError::Truncated);
    }
    let len = u32::from_le_bytes(bytes[*pos..*pos + 4].try_into().unwrap()) as usize;
    *pos += 4;
    let end = pos.checked_add(len).ok_or(StateError::Truncated)?;
    if end > bytes.len() {
        return Err(StateError::Truncated);
    }
    let text = std::str::from_utf8(&bytes[*pos..end])
        .map_err(|_| StateError::Truncated)?
        .to_string();
    *pos = end;
    Ok(text)
}

fn write_string(out: &mut Vec<u8>, text: &str) -> Result<(), StateError> {
    let len = u32::try_from(text.len()).map_err(|_| StateError::Truncated)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::with_capacity(data.len() / 4), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

pub(crate) fn decompress(data: &[u8], capacity: usize) -> Vec<u8> {
    let mut decoder = ZlibDecoder::new(data);
    let mut output = Vec::with_capacity(capacity);
    decoder.read_to_end(&mut output).unwrap();
    output
}

impl StateBackend for crate::emulation::libretro::Core {
    fn serialize(&self, data: &mut [u8]) -> bool {
        self.serialize_state(data)
    }

    fn unserialize(&self, data: &[u8]) -> bool {
        self.unserialize_state(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    struct FakeBackend {
        state: Cell<u64>,
    }

    impl StateBackend for FakeBackend {
        fn serialize(&self, data: &mut [u8]) -> bool {
            data.copy_from_slice(&self.state.get().to_le_bytes());
            true
        }

        fn unserialize(&self, data: &[u8]) -> bool {
            if data.len() < 8 || data[0] == 0xff {
                return false;
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&data[..8]);
            self.state.set(u64::from_le_bytes(bytes));
            true
        }
    }

    fn ts(text: &str) -> Timestamp {
        Timestamp::parse(text).unwrap()
    }

    fn save_at(
        root: &Path,
        core: &str,
        game: &str,
        backend: &dyn StateBackend,
        timestamp: Timestamp,
    ) -> PathBuf {
        save_state_in(root, backend, 8, core, game, timestamp).unwrap()
    }

    #[test]
    fn saving_and_loading_restores_a_backend_state() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(42),
        };
        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );

        backend.state.set(7);
        load_from_path(&backend, &path, "fceumm", "game.nes", 8).unwrap();

        assert_eq!(backend.state.get(), 42);
    }

    #[test]
    fn each_save_creates_a_new_timestamped_file() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let first = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let second = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T154831.027+0800"),
        );

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, second);
    }

    #[test]
    fn timestamp_collisions_get_deterministic_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let first = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let second = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let third = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );

        assert_eq!(
            first.file_name().unwrap(),
            "game.nes.20260802T151205.903+0800.state"
        );
        assert_eq!(
            second.file_name().unwrap(),
            "game.nes.20260802T151205.903+0800-2.state"
        );
        assert_eq!(
            third.file_name().unwrap(),
            "game.nes.20260802T151205.903+0800-3.state"
        );
    }

    #[test]
    fn save_never_overwrites_a_pre_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("fceumm").join("game.nes");
        fs::create_dir_all(&game_dir).unwrap();
        let target = game_dir.join("game.nes.20260802T151205.903+0800.state");
        fs::write(&target, b"pre-existing").unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };

        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );

        assert_eq!(
            path.file_name().unwrap(),
            "game.nes.20260802T151205.903+0800-2.state"
        );
        assert_eq!(fs::read(&target).unwrap(), b"pre-existing");
    }

    #[test]
    fn newest_state_prefers_absolute_instants_over_wall_clock_order() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        // During the 2026 US fall-back, 01:00 PST (-0800) occurs after
        // 01:30 PDT (-0700) even though its wall-clock time is earlier.
        save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20261101T013000.000-0700"),
        );
        let later = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20261101T010000.000-0800"),
        );

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, later);
    }

    #[test]
    fn newest_state_ignores_file_modification_times() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let older = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let newer = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T154831.027+0800"),
        );
        let base = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let touched = base + Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&newer)
            .unwrap()
            .set_modified(touched)
            .unwrap();
        fs::File::options()
            .write(true)
            .open(&older)
            .unwrap()
            .set_modified(base)
            .unwrap();

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, newer);
    }

    #[test]
    fn load_newest_skips_a_corrupt_newest_and_loads_the_older_valid_state() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let older = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let newest = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T154831.027+0800"),
        );
        fs::write(&newest, b"corrupt contents").unwrap();

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8)
            .unwrap()
            .unwrap();

        assert_eq!(loaded, older);
        assert_eq!(backend.state.get(), 1);
    }

    #[test]
    fn load_newest_stops_on_unserialize_failure() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        // A newer file with a valid container whose payload the core rejects.
        let game_dir = dir.path().join("fceumm").join("game.nes");
        let rejected = game_dir.join("game.nes.20260802T154831.027+0800.state");
        let container = encode(
            "fceumm",
            "game.nes",
            ts("20260802T154831.027+0800"),
            &[0xff; 8],
        )
        .unwrap();
        fs::write(&rejected, container).unwrap();

        let error = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8).unwrap_err();

        assert!(matches!(error, StateError::UnserializationFailed));
        assert_eq!(backend.state.get(), 1);
    }

    #[test]
    fn load_newest_reports_when_every_candidate_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        fs::write(&path, b"corrupt contents").unwrap();

        let error = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8).unwrap_err();

        assert!(matches!(error, StateError::NotAState));
    }

    #[test]
    fn load_newest_returns_none_when_no_states_exist() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };

        assert!(
            load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn state_paths_preserve_full_rom_names_and_arbitrary_cores() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = save_at(
            dir.path(),
            "some_core",
            "My Game (v1.2) [rev A].nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );

        assert_eq!(
            path.parent().unwrap(),
            dir.path()
                .join("some_core")
                .join("My Game (v1.2) [rev A].nes")
        );
        assert!(
            path.to_string_lossy()
                .ends_with("My Game (v1.2) [rev A].nes.20260802T151205.903+0800.state")
        );
    }

    #[test]
    fn unicode_rom_names_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let game = "ドンキーコング (J).nes";
        let path = save_at(
            dir.path(),
            "fceumm",
            game,
            &backend,
            ts("20260802T151205.903+0800"),
        );

        let list = list_states_in(dir.path()).unwrap();
        assert_eq!(list.entries[0].game, game);
        let loaded = load_newest_in(dir.path(), &backend, "fceumm", game, 8)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, path);
    }

    #[test]
    fn core_name_strips_libretro_suffix_and_platform_extension() {
        assert_eq!(
            core_name_from_path(Path::new("/cores/fceumm_libretro.dylib")),
            "fceumm"
        );
        assert_eq!(
            core_name_from_path(Path::new("nestopia_libretro.so")),
            "nestopia"
        );
        assert_eq!(
            core_name_from_path(Path::new("genesis_plus_gx_libretro.dll")),
            "genesis_plus_gx"
        );
        assert_eq!(
            core_name_from_path(Path::new("/tmp/custom_core.so")),
            "custom_core"
        );
        assert_eq!(core_name_from_path(Path::new("plain_name")), "plain_name");
    }

    #[test]
    fn timestamps_roundtrip_and_format_for_humans() {
        let positive = ts("20260802T151205.903+0800");
        assert_eq!(positive.filename(), "20260802T151205.903+0800");
        assert_eq!(positive.human(), "2026-08-02 15:12:05 +08:00");

        let negative = ts("20261101T010000.000-0800");
        assert_eq!(negative.human(), "2026-11-01 01:00:00 -08:00");

        assert!(Timestamp::parse("not-a-timestamp").is_none());
        assert!(Timestamp::parse("20260802T151205+0800").is_none());
    }

    #[test]
    fn decode_rejects_corrupt_containers() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let bytes = fs::read(&path).unwrap();
        let header_len = HEADER_FIXED + "fceumm".len() + "game.nes".len();

        let mut bad_magic = bytes.clone();
        bad_magic[0] = b'X';
        assert!(matches!(
            decode(&bad_magic, "fceumm", "game.nes", Some(8)),
            Err(StateError::NotAState)
        ));

        let mut bad_version = bytes.clone();
        bad_version[MAGIC.len()] = 99;
        assert!(matches!(
            decode(&bad_version, "fceumm", "game.nes", Some(8)),
            Err(StateError::UnsupportedVersion(99))
        ));

        assert!(matches!(
            decode(&bytes[..12], "fceumm", "game.nes", Some(8)),
            Err(StateError::Truncated)
        ));
        assert!(matches!(
            decode(&bytes[..header_len + 5], "fceumm", "game.nes", Some(8)),
            Err(StateError::Decompression(_) | StateError::SizeMismatch { .. })
        ));

        assert!(matches!(
            decode(&bytes, "nestopia", "game.nes", Some(8)),
            Err(StateError::CoreMismatch { .. })
        ));
        assert!(matches!(
            decode(&bytes, "fceumm", "other.nes", Some(8)),
            Err(StateError::RomMismatch { .. })
        ));
        assert!(matches!(
            decode(&bytes, "fceumm", "game.nes", Some(16)),
            Err(StateError::SizeMismatch { .. })
        ));

        let mut garbage_payload = bytes[..header_len].to_vec();
        garbage_payload.extend_from_slice(b"this is not zlib data");
        assert!(matches!(
            decode(&garbage_payload, "fceumm", "game.nes", Some(8)),
            Err(StateError::Decompression(_))
        ));
    }

    #[test]
    fn decode_rejects_payloads_larger_than_the_declared_size() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let bytes = fs::read(&path).unwrap();
        let header_len = HEADER_FIXED + "fceumm".len() + "game.nes".len();
        // Header declares 8 bytes but the payload expands to 16.
        let mut oversized = bytes[..header_len].to_vec();
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&[0xaa; 16]).unwrap();
        oversized.extend_from_slice(&encoder.finish().unwrap());

        assert!(matches!(
            decode(&oversized, "fceumm", "game.nes", Some(8)),
            Err(StateError::SizeMismatch { .. })
        ));
        assert!(matches!(
            decode(&oversized, "fceumm", "game.nes", None),
            Err(StateError::SizeMismatch { .. })
        ));
    }

    #[test]
    fn decode_rejects_invalid_embedded_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let mut bytes = fs::read(&path).unwrap();
        // The offset sits 8 + 4+6 + 4+8 bytes after the magic, followed by
        // the 8-byte millis field: patch the 4-byte offset with a value
        // beyond the valid +/-24h range.
        let offset_pos = 8 + 4 + "fceumm".len() + 4 + "game.nes".len() + 8;
        bytes[offset_pos..offset_pos + 4].copy_from_slice(&9_999_999i32.to_le_bytes());

        assert!(matches!(
            decode(&bytes, "fceumm", "game.nes", Some(8)),
            Err(StateError::InvalidTimestamp)
        ));
    }

    #[test]
    fn decode_rejects_oversized_declared_sizes_when_listing() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let mut bytes = fs::read(&path).unwrap();
        let size_pos = 8 + 4 + "fceumm".len() + 4 + "game.nes".len() + 8 + 4;
        bytes[size_pos..size_pos + 8]
            .copy_from_slice(&(MAX_STATE_FILE_SIZE as u64 + 1).to_le_bytes());

        assert!(matches!(
            decode(&bytes, "fceumm", "game.nes", None),
            Err(StateError::TooLarge { .. })
        ));
        assert!(matches!(
            decode(&bytes, "fceumm", "game.nes", Some(8)),
            Err(StateError::SizeMismatch { .. })
        ));
    }

    #[test]
    fn list_states_reports_timestamp_mismatches() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let game_dir = dir.path().join("fceumm").join("game.nes");
        let source = game_dir.join("game.nes.20260802T151205.903+0800.state");
        // Renaming the file invents a newer-looking timestamp.
        let renamed = game_dir.join("game.nes.20260802T160000.000+0800.state");
        fs::copy(&source, &renamed).unwrap();

        let list = list_states_in(dir.path()).unwrap();

        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].timestamp, ts("20260802T151205.903+0800"));
        assert_eq!(list.malformed.len(), 1);
        assert_eq!(list.malformed[0].path, renamed);
        assert!(list.malformed[0].reason.contains("does not match"));
    }

    #[test]
    fn load_newest_skips_renamed_states() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let newer = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T154831.027+0800"),
        );
        // A copy renamed to a later timestamp must not become the newest.
        let game_dir = dir.path().join("fceumm").join("game.nes");
        fs::copy(
            &newer,
            game_dir.join("game.nes.20260802T160000.000+0800.state"),
        )
        .unwrap();

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8)
            .unwrap()
            .unwrap();

        assert_eq!(loaded, newer);
        assert_eq!(backend.state.get(), 1);
    }

    #[test]
    fn load_from_path_accepts_renamed_files() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(42),
        };
        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let renamed = dir.path().join("my-backup.state");
        fs::copy(&path, &renamed).unwrap();
        backend.state.set(7);

        load_from_path(&backend, &renamed, "fceumm", "game.nes", 8).unwrap();

        assert_eq!(backend.state.get(), 42);
    }

    #[test]
    fn list_states_reports_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let game_dir = dir.path().join("fceumm").join("game.nes");
        let oversized = game_dir.join("game.nes.20260802T160000.000+0800.state");
        fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_STATE_FILE_SIZE as u64 + 1)
            .unwrap();

        let list = list_states_in(dir.path()).unwrap();

        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.malformed.len(), 1);
        assert_eq!(list.malformed[0].path, oversized);
        assert!(list.malformed[0].reason.contains("maximum"));
    }

    #[test]
    fn load_newest_skips_oversized_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let valid = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let game_dir = dir.path().join("fceumm").join("game.nes");
        let oversized = game_dir.join("game.nes.20260802T160000.000+0800.state");
        fs::File::create(&oversized)
            .unwrap()
            .set_len(load_file_cap(8) as u64 + 1)
            .unwrap();

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8)
            .unwrap()
            .unwrap();

        assert_eq!(loaded, valid);
        assert_eq!(backend.state.get(), 1);
    }

    #[test]
    fn failed_save_leaves_no_discoverable_state() {
        struct FailingBackend;
        impl StateBackend for FailingBackend {
            fn serialize(&self, _data: &mut [u8]) -> bool {
                false
            }

            fn unserialize(&self, _data: &[u8]) -> bool {
                false
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let result = save_state_in(
            dir.path(),
            &FailingBackend,
            8,
            "fceumm",
            "game.nes",
            ts("20260802T151205.903+0800"),
        );

        assert!(matches!(result, Err(StateError::SerializationFailed)));
        let list = list_states_in(dir.path()).unwrap();
        assert!(list.entries.is_empty());
        assert!(list.malformed.is_empty());
    }

    #[test]
    fn list_states_skips_temp_files_and_reports_malformed_names() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        fs::write(path.parent().unwrap().join(".asoby-state-abc"), b"x").unwrap();
        let stray = path.parent().unwrap().join("garbage.bin");
        fs::write(&stray, b"x").unwrap();

        let list = list_states_in(dir.path()).unwrap();

        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].core, "fceumm");
        assert_eq!(list.entries[0].game, "game.nes");
        assert_eq!(list.entries[0].timestamp, ts("20260802T151205.903+0800"));
        assert_eq!(list.entries[0].path, path);
        assert_eq!(list.malformed.len(), 1);
        assert_eq!(list.malformed[0].path, stray);
        assert!(list.malformed[0].reason.contains("naming scheme"));
    }

    #[test]
    fn list_states_reports_corrupt_containers_with_valid_names() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let game_dir = dir.path().join("fceumm").join("game.nes");
        let corrupt = game_dir.join("game.nes.20260802T160000.000+0800.state");
        fs::write(&corrupt, b"not a state").unwrap();

        let list = list_states_in(dir.path()).unwrap();

        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.malformed.len(), 1);
        assert_eq!(list.malformed[0].path, corrupt);
        assert!(list.malformed[0].reason.contains("not an Asoby state"));
    }

    #[test]
    fn list_states_reports_states_in_the_wrong_game_directory() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let source = dir
            .path()
            .join("fceumm")
            .join("game.nes")
            .join("game.nes.20260802T151205.903+0800.state");
        let other = dir.path().join("fceumm").join("other.nes");
        fs::create_dir_all(&other).unwrap();
        let copied = other.join("other.nes.20260802T151205.903+0800.state");
        fs::copy(&source, &copied).unwrap();

        let list = list_states_in(dir.path()).unwrap();

        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.malformed.len(), 1);
        assert_eq!(list.malformed[0].path, copied);
        assert!(list.malformed[0].reason.contains("game.nes"));
    }

    #[test]
    fn list_states_propagates_directory_errors() {
        let dir = tempfile::tempdir().unwrap();
        let states = dir.path().join("asoby").join("states");
        fs::create_dir_all(states.parent().unwrap()).unwrap();
        fs::write(&states, b"not a directory").unwrap();

        let error = list_states_in(&states).unwrap_err();

        assert!(matches!(error, StateError::ReadDir { .. }));
    }

    #[test]
    fn list_states_parses_counter_suffixes_and_dotted_rom_names() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let collided = save_at(
            dir.path(),
            "fceumm",
            "Super Mario Bros.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let suffixed = save_at(
            dir.path(),
            "fceumm",
            "Super Mario Bros.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );

        let list = list_states_in(dir.path()).unwrap();

        assert_eq!(list.entries.len(), 2);
        assert_eq!(
            list.entries[0].path.file_name().unwrap(),
            "Super Mario Bros.nes.20260802T151205.903+0800.state"
        );
        assert_eq!(
            list.entries[1].path.file_name().unwrap(),
            "Super Mario Bros.nes.20260802T151205.903+0800-2.state"
        );
        assert_eq!(list.entries[0].game, "Super Mario Bros.nes");
        assert_eq!(list.entries[1].game, "Super Mario Bros.nes");
        let _ = (collided, suffixed);
    }

    #[test]
    fn newest_state_handles_counter_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );
        let suffixed = save_at(
            dir.path(),
            "fceumm",
            "game.nes",
            &backend,
            ts("20260802T151205.903+0800"),
        );

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes", 8)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, suffixed);
    }
}
