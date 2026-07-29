use std::process::Command;

fn run(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .args(args)
        .output()
        .expect("failed to run asoby");
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
fn core_list_shows_header() {
    let (stdout, _stderr, code) = run(&["core", "list"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("CORE"));
    assert!(stdout.contains("STATUS"));
}

#[test]
fn core_install_unknown_core_fails() {
    let (_stdout, stderr, code) = run(&["core", "install", "nonexistent_core"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("Unknown core"));
}

#[test]
fn core_remove_unknown_core_is_noop() {
    let (_stdout, stderr, code) = run(&["core", "remove", "nonexistent_core"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("Unknown core"));
}

#[test]
fn core_remove_uninstalled_core_is_noop() {
    let (_stdout, stderr, code) = run(&["core", "remove", "nestopia"]);
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
