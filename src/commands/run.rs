use crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use std::io::{self, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

static RUNNING: AtomicBool = AtomicBool::new(true);
const TERMINAL_INPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(50);
const TERMINAL_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(5);
const STATUS_MESSAGE_DURATION: Duration = Duration::from_secs(2);

struct LatestFrameMailbox {
    state: Mutex<LatestFrameState>,
    ready: Condvar,
}

#[derive(Default)]
struct LatestFrameState {
    frame: Option<Arc<crate::renderer::Frame>>,
    closed: bool,
    waiting: bool,
}

impl LatestFrameMailbox {
    fn new() -> Self {
        Self {
            state: Mutex::new(LatestFrameState {
                waiting: true,
                ..LatestFrameState::default()
            }),
            ready: Condvar::new(),
        }
    }

    fn publish(&self, frame: Arc<crate::renderer::Frame>) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return false;
        }
        state.frame = Some(frame);
        state.waiting = false;
        self.ready.notify_one();
        true
    }

    fn receive(&self) -> Option<Arc<crate::renderer::Frame>> {
        let mut state = self.state.lock().unwrap();
        while state.frame.is_none() && !state.closed {
            state.waiting = true;
            state = self.ready.wait(state).unwrap();
        }
        if state.closed {
            None
        } else {
            state.frame.take()
        }
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        state.frame = None;
        state.waiting = false;
        self.ready.notify_all();
    }

    fn wants_frame(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.waiting && !state.closed
    }
}

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
                msg.push_str(&format!("  asoby {rom_path} --core {core}\n"));
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

pub struct RunConfig {
    pub renderer: crate::renderer::RendererMode,
    pub core: Option<String>,
    pub render_fps: u32,
    pub no_alt_screen: bool,
    pub muted: bool,
    pub rom: PathBuf,
    pub input_bindings: crate::input::InputBindings,
    pub rewind: crate::config::RewindSettings,
    pub startup_state: Option<PathBuf>,
    pub save_on_exit: bool,
}

struct TerminalGuard {
    focus_enabled: bool,
    enhanced_keyboard_enabled: bool,
    release_events_supported: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        let focus_enabled = crossterm::execute!(stdout, EnableFocusChange).is_ok();
        let keyboard_enhancement_supported =
            crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        let enhanced_keyboard_enabled = keyboard_enhancement_supported
            && crossterm::execute!(stdout, PushKeyboardEnhancementFlags(flags)).is_ok();
        Ok(Self {
            focus_enabled,
            enhanced_keyboard_enabled,
            release_events_supported: cfg!(windows) || enhanced_keyboard_enabled,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        if self.enhanced_keyboard_enabled {
            let _ = crossterm::execute!(stdout, PopKeyboardEnhancementFlags);
        }
        if self.focus_enabled {
            let _ = crossterm::execute!(stdout, DisableFocusChange);
        }
        let _ = stdout.flush();
        if self.enhanced_keyboard_enabled {
            drain_pending_terminal_events();
        }
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn drain_pending_terminal_events() {
    let deadline = Instant::now() + TERMINAL_INPUT_DRAIN_TIMEOUT;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match event::poll(remaining.min(TERMINAL_INPUT_POLL_INTERVAL)) {
            Ok(true) => {
                if event::read().is_err() {
                    break;
                }
            }
            Ok(false) => {}
            Err(_) => break,
        }
    }
}

fn rewind_run_frame(core: &crate::emulation::libretro::Core, frame_mailbox: &LatestFrameMailbox) {
    crate::emulation::libretro::set_video_capture_enabled(true);
    unsafe {
        (core.retro_run)();
    }
    let frame = crate::emulation::libretro::FRAME
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned());
    if let Some(frame) = frame {
        frame_mailbox.publish(frame);
    }
}

fn format_status_line(status: &str, message: Option<&str>, width: usize) -> String {
    let mut line = String::new();
    if let Some(message) = message {
        line.push_str(message);
        line.push_str("  ");
    }
    line.push_str(status);
    line.chars().take(width).collect()
}

pub fn run(config: RunConfig) -> Result<(), Box<dyn std::error::Error>> {
    let RunConfig {
        renderer: renderer_mode,
        core: core_arg,
        render_fps,
        no_alt_screen,
        muted,
        rom,
        input_bindings,
        rewind: rewind_settings,
        startup_state,
        save_on_exit,
    } = config;

    if let Some(state_path) = &startup_state
        && !state_path.exists()
    {
        eprintln!("Error: state file not found: {}", state_path.display());
        std::process::exit(1);
    }

    ctrlc::set_handler(|| RUNNING.store(false, Ordering::SeqCst))?;
    RUNNING.store(true, Ordering::SeqCst);
    crate::emulation::libretro::set_joypad_buttons(0);

    let cores_dir = crate::cores::cores_dir();
    std::fs::create_dir_all(&cores_dir).ok();

    let core_path = match resolve_core(
        core_arg.as_deref(),
        &cores_dir,
        rom.to_string_lossy().as_ref(),
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {e}");
            eprintln!(
                "  Place cores in {} or use -c to specify a path",
                cores_dir.display()
            );
            std::process::exit(1);
        }
    };

    if !rom.exists() {
        eprintln!("Error: file not found: {}", rom.display());
        std::process::exit(1);
    }

    let renderer = crate::renderer::create(renderer_mode, &rom, no_alt_screen)?;
    let core = unsafe { crate::emulation::libretro::load_core(&core_path)? };
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
    crate::emulation::libretro::set_target_sample_rate(target_sample_rate);

    unsafe {
        let _version = (core.retro_api_version)();
        crate::emulation::libretro::setup_callbacks(&core);
        (core.retro_init)();

        let mut sys_info: crate::emulation::libretro::RetroSystemInfo = mem::zeroed();
        (core.retro_get_system_info)(&mut sys_info);

        let loaded = crate::emulation::libretro::load_rom(&core, &rom)?;
        if !loaded {
            eprintln!("Failed to load ROM: {}", rom.display());
            (core.retro_deinit)();
            return Ok(());
        }

        let serialization_supported = core.supports_complete_serialization();
        if !serialization_supported {
            eprintln!("Save states and rewind disabled: core does not support complete savestates");
        }
        let state_size = core.state_size().unwrap_or(0);
        let core_name = crate::emulation::state::core_name_from_path(&core_path);
        let game_name = rom
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(startup) = &startup_state {
            if !serialization_supported {
                eprintln!(
                    "Error: core does not support savestates, cannot load {}",
                    startup.display()
                );
                (core.retro_unload_game)();
                (core.retro_deinit)();
                std::process::exit(1);
            }
            if let Err(error) = crate::emulation::state::load_from_path(
                &core, startup, &core_name, &game_name, state_size,
            ) {
                eprintln!("Error: failed to load state {}: {error}", startup.display());
                (core.retro_unload_game)();
                (core.retro_deinit)();
                std::process::exit(1);
            }
        }

        let mut av_info: crate::emulation::libretro::RetroSystemAvInfo = mem::zeroed();
        (core.retro_get_system_av_info)(&mut av_info);
        let w = av_info.geometry.base_width;
        let h = av_info.geometry.base_height;

        let audio_sink = match audio_backend.start(av_info.timing.sample_rate) {
            Ok(sink) => sink,
            Err(e) => {
                eprintln!("Audio init failed: {e}, falling back to null");
                audio_backend = crate::audio::muted();
                audio_backend.start(av_info.timing.sample_rate)?
            }
        };
        *crate::emulation::libretro::AUDIO.lock().unwrap() = Some(audio_sink);

        let mut renderer = renderer;
        renderer.setup(w, h);

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
            rewind.capture(&core, 0);
        }

        let terminal = TerminalGuard::enter()?;
        let status = input_bindings.status_line();
        let frame_mailbox = Arc::new(LatestFrameMailbox::new());
        let render_mailbox = Arc::clone(&frame_mailbox);
        let status_messages = Arc::new(Mutex::new(None::<(String, Instant)>));
        let status_messages_render = Arc::clone(&status_messages);
        let render_thread = thread::spawn(move || -> io::Result<()> {
            let mut renderer = renderer;
            let mut stdout = io::stdout().lock();
            let result = (|| {
                while let Some(frame) = render_mailbox.receive() {
                    renderer.render(&frame, &mut stdout)?;
                    let (columns, rows) = crossterm::terminal::size()?;
                    let width = usize::from(columns);
                    let message = {
                        let mut guard = status_messages_render.lock().unwrap();
                        match &*guard {
                            Some((text, at)) if at.elapsed() < STATUS_MESSAGE_DURATION => {
                                Some(text.clone())
                            }
                            _ => {
                                *guard = None;
                                None
                            }
                        }
                    };
                    let line = format_status_line(&status, message.as_deref(), width);
                    write!(stdout, "\x1b[{};1H\x1b[K{line:.width$}", rows)?;
                    stdout.flush()?;
                }
                Ok(())
            })();
            render_mailbox.close();
            drop(stdout);
            renderer.cleanup();
            result
        });

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
            terminal.release_events_supported,
        );
        let mut input_error = None;
        let set_message = |text: &str| {
            if let Ok(mut guard) = status_messages.lock() {
                *guard = Some((text.to_string(), Instant::now()));
            }
        };
        let mut frame_count = 0u64;

        while RUNNING.load(Ordering::SeqCst) {
            loop {
                match event::poll(Duration::ZERO) {
                    Ok(true) => match event::read() {
                        Ok(Event::Key(key)) => {
                            input.handle_key(key, Instant::now());
                            if input.quit_requested() {
                                RUNNING.store(false, Ordering::SeqCst);
                            }
                        }
                        Ok(Event::FocusLost) => input.clear(),
                        Ok(_) => {}
                        Err(error) => {
                            input.clear();
                            input_error = Some(error);
                            RUNNING.store(false, Ordering::SeqCst);
                            break;
                        }
                    },
                    Ok(false) => break,
                    Err(error) => {
                        input.clear();
                        input_error = Some(error);
                        RUNNING.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }

            input.expire(Instant::now());
            if serialization_supported {
                if input.take_save() {
                    match crate::emulation::state::save_state(
                        &core, state_size, &core_name, &game_name,
                    ) {
                        Ok(_) => set_message("State saved"),
                        Err(error) => set_message(&format!("Save failed: {error}")),
                    }
                }
                if input.take_load() {
                    match crate::emulation::state::load_newest(
                        &core, &core_name, &game_name, state_size,
                    ) {
                        Ok(Some(_)) => set_message("State loaded"),
                        Ok(None) => set_message("No save state found"),
                        Err(error) => set_message(&format!("Load failed: {error}")),
                    }
                }
            }
            crate::emulation::libretro::set_joypad_buttons(input.button_mask());
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }

            let rewound = if input.rewind_pressed() {
                if let Some(rewind) = rewind.as_mut() {
                    crate::emulation::libretro::set_audio_muted(true);
                    let target = rewind.rewind(&core, frame_count, &mut || {
                        rewind_run_frame(&core, &frame_mailbox)
                    });
                    crate::emulation::libretro::set_audio_muted(false);
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
                let capture_frame = Instant::now() >= next_render && frame_mailbox.wants_frame();
                crate::emulation::libretro::set_video_capture_enabled(capture_frame);
                (core.retro_run)();
                frame_count += 1;
                if let Some(rewind) = rewind.as_mut() {
                    rewind.capture(&core, frame_count);
                }

                if capture_frame {
                    let frame = crate::emulation::libretro::FRAME
                        .lock()
                        .ok()
                        .and_then(|guard| guard.as_ref().cloned());
                    if let Some(frame) = frame
                        && !frame_mailbox.publish(frame)
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
        crate::emulation::libretro::set_joypad_buttons(0);
        crate::emulation::libretro::set_video_capture_enabled(true);
        frame_mailbox.close();
        match render_thread.join() {
            Ok(result) => result?,
            Err(_) => return Err(io::Error::other("renderer thread panicked").into()),
        }
        drop(terminal);

        if serialization_supported && save_on_exit {
            match crate::emulation::state::save_state(&core, state_size, &core_name, &game_name) {
                Ok(path) => println!("Saved state: {}", path.display()),
                Err(error) => eprintln!("Warning: failed to save state on exit: {error}"),
            }
        }

        if let Ok(mut audio) = crate::emulation::libretro::AUDIO.lock() {
            audio.take();
        }
        audio_backend.stop();

        (core.retro_unload_game)();
        (core.retro_deinit)();

        if let Some(error) = input_error {
            return Err(error.into());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: u8) -> Arc<crate::renderer::Frame> {
        Arc::new(crate::renderer::Frame {
            data: vec![value; 3],
            width: 1,
            height: 1,
        })
    }

    fn mailbox() -> LatestFrameMailbox {
        LatestFrameMailbox::new()
    }

    #[test]
    fn mailbox_replaces_a_waiting_frame_with_the_latest() {
        let mailbox = mailbox();
        mailbox.publish(frame(1));
        mailbox.publish(frame(2));

        assert_eq!(mailbox.receive().unwrap().data, vec![2; 3]);
    }

    #[test]
    fn closed_mailbox_discards_frames_and_rejects_new_ones() {
        let mailbox = mailbox();
        mailbox.publish(frame(1));
        mailbox.close();

        assert!(mailbox.receive().is_none());
        assert!(!mailbox.publish(frame(2)));
    }

    #[test]
    fn mailbox_requests_another_frame_after_the_current_one_is_received() {
        let mailbox = Arc::new(mailbox());
        assert!(mailbox.wants_frame());
        mailbox.publish(frame(1));
        assert!(!mailbox.wants_frame());
        mailbox.receive();

        let receiver_mailbox = Arc::clone(&mailbox);
        let receiver = thread::spawn(move || receiver_mailbox.receive());
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut wanted = mailbox.wants_frame();
        while !wanted && Instant::now() < deadline {
            thread::yield_now();
            wanted = mailbox.wants_frame();
        }
        mailbox.close();

        assert!(wanted);
        assert!(receiver.join().unwrap().is_none());
    }

    #[test]
    fn status_line_shows_the_message_before_the_controls() {
        let controls = "↑-up ↓-down ←-left →-right Select-rshift Start-enter X-s Y-a A-x B-z Exit-escape Rewind-r Save-f2 Load-f4";

        let line = format_status_line(controls, Some("State saved"), 80);

        assert!(line.starts_with("State saved  ↑-up"));
        assert_eq!(line.chars().count(), 80);
    }

    #[test]
    fn status_line_without_a_message_is_not_padded() {
        assert_eq!(format_status_line("Exit-escape", None, 20), "Exit-escape");
    }
}
