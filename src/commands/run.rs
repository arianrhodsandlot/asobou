use crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use std::io::{self, Read, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn core_ext() -> &'static str {
    match std::env::consts::OS {
        "macos" => "dylib",
        "linux" => "so",
        "windows" => "dll",
        _ => "so",
    }
}

fn buildbot_base() -> Option<&'static str> {
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

fn http_agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into()
}

fn download_core(core_name: &str, cores_dir: &Path) -> Result<PathBuf, String> {
    let base = buildbot_base()
        .ok_or_else(|| "Auto-download not supported on this platform".to_string())?;
    let ext = core_ext();
    let url = format!("{base}{core_name}_libretro.{ext}.zip");
    eprintln!("Downloading {core_name} core...");
    eprintln!("  From: {url}");

    let agent = http_agent();
    let resp = agent
        .get(&url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;

    if resp.status() != 200 {
        return Err(format!(
            "HTTP {} — core '{core_name}' not found on buildbot",
            resp.status()
        ));
    }

    let mut data = Vec::new();
    resp.into_body()
        .into_reader()
        .read_to_end(&mut data)
        .map_err(|e| format!("Read failed: {e}"))?;

    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid zip: {e}"))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Zip read error: {e}"))?;
        let name = file.name().to_string();
        if name.to_lowercase().ends_with(core_ext()) {
            let fname = std::path::Path::new(&name)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(name);
            let out_path = cores_dir.join(&fname);
            let mut out =
                std::fs::File::create(&out_path).map_err(|e| format!("Cannot create file: {e}"))?;
            std::io::copy(&mut file, &mut out).map_err(|e| format!("Extract failed: {e}"))?;
            eprintln!("  Saved: {}", out_path.display());
            return Ok(out_path);
        }
    }

    Err(format!("No .{} file found in downloaded zip", core_ext()))
}

static RUNNING: AtomicBool = AtomicBool::new(true);

struct LatestFrameMailbox {
    state: Mutex<LatestFrameState>,
    ready: Condvar,
}

#[derive(Default)]
struct LatestFrameState {
    frame: Option<Arc<crate::render::Frame>>,
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

    fn publish(&self, frame: Arc<crate::render::Frame>) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return false;
        }
        state.frame = Some(frame);
        state.waiting = false;
        self.ready.notify_one();
        true
    }

    fn receive(&self) -> Option<Arc<crate::render::Frame>> {
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

fn resolve_core(user_input: Option<&Path>, cores_dir: &Path, default_name: &str) -> PathBuf {
    let input = match user_input {
        Some(p) => p,
        None => {
            let path = cores_dir.join(format!("{default_name}_libretro.{}", core_ext()));
            return path;
        }
    };

    if input.exists() {
        return input.to_path_buf();
    }

    if input.parent().is_none() || input.parent() == Some(Path::new("")) {
        let name = input.to_string_lossy();

        let candidate = cores_dir.join(format!("{name}_libretro.{}", core_ext()));
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

pub struct RunConfig {
    pub renderer: String,
    pub core: Option<PathBuf>,
    pub render_fps: u32,
    pub keep_scrollback: bool,
    pub audio: String,
    pub rom: PathBuf,
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

    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::data_local_dir().unwrap());
    let cores_dir = data_home.join("asoby").join("cores");
    std::fs::create_dir_all(&cores_dir).ok();

    let core_name = crate::cores::for_rom(&config.rom);
    let core_path = resolve_core(config.core.as_deref(), &cores_dir, core_name);

    let core_path = if core_path.exists() {
        core_path
    } else if config.core.is_none() {
        match download_core(core_name, &cores_dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {e}");
                eprintln!(
                    "  Place cores in {} or use -c to specify a path",
                    cores_dir.display()
                );
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("Error: core not found: {}", core_path.display());
        eprintln!(
            "  Place cores in {} or use -c to specify a path",
            cores_dir.display()
        );
        std::process::exit(1);
    };

    if !config.rom.exists() {
        eprintln!("Error: file not found: {}", config.rom.display());
        std::process::exit(1);
    }

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

        let renderer = crate::render::create(&config.renderer, &config.rom, config.keep_scrollback);

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
        let frame_mailbox = Arc::new(LatestFrameMailbox::new());
        let render_mailbox = Arc::clone(&frame_mailbox);
        let render_thread = thread::spawn(move || -> io::Result<()> {
            let mut renderer = renderer;
            let mut stdout = io::stdout().lock();
            let status = "Press Q, Esc, or ctrl+c to exit  |  ctrl+r resets stuck input";
            let result = (|| {
                while let Some(frame) = render_mailbox.receive() {
                    renderer.render(&frame, &mut stdout)?;
                    let (_, rows) = crossterm::terminal::size()?;
                    write!(stdout, "\x1b[{};1H\x1b[K{status}", rows)?;
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
        let mut input = crate::input::InputState::with_release_events_supported(
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

    fn frame(value: u8) -> Arc<crate::render::Frame> {
        Arc::new(crate::render::Frame {
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
