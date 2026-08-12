use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::{Decompress, FlushDecompress, Status};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const RASTATE_VERSION: u8 = 1;
const TIMESTAMP_LEN: usize = 24;
const TEMP_PREFIX: &str = ".asobou-state-";
const MAX_STATE_FILE_SIZE: usize = 256 * 1024 * 1024;
const RZIP_HEADER_SIZE: usize = 20;
const RZIP_DEFAULT_CHUNK_SIZE: usize = 128 * 1024;
const RZIP_MAGIC: &[u8; 8] = b"#RZIPv\x01#";
const RZIP_MAX_CHUNK_SIZE: usize = 64 * 1024 * 1024;
// An RZIP payload is a RASTATE1 container, which adds at most 31 bytes
// over the core state: 8 identifier, 8 MEM header, 7 padding, 8 END.
const MAX_RZIP_CONTENT: usize = MAX_STATE_FILE_SIZE + 32;

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
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.filename())
    }
}

#[derive(Debug)]
pub enum StateError {
    Io(io::Error),
    ReadDir { path: PathBuf, source: io::Error },
    NotAState,
    UnsupportedVersion(u8),
    Truncated,
    TooLarge { size: usize, max: usize },
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
            Self::NotAState => write!(formatter, "not a valid state file"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported state format version {version}")
            }
            Self::Truncated => write!(formatter, "truncated state file"),
            Self::TooLarge { size, max } => {
                write!(formatter, "state is {size} bytes, maximum is {max} bytes")
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
            Self::Io(source) | Self::ReadDir { source, .. } => Some(source),
            _ => None,
        }
    }
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
    states_dir: &Path,
    backend: &dyn StateBackend,
    state_size: usize,
    core: &str,
    game: &str,
) -> Result<PathBuf, StateError> {
    save_state_in(
        states_dir,
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
    let container = encode(&scratch)?;
    let file_bytes = rzip_wrap(&container);
    let dir = root.join(core).join(game);
    fs::create_dir_all(&dir)?;
    let mut temp = tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .tempfile_in(&dir)?;
    temp.write_all(&file_bytes)?;
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

fn read_state_file(path: &Path) -> Result<Vec<u8>, StateError> {
    let len = fs::metadata(path).map_err(StateError::Io)?.len();
    if len > MAX_STATE_FILE_SIZE as u64 {
        return Err(StateError::TooLarge {
            size: usize::try_from(len).unwrap_or(usize::MAX),
            max: MAX_STATE_FILE_SIZE,
        });
    }
    fs::read(path).map_err(StateError::Io)
}

pub fn load_newest(
    states_dir: &Path,
    backend: &dyn StateBackend,
    core: &str,
    game: &str,
) -> Result<Option<PathBuf>, StateError> {
    load_newest_in(states_dir, backend, core, game)
}

fn load_newest_in(
    root: &Path,
    backend: &dyn StateBackend,
    core: &str,
    game: &str,
) -> Result<Option<PathBuf>, StateError> {
    let mut candidates = scan_candidates(root, core, game)?;
    candidates.sort_by_key(|candidate| std::cmp::Reverse((candidate.millis, candidate.counter)));
    let mut first_invalid = None;
    for candidate in candidates {
        let bytes = match read_state_file(&candidate.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                first_invalid.get_or_insert(error);
                continue;
            }
        };
        match load_state_bytes(backend, &bytes) {
            Ok(()) => return Ok(Some(candidate.path)),
            Err(StateError::UnserializationFailed) => {
                return Err(StateError::UnserializationFailed);
            }
            Err(error) => {
                first_invalid.get_or_insert(error);
                continue;
            }
        }
    }
    match first_invalid {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

pub fn load_from_path(backend: &dyn StateBackend, path: &Path) -> Result<(), StateError> {
    let bytes = read_state_file(path)?;
    load_state_bytes(backend, &bytes)
}

fn load_state_bytes(backend: &dyn StateBackend, bytes: &[u8]) -> Result<(), StateError> {
    let plain;
    let payload = if is_rzip(bytes) {
        plain = rzip_unwrap(bytes)?;
        parse_state(&plain)?
    } else {
        parse_state(bytes)?
    };
    if !backend.unserialize(payload) {
        return Err(StateError::UnserializationFailed);
    }
    Ok(())
}

struct Candidate {
    millis: i64,
    counter: u32,
    path: PathBuf,
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

pub fn list_states(states_dir: &Path) -> Result<StateList, StateError> {
    list_states_in(states_dir)
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
                let Some((name_game, timestamp, counter)) = parse_state_file_name(&name) else {
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
                list.entries.push(StateEntry {
                    core: core.clone(),
                    game: game.clone(),
                    timestamp,
                    path,
                    counter,
                });
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

fn is_rzip(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[..8] == RZIP_MAGIC
}

fn rzip_wrap(plain: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RZIP_HEADER_SIZE + plain.len() / 2);
    out.extend_from_slice(RZIP_MAGIC);
    out.extend_from_slice(&(RZIP_DEFAULT_CHUNK_SIZE as u32).to_le_bytes());
    out.extend_from_slice(&(plain.len() as u64).to_le_bytes());
    for chunk in plain.chunks(RZIP_DEFAULT_CHUNK_SIZE) {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(chunk).unwrap();
        let compressed = encoder.finish().unwrap();
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
    }
    out
}

fn rzip_unwrap(bytes: &[u8]) -> Result<Vec<u8>, StateError> {
    if bytes.len() < RZIP_HEADER_SIZE {
        return Err(StateError::Truncated);
    }
    let chunk_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let total = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
    if chunk_size == 0 || chunk_size > RZIP_MAX_CHUNK_SIZE {
        return Err(StateError::Truncated);
    }
    if total == 0 {
        return Err(StateError::Truncated);
    }
    if total > MAX_RZIP_CONTENT as u64 {
        return Err(StateError::TooLarge {
            size: usize::try_from(total).unwrap_or(usize::MAX),
            max: MAX_RZIP_CONTENT,
        });
    }
    let total = total as usize;
    let mut out = Vec::with_capacity(total);
    let mut pos = RZIP_HEADER_SIZE;
    while out.len() < total {
        if pos + 4 > bytes.len() {
            return Err(StateError::Truncated);
        }
        let compressed_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if compressed_len == 0 || compressed_len > chunk_size * 2 {
            return Err(StateError::Truncated);
        }
        if pos + compressed_len > bytes.len() {
            return Err(StateError::Truncated);
        }
        let expected = (total - out.len()).min(chunk_size);
        let decoded = inflate_chunk(&bytes[pos..pos + compressed_len], expected)?;
        if decoded.len() != expected {
            return Err(StateError::Truncated);
        }
        pos += compressed_len;
        out.extend_from_slice(&decoded);
    }
    Ok(out)
}

fn inflate_chunk(chunk: &[u8], expected: usize) -> Result<Vec<u8>, StateError> {
    let mut decoder = Decompress::new(true);
    let mut out = Vec::with_capacity(expected);
    let mut in_pos = 0usize;
    let mut buf = [0u8; 8192];
    loop {
        // miniz_oxide cannot resume a Finish call that did not complete,
        // so flush with None until the input is exhausted, then Finish.
        let flush = if in_pos == chunk.len() {
            FlushDecompress::Finish
        } else {
            FlushDecompress::None
        };
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(&chunk[in_pos..], &mut buf, flush)
            .map_err(|_| StateError::Truncated)?;
        let consumed = (decoder.total_in() - before_in) as usize;
        let produced = (decoder.total_out() - before_out) as usize;
        in_pos += consumed;
        out.extend_from_slice(&buf[..produced]);
        if out.len() > expected {
            return Err(StateError::Truncated);
        }
        match status {
            Status::StreamEnd => break,
            // BufError means the output buffer filled before the stream
            // ended; keep going as long as each call makes progress.
            Status::Ok | Status::BufError if consumed > 0 || produced > 0 => {}
            Status::Ok | Status::BufError => return Err(StateError::Truncated),
        }
    }
    if in_pos != chunk.len() {
        return Err(StateError::Truncated);
    }
    Ok(out)
}

fn encode(payload: &[u8]) -> Result<Vec<u8>, StateError> {
    let len = u32::try_from(payload.len()).map_err(|_| StateError::TooLarge {
        size: payload.len(),
        max: u32::MAX as usize,
    })?;
    let mut out = Vec::with_capacity(8 + 8 + payload.len().next_multiple_of(8) + 8);
    out.extend_from_slice(b"RASTATE");
    out.push(RASTATE_VERSION);
    out.extend_from_slice(b"MEM ");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(payload);
    out.resize(out.len().next_multiple_of(8), 0);
    out.extend_from_slice(b"END ");
    out.extend_from_slice(&0u32.to_le_bytes());
    Ok(out)
}

fn parse_state(bytes: &[u8]) -> Result<&[u8], StateError> {
    if bytes.len() < 8 || &bytes[..7] != b"RASTATE" {
        return Err(StateError::NotAState);
    }
    if bytes[7] != RASTATE_VERSION {
        return Err(StateError::UnsupportedVersion(bytes[7]));
    }
    let mut pos = 8usize;
    let mut payload = None;
    while pos + 8 <= bytes.len() {
        let marker = &bytes[pos..pos + 4];
        let block_len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        pos += 8;
        if marker == b"END " {
            break;
        }
        let remaining = bytes.len() - pos;
        if block_len > remaining {
            return Err(StateError::Truncated);
        }
        if marker == b"MEM " {
            payload = Some(&bytes[pos..pos + block_len]);
        }
        pos += block_len.next_multiple_of(8).min(remaining);
    }
    payload.ok_or(StateError::NotAState)
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

impl StateBackend for crate::emulation::libretro::LoadedGame {
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
        load_from_path(&backend, &path).unwrap();

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
        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
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

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
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

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
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

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
            .unwrap()
            .unwrap();

        assert_eq!(loaded, older);
        assert_eq!(backend.state.get(), 1);
    }

    #[test]
    fn load_newest_skips_rastate_files_without_a_mem_block() {
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
        let game_dir = dir.path().join("fceumm").join("game.nes");
        let mut headerless = Vec::new();
        headerless.extend_from_slice(b"RASTATE");
        headerless.push(1);
        headerless.extend_from_slice(b"END ");
        headerless.extend_from_slice(&0u32.to_le_bytes());
        fs::write(
            game_dir.join("game.nes.20260802T154831.027+0800.state"),
            headerless,
        )
        .unwrap();

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
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
        fs::write(&rejected, encode(&[0xff; 8]).unwrap()).unwrap();

        let error = load_newest_in(dir.path(), &backend, "fceumm", "game.nes").unwrap_err();

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

        let error = load_newest_in(dir.path(), &backend, "fceumm", "game.nes").unwrap_err();

        assert!(matches!(error, StateError::NotAState));
    }

    #[test]
    fn load_newest_returns_none_when_no_states_exist() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };

        assert!(
            load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
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
        let loaded = load_newest_in(dir.path(), &backend, "fceumm", game)
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
    fn saved_container_matches_the_retroarch_layout() {
        let container = encode(&[1, 2, 3, 4, 5]).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(b"RASTATE");
        expected.push(1);
        expected.extend_from_slice(b"MEM ");
        expected.extend_from_slice(&5u32.to_le_bytes());
        expected.extend_from_slice(&[1, 2, 3, 4, 5]);
        expected.extend_from_slice(&[0, 0, 0]);
        expected.extend_from_slice(b"END ");
        expected.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(container, expected);
    }

    #[test]
    fn saved_state_is_an_rzip_file() {
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

        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(RZIP_MAGIC));
        assert_eq!(
            rzip_unwrap(&bytes).unwrap(),
            encode(&42u64.to_le_bytes()).unwrap()
        );
    }

    #[test]
    fn rzip_wrap_matches_the_retroarch_layout() {
        let plain = encode(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let wrapped = rzip_wrap(&plain);

        assert_eq!(&wrapped[..8], RZIP_MAGIC);
        assert_eq!(
            u32::from_le_bytes(wrapped[8..12].try_into().unwrap()),
            RZIP_DEFAULT_CHUNK_SIZE as u32
        );
        assert_eq!(
            u64::from_le_bytes(wrapped[12..20].try_into().unwrap()),
            plain.len() as u64
        );
        // A single chunk: 4-byte compressed length followed by a zlib stream.
        let chunk_len = u32::from_le_bytes(wrapped[20..24].try_into().unwrap()) as usize;
        assert_eq!(wrapped.len(), 20 + 4 + chunk_len);
        let mut decoder = ZlibDecoder::new(&wrapped[24..]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, plain);
    }

    #[test]
    fn rzip_roundtrips_across_chunk_boundaries() {
        let near = (0..300_000u32).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        assert_eq!(rzip_unwrap(&rzip_wrap(&near)).unwrap(), near);

        let exact = (0..RZIP_DEFAULT_CHUNK_SIZE as u32)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>();
        assert_eq!(rzip_unwrap(&rzip_wrap(&exact)).unwrap(), exact);

        let one_over = (0..RZIP_DEFAULT_CHUNK_SIZE as u32 + 1)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>();
        assert_eq!(rzip_unwrap(&rzip_wrap(&one_over)).unwrap(), one_over);
    }

    fn retroarch_style_container() -> Vec<u8> {
        // Matches RetroArch's writer: identifier, an unknown block
        // (e.g. ACHV/RPLY or a future block), the MEM block, then END.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RASTATE");
        bytes.push(1);
        bytes.extend_from_slice(b"XXXX");
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&[0xaa; 4]);
        bytes.extend_from_slice(b"MEM ");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[42, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(b"END ");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    }

    #[test]
    fn load_accepts_an_uncompressed_retroarch_file() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = dir.path().join("fixture.state");
        fs::write(&path, retroarch_style_container()).unwrap();

        backend.state.set(7);
        load_from_path(&backend, &path).unwrap();

        assert_eq!(backend.state.get(), 42);
    }

    #[test]
    fn load_accepts_an_rzip_wrapped_retroarch_file() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let path = dir.path().join("fixture.state");
        fs::write(&path, rzip_wrap(&retroarch_style_container())).unwrap();

        backend.state.set(7);
        load_from_path(&backend, &path).unwrap();

        assert_eq!(backend.state.get(), 42);
    }

    #[test]
    fn saved_state_pads_payloads_to_8_byte_alignment() {
        assert_eq!(encode(&[1u8; 8]).unwrap().len(), 32);
        assert_eq!(encode(&[1u8; 9]).unwrap().len(), 40);
        assert_eq!(encode(&[1u8; 15]).unwrap().len(), 40);
        assert_eq!(encode(&[1u8; 16]).unwrap().len(), 40);
    }

    #[test]
    fn rzip_unwrap_rejects_malformed_headers() {
        let wrapped = rzip_wrap(&encode(&[1u8; 8]).unwrap());

        let mut no_chunk_size = wrapped.clone();
        no_chunk_size[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            rzip_unwrap(&no_chunk_size),
            Err(StateError::Truncated)
        ));

        let mut huge_chunk_size = wrapped.clone();
        huge_chunk_size[8..12].copy_from_slice(&(RZIP_MAX_CHUNK_SIZE as u32 + 1).to_le_bytes());
        assert!(matches!(
            rzip_unwrap(&huge_chunk_size),
            Err(StateError::Truncated)
        ));

        let mut no_total = wrapped.clone();
        no_total[12..20].copy_from_slice(&0u64.to_le_bytes());
        assert!(matches!(rzip_unwrap(&no_total), Err(StateError::Truncated)));

        let mut oversized = wrapped.clone();
        oversized[12..20].copy_from_slice(&(MAX_RZIP_CONTENT as u64 + 1).to_le_bytes());
        assert!(matches!(
            rzip_unwrap(&oversized),
            Err(StateError::TooLarge { .. })
        ));
    }

    #[test]
    fn rzip_unwrap_rejects_truncated_or_corrupt_chunks() {
        let wrapped = rzip_wrap(&encode(&[1u8; 64]).unwrap());

        assert!(matches!(
            rzip_unwrap(&wrapped[..20]),
            Err(StateError::Truncated)
        ));
        assert!(matches!(
            rzip_unwrap(&wrapped[..wrapped.len() - 3]),
            Err(StateError::Truncated)
        ));

        let mut corrupt = wrapped.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0xff;
        assert!(matches!(rzip_unwrap(&corrupt), Err(StateError::Truncated)));

        let mut lying = wrapped.clone();
        lying[20..24].copy_from_slice(&(wrapped.len() as u32).to_le_bytes());
        assert!(matches!(rzip_unwrap(&lying), Err(StateError::Truncated)));
    }

    #[test]
    fn load_newest_skips_corrupt_rzip_states() {
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
        let game_dir = dir.path().join("fceumm").join("game.nes");
        let mut corrupt = rzip_wrap(&encode(&[7u8; 8]).unwrap());
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0xff;
        fs::write(
            game_dir.join("game.nes.20260802T154831.027+0800.state"),
            corrupt,
        )
        .unwrap();

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
            .unwrap()
            .unwrap();

        assert_eq!(loaded, older);
        assert_eq!(backend.state.get(), 1);
    }

    #[test]
    fn load_rejects_unknown_rzip_versions() {
        let dir = tempfile::tempdir().unwrap();
        let backend = FakeBackend {
            state: Cell::new(1),
        };
        let mut wrapped = rzip_wrap(&encode(&[1u8; 8]).unwrap());
        wrapped[6] = 2;
        let path = dir.path().join("v2.state");
        fs::write(&path, wrapped).unwrap();

        let error = load_from_path(&backend, &path).unwrap_err();

        assert!(matches!(error, StateError::NotAState));
    }

    #[test]
    fn parse_state_skips_unknown_blocks_and_ignores_padding_bytes() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RASTATE");
        bytes.push(1);
        bytes.extend_from_slice(b"XXXX");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"abc");
        bytes.extend_from_slice(&[0xaa; 5]);
        bytes.extend_from_slice(b"MEM ");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[42, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(b"END ");
        bytes.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(parse_state(&bytes).unwrap(), &[42, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn parse_state_uses_the_last_mem_block() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RASTATE");
        bytes.push(1);
        bytes.extend_from_slice(b"MEM ");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[7, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(b"MEM ");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[42, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(b"END ");
        bytes.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(parse_state(&bytes).unwrap(), &[42, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn parse_state_loads_files_without_an_end_block() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RASTATE");
        bytes.push(1);
        bytes.extend_from_slice(b"MEM ");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[42, 0, 0, 0, 0, 0, 0, 0]);

        assert_eq!(parse_state(&bytes).unwrap(), &[42, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn parse_state_rejects_an_end_block_without_a_mem_block() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RASTATE");
        bytes.push(1);
        bytes.extend_from_slice(b"END ");
        bytes.extend_from_slice(&0u32.to_le_bytes());

        assert!(matches!(parse_state(&bytes), Err(StateError::NotAState)));
    }

    #[test]
    fn parse_state_rejects_truncated_blocks() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RASTATE");
        bytes.push(1);
        bytes.extend_from_slice(b"MEM ");
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);

        assert!(matches!(parse_state(&bytes), Err(StateError::Truncated)));
    }

    #[test]
    fn parse_state_rejects_every_other_format() {
        assert!(matches!(parse_state(&[]), Err(StateError::NotAState)));
        assert!(matches!(
            parse_state(b"RASTATE"),
            Err(StateError::NotAState)
        ));
        assert!(matches!(
            parse_state(b"JARO...."),
            Err(StateError::NotAState)
        ));
        // RZIP-compressed RetroArch states are treated as unknown formats.
        assert!(matches!(
            parse_state(b"#RZIPv\x01#........"),
            Err(StateError::NotAState)
        ));
        // Pre-2021 RetroArch states were bare core data.
        assert!(matches!(
            parse_state(&[1, 2, 3, 4, 5, 6, 7, 8]),
            Err(StateError::NotAState)
        ));
    }

    #[test]
    fn parse_state_rejects_future_versions() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RASTATE");
        bytes.push(2);
        bytes.extend_from_slice(b"MEM ");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[42, 0, 0, 0, 0, 0, 0, 0]);

        assert!(matches!(
            parse_state(&bytes),
            Err(StateError::UnsupportedVersion(2))
        ));
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
            .set_len(MAX_STATE_FILE_SIZE as u64 + 1)
            .unwrap();

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
            .unwrap()
            .unwrap();

        assert_eq!(loaded, valid);
        assert_eq!(backend.state.get(), 1);
    }

    #[test]
    fn read_state_file_rejects_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let oversized = dir.path().join("huge.state");
        fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_STATE_FILE_SIZE as u64 + 1)
            .unwrap();

        let error = read_state_file(&oversized).unwrap_err();

        assert!(matches!(error, StateError::TooLarge { .. }));
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
        fs::write(path.parent().unwrap().join(".asobou-state-abc"), b"x").unwrap();
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
        // A state file whose filename names a different game than its
        // directory cannot be distinguished from a foreign copy.
        let copied = other.join("game.nes.20260802T151205.903+0800.state");
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
        let states = dir.path().join("asobou").join("states");
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

        let loaded = load_newest_in(dir.path(), &backend, "fceumm", "game.nes")
            .unwrap()
            .unwrap();
        assert_eq!(loaded, suffixed);
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

        load_from_path(&backend, &renamed).unwrap();

        assert_eq!(backend.state.get(), 42);
    }
}
