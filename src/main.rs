mod audio;
mod commands;
mod config;
mod cores;
mod emulation;
mod input;
mod renderer;

use clap::{FromArgMatches, Parser, Subcommand};
use commands::run::RunConfig;
use renderer::RendererMode;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "asoby",
    about = "Retro game emulator for the terminal",
    version,
    disable_version_flag = true,
    disable_help_subcommand = true,
    after_help = "Examples:
  asoby 'Super Mario Bros.nes'                       Start a game
  asoby 'Streets of Rage 2.md' -r ascii              Start a game and render as ASCII characters
  asoby 'Super Castlevania IV.zip' -c snes9x         Run with an explicit core
  asoby 'Super Metroid.sfc' --state ~/backup.state   Load a save state at startup
  asoby core install genesis_plus_gx                 Install a libretro core
  asoby state list 'Pokemon Emerald.gba' --core mgba List saved states, filtered"
)]
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
        short = 'f',
        long = "fps",
        default_value_t = 60,
        value_parser = clap::value_parser!(u32).range(1..=240),
        help = "Maximum terminal refresh rate"
    )]
    fps: u32,

    #[arg(
        short = 'p',
        long = "primary-screen",
        help = "Render in the primary terminal buffer, leaving the final frame visible on exit"
    )]
    no_alt_screen: bool,

    #[arg(short = 'm', long = "mute", help = "Disable game audio")]
    no_audio: bool,

    #[arg(
        short = 'v',
        long = "version",
        action = clap::ArgAction::Version,
        help = "Print version"
    )]
    version: (),

    #[arg(
        short = 's',
        long = "state",
        value_name = "PATH",
        help = "Load a save state file after the core starts"
    )]
    state: Option<PathBuf>,

    #[arg(help = "Path to the ROM file to load")]
    rom: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Manage libretro cores", disable_help_subcommand = true)]
    Core {
        #[command(subcommand)]
        action: CoreAction,
    },
    #[command(about = "Manage save states", disable_help_subcommand = true)]
    State {
        #[command(subcommand)]
        action: StateAction,
    },
}

#[derive(Subcommand)]
enum CoreAction {
    #[command(about = "List installed cores")]
    List,
    #[command(about = "Install a core from buildbot.libretro.com")]
    Install { name: String },
    #[command(about = "Update installed cores from buildbot.libretro.com")]
    Update { name: Option<String> },
    #[command(about = "Remove an installed core")]
    Remove { name: String },
}

#[derive(Subcommand)]
enum StateAction {
    #[command(about = "List managed save states")]
    List {
        #[arg(help = "Filter by the complete ROM filename")]
        rom: Option<String>,
        #[arg(short = 'c', long = "core", help = "Filter by core name")]
        core: Option<String>,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = <Args as clap::CommandFactory>::command().color(clap::ColorChoice::Never);
    let mut matches = command.get_matches_mut();
    let args = Args::from_arg_matches_mut(&mut matches)?;

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

    if let Some(Command::State { action }) = args.command {
        match action {
            StateAction::List { rom, core } => {
                return commands::state::list(rom.as_deref(), core.as_deref());
            }
        }
    }

    let Some(rom) = args.rom else {
        command.print_help()?;
        return Ok(());
    };

    let settings = match config::load_settings() {
        Ok(settings) => settings,
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
        input_bindings: settings.input_bindings,
        rewind: settings.rewind,
        startup_state: args.state,
        save_on_exit: settings.state.save_on_exit,
    };
    commands::run::run(config)
}
