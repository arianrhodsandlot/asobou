use std::path::Path;
use std::process::Command;

fn run(args: &[&str]) -> (String, String, i32) {
    run_in(None, args)
}

// Point the binary at an isolated data dir so tests never touch the real
// user cores dir (~/Library/Application Support/asoby/cores).
fn run_in(data_home: Option<&Path>, args: &[&str]) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_asoby"));
    if let Some(dir) = data_home {
        cmd.env("XDG_DATA_HOME", dir);
    }
    let output = cmd.args(args).output().expect("failed to run asoby");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

fn write_state(data_home: &Path, core: &str, game: &str, name: &str) -> std::path::PathBuf {
    let dir = data_home.join("asoby").join("states").join(core).join(game);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let stem = name.strip_suffix(".state").unwrap();
    let timestamp = &stem[stem.len() - 24..];
    std::fs::write(&path, container(core, game, timestamp, &[1, 2, 3, 4])).unwrap();
    path
}

fn container(core: &str, game: &str, timestamp: &str, payload: &[u8]) -> Vec<u8> {
    use std::io::Write;

    let dt = chrono::DateTime::parse_from_str(timestamp, "%Y%m%dT%H%M%S%.3f%z").unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(b"ASOBYST");
    out.push(1);
    for text in [core, game] {
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
    }
    out.extend_from_slice(&dt.timestamp_millis().to_le_bytes());
    out.extend_from_slice(&(dt.offset().local_minus_utc() / 60).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(payload).unwrap();
    out.extend_from_slice(&encoder.finish().unwrap());
    out
}

#[test]
fn running_without_rom_shows_help() {
    let (stdout, _stderr, code) = run(&[]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("asoby"));
}

#[test]
fn version_flags_print_the_version() {
    for flag in ["-v", "--version"] {
        let (stdout, _stderr, code) = run(&[flag]);
        assert_eq!(code, 0);
        assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
    }
}

#[test]
fn help_lists_graphic_renderer() {
    let (stdout, _stderr, _code) = run(&[]);

    assert!(stdout.contains("graphic"));
}

#[test]
fn help_lists_primary_screen() {
    let (stdout, _stderr, _code) = run(&[]);

    assert!(stdout.contains("-p, --primary-screen"));
    assert!(!stdout.contains("--no-alt-screen"));
}

#[test]
fn help_lists_fps() {
    let (stdout, _stderr, _code) = run(&[]);

    assert!(stdout.contains("-f, --fps"));
}

#[test]
fn help_lists_mute() {
    let (stdout, _stderr, _code) = run(&[]);

    assert!(stdout.contains("-m, --mute"));
    assert!(!stdout.contains("--no-audio"));
}

#[test]
fn core_list_shows_header() {
    // An empty cores dir prints "No cores installed." without a header, so
    // stub an installed registered core file.
    let dir = tempfile::tempdir().unwrap();
    let cores = dir.path().join("asoby").join("cores");
    std::fs::create_dir_all(&cores).unwrap();
    let ext = match std::env::consts::OS {
        "macos" => "dylib",
        "windows" => "dll",
        _ => "so",
    };
    std::fs::write(cores.join(format!("nestopia_libretro.{ext}")), b"").unwrap();

    let (stdout, _stderr, code) = run_in(Some(dir.path()), &["core", "list"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("CORE"));
    assert!(stdout.contains("STATUS"));
}

#[test]
fn core_list_shows_arbitrary_installed_cores() {
    let dir = tempfile::tempdir().unwrap();
    let cores = dir.path().join("asoby").join("cores");
    std::fs::create_dir_all(&cores).unwrap();
    let ext = match std::env::consts::OS {
        "macos" => "dylib",
        "windows" => "dll",
        _ => "so",
    };
    std::fs::write(cores.join(format!("fceumm_libretro.{ext}")), b"").unwrap();

    let (stdout, _stderr, code) = run_in(Some(dir.path()), &["core", "list"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("fceumm"));
}

#[test]
fn core_install_rejects_invalid_names_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let (_stdout, stderr, code) = run_in(Some(dir.path()), &["core", "install", "a/b"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("Invalid core name"));
}

#[test]
fn core_remove_unknown_core_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let (_stdout, stderr, code) = run_in(Some(dir.path()), &["core", "remove", "nonexistent_core"]);
    assert_eq!(code, 0);
    assert!(stderr.contains("not installed"));
}

#[test]
fn help_lists_state_flag_and_subcommand() {
    let (stdout, _stderr, _code) = run(&[]);
    assert!(stdout.contains("--state"));
    assert!(stdout.contains("state"));

    let (stdout, _stderr, code) = run(&["state", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(!stdout.contains("remove"));
    assert!(!stdout.contains("info"));
}

#[test]
fn state_list_shows_core_game_saved_and_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_state(
        dir.path(),
        "fceumm",
        "Super Mario Bros.nes",
        "Super Mario Bros.nes.20260802T151205.903+0800.state",
    );

    let (stdout, _stderr, code) = run_in(Some(dir.path()), &["state", "list"]);

    assert_eq!(code, 0);
    assert!(stdout.contains("CORE"));
    assert!(stdout.contains("GAME"));
    assert!(stdout.contains("SAVED"));
    assert!(stdout.contains("PATH"));
    assert!(stdout.contains("fceumm"));
    assert!(stdout.contains("Super Mario Bros.nes"));
    assert!(stdout.contains("2026-08-02 15:12:05 +08:00"));
    assert!(stdout.contains(path.to_str().unwrap()));
    assert!(!stdout.contains("SIZE"));
}

#[test]
fn state_list_filters_by_rom_and_core() {
    let dir = tempfile::tempdir().unwrap();
    write_state(
        dir.path(),
        "fceumm",
        "Super Mario Bros.nes",
        "Super Mario Bros.nes.20260802T151205.903+0800.state",
    );
    write_state(
        dir.path(),
        "nestopia",
        "Super Mario Bros.nes",
        "Super Mario Bros.nes.20260802T154831.027+0800.state",
    );
    write_state(
        dir.path(),
        "fceumm",
        "Contra.zip",
        "Contra.zip.20260802T160011.412+0800.state",
    );

    let (stdout, _stderr, code) =
        run_in(Some(dir.path()), &["state", "list", "Super Mario Bros.nes"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("20260802T151205.903+0800"));
    assert!(stdout.contains("20260802T154831.027+0800"));
    assert!(!stdout.contains("Contra"));

    let (stdout, _stderr, code) = run_in(
        Some(dir.path()),
        &[
            "state",
            "list",
            "Super Mario Bros.nes",
            "--core",
            "nestopia",
        ],
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("nestopia"));
    assert!(stdout.contains("20260802T154831.027+0800"));
    assert!(!stdout.contains("fceumm"));
    assert!(!stdout.contains("20260802T151205.903+0800"));
}

#[test]
fn state_list_skips_temp_files_and_reports_malformed_names() {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir
        .path()
        .join("asoby")
        .join("states")
        .join("fceumm")
        .join("game.nes");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::write(game_dir.join(".asoby-state-abc"), b"x").unwrap();
    std::fs::write(game_dir.join("garbage.bin"), b"x").unwrap();
    let corrupt = game_dir.join("game.nes.20260802T160000.000+0800.state");
    std::fs::write(&corrupt, b"not a state").unwrap();
    write_state(
        dir.path(),
        "fceumm",
        "game.nes",
        "game.nes.20260802T151205.903+0800.state",
    );

    let (stdout, stderr, code) = run_in(Some(dir.path()), &["state", "list"]);

    assert_eq!(code, 0);
    assert!(stdout.contains("game.nes"));
    assert!(!stdout.contains(".asoby-state-abc"));
    assert!(!stdout.contains("garbage.bin"));
    assert!(!stdout.contains("20260802T160000.000+0800"));
    assert!(stderr.contains("malformed"));
    assert!(stderr.contains("garbage.bin"));
    assert!(stderr.contains("20260802T160000.000+0800"));
}

#[test]
fn state_list_empty_reports_no_states() {
    let dir = tempfile::tempdir().unwrap();
    let (stdout, _stderr, code) = run_in(Some(dir.path()), &["state", "list"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("No save states found."));
}

#[cfg(unix)]
#[test]
fn state_list_shortens_home_paths_with_tilde() {
    let home = tempfile::tempdir().unwrap();
    let data = home.path().join("data");
    write_state(
        &data,
        "fceumm",
        "game.nes",
        "game.nes.20260802T151205.903+0800.state",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .env("XDG_DATA_HOME", &data)
        .env("HOME", home.path())
        .args(["state", "list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        stdout.contains(
            "~/data/asoby/states/fceumm/game.nes/game.nes.20260802T151205.903+0800.state"
        )
    );
    assert!(!stdout.contains(home.path().to_str().unwrap()));
}

#[test]
fn nonexistent_state_path_fails_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let (_stdout, stderr, code) = run_in(
        Some(dir.path()),
        &["missing.rom", "--state", "/nonexistent/path.state"],
    );
    assert_ne!(code, 0);
    assert!(stderr.contains("state file not found"));
}

#[test]
fn asoby_config_loads_the_explicit_path() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("custom.toml");
    std::fs::write(&config, "[input\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .env("ASOBY_CONFIG", config)
        .arg("missing.rom")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("failed to parse config"),
        "unexpected stderr: {stderr}"
    );
}
