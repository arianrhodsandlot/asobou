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
        ("linux", "aarch64") => {
            Some("https://buildbot.libretro.com/nightly/linux/aarch64/latest/")
        }
        ("linux", "x86_64") => {
            Some("https://buildbot.libretro.com/nightly/linux/x86_64/latest/")
        }
        ("windows", "x86") => {
            Some("https://buildbot.libretro.com/nightly/windows/x86/latest/")
        }
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
            recommended_core: "mgba",
        },
        System {
            extensions: &["gbc"],
            signatures: &[],
            recommended_core: "mgba",
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

// ── Core registry ──────────────────────────────────────────────────────────

pub struct CoreEntry {
    pub name: &'static str,
    pub artifact: &'static str,
}

static CORES: LazyLock<Vec<CoreEntry>> = LazyLock::new(|| {
    vec![
        CoreEntry {
            name: "nestopia",
            artifact: "nestopia",
        },
        CoreEntry {
            name: "snes9x",
            artifact: "snes9x",
        },
        CoreEntry {
            name: "genesis_plus_gx",
            artifact: "genesis_plus_gx",
        },
        CoreEntry {
            name: "picodrive",
            artifact: "picodrive",
        },
        CoreEntry {
            name: "mgba",
            artifact: "mgba",
        },
        CoreEntry {
            name: "stella",
            artifact: "stella",
        },
        CoreEntry {
            name: "a5200",
            artifact: "a5200",
        },
    ]
});

pub fn find_core(name: &str) -> Option<&'static CoreEntry> {
    CORES
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name))
}

pub fn is_installed(core: &CoreEntry, dir: &Path) -> bool {
    let ext = core_extension();
    let path = dir.join(format!("{}_libretro.{}", core.artifact, ext));
    path.exists()
}

pub fn installed_cores(dir: &Path) -> Vec<&'static CoreEntry> {
    CORES.iter().filter(|c| is_installed(c, dir)).collect()
}

// ── ROM detection ──────────────────────────────────────────────────────────

pub enum Detection {
    Detected { core_name: &'static str },
    Ambiguous { candidates: Vec<&'static str> },
    Unknown,
}

pub fn detect_rom(rom_path: &Path, explicit_core: Option<&str>) -> Detection {
    if let Some(core_name) = explicit_core {
        if let Some(core) = find_core(core_name) {
            return Detection::Detected {
                core_name: core.name,
            };
        }
        return Detection::Detected { core_name: "" };
    }

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
                if system.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
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
        [core_name] => Detection::Detected {
            core_name: *core_name,
        },
        _ => Detection::Ambiguous { candidates },
    }
}

// ── Core path resolution ───────────────────────────────────────────────────

pub fn resolve_core_path(user_input: Option<&Path>, cores_dir: &Path, default_name: &str) -> PathBuf {
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
    dir.join(format!("{}_libretro.{}", name, core_extension()))
}

// ── Download & install ─────────────────────────────────────────────────────

const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024; // 256 MB

pub fn download_and_install(
    core: &CoreEntry,
    cores_dir: &Path,
    quiet: bool,
) -> Result<PathBuf, String> {
    let base = buildbot_base_url()
        .ok_or_else(|| "Auto-download not supported on this platform".to_string())?;
    let ext = core_extension();
    let url = format!("{}{}_libretro.{}.zip", base, core.artifact, ext);

    if !quiet {
        eprintln!("Downloading {} core...", core.name);
        eprintln!("  From: {url}");
    }

    let agent = http_agent();
    let resp = agent
        .get(&url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;

    if resp.status() != 200 {
        return Err(format!(
            "HTTP {} — core '{}' not found on buildbot",
            resp.status(),
            core.name
        ));
    }

    let content_length: Option<u64> = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());

    if let Some(len) = content_length {
        if len > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "Download size ({len} bytes) exceeds maximum ({MAX_DOWNLOAD_BYTES} bytes)"
            ));
        }
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

    let cursor = std::io::Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid zip: {e}"))?;

    let expected_name = format!("{}_libretro.{}", core.artifact, ext);

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
        let temp_path = final_path.with_extension(format!(
            "{ext}.tmp",
            ext = core_extension()
        ));

        if let Some(parent) = temp_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Cannot create directory: {e}"))?;
        }

        let mut out = fs::File::create(&temp_path)
            .map_err(|e| format!("Cannot create temp file: {e}"))?;

        let copied = io::copy(&mut file, &mut out)
            .map_err(|e| format!("Extract failed: {e}"))?;

        if copied > MAX_DOWNLOAD_BYTES {
            let _ = fs::remove_file(&temp_path);
            return Err("Extracted file exceeds maximum size".to_string());
        }

        if fname_lower == expected_name.to_lowercase() || fname_lower.ends_with(&format!(".{ext}")) {
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
        fs::rename(temp_path, final_path)
            .map_err(|e| format!("Failed to install core: {e}"))?;
    }

    match extracted_path {
        Some(p) => {
            if !quiet {
                eprintln!("  Installed: {}", p.display());
            }
            Ok(p)
        }
        None => Ok(temp_files[0].1.clone()),
    }
}

pub fn remove_core_file(core: &CoreEntry, dir: &Path) -> Result<(), String> {
    let path = resolve_core_library_path(core.artifact, dir);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to remove core: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Detection, detect_by_extension};

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
}
