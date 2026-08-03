use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::{fs, io};

// ── Platform helpers ───────────────────────────────────────────────────────

pub fn core_extension() -> &'static str {
    match std::env::consts::OS {
        "macos" => "dylib",
        "linux" => "so",
        "windows" => "dll",
        _ => "so",
    }
}

pub fn buildbot_base_url() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => {
            Some("https://buildbot.libretro.com/nightly/apple/osx/arm64/latest/")
        }
        ("macos", "x86_64") => {
            Some("https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/")
        }
        ("linux", "aarch64") => Some("https://buildbot.libretro.com/nightly/linux/aarch64/latest/"),
        ("linux", "x86_64") => Some("https://buildbot.libretro.com/nightly/linux/x86_64/latest/"),
        ("windows", "x86") => Some("https://buildbot.libretro.com/nightly/windows/x86/latest/"),
        ("windows", "x86_64") => {
            Some("https://buildbot.libretro.com/nightly/windows/x86_64/latest/")
        }
        _ => None,
    }
}

pub fn cores_dir() -> PathBuf {
    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::data_local_dir().unwrap());
    data_home.join("asoby").join("cores")
}

pub fn http_agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .build()
        .into()
}

// ── ROM signatures ─────────────────────────────────────────────────────────

struct Signature {
    offset: usize,
    bytes: &'static [u8],
}

// ── Systems registry ───────────────────────────────────────────────────────

struct System {
    extensions: &'static [&'static str],
    signatures: &'static [Signature],
    recommended_core: &'static str,
}

static SYSTEMS: LazyLock<Vec<System>> = LazyLock::new(|| {
    vec![
        System {
            extensions: &["nes", "fds", "unf", "unif"],
            signatures: &[Signature {
                offset: 0,
                bytes: b"NES\x1a",
            }],
            recommended_core: "nestopia",
        },
        System {
            extensions: &["sfc", "smc"],
            signatures: &[],
            recommended_core: "snes9x",
        },
        System {
            extensions: &["gen", "md", "smd", "sg", "bin"],
            signatures: &[Signature {
                offset: 0x100,
                bytes: b"SEGA",
            }],
            recommended_core: "genesis_plus_gx",
        },
        System {
            extensions: &["sms"],
            signatures: &[],
            recommended_core: "genesis_plus_gx",
        },
        System {
            extensions: &["gg"],
            signatures: &[],
            recommended_core: "genesis_plus_gx",
        },
        System {
            extensions: &["32x"],
            signatures: &[],
            recommended_core: "picodrive",
        },
        System {
            extensions: &["gb"],
            signatures: &[],
            recommended_core: "gambatte",
        },
        System {
            extensions: &["gbc"],
            signatures: &[],
            recommended_core: "gambatte",
        },
        System {
            extensions: &["gba"],
            signatures: &[],
            recommended_core: "mgba",
        },
        System {
            extensions: &["a26", "bin"],
            signatures: &[],
            recommended_core: "stella",
        },
        System {
            extensions: &["a52"],
            signatures: &[],
            recommended_core: "a5200",
        },
    ]
});

// ── Core names ─────────────────────────────────────────────────────────────

fn normalize_core_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn validate_core_name(name: &str) -> Result<String, String> {
    let name = normalize_core_name(name);
    if name.is_empty() {
        return Err("Empty core name".to_string());
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(format!("Invalid core name: '{name}'"));
    }
    Ok(name)
}

pub fn is_installed(name: &str, dir: &Path) -> bool {
    resolve_core_library_path(name, dir).exists()
}

pub fn installed_cores(dir: &Path) -> Vec<String> {
    let suffix = format!("_libretro.{}", core_extension());
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if let Some(stem) = fname.strip_suffix(&suffix) {
                names.push(stem.to_lowercase());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

// ── ROM detection ──────────────────────────────────────────────────────────

pub enum Detection {
    Detected { core_name: &'static str },
    Ambiguous { candidates: Vec<&'static str> },
    Unknown,
}

pub fn detect_rom(rom_path: &Path) -> Detection {
    let ext = rom_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    if ext.as_deref() == Some("zip") {
        return detect_zip(rom_path);
    }

    if let Some(detection) = detect_by_signature(rom_path) {
        return detection;
    }

    if let Some(ref ext) = ext {
        return detect_by_extension(ext);
    }

    Detection::Unknown
}

fn detect_by_signature(rom_path: &Path) -> Option<Detection> {
    let mut file = fs::File::open(rom_path).ok()?;
    let mut buf = [0u8; 0x200];
    let n = file.read(&mut buf).ok()?;

    let mut matched: Vec<&System> = Vec::new();
    for system in SYSTEMS.iter() {
        for sig in system.signatures {
            if sig.offset + sig.bytes.len() <= n
                && buf[sig.offset..sig.offset + sig.bytes.len()] == *sig.bytes
            {
                matched.push(system);
                break;
            }
        }
    }

    if matched.is_empty() {
        None
    } else {
        Some(detection_from_candidates(
            matched
                .iter()
                .map(|system| system.recommended_core)
                .collect(),
        ))
    }
}

fn detect_by_extension(ext: &str) -> Detection {
    let candidates = SYSTEMS
        .iter()
        .filter(|s| s.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
        .map(|system| system.recommended_core)
        .collect();

    detection_from_candidates(candidates)
}

fn detect_zip(rom_path: &Path) -> Detection {
    let file = match fs::File::open(rom_path) {
        Ok(f) => f,
        Err(_) => return Detection::Unknown,
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return Detection::Unknown,
    };

    let mut matched = BTreeSet::new();

    for i in 0..archive.len() {
        let entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name();
        let entry_ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());

        if let Some(ref ext) = entry_ext {
            for system in SYSTEMS.iter() {
                if system
                    .extensions
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(ext))
                {
                    matched.insert(system.recommended_core);
                }
            }
        }
    }

    detection_from_candidates(matched)
}

fn detection_from_candidates(candidates: BTreeSet<&'static str>) -> Detection {
    let candidates: Vec<_> = candidates.into_iter().collect();
    match candidates.as_slice() {
        [] => Detection::Unknown,
        [core_name] => Detection::Detected { core_name },
        _ => Detection::Ambiguous { candidates },
    }
}

// ── Core path resolution ───────────────────────────────────────────────────

pub fn resolve_core_path(
    user_input: Option<&Path>,
    cores_dir: &Path,
    default_name: &str,
) -> PathBuf {
    let input = match user_input {
        Some(p) => p,
        None => {
            return cores_dir.join(format!("{}_libretro.{}", default_name, core_extension()));
        }
    };

    if input.exists() {
        return input.to_path_buf();
    }

    if input.parent().is_none() || input.parent() == Some(Path::new("")) {
        let name = input.to_string_lossy();

        let candidate = cores_dir.join(format!("{}_libretro.{}", name, core_extension()));
        if candidate.exists() {
            return candidate;
        }

        if let Ok(entries) = std::fs::read_dir(cores_dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name();
                let fname = fname.to_string_lossy();
                if fname.starts_with(&*name) {
                    return entry.path();
                }
            }
        }

        return candidate;
    }

    input.to_path_buf()
}

pub fn resolve_core_library_path(name: &str, dir: &Path) -> PathBuf {
    dir.join(format!(
        "{}_libretro.{}",
        normalize_core_name(name),
        core_extension()
    ))
}

// ── Download & install ─────────────────────────────────────────────────────

const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024; // 256 MB

pub fn download_and_install(name: &str, cores_dir: &Path, quiet: bool) -> Result<PathBuf, String> {
    let name = validate_core_name(name)?;
    let base = buildbot_base_url()
        .ok_or_else(|| "Auto-download not supported on this platform".to_string())?;
    let ext = core_extension();
    let url = format!("{}{}_libretro.{}.zip", base, name, ext);

    if !quiet {
        eprintln!("Downloading {name} core...");
        eprintln!("  From: {url}");
    }

    let agent = http_agent();
    let resp = agent
        .get(&url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;

    if resp.status() != 200 {
        return Err(format!(
            "HTTP {} — core '{name}' not found on buildbot",
            resp.status()
        ));
    }

    let content_length: Option<u64> = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());

    if let Some(len) = content_length
        && len > MAX_DOWNLOAD_BYTES
    {
        return Err(format!(
            "Download size ({len} bytes) exceeds maximum ({MAX_DOWNLOAD_BYTES} bytes)"
        ));
    }

    let mut data = Vec::new();
    resp.into_body()
        .into_reader()
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|e| format!("Read failed: {e}"))?;

    if data.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("Download exceeded maximum size".to_string());
    }

    let expected_name = format!("{}_libretro.{}", name, ext);
    if data.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("Download exceeded maximum size".to_string());
    }

    let installed = extract_core_archive(data, cores_dir, &expected_name)?;
    if !quiet {
        eprintln!("  Installed: {}", installed.display());
    }
    Ok(installed)
}

fn extract_core_archive(
    data: Vec<u8>,
    cores_dir: &Path,
    expected_name: &str,
) -> Result<PathBuf, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid zip: {e}"))?;
    let ext = core_extension();

    let mut extracted_path: Option<PathBuf> = None;
    let mut temp_files: Vec<(PathBuf, PathBuf)> = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Zip read error: {e}"))?;

        let name = file.name().to_string();
        let entry_path = Path::new(&name);

        if entry_path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            continue;
        }

        let fname = entry_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(name);

        let fname_lower = fname.to_lowercase();

        if !fname_lower.ends_with(&format!(".{ext}")) && !fname_lower.ends_with(".dll") {
            continue;
        }

        let final_path = cores_dir.join(&fname);
        let temp_path = final_path.with_extension(format!("{ext}.tmp", ext = core_extension()));

        if let Some(parent) = temp_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Cannot create directory: {e}"))?;
        }

        let mut out =
            fs::File::create(&temp_path).map_err(|e| format!("Cannot create temp file: {e}"))?;

        let copied = io::copy(&mut file, &mut out).map_err(|e| format!("Extract failed: {e}"))?;

        if copied > MAX_DOWNLOAD_BYTES {
            let _ = fs::remove_file(&temp_path);
            return Err("Extracted file exceeds maximum size".to_string());
        }

        if fname_lower == expected_name.to_lowercase() || fname_lower.ends_with(&format!(".{ext}"))
        {
            extracted_path = Some(final_path.clone());
        }

        temp_files.push((temp_path, final_path));
    }

    if temp_files.is_empty() {
        return Err(format!(
            "No .{} file found in downloaded zip",
            core_extension()
        ));
    }

    for (temp_path, final_path) in &temp_files {
        fs::rename(temp_path, final_path).map_err(|e| format!("Failed to install core: {e}"))?;
    }

    match extracted_path {
        Some(p) => Ok(p),
        None => Ok(temp_files[0].1.clone()),
    }
}

pub fn remove_core_file(name: &str, dir: &Path) -> Result<(), String> {
    let name = validate_core_name(name)?;
    let path = resolve_core_library_path(&name, dir);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to remove core: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            for (name, data) in entries {
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        buf.into_inner()
    }

    // ── Extension detection ────────────────────────────────────────────────

    #[test]
    fn nes_extension_selects_nestopia() {
        let Detection::Detected { core_name } = detect_by_extension("nes") else {
            panic!("expected a detected core");
        };

        assert_eq!(core_name, "nestopia");
    }

    #[test]
    fn bin_extension_returns_core_candidates() {
        let Detection::Ambiguous { candidates } = detect_by_extension("bin") else {
            panic!("expected ambiguous core candidates");
        };

        assert_eq!(candidates, vec!["genesis_plus_gx", "stella"]);
    }

    #[test]
    fn sfc_extension_selects_snes9x() {
        let Detection::Detected { core_name } = detect_by_extension("sfc") else {
            panic!("expected a detected core");
        };

        assert_eq!(core_name, "snes9x");
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        let Detection::Detected { core_name } = detect_by_extension("SMS") else {
            panic!("expected a detected core");
        };

        assert_eq!(core_name, "genesis_plus_gx");
    }

    #[test]
    fn unknown_extension_is_unknown() {
        assert!(matches!(detect_by_extension("xyz"), Detection::Unknown));
    }

    // ── Signature detection ────────────────────────────────────────────────

    #[test]
    fn nes_signature_detects_nestopia() {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("game.bin");
        fs::write(&rom, b"NES\x1a\x02\x03\x04").unwrap();

        let Detection::Detected { core_name } = detect_by_signature(&rom).unwrap() else {
            panic!("expected a detected core");
        };
        assert_eq!(core_name, "nestopia");
    }

    #[test]
    fn sega_signature_at_offset_0x100_detects_genesis() {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("cart.bin");
        let mut data = vec![0u8; 0x100];
        data.extend_from_slice(b"SEGA GENESIS");
        fs::write(&rom, &data).unwrap();

        let Detection::Detected { core_name } = detect_by_signature(&rom).unwrap() else {
            panic!("expected a detected core");
        };
        assert_eq!(core_name, "genesis_plus_gx");
    }

    #[test]
    fn unknown_signature_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("game.bin");
        fs::write(&rom, b"\x00\x01\x02\x03\x04\x05").unwrap();

        assert!(detect_by_signature(&rom).is_none());
    }

    #[test]
    fn truncated_signature_region_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("short.bin");
        fs::write(&rom, b"SEGA").unwrap();

        assert!(detect_by_signature(&rom).is_none());
    }

    #[test]
    fn missing_rom_signature_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_by_signature(&dir.path().join("missing.nes")).is_none());
    }

    // ── Full detection ─────────────────────────────────────────────────────

    #[test]
    fn signature_takes_precedence_over_extension() {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("game.bin");
        fs::write(&rom, b"NES\x1a\x02\x03\x04").unwrap();

        let Detection::Detected { core_name } = detect_rom(&rom) else {
            panic!("expected a detected core");
        };
        assert_eq!(core_name, "nestopia");
    }

    #[test]
    fn extension_detection_does_not_require_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let Detection::Detected { core_name } = detect_rom(&dir.path().join("missing.nes")) else {
            panic!("expected a detected core");
        };
        assert_eq!(core_name, "nestopia");
    }

    // ── Zip introspection ──────────────────────────────────────────────────

    #[test]
    fn zip_with_single_rom_detects_its_core() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("game.zip");
        fs::write(&zip_path, make_zip(&[("game.sfc", b"ROM")])).unwrap();

        let Detection::Detected { core_name } = detect_rom(&zip_path) else {
            panic!("expected a detected core");
        };
        assert_eq!(core_name, "snes9x");
    }

    #[test]
    fn zip_extension_matching_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("game.zip");
        fs::write(&zip_path, make_zip(&[("GAME.SFC", b"ROM")])).unwrap();

        let Detection::Detected { core_name } = detect_rom(&zip_path) else {
            panic!("expected a detected core");
        };
        assert_eq!(core_name, "snes9x");
    }

    #[test]
    fn zip_with_multiple_systems_is_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("multi.zip");
        fs::write(&zip_path, make_zip(&[("a.nes", b"1"), ("b.sfc", b"2")])).unwrap();

        let Detection::Ambiguous { candidates } = detect_rom(&zip_path) else {
            panic!("expected ambiguous core candidates");
        };
        assert_eq!(candidates, vec!["nestopia", "snes9x"]);
    }

    #[test]
    fn zip_with_unmatched_files_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("data.zip");
        fs::write(&zip_path, make_zip(&[("readme.txt", b"hi")])).unwrap();

        assert!(matches!(detect_rom(&zip_path), Detection::Unknown));
    }

    #[test]
    fn invalid_zip_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("broken.zip");
        fs::write(&zip_path, b"not a zip").unwrap();

        assert!(matches!(detect_rom(&zip_path), Detection::Unknown));
    }

    // ── Core file operations ───────────────────────────────────────────────

    #[test]
    fn installed_cores_lists_only_present_core_files() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        fs::write(dir.path().join(format!("nestopia_libretro.{ext}")), b"").unwrap();

        let installed = installed_cores(dir.path());
        assert_eq!(installed, vec!["nestopia".to_string()]);
        assert!(is_installed("nestopia", dir.path()));
    }

    #[test]
    fn installed_cores_accepts_any_core_name() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        fs::write(dir.path().join(format!("fceumm_libretro.{ext}")), b"").unwrap();
        fs::write(dir.path().join("readme.txt"), b"").unwrap();

        let installed = installed_cores(dir.path());
        assert_eq!(installed, vec!["fceumm".to_string()]);
        assert!(is_installed("fceumm", dir.path()));
        assert!(is_installed("FCEUMM", dir.path()));
    }

    #[test]
    fn remove_core_file_deletes_and_tolerates_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        let stub = dir.path().join(format!("snes9x_libretro.{ext}"));
        fs::write(&stub, b"").unwrap();

        remove_core_file("snes9x", dir.path()).unwrap();
        assert!(!stub.exists());

        remove_core_file("snes9x", dir.path()).unwrap();
    }

    #[test]
    fn resolve_core_library_path_joins_artifact_and_extension() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        assert_eq!(
            resolve_core_library_path("mgba", dir.path()),
            dir.path().join(format!("mgba_libretro.{ext}"))
        );
    }

    #[test]
    fn resolve_core_path_defaults_to_named_core_in_cores_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        assert_eq!(
            resolve_core_path(None, dir.path(), "nestopia"),
            dir.path().join(format!("nestopia_libretro.{ext}"))
        );
    }

    #[test]
    fn resolve_core_path_returns_existing_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        let core_file = dir.path().join(format!("mycore_libretro.{ext}"));
        fs::write(&core_file, b"").unwrap();

        assert_eq!(
            resolve_core_path(Some(&core_file), dir.path(), "nestopia"),
            core_file
        );
    }

    #[test]
    fn resolve_core_path_finds_bare_name_in_cores_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        let core_file = dir.path().join(format!("stella_libretro.{ext}"));
        fs::write(&core_file, b"").unwrap();

        assert_eq!(
            resolve_core_path(Some(Path::new("stella")), dir.path(), "nestopia"),
            core_file
        );
    }

    #[test]
    fn resolve_core_path_matches_prefix_when_exact_name_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        let core_file = dir.path().join(format!("nestopia_libretro.{ext}"));
        fs::write(&core_file, b"").unwrap();

        assert_eq!(
            resolve_core_path(Some(Path::new("nes")), dir.path(), "nestopia"),
            core_file
        );
    }

    // ── Zip extraction ─────────────────────────────────────────────────────

    #[test]
    fn extract_installs_the_expected_core_file() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        let expected = dir.path().join(format!("nestopia_libretro.{ext}"));
        let zip_bytes = make_zip(&[(&format!("nestopia_libretro.{ext}"), b"\x7fELF fake core")]);

        let path = extract_core_archive(zip_bytes, dir.path(), &format!("nestopia_libretro.{ext}"))
            .unwrap();
        assert_eq!(path, expected);
        assert_eq!(fs::read(&expected).unwrap(), b"\x7fELF fake core");
    }

    #[test]
    fn extract_installs_matching_extension_files_even_with_wrong_name() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        let expected = dir.path().join(format!("libsomething.{ext}"));
        let zip_bytes = make_zip(&[(&format!("libsomething.{ext}"), b"data")]);

        let path = extract_core_archive(zip_bytes, dir.path(), &format!("nestopia_libretro.{ext}"))
            .unwrap();
        assert_eq!(path, expected);
    }

    #[test]
    fn extract_matches_expected_name_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        let stored_name = format!("NESTopia_Libretro.{ext}");
        let expected = dir.path().join(&stored_name);
        let zip_bytes = make_zip(&[(&stored_name, b"core")]);

        let path = extract_core_archive(zip_bytes, dir.path(), &format!("nestopia_libretro.{ext}"))
            .unwrap();
        assert_eq!(path, expected);
    }

    #[test]
    fn extract_skips_path_traversal_entries() {
        let dir = tempfile::tempdir().unwrap();
        let ext = core_extension();
        let zip_bytes = make_zip(&[
            (&format!("../evil.{ext}"), b"boom"),
            (&format!("mgba_libretro.{ext}"), b"core"),
        ]);

        extract_core_archive(zip_bytes, dir.path(), &format!("mgba_libretro.{ext}")).unwrap();
        assert!(
            !dir.path()
                .parent()
                .unwrap()
                .join(format!("evil.{ext}"))
                .exists()
        );
        assert!(dir.path().join(format!("mgba_libretro.{ext}")).exists());
    }

    #[test]
    fn extract_rejects_zip_without_core_files() {
        let dir = tempfile::tempdir().unwrap();
        let zip_bytes = make_zip(&[("readme.txt", b"hello")]);

        let err =
            extract_core_archive(zip_bytes, dir.path(), "nestopia_libretro.dylib").unwrap_err();
        assert!(err.contains(&format!("No .{} file found", core_extension())));
    }

    #[test]
    fn extract_rejects_invalid_zip_data() {
        let dir = tempfile::tempdir().unwrap();
        let err = extract_core_archive(b"garbage".to_vec(), dir.path(), "x.dylib").unwrap_err();
        assert!(err.contains("Invalid zip"));
    }
}
