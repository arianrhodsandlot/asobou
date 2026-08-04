use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
struct Source {
    extension: &'static str,
    repository: &'static str,
}

const SOURCES: &[Source] = &[
    Source {
        extension: "gbc",
        repository: "retrobrews/gbc-games",
    },
    Source {
        extension: "rom",
        repository: "retrobrews/colecovision-games",
    },
    Source {
        extension: "nes",
        repository: "retrobrews/nes-games",
    },
    Source {
        extension: "sms",
        repository: "retrobrews/sms-games",
    },
    Source {
        extension: "gba",
        repository: "retrobrews/gba-games",
    },
    Source {
        extension: "sfc",
        repository: "retrobrews/snes-games",
    },
    Source {
        extension: "d64",
        repository: "retrobrews/c64-games",
    },
    Source {
        extension: "tap",
        repository: "retrobrews/zxspectrum-games",
    },
];

pub fn download(game: &str) -> Result<PathBuf, String> {
    let source = source_for(game)?;
    let cache_dir = cache_dir()?;
    if let Some(cached) = cached_rom(&cache_dir, game) {
        return Ok(cached);
    }
    let cached = cache_dir.join(game);

    std::fs::create_dir_all(&cache_dir).map_err(|error| {
        format!(
            "Could not create cache directory {}: {error}",
            cache_dir.display()
        )
    })?;

    let url = source_url(source, game);
    eprintln!("Downloading {game}...");
    eprintln!("  From: {url}");

    let response = crate::cores::http_agent()
        .get(&url)
        .call()
        .map_err(|error| format!("Download failed: {error}"))?;

    if response.status() != 200 {
        return Err(format!(
            "HTTP {} while downloading {game}",
            response.status()
        ));
    }

    let content_length = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if let Some(length) = content_length
        && length > MAX_DOWNLOAD_BYTES
    {
        return Err(format!(
            "Download size ({length} bytes) exceeds maximum ({MAX_DOWNLOAD_BYTES} bytes)"
        ));
    }

    let mut temporary = tempfile::NamedTempFile::new_in(&cache_dir)
        .map_err(|error| format!("Could not create cache file: {error}"))?;
    let mut reader = response
        .into_body()
        .into_reader()
        .take(MAX_DOWNLOAD_BYTES + 1);
    let copied = std::io::copy(&mut reader, &mut temporary)
        .map_err(|error| format!("Could not read download: {error}"))?;
    if copied > MAX_DOWNLOAD_BYTES {
        return Err("Download exceeded maximum size".to_string());
    }
    temporary
        .flush()
        .map_err(|error| format!("Could not write cache file: {error}"))?;

    match temporary.persist(&cached) {
        Ok(_) => Ok(cached),
        Err(_error) if cached.is_file() => Ok(cached),
        Err(error) => Err(format!("Could not save downloaded ROM: {}", error.error)),
    }
}

fn cache_dir() -> Result<PathBuf, String> {
    cache_dir_from(
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
        dirs::cache_dir(),
    )
}

fn cache_dir_from(
    cache_home: Option<PathBuf>,
    platform_cache: Option<PathBuf>,
) -> Result<PathBuf, String> {
    cache_home
        .or(platform_cache)
        .map(|base| base.join("asoby").join("brew"))
        .ok_or_else(|| "Could not determine a cache directory".to_string())
}

fn cached_rom(cache_dir: &Path, game: &str) -> Option<PathBuf> {
    let cached = cache_dir.join(game);
    cached.is_file().then_some(cached)
}

fn source_for(game: &str) -> Result<&'static Source, String> {
    let path = Path::new(game);
    if path.file_name().and_then(|name| name.to_str()) != Some(game) {
        return Err(format!("Game must be a filename, not a path: {game}"));
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .ok_or_else(|| format!("Unsupported homebrew game: {game}"))?;
    SOURCES
        .iter()
        .find(|source| source.extension.eq_ignore_ascii_case(extension))
        .ok_or_else(|| format!("Unsupported homebrew game extension: .{extension}"))
}

fn source_url(source: &Source, game: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/refs/heads/master/{}",
        source.repository,
        percent_encode_path_segment(game)
    )
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions_select_the_expected_repository() {
        let cases = [
            ("game.gbc", "retrobrews/gbc-games"),
            ("game.rom", "retrobrews/colecovision-games"),
            ("game.nes", "retrobrews/nes-games"),
            ("game.sms", "retrobrews/sms-games"),
            ("game.gba", "retrobrews/gba-games"),
            ("game.sfc", "retrobrews/snes-games"),
            ("game.d64", "retrobrews/c64-games"),
            ("game.tap", "retrobrews/zxspectrum-games"),
        ];

        for (game, repository) in cases {
            assert_eq!(source_for(game).unwrap().repository, repository);
        }
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        let error = source_for("game.bin").unwrap_err();

        assert!(error.contains(".bin"));
    }

    #[test]
    fn path_is_rejected() {
        let error = source_for("games/game.nes").unwrap_err();

        assert!(error.contains("filename"));
    }

    #[test]
    fn url_encodes_the_game_filename() {
        let source = source_for("my game.nes").unwrap();

        assert_eq!(
            source_url(source, "my game.nes"),
            "https://raw.githubusercontent.com/retrobrews/nes-games/refs/heads/master/my%20game.nes"
        );
    }

    #[test]
    fn cache_directory_uses_xdg_cache_home() {
        let directory =
            cache_dir_from(Some(PathBuf::from("/cache")), Some(PathBuf::from("/other"))).unwrap();

        assert_eq!(directory, PathBuf::from("/cache/asoby/brew"));
    }

    #[test]
    fn cache_directory_uses_platform_cache_when_xdg_is_unset() {
        let directory = cache_dir_from(None, Some(PathBuf::from("/cache"))).unwrap();

        assert_eq!(directory, PathBuf::from("/cache/asoby/brew"));
    }

    #[test]
    fn cached_rom_is_returned_without_a_download() {
        let directory = tempfile::tempdir().unwrap();
        let cached = directory.path().join("game.nes");
        std::fs::write(&cached, b"rom").unwrap();

        assert_eq!(cached_rom(directory.path(), "game.nes"), Some(cached));
    }
}
