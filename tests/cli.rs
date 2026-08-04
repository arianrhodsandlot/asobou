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

fn run_with_config(config: &Path, args: &[&str]) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .env("ASOBY_CONFIG", config)
        .args(args)
        .output()
        .expect("failed to run asoby");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
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
fn help_lists_muted() {
    let (stdout, _stderr, _code) = run(&[]);

    assert!(stdout.contains("-m, --muted"));
    assert!(!stdout.contains("--no-audio"));
}

#[test]
fn boolean_display_and_audio_flags_accept_bare_and_explicit_values() {
    for args in [
        &["--primary-screen"][..],
        &["--primary-screen=true"],
        &["--primary-screen=false"],
        &["--muted"],
        &["--muted=true"],
        &["--muted=false"],
    ] {
        let (stdout, stderr, code) = run(args);
        assert_eq!(code, 0, "{args:?}: {stderr}");
        assert!(stdout.contains("Usage:"), "{args:?}: {stdout}");
    }

    let (_stdout, stderr, code) = run(&["--muted", "not-a-rom"]);
    assert_ne!(code, 2);
    assert!(stderr.contains("No known system"), "{stderr}");
}

#[test]
fn help_shows_examples() {
    let (stdout, _stderr, _code) = run(&[]);

    assert!(stdout.contains("Examples:"));
    assert!(stdout.contains("asoby 'Streets of Rage 2.md'"));
    assert!(stdout.contains("asoby 'Super Metroid.sfc' --state ~/backup.state"));
    assert!(stdout.contains("asoby state list 'Pokemon Emerald.gba' --core mgba"));
    assert!(stdout.contains("asoby core install genesis_plus_gx"));
}

#[test]
fn subcommand_help_shows_examples() {
    for (args, examples) in [
        (
            &["core", "--help"][..],
            &["asoby core install mgba", "asoby core update"][..],
        ),
        (
            &["config", "--help"],
            &[
                "asoby config list",
                "asoby config edit",
                "asoby config get rewind.enabled",
                "asoby config set rewind.buffer_size_mb 64",
                "asoby config set display.fps 30",
                "asoby config set audio.muted true",
                "asoby config unset rewind.buffer_size_mb",
            ],
        ),
        (
            &["state", "--help"],
            &[
                "asoby state list",
                "asoby state list 'Pokemon Emerald.gba' --core mgba",
            ],
        ),
    ] {
        let (stdout, stderr, code) = run(args);
        assert_eq!(code, 0, "{stderr}");
        assert!(stdout.contains("Examples:"));
        for example in examples {
            assert!(stdout.contains(example), "missing {example:?} in {args:?}");
        }
    }
}

#[test]
fn help_has_no_ansi_styling() {
    for args in [
        &["--help"][..],
        &["config", "--help"],
        &["state", "--help"],
        &["core", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
            .env("CLICOLOR_FORCE", "1")
            .args(args)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        assert!(
            !stdout.contains('\x1b'),
            "help for {args:?} contains ANSI styling"
        );
    }

    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .env("CLICOLOR_FORCE", "1")
        .arg("--version")
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains('\x1b'));
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
    assert!(stdout.contains("--resume"));
    assert!(stdout.contains("state"));

    let (stdout, _stderr, code) = run(&["state", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(!stdout.contains("remove"));
    assert!(!stdout.contains("info"));
}

#[test]
fn resume_and_state_cannot_be_combined() {
    let (_stdout, stderr, code) = run(&["--state", "backup.state", "--resume"]);

    assert_ne!(code, 0);
    assert!(stderr.contains("cannot be used with"));
    assert!(stderr.contains("--state"));
    assert!(stderr.contains("--resume"));
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

#[test]
fn config_help_lists_supported_operations() {
    let (stdout, _stderr, code) = run(&["config", "--help"]);

    assert_eq!(code, 0);
    for operation in ["list", "edit", "get", "set", "unset"] {
        assert!(stdout.contains(operation));
    }
    assert!(!stdout.contains("-e"));
}

#[test]
fn config_list_shows_supported_keys_values_and_sources() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "[rewind]\nenabled = false\n").unwrap();

    let (stdout, stderr, code) = run_with_config(&config, &["config", "list"]);

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.starts_with("KEY"));
    assert_eq!(stdout.lines().count(), 33);
    let configured = stdout
        .lines()
        .find(|line| line.starts_with("rewind.enabled"))
        .unwrap();
    assert!(configured.contains("false"));
    assert!(configured.ends_with("config"));
    let default = stdout
        .lines()
        .find(|line| line.starts_with("input.a"))
        .unwrap();
    assert!(default.contains("x"));
    assert!(default.ends_with("default"));
    let resume = stdout
        .lines()
        .find(|line| line.starts_with("state.resume"))
        .unwrap();
    assert!(resume.contains("false"));
    assert!(resume.ends_with("default"));
    let optional = stdout
        .lines()
        .find(|line| line.starts_with("input.l "))
        .unwrap();
    assert!(optional.contains("<unset>"));
    assert!(optional.ends_with("default"));
}

#[test]
fn config_get_prints_effective_values() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("missing").join("config.toml");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "get", "input.a"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "x\n");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "get", "rewind.enabled"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "true\n");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "get", "status.controls"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "true\n");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "get", "display.renderer"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "auto\n");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "get", "display.fps"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "60\n");

    let (stdout, stderr, code) =
        run_with_config(&config, &["config", "get", "display.primary_screen"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "false\n");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "get", "audio.muted"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "false\n");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "get", "input.l"]);
    assert_ne!(code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.contains("is unset"));
}

#[test]
fn config_set_creates_minimal_typed_overrides() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("nested").join("config.toml");

    let (stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "rewind.buffer_size_mb", "64"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "rewind.buffer_size_mb = 64\n");
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "[rewind]\nbuffer_size_mb = 64\n"
    );

    let (stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "state.save_on_exit", "true"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "state.save_on_exit = true\n");
    let contents = std::fs::read_to_string(&config).unwrap();
    assert!(contents.contains("buffer_size_mb = 64"));
    assert!(contents.contains("save_on_exit = true"));

    let (stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "state.resume", "true"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "state.resume = true\n");
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("resume = true")
    );

    let (stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "status.gamepad", "false"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "status.gamepad = false\n");
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("gamepad = false")
    );
}

#[test]
fn config_set_preserves_comments_and_rejects_invalid_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "# controls rewind\n[rewind]\nenabled = true # keep this\n\n[input]\na = \"v\"\n",
    )
    .unwrap();

    let (_stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "rewind.granularity", "4"]);
    assert_eq!(code, 0, "{stderr}");
    let valid = std::fs::read_to_string(&config).unwrap();
    assert!(valid.contains("# controls rewind"));
    assert!(valid.contains("enabled = true # keep this"));
    assert!(valid.contains("granularity = 4"));

    let (_stdout, stderr, code) = run_with_config(&config, &["config", "set", "input.b", "v"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("already bound"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), valid);

    let (_stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "rewind.enabled", "yes"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("expected true or false"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), valid);
}

#[test]
fn config_set_supports_display_and_audio_settings() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");

    for (key, value, expected) in [
        ("display.renderer", "ascii", "display.renderer = ascii\n"),
        ("display.fps", "30", "display.fps = 30\n"),
        (
            "display.primary_screen",
            "true",
            "display.primary_screen = true\n",
        ),
        ("audio.muted", "true", "audio.muted = true\n"),
    ] {
        let (stdout, stderr, code) = run_with_config(&config, &["config", "set", key, value]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, expected);
    }

    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "[display]\nrenderer = \"ascii\"\nfps = 30\nprimary_screen = true\n\n[audio]\nmuted = true\n"
    );
}

#[cfg(unix)]
#[test]
fn config_set_preserves_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "[rewind]\nenabled = true\n").unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o640)).unwrap();

    let (_stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "rewind.enabled", "false"]);

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        std::fs::metadata(&config).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn config_unset_restores_defaults_and_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "input = { l = \"q\" }\n[rewind]\nenabled = false\n",
    )
    .unwrap();

    let (stdout, stderr, code) = run_with_config(&config, &["config", "unset", "rewind.enabled"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "rewind.enabled = true\n");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "unset", "rewind.enabled"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "rewind.enabled = true\n");

    let (stdout, stderr, code) = run_with_config(&config, &["config", "unset", "input.l"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "input.l = <unset>\n");
    assert!(!std::fs::read_to_string(&config).unwrap().contains("l ="));
}

#[test]
fn config_mutations_reject_unknown_keys_and_malformed_files() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");

    let (_stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "rewind.unknown", "1"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("unknown config key"));
    assert!(!config.exists());

    let (_stdout, stderr, code) =
        run_with_config(&config, &["config", "get", "rewind.buffer_size"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("Did you mean \"rewind.buffer_size_mb\"?"));

    std::fs::write(&config, "[rewind\n").unwrap();
    let original = std::fs::read_to_string(&config).unwrap();
    let (_stdout, stderr, code) =
        run_with_config(&config, &["config", "set", "rewind.enabled", "false"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("failed to parse config"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn config_edit_creates_and_validates_the_file() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("nested").join("config.toml");
    let editor = directory.path().join("fake editor");
    std::fs::write(
        &editor,
        "#!/bin/sh\nprintf '[state]\\nsave_on_exit = true\\n' > \"$1\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .env("ASOBY_CONFIG", &config)
        .env("VISUAL", format!("'{}'", editor.display()))
        .env("EDITOR", "false")
        .args(["config", "edit"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "[state]\nsave_on_exit = true\n"
    );

    std::fs::write(&editor, "#!/bin/sh\nprintf '[state\\n' > \"$1\"\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .env("ASOBY_CONFIG", &config)
        .env("VISUAL", format!("'{}'", editor.display()))
        .args(["config", "edit"])
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to parse config"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "[state\n");
}

#[test]
fn config_edit_requires_an_editor() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .env("ASOBY_CONFIG", &config)
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .args(["config", "edit"])
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("VISUAL and EDITOR are not set"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "");
}

#[cfg(unix)]
#[test]
fn config_edit_reports_editor_failure() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_asoby"))
        .env("ASOBY_CONFIG", &config)
        .env("VISUAL", "false")
        .args(["config", "edit"])
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("exited unsuccessfully"));
}
