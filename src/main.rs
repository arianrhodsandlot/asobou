mod audio;
mod commands;
mod config;
mod cores;
mod emulation;
mod input;
mod renderer;

use clap::{Parser, Subcommand};
use commands::run::RunConfig;
use renderer::RendererMode;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "asoby", about = "Retro game emulator for the terminal")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(
        short = 'r',
        long = "renderer",
        default_value = "auto",
        value_enum,
        help = "Rendering backend"
    )]
    renderer: RendererMode,

    #[arg(
        short = 'c',
        long = "core",
        help = "Core name or path to a libretro core (.dylib/.so/.dll)"
    )]
    core: Option<String>,

    #[arg(
        long = "fps",
        default_value_t = 60,
        value_parser = clap::value_parser!(u32).range(1..=240),
        help = "Maximum terminal refresh rate"
    )]
    fps: u32,

    #[arg(
        long = "no-alt-screen",
        help = "Render in the primary terminal buffer, leaving the final frame visible on exit"
    )]
    no_alt_screen: bool,

    #[arg(long = "no-audio", help = "Disable game audio")]
    no_audio: bool,

    #[arg(help = "Path to the ROM file to load")]
    rom: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Manage libretro cores")]
    Core {
        #[command(subcommand)]
        action: CoreAction,
    },
}

#[derive(Subcommand)]
enum CoreAction {
    #[command(about = "List available and installed cores")]
    List,
    #[command(about = "Install a core from buildbot.libretro.com")]
    Install {
        name: String,
    },
    #[command(about = "Update installed cores from buildbot.libretro.com")]
    Update {
        name: Option<String>,
    },
    #[command(about = "Remove an installed managed core")]
    Remove {
        name: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if let Some(Command::Core { action }) = args.command {
        match action {
            CoreAction::List => {
                commands::core::list();
                return Ok(());
            }
            CoreAction::Install { name } => {
                return commands::core::install(&name);
            }
            CoreAction::Update { name } => {
                return commands::core::update(name.as_deref());
            }
            CoreAction::Remove { name } => {
                return commands::core::remove(&name);
            }
        }
    }

    let Some(rom) = args.rom else {
        let mut cmd = <Args as clap::CommandFactory>::command();
        cmd.print_help()?;
        return Ok(());
    };

    let input_bindings = match config::load_input_bindings() {
        Ok(bindings) => bindings,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    };
    let config = RunConfig {
        renderer: args.renderer,
        core: args.core,
        render_fps: args.fps,
        no_alt_screen: args.no_alt_screen,
        muted: args.no_audio,
        rom,
        input_bindings,
    };
    commands::run::run(config)
}
