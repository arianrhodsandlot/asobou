use crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use std::io::{self, BufRead, IsTerminal, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

static RUNNING: AtomicBool = AtomicBool::new(true);

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

fn prompt_yes(question: &str, default_yes: bool) -> bool {
    if default_yes {
        eprint!("{question} [Y/n] ");
    } else {
        eprint!("{question} [y/N] ");
    }
    let _ = std::io::stderr().flush();
    let stdin = std::io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_err() {
        return default_yes;
    }
    let answer = line.trim().to_ascii_lowercase();
    if answer.is_empty() {
        return default_yes;
    }
    answer.starts_with('y')
}

fn resolve_core(
    user_input: Option<&str>,
    cores_dir: &Path,
    default_name: &str,
    yes: bool,
    no_download: bool,
) -> Result<PathBuf, String> {
    if let Some(input) = user_input {
        let input_path = Path::new(input);

        if input_path.exists() {
            return Ok(input_path.to_path_buf());
        }

        let is_name = input_path.parent().is_none()
            || input_path.parent() == Some(Path::new(""));

        if is_name {
            if let Some(core) = crate::cores::find_core(input) {
                if !crate::cores::is_installed(core, cores_dir) {
                    if no_download {
                        return Err(format!(
                            "Core '{}' is not installed and --no-download prevents automatic installation.\n\
                             Install it manually with: asoby core install {}",
                            core.name, core.name
                        ));
                    }
                    let interactive = std::io::stdin().is_terminal();
                    if !yes && !interactive {
                        return Err(format!(
                            "Core '{}' is not installed. Use --yes to install automatically, or --no-download to forbid network access.\n  Install target: {}",
                            core.name, cores_dir.display()
                        ));
                    }
                    if !yes && interactive {
                        eprintln!(
                            "The recommended core, {}, is not installed.",
                            core.name
                        );
                        if !prompt_yes(
                            &format!(
                                "Install it from buildbot.libretro.com to {}?",
                                cores_dir.display()
                            ),
                            true,
                        ) {
                            return Err("Core installation declined.".to_string());
                        }
                    }
                    crate::cores::download_and_install(core, cores_dir, false)?;
                }
                return Ok(crate::cores::resolve_core_library_path(
                    core.artifact,
                    cores_dir,
                ));
            }

            let candidate =
                crate::cores::resolve_core_path(Some(input_path), cores_dir, default_name);
            if candidate.exists() {
                return Ok(candidate);
            }
            return Err(format!(
                "Unknown core: '{input}'. Use 'asoby core list' to see available cores."
            ));
        }

        let candidate = crate::cores::resolve_core_path(Some(input_path), cores_dir, default_name);
        if candidate.exists() {
            return Ok(candidate);
        }
        return Err(format!("Core not found: {input}"));
    }

    // No --core provided: detect from ROM, then ensure installed
    let detection = crate::cores::detect_rom(std::path::Path::new(default_name), None);
    match detection {
        crate::cores::Detection::Detected {
            core_name,
            system_name,
        } => {
            if core_name.is_empty() {
                return Err("Could not detect system for this ROM. Use --core to specify a core.".to_string());
            }
            let core = crate::cores::find_core(core_name).unwrap();
            eprintln!("Detected {system_name}");

            if !crate::cores::is_installed(core, cores_dir) {
                if no_download {
                    return Err(format!(
                        "Core '{}' is not installed and --no-download prevents automatic installation.\n\
                         Install it manually with: asoby core install {}",
                        core.name, core.name
                    ));
                }
                let interactive = std::io::stdin().is_terminal();
                if !yes && !interactive {
                    return Err(format!(
                        "Core '{}' is not installed. Use --yes to install automatically, or --no-download to forbid network access.\n  Install target: {}",
                        core.name, cores_dir.display()
                    ));
                }
                if !yes && interactive {
                    eprintln!(
                        "The recommended core, {}, is not installed.",
                        core.name
                    );
                    if !prompt_yes(
                        &format!(
                            "Install it from buildbot.libretro.com to {}?",
                            cores_dir.display()
                        ),
                        true,
                    ) {
                        return Err("Core installation declined.".to_string());
                    }
                }
                crate::cores::download_and_install(core, cores_dir, false)?;
            }
            Ok(crate::cores::resolve_core_library_path(
                core.artifact,
                cores_dir,
            ))
        }
        crate::cores::Detection::Ambiguous { candidates } => {
            let mut msg = format!("error: \"{default_name}\" could be a");
            let names: Vec<_> = candidates.iter().map(|(sys, _)| *sys).collect();
            match names.len() {
                0 => {}
                1 => {
                    msg.push_str(&format!(" {} ROM", names[0]));
                }
                2 => {
                    msg.push_str(&format!(" {} or {} ROM", names[0], names[1]));
                }
                _ => {
                    let last = names.last().unwrap();
                    let rest = &names[..names.len() - 1];
                    for sys in rest {
                        msg.push_str(&format!(" {},", sys));
                    }
                    msg.push_str(&format!(" or {} ROM", last));
                }
            }
            msg.push_str("\n\nSelect a core explicitly:\n");
            for (_sys, core) in &candidates {
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
    pub keep_scrollback: bool,
    pub audio: String,
    pub rom: PathBuf,
    pub yes: bool,
    pub no_download: bool,
    pub input_bindings: crate::input::InputBindings,
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
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub fn run(config: RunConfig) -> Result<(), Box<dyn std::error::Error>> {
    ctrlc::set_handler(|| std::process::exit(0))?;
    RUNNING.store(true, Ordering::SeqCst);
    crate::libretro::set_joypad_buttons(0);

    let cores_dir = crate::cores::cores_dir();
    std::fs::create_dir_all(&cores_dir).ok();

    let core_path = match resolve_core(
        config.core.as_deref(),
        &cores_dir,
        config.rom.to_string_lossy().as_ref(),
        config.yes,
        config.no_download,
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

    if !config.rom.exists() {
        eprintln!("Error: file not found: {}", config.rom.display());
        std::process::exit(1);
    }

    let renderer = crate::renderer::create(config.renderer, &config.rom, config.keep_scrollback)?;
    let core = unsafe { crate::libretro::load_core(&core_path)? };
    let mut audio_backend = crate::audio::create(&config.audio);
    let target_sample_rate = match audio_backend.preferred_sample_rate() {
        Ok(sample_rate) => sample_rate,
        Err(e) => {
            eprintln!("Audio device unavailable: {e}, falling back to null");
            audio_backend = crate::audio::create("null");
            None
        }
    };
    crate::libretro::set_target_sample_rate(target_sample_rate);

    unsafe {
        let _version = (core.retro_api_version)();
        crate::libretro::setup_callbacks(&core);
        (core.retro_init)();

        let mut sys_info: crate::libretro::RetroSystemInfo = mem::zeroed();
        (core.retro_get_system_info)(&mut sys_info);

        let loaded = crate::libretro::load_rom(&core, &config.rom)?;
        if !loaded {
            eprintln!("Failed to load ROM: {}", config.rom.display());
            (core.retro_deinit)();
            return Ok(());
        }

        let mut av_info: crate::libretro::RetroSystemAvInfo = mem::zeroed();
        (core.retro_get_system_av_info)(&mut av_info);
        let w = av_info.geometry.base_width;
        let h = av_info.geometry.base_height;

        let audio_sink = match audio_backend.start(av_info.timing.sample_rate) {
            Ok(sink) => sink,
            Err(e) => {
                eprintln!("Audio init failed: {e}, falling back to null");
                audio_backend = crate::audio::create("null");
                audio_backend.start(av_info.timing.sample_rate)?
            }
        };
        *crate::libretro::AUDIO.lock().unwrap() = Some(audio_sink);

        let mut renderer = renderer;
        renderer.setup(w, h);

        let terminal = TerminalGuard::enter()?;
        let status = config.input_bindings.status_line();
        let input_bindings = config.input_bindings;
        let frame_mailbox = Arc::new(LatestFrameMailbox::new());
        let render_mailbox = Arc::clone(&frame_mailbox);
        let render_thread = thread::spawn(move || -> io::Result<()> {
            let mut renderer = renderer;
            let mut stdout = io::stdout().lock();
            let result = (|| {
                while let Some(frame) = render_mailbox.receive() {
                    renderer.render(&frame, &mut stdout)?;
                    let (columns, rows) = crossterm::terminal::size()?;
                    let width = usize::from(columns);
                    write!(stdout, "\x1b[{};1H\x1b[K{status:.width$}", rows)?;
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
        let render_duration = Duration::from_secs_f64(1.0 / config.render_fps as f64);
        let schedule_start = Instant::now();
        let mut next_frame = schedule_start;
        let mut next_render = schedule_start;
        let mut input = crate::input::InputState::with_bindings(
            input_bindings,
            terminal.release_events_supported,
        );
        let mut input_error = None;

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
            crate::libretro::set_joypad_buttons(input.button_mask());
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }

            let capture_frame = Instant::now() >= next_render && frame_mailbox.wants_frame();
            crate::libretro::set_video_capture_enabled(capture_frame);
            (core.retro_run)();

            if capture_frame {
                let frame = crate::libretro::FRAME
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

            next_frame += frame_duration;
            let now = Instant::now();
            if let Some(delay) = next_frame.checked_duration_since(now) {
                thread::sleep(delay);
            } else {
                next_frame = now;
            }
        }

        input.clear();
        crate::libretro::set_joypad_buttons(0);
        crate::libretro::set_video_capture_enabled(true);
        frame_mailbox.close();
        match render_thread.join() {
            Ok(result) => result?,
            Err(_) => return Err(io::Error::other("renderer thread panicked").into()),
        }
        drop(terminal);

        if let Ok(mut audio) = crate::libretro::AUDIO.lock() {
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
}
