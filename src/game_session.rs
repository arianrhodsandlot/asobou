use crossterm::event::Event;
use gilrs::{Axis, Button, EventType, GamepadId};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static RUNNING: AtomicBool = AtomicBool::new(true);
static CTRL_C_HANDLER: OnceLock<Result<(), ctrlc::Error>> = OnceLock::new();

const GAMEPAD_BUTTONS: [Button; 16] = [
    Button::South,
    Button::East,
    Button::North,
    Button::West,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
];

fn resolve_core(
    user_input: Option<&str>,
    cores_dir: &Path,
    default_name: &str,
) -> Result<PathBuf, String> {
    if let Some(input) = user_input {
        let input_path = Path::new(input);

        if input_path.exists() {
            return Ok(input_path.to_path_buf());
        }

        let is_name = input_path.parent().is_none() || input_path.parent() == Some(Path::new(""));

        if is_name {
            if crate::cores::is_installed(input, cores_dir) {
                return Ok(crate::cores::resolve_core_library_path(input, cores_dir));
            }

            let candidate =
                crate::cores::resolve_core_path(Some(input_path), cores_dir, default_name);
            if candidate.exists() {
                return Ok(candidate);
            }

            crate::cores::download_and_install(input, cores_dir, false)?;
            return Ok(crate::cores::resolve_core_library_path(input, cores_dir));
        }

        let candidate = crate::cores::resolve_core_path(Some(input_path), cores_dir, default_name);
        if candidate.exists() {
            return Ok(candidate);
        }
        return Err(format!("Core not found: {input}"));
    }

    // No --core provided: detect from ROM, then ensure installed
    let detection = crate::cores::detect_rom(std::path::Path::new(default_name));
    match detection {
        crate::cores::Detection::Detected { core_name } => {
            if !crate::cores::is_installed(core_name, cores_dir) {
                crate::cores::download_and_install(core_name, cores_dir, false)?;
            }
            Ok(crate::cores::resolve_core_library_path(
                core_name, cores_dir,
            ))
        }
        crate::cores::Detection::Ambiguous { candidates } => {
            let mut msg =
                format!("Multiple cores support \"{default_name}\".\n\nSelect one explicitly:\n");
            for core in &candidates {
                let rom_path = default_name;
                msg.push_str(&format!("  asobou {rom_path} --core {core}\n"));
            }
            Err(msg)
        }
        crate::cores::Detection::Unknown => {
            let ext = std::path::Path::new(default_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown");
            Err(format!(
                "No known system uses the '.{ext}' extension. Use --core to specify a core explicitly."
            ))
        }
    }
}

pub struct Launch {
    pub renderer: crate::renderer::RendererMode,
    pub core: Option<String>,
    pub render_fps: u32,
    pub primary_screen: bool,
    pub muted: bool,
    pub rom: PathBuf,
    pub cores_dir: PathBuf,
    pub states_dir: PathBuf,
    pub input_bindings: crate::input::InputBindings,
    pub rewind: crate::config::RewindSettings,
    pub status: crate::config::StatusSettings,
    pub startup_state: Option<PathBuf>,
    pub resume: bool,
    pub save_on_exit: bool,
}

#[derive(Default)]
struct GhosttyCtrlCWorkaround {
    control_pressed_without_key: bool,
}

impl GhosttyCtrlCWorkaround {
    fn handle_key(&mut self, event: crossterm::event::KeyEvent) -> bool {
        let control = matches!(
            event.code,
            crossterm::event::KeyCode::Modifier(
                crossterm::event::ModifierKeyCode::LeftControl
                    | crossterm::event::ModifierKeyCode::RightControl
            )
        );
        match (control, event.kind) {
            (true, crossterm::event::KeyEventKind::Press) => {
                self.control_pressed_without_key = true;
                false
            }
            (true, crossterm::event::KeyEventKind::Release) => {
                std::mem::take(&mut self.control_pressed_without_key)
            }
            (true, crossterm::event::KeyEventKind::Repeat) => false,
            (false, _) => {
                self.control_pressed_without_key = false;
                false
            }
        }
    }

    fn clear(&mut self) {
        self.control_pressed_without_key = false;
    }
}

fn poll_gamepads(
    gamepads: &mut gilrs::Gilrs,
    input: &mut crate::input::InputState,
    active_gamepad: &mut Option<GamepadId>,
) {
    while let Some(event) = gamepads.next_event() {
        match event.event {
            EventType::Connected => {
                if active_gamepad.is_none() {
                    *active_gamepad = Some(event.id);
                }
            }
            EventType::Disconnected => {
                if *active_gamepad == Some(event.id) {
                    *active_gamepad = None;
                    input.clear_gamepad();
                }
            }
            // Only deliberate mapped button presses claim the active slot;
            // axis traffic (stick jitter, trigger drift) on a second pad
            // must not steal control from the pad in use.
            EventType::ButtonPressed(button, _)
                if crate::input::default_gamepad_button(button).is_some() =>
            {
                *active_gamepad = Some(event.id);
            }
            _ => {}
        }
    }
    gamepads.inc();

    let id = active_gamepad.or_else(|| gamepads.gamepads().next().map(|(id, _)| id));
    let Some(id) = id else {
        return;
    };
    let Some(gamepad) = gamepads.connected_gamepad(id) else {
        return;
    };

    let mut buttons = [false; crate::input::JOYPAD_BUTTON_COUNT];
    for button in GAMEPAD_BUTTONS {
        if gamepad.is_pressed(button)
            && let Some(index) = crate::input::default_gamepad_button(button)
        {
            buttons[index] = true;
        }
    }
    crate::input::apply_left_stick(
        &mut buttons,
        gamepad.value(Axis::LeftStickX),
        gamepad.value(Axis::LeftStickY),
    );
    input.update_gamepad(buttons);
}

fn rewind_run_frame(
    game: &crate::emulation::libretro::LoadedGame,
    terminal: &crate::terminal::TerminalSession,
) {
    game.set_video_capture_enabled(true);
    game.run_frame();
    let frame = game.latest_frame();
    if let Some(frame) = frame {
        terminal.present(frame);
    }
}

struct AudioSession<'a> {
    game: &'a crate::emulation::libretro::LoadedGame,
    backend: Box<dyn crate::audio::AudioBackend>,
}

impl Drop for AudioSession<'_> {
    fn drop(&mut self) {
        self.game.clear_audio_sink();
        self.backend.stop();
    }
}

fn status_lines(
    input_bindings: &crate::input::InputBindings,
    settings: crate::config::StatusSettings,
) -> Vec<String> {
    if !settings.enabled {
        return Vec::new();
    }
    let mut lines = Vec::with_capacity(2);
    if settings.gamepad {
        lines.push(input_bindings.gamepad_status_line());
    }
    if settings.controls {
        lines.push(input_bindings.controls_status_line());
    }
    lines
}

pub enum Outcome {
    Completed,
    LaunchRejected(PathBuf),
}

#[derive(Debug)]
pub enum LaunchFailure {
    MissingRom(PathBuf),
    MissingState(PathBuf),
    CoreResolution { message: String, cores_dir: PathBuf },
    UnsupportedStartupState(PathBuf),
    StartupState { path: PathBuf, message: String },
}

impl fmt::Display for LaunchFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRom(path) => write!(formatter, "file not found: {}", path.display()),
            Self::MissingState(path) => {
                write!(formatter, "state file not found: {}", path.display())
            }
            Self::CoreResolution { message, cores_dir } => write!(
                formatter,
                "{message}\n  Place cores in {} or use -c to specify a path",
                cores_dir.display()
            ),
            Self::UnsupportedStartupState(path) => write!(
                formatter,
                "core does not support savestates, cannot load {}",
                path.display()
            ),
            Self::StartupState { path, message } => {
                write!(
                    formatter,
                    "failed to load state {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for LaunchFailure {}

fn install_ctrl_c_handler() -> io::Result<()> {
    match CTRL_C_HANDLER
        .get_or_init(|| ctrlc::set_handler(|| RUNNING.store(false, Ordering::SeqCst)))
    {
        Ok(()) => Ok(()),
        Err(error) => Err(io::Error::other(error.to_string())),
    }
}

struct PreparedGame {
    game: crate::emulation::libretro::LoadedGame,
    renderer_mode: crate::renderer::RendererMode,
    primary_screen: bool,
    audio_backend: Box<dyn crate::audio::AudioBackend>,
    av_info: crate::emulation::libretro::RetroSystemAvInfo,
    status_lines: Vec<String>,
    states_dir: PathBuf,
    input_bindings: crate::input::InputBindings,
    rewind_settings: crate::config::RewindSettings,
    render_fps: u32,
    serialization_supported: bool,
    state_size: usize,
    core_name: String,
    game_name: String,
    save_on_exit: bool,
}

enum Preparation {
    Ready(Box<PreparedGame>),
    Rejected(PathBuf),
}

fn prepare(config: Launch) -> Result<Preparation, Box<dyn std::error::Error>> {
    let Launch {
        renderer: renderer_mode,
        core: core_arg,
        render_fps,
        primary_screen,
        muted,
        rom,
        cores_dir,
        states_dir,
        input_bindings,
        rewind: rewind_settings,
        status: status_settings,
        startup_state,
        resume,
        save_on_exit,
    } = config;

    if let Some(state_path) = &startup_state
        && !state_path.exists()
    {
        return Err(Box::new(LaunchFailure::MissingState(state_path.clone())));
    }

    if !rom.exists() {
        return Err(Box::new(LaunchFailure::MissingRom(rom)));
    }

    install_ctrl_c_handler()?;
    RUNNING.store(true, Ordering::SeqCst);
    std::fs::create_dir_all(&cores_dir).ok();

    let core_path = match resolve_core(
        core_arg.as_deref(),
        &cores_dir,
        rom.to_string_lossy().as_ref(),
    ) {
        Ok(p) => p,
        Err(message) => {
            return Err(Box::new(LaunchFailure::CoreResolution {
                message,
                cores_dir,
            }));
        }
    };

    let status_lines = status_lines(&input_bindings, status_settings);
    let core = crate::emulation::libretro::load_core(&core_path)?;
    let mut audio_backend = if muted {
        crate::audio::muted()
    } else {
        crate::audio::output()
    };
    let target_sample_rate = match audio_backend.preferred_sample_rate() {
        Ok(sample_rate) => sample_rate,
        Err(e) => {
            eprintln!("Audio device unavailable: {e}, falling back to null");
            audio_backend = crate::audio::muted();
            None
        }
    };
    let game = match core.load_game(&rom, target_sample_rate) {
        Ok(game) => game,
        Err(crate::emulation::libretro::LoadGameError::Rejected) => {
            return Ok(Preparation::Rejected(rom));
        }
        Err(error) => return Err(error.into()),
    };

    let serialization_supported = game.supports_complete_serialization();
    if !serialization_supported {
        eprintln!("Save states and rewind disabled: core does not support complete savestates");
    }
    let state_size = game.state_size().unwrap_or(0);
    let core_name = crate::emulation::state::core_name_from_path(&core_path);
    let game_name = rom
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(startup) = &startup_state {
        if !serialization_supported {
            return Err(Box::new(LaunchFailure::UnsupportedStartupState(
                startup.clone(),
            )));
        }
        if let Err(error) = crate::emulation::state::load_from_path(&game, startup) {
            return Err(Box::new(LaunchFailure::StartupState {
                path: startup.clone(),
                message: error.to_string(),
            }));
        }
    } else if resume && serialization_supported {
        let _ = crate::emulation::state::load_newest(&states_dir, &game, &core_name, &game_name);
    }

    let av_info = game.av_info();
    Ok(Preparation::Ready(Box::new(PreparedGame {
        game,
        renderer_mode,
        primary_screen,
        audio_backend,
        av_info,
        status_lines,
        states_dir,
        input_bindings,
        rewind_settings,
        render_fps,
        serialization_supported,
        state_size,
        core_name,
        game_name,
        save_on_exit,
    })))
}

pub fn run(config: Launch) -> Result<Outcome, Box<dyn std::error::Error>> {
    let prepared = match prepare(config)? {
        Preparation::Ready(prepared) => *prepared,
        Preparation::Rejected(rom) => return Ok(Outcome::LaunchRejected(rom)),
    };
    let PreparedGame {
        game,
        renderer_mode,
        primary_screen,
        mut audio_backend,
        av_info,
        status_lines,
        states_dir,
        input_bindings,
        rewind_settings,
        render_fps,
        serialization_supported,
        state_size,
        core_name,
        game_name,
        save_on_exit,
    } = prepared;
    let audio_sink = match audio_backend.start(av_info.timing.sample_rate) {
        Ok(sink) => sink,
        Err(e) => {
            eprintln!("Audio init failed: {e}, falling back to null");
            audio_backend = crate::audio::muted();
            audio_backend.start(av_info.timing.sample_rate)?
        }
    };
    game.install_audio_sink(audio_sink);
    let audio_session = AudioSession {
        game: &game,
        backend: audio_backend,
    };

    let mut rewind = if rewind_settings.enabled && serialization_supported {
        crate::emulation::rewind::Rewind::new(
            state_size,
            rewind_settings.granularity,
            rewind_settings.buffer_size,
        )
    } else {
        None
    };
    if let Some(rewind) = rewind.as_mut() {
        rewind.capture(&game, 0);
    }

    let terminal = crate::terminal::TerminalSession::start(crate::terminal::Settings {
        renderer: renderer_mode,
        primary_screen,
        status_lines,
    })?;

    let fps = if av_info.timing.fps.is_finite() && av_info.timing.fps > 0.0 {
        av_info.timing.fps
    } else {
        60.0
    };
    let frame_duration = Duration::from_secs_f64(1.0 / fps);
    let render_duration = Duration::from_secs_f64(1.0 / render_fps as f64);
    let schedule_start = Instant::now();
    let mut next_frame = schedule_start;
    let mut next_render = schedule_start;
    let mut input = crate::input::InputState::with_bindings(
        input_bindings,
        terminal.release_events_supported(),
    );
    let mut gamepads = match gilrs::Gilrs::new() {
        Ok(gamepads) => Some(gamepads),
        Err(error) => {
            eprintln!("Gamepad input unavailable: {error}");
            None
        }
    };
    let mut active_gamepad: Option<GamepadId> = None;
    let mut ghostty_ctrl_c = crate::terminal::is_ghostty().then(GhosttyCtrlCWorkaround::default);
    let mut input_error = None;
    let mut frame_count = 0u64;

    while RUNNING.load(Ordering::SeqCst) {
        loop {
            match terminal.next_event() {
                Ok(Some(event)) => match event {
                    Event::Key(key) => {
                        let ghostty_ctrl_c_requested = ghostty_ctrl_c
                            .as_mut()
                            .is_some_and(|workaround| workaround.handle_key(key));
                        input.handle_key(key, Instant::now());
                        if ghostty_ctrl_c_requested || input.quit_requested() {
                            RUNNING.store(false, Ordering::SeqCst);
                        }
                    }
                    Event::FocusLost => {
                        input.clear();
                        if let Some(workaround) = ghostty_ctrl_c.as_mut() {
                            workaround.clear();
                        }
                    }
                    _ => {}
                },
                Ok(None) => break,
                Err(error) => {
                    input.clear();
                    input_error = Some(error);
                    RUNNING.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }

        if let Some(gamepads) = gamepads.as_mut() {
            poll_gamepads(gamepads, &mut input, &mut active_gamepad);
        }
        if input.quit_requested() {
            RUNNING.store(false, Ordering::SeqCst);
        }

        input.expire(Instant::now());
        if serialization_supported {
            if input.take_save() {
                match crate::emulation::state::save_state(
                    &states_dir,
                    &game,
                    state_size,
                    &core_name,
                    &game_name,
                ) {
                    Ok(_) => terminal.show_message("State saved"),
                    Err(error) => terminal.show_message(&format!("Save failed: {error}")),
                }
            }
            if input.take_load() {
                match crate::emulation::state::load_newest(
                    &states_dir,
                    &game,
                    &core_name,
                    &game_name,
                ) {
                    Ok(Some(_)) => terminal.show_message("State loaded"),
                    Ok(None) => terminal.show_message("No save state found"),
                    Err(error) => terminal.show_message(&format!("Load failed: {error}")),
                }
            }
        }
        game.set_joypad_buttons(input.button_mask());
        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        let rewound = if input.rewind_pressed() {
            if let Some(rewind) = rewind.as_mut() {
                game.set_audio_muted(true);
                let target = rewind.rewind(&game, frame_count, &mut || {
                    rewind_run_frame(&game, &terminal)
                });
                game.set_audio_muted(false);
                match target {
                    Some(target) => {
                        frame_count = target;
                        true
                    }
                    None => false,
                }
            } else {
                false
            }
        } else {
            false
        };

        if !rewound {
            let capture_frame = Instant::now() >= next_render && terminal.wants_frame();
            game.set_video_capture_enabled(capture_frame);
            game.run_frame();
            frame_count += 1;
            if let Some(rewind) = rewind.as_mut() {
                rewind.capture(&game, frame_count);
            }

            if capture_frame {
                let frame = game.latest_frame();
                if let Some(frame) = frame
                    && !terminal.present(frame)
                {
                    break;
                }
                next_render += render_duration;
                let now = Instant::now();
                if next_render < now {
                    next_render = now;
                }
            }
        }

        next_frame += frame_duration;
        let now = Instant::now();
        if let Some(delay) = next_frame.checked_duration_since(now) {
            thread::sleep(delay);
        } else {
            next_frame = now;
        }
    }

    input.clear();
    game.set_joypad_buttons(0);
    game.set_video_capture_enabled(true);
    let terminal_result = terminal.finish();

    if serialization_supported && save_on_exit {
        match crate::emulation::state::save_state(
            &states_dir,
            &game,
            state_size,
            &core_name,
            &game_name,
        ) {
            Ok(path) => println!("Saved state: {}", path.display()),
            Err(error) => eprintln!("Warning: failed to save state on exit: {error}"),
        }
    }

    drop(audio_session);

    if let Some(error) = input_error {
        return Err(error.into());
    }
    terminal_result?;
    Ok(Outcome::Completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(rom: PathBuf, directory: &Path) -> Launch {
        Launch {
            renderer: crate::renderer::RendererMode::Block,
            core: None,
            render_fps: 30,
            primary_screen: false,
            muted: true,
            rom,
            cores_dir: directory.join("cores"),
            states_dir: directory.join("states"),
            input_bindings: crate::input::InputBindings::default(),
            rewind: crate::config::RewindSettings {
                enabled: false,
                granularity: 2,
                buffer_size: 1024,
            },
            status: crate::config::StatusSettings {
                enabled: false,
                gamepad: false,
                controls: false,
            },
            startup_state: None,
            resume: false,
            save_on_exit: false,
        }
    }

    #[test]
    fn prepare_returns_structured_failure_for_a_missing_rom() {
        let directory = tempfile::tempdir().unwrap();
        let rom = directory.path().join("missing.rom");

        let error = match prepare(launch(rom.clone(), directory.path())) {
            Ok(_) => panic!("missing ROM unexpectedly prepared"),
            Err(error) => error,
        };
        let failure = error.downcast::<LaunchFailure>().unwrap();

        assert!(matches!(*failure, LaunchFailure::MissingRom(path) if path == rom));
    }

    #[test]
    fn prepare_validates_an_explicit_state_before_the_rom() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("missing.state");
        let mut request = launch(directory.path().join("missing.rom"), directory.path());
        request.startup_state = Some(state.clone());

        let error = match prepare(request) {
            Ok(_) => panic!("missing state unexpectedly prepared"),
            Err(error) => error,
        };
        let failure = error.downcast::<LaunchFailure>().unwrap();

        assert!(matches!(*failure, LaunchFailure::MissingState(path) if path == state));
    }

    #[test]
    fn ghostty_ctrl_c_workaround_exits_after_an_unreported_key() {
        let mut workaround = GhosttyCtrlCWorkaround::default();
        let press = crossterm::event::KeyEvent::new_with_kind(
            crossterm::event::KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftControl),
            crossterm::event::KeyModifiers::CONTROL,
            crossterm::event::KeyEventKind::Press,
        );
        let release = crossterm::event::KeyEvent::new_with_kind(
            crossterm::event::KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftControl),
            crossterm::event::KeyModifiers::CONTROL,
            crossterm::event::KeyEventKind::Release,
        );

        workaround.handle_key(press);

        assert!(workaround.handle_key(release));
    }

    #[test]
    fn ghostty_ctrl_c_workaround_ignores_control_chords_with_reported_keys() {
        let mut workaround = GhosttyCtrlCWorkaround::default();
        let control_press = crossterm::event::KeyEvent::new_with_kind(
            crossterm::event::KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftControl),
            crossterm::event::KeyModifiers::CONTROL,
            crossterm::event::KeyEventKind::Press,
        );
        let key_press = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        let control_release = crossterm::event::KeyEvent::new_with_kind(
            crossterm::event::KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftControl),
            crossterm::event::KeyModifiers::CONTROL,
            crossterm::event::KeyEventKind::Release,
        );

        workaround.handle_key(control_press);
        workaround.handle_key(key_press);

        assert!(!workaround.handle_key(control_release));
    }

    #[test]
    fn universal_status_switch_hides_both_groups() {
        let lines = status_lines(
            &crate::input::InputBindings::default(),
            crate::config::StatusSettings {
                enabled: false,
                gamepad: true,
                controls: true,
            },
        );

        assert!(lines.is_empty());
    }

    #[test]
    fn group_status_switches_are_independent() {
        let lines = status_lines(
            &crate::input::InputBindings::default(),
            crate::config::StatusSettings {
                enabled: true,
                gamepad: false,
                controls: true,
            },
        );

        assert_eq!(lines, ["Save-f2 Load-f4 Rewind-r Exit-escape"]);
    }
}
