use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use std::io::{self, Read, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{TrySendError, sync_channel};
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

fn download_core(core_name: &str, cores_dir: &Path) -> Result<PathBuf, String> {
    let base = buildbot_base()
        .ok_or_else(|| "Auto-download not supported on this platform".to_string())?;
    let ext = core_ext();
    let url = format!("{base}{core_name}_libretro.{ext}.zip");
    eprintln!("Downloading {core_name} core...");
    eprintln!("  From: {url}");

    let resp = ureq::get(&url)
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

struct RawGuard;

impl RawGuard {
    fn enter() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub fn run(config: RunConfig) -> Result<(), Box<dyn std::error::Error>> {
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

    ctrlc::set_handler(|| {
        RUNNING.store(false, Ordering::SeqCst);
    })?;

    println!("Cores directory: {}", cores_dir.display());
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
        let name = std::ffi::CStr::from_ptr(sys_info.library_name).to_string_lossy();
        let version = std::ffi::CStr::from_ptr(sys_info.library_version).to_string_lossy();

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

        println!(
            "Core: {name} {version}  |  Video: {w}x{h} @ {:.0}fps  |  Renderer: {}  |  Audio: {}",
            av_info.timing.fps,
            config.renderer,
            audio_backend.name()
        );
        if config.renderer == "debug" {
            println!("Saving screenshots to debug/  (ctrl+c to exit)\n");
        } else {
            println!("Press ctrl+c to exit\n");
        }

        let (frame_tx, frame_rx) = sync_channel::<Arc<crate::render::Frame>>(1);
        let render_thread = thread::spawn(move || -> io::Result<()> {
            let mut renderer = renderer;
            renderer.setup(w, h);
            let mut stdout = io::stdout().lock();
            let result = (|| {
                while let Ok(frame) = frame_rx.recv() {
                    renderer.render(&frame, &mut stdout)?;
                    stdout.flush()?;
                }
                Ok(())
            })();
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
        let mut next_frame = Instant::now();
        let mut next_render = Instant::now();

        let _raw = RawGuard::enter()?;

        while RUNNING.load(Ordering::SeqCst) {
            if event::poll(Duration::ZERO).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        RUNNING.store(false, Ordering::SeqCst);
                    }
                }
            }

            (core.retro_run)();

            let now = Instant::now();
            if now >= next_render {
                let frame = crate::libretro::FRAME
                    .lock()
                    .ok()
                    .and_then(|guard| guard.as_ref().cloned());
                if let Some(frame) = frame {
                    match frame_tx.try_send(frame) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
                next_render = now + render_duration;
            }

            next_frame += frame_duration;
            let now = Instant::now();
            if let Some(delay) = next_frame.checked_duration_since(now) {
                thread::sleep(delay);
            } else {
                next_frame = now;
            }
        }

        drop(frame_tx);
        match render_thread.join() {
            Ok(result) => result?,
            Err(_) => return Err(io::Error::other("renderer thread panicked").into()),
        }

        crate::libretro::AUDIO.lock().unwrap().take();
        audio_backend.stop();

        (core.retro_unload_game)();
        (core.retro_deinit)();
    }

    Ok(())
}
