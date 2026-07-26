mod cores;
mod libretro;
mod render;

use clap::Parser;
use std::io::{self, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
const CORE_EXT: &str = "dylib";
#[cfg(target_os = "linux")]
const CORE_EXT: &str = "so";
#[cfg(target_os = "windows")]
const CORE_EXT: &str = "dll";

static RUNNING: AtomicBool = AtomicBool::new(true);

fn resolve_core(user_input: Option<&Path>, cores_dir: &Path, default_name: &str) -> PathBuf {
    let input = match user_input {
        Some(p) => p,
        None => {
            let path = cores_dir.join(format!("{default_name}_libretro.{CORE_EXT}"));
            return path;
        }
    };

    if input.exists() {
        return input.to_path_buf();
    }

    if input.parent().is_none() || input.parent() == Some(Path::new("")) {
        let name = input.to_string_lossy();

        let candidate = cores_dir.join(format!("{name}_libretro.{CORE_EXT}"));
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

#[derive(Parser)]
#[command(name = "asoby", about = "Retro game emulator for the terminal")]
struct Args {
    #[arg(
        short = 'r',
        long = "renderer",
        default_value = "viuer",
        help = "Rendering backend (viuer, ascii, debug)"
    )]
    renderer: String,

    #[arg(
        short = 'c',
        long = "core",
        help = "Path to a libretro core (.dylib/.so/.dll)"
    )]
    core: Option<PathBuf>,

    #[arg(
        long = "render-fps",
        default_value_t = 30,
        value_parser = clap::value_parser!(u32).range(1..=240),
        help = "Maximum terminal refresh rate"
    )]
    render_fps: u32,

    #[arg(
        long = "keep-scrollback",
        help = "Append rendered frames to normal terminal scrollback"
    )]
    keep_scrollback: bool,

    #[arg(help = "Path to the ROM file to load")]
    rom: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::data_local_dir().unwrap());
    let cores_dir = data_home.join("asoby").join("cores");
    std::fs::create_dir_all(&cores_dir).ok();

    let core_name = cores::for_rom(&args.rom);
    let core_path = resolve_core(args.core.as_deref(), &cores_dir, core_name);

    if !core_path.exists() {
        eprintln!("Error: core not found: {}", core_path.display());
        eprintln!("  Place cores in {} or use -c to specify a path", cores_dir.display());
        std::process::exit(1);
    }

    if !args.rom.exists() {
        eprintln!("Error: file not found: {}", args.rom.display());
        std::process::exit(1);
    }

    ctrlc::set_handler(|| {
        RUNNING.store(false, Ordering::SeqCst);
    })?;

    println!("Cores directory: {}", cores_dir.display());
    let core = unsafe { libretro::load_core(&core_path)? };

    unsafe {
        let _version = (core.retro_api_version)();
        libretro::setup_callbacks(&core);
        (core.retro_init)();

        let mut sys_info: libretro::RetroSystemInfo = mem::zeroed();
        (core.retro_get_system_info)(&mut sys_info);
        let name = std::ffi::CStr::from_ptr(sys_info.library_name).to_string_lossy();
        let version = std::ffi::CStr::from_ptr(sys_info.library_version).to_string_lossy();

        let loaded = libretro::load_rom(&core, &args.rom)?;
        if !loaded {
            eprintln!("Failed to load ROM: {}", args.rom.display());
            (core.retro_deinit)();
            return Ok(());
        }

        let mut av_info: libretro::RetroSystemAvInfo = mem::zeroed();
        (core.retro_get_system_av_info)(&mut av_info);
        let w = av_info.geometry.base_width;
        let h = av_info.geometry.base_height;

        let renderer = render::create(&args.renderer, &args.rom, args.keep_scrollback);

        println!(
            "Core: {name} {version}  |  Video: {w}x{h} @ {:.0}fps  |  Renderer: {}",
            av_info.timing.fps, args.renderer
        );
        if args.renderer == "debug" {
            println!("Saving screenshots to debug/  (ctrl+c to exit)\n");
        } else {
            println!("Press ctrl+c to exit\n");
        }

        let (frame_tx, frame_rx) = sync_channel::<Arc<render::Frame>>(1);
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
        let render_duration = Duration::from_secs_f64(1.0 / args.render_fps as f64);
        let mut next_frame = Instant::now();
        let mut next_render = Instant::now();

        while RUNNING.load(Ordering::SeqCst) {
            (core.retro_run)();

            let now = Instant::now();
            if now >= next_render {
                let frame = libretro::FRAME
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

        (core.retro_unload_game)();
        (core.retro_deinit)();
    }

    Ok(())
}
