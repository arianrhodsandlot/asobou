mod audio;
mod commands;
mod cores;
mod input;
mod libretro;
mod render;

use clap::{Parser, Subcommand};
use commands::run::RunConfig;
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
        help = "Rendering backend (auto, block, ascii, debug)"
    )]
    renderer: String,

    #[arg(
        short = 'c',
        long = "core",
        help = "Core name or path to a libretro core (.dylib/.so/.dll)"
    )]
    core: Option<String>,

    #[arg(
        long = "render-fps",
        default_value_t = 60,
        value_parser = clap::value_parser!(u32).range(1..=240),
        help = "Maximum terminal refresh rate"
    )]
    render_fps: u32,

    #[arg(
        long = "keep-scrollback",
        help = "Append rendered frames to normal terminal scrollback"
    )]
    keep_scrollback: bool,

    #[arg(
        short = 'a',
        long = "audio",
        default_value = "cpal",
        help = "Audio backend (cpal, null)"
    )]
    audio: String,

    #[arg(
        short = 'y',
        long = "yes",
        help = "Automatically confirm prompts (install cores, remove cores)"
    )]
    yes: bool,

    #[arg(
        long = "no-download",
        help = "Forbid automatic core downloads"
    )]
    no_download: bool,

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
                commands::core::list(args.yes, args.no_download);
                return Ok(());
            }
            CoreAction::Install { name } => {
                return commands::core::install(&name, args.yes, args.no_download);
            }
            CoreAction::Update { name } => {
                return commands::core::update(name.as_deref(), args.yes, args.no_download);
            }
            CoreAction::Remove { name } => {
                return commands::core::remove(&name, args.yes);
            }
        }
    }

    let Some(rom) = args.rom else {
        let mut cmd = <Args as clap::CommandFactory>::command();
        cmd.print_help()?;
        return Ok(());
    };

    let config = RunConfig {
        renderer: args.renderer,
        core: args.core,
        render_fps: args.render_fps,
        keep_scrollback: args.keep_scrollback,
        audio: args.audio,
        rom,
        yes: args.yes,
        no_download: args.no_download,
    };
    commands::run::run(config)
}
