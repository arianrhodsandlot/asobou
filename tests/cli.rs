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
