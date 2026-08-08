mod audio;
mod commands;
mod config;
mod cores;
mod emulation;
mod input;
mod paths;
mod renderer;
mod terminal;

use clap::{Args as ClapArgs, FromArgMatches, Parser, Subcommand};
use commands::run::RunConfig;
use renderer::RendererMode;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "asobou",
    about = "Play retro games directly in your terminal",
    version,
    disable_version_flag = true,
    disable_help_subcommand = true,
    after_help = "Examples:
  asobou 'Super Mario Bros.nes'                       Start a game
  asobou 'Streets of Rage 2.md' --renderer=ascii      Start a game and render as ASCII characters
  asobou 'Super Castlevania IV.zip' --core=snes9x     Run with an explicit core
  asobou 'Super Metroid.sfc' --state=~/backup.state   Load a save state at startup
  asobou 'Super Metroid.sfc' --resume                 Load the latest managed state at startup
  asobou brew flappybird.nes                          Download and play a homebrew game
  asobou core install genesis_plus_gx                 Install a libretro core
  asobou config set rewind.buffer_size_mb 64          Set a configuration value
  asobou state list 'Pokemon Emerald.gba' --core=mgba List saved states, filtered"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    launch: LaunchArgs,

    #[arg(
        short = 'v',
        long = "version",
        action = clap::ArgAction::Version,
        help = "Print version"
    )]
    version: (),

    #[arg(help = "Path to a ROM file; the system is detected automatically")]
    rom: Option<PathBuf>,
}

#[derive(ClapArgs)]
struct LaunchArgs {
    #[arg(
        short = 'r',
        long = "renderer",
        value_enum,
        help = "Rendering backend; 'auto' uses kitty graphics when the terminal supports it"
    )]
    renderer: Option<RendererMode>,

    #[arg(
        short = 'c',
        long = "core",
        help = "Core name (auto-downloaded if not installed) or path to a libretro core (.dylib/.so/.dll)"
    )]
    core: Option<String>,

    #[arg(
        short = 'f',
        long = "fps",
        value_parser = clap::value_parser!(u32).range(1..=240),
        help = "Cap terminal redraws per second; does not change emulation speed"
    )]
    fps: Option<u32>,

    #[arg(
        short = 'p',
        long = "primary-screen",
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        default_missing_value = "true",
        require_equals = true,
        value_parser = clap::value_parser!(bool),
        value_name = "BOOL",
        help = "Render in the main terminal buffer instead of the alternate screen, so the final frame stays visible after exit"
    )]
    primary_screen: Option<bool>,

    #[arg(
        short = 'm',
        long = "muted",
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        default_missing_value = "true",
        require_equals = true,
        value_parser = clap::value_parser!(bool),
        value_name = "BOOL",
        help = "Disable game audio"
    )]
    muted: Option<bool>,

    #[arg(
        short = 's',
        long = "state",
        value_name = "PATH",
        help = "Load a save state file when the game starts"
    )]
    state: Option<PathBuf>,

    #[arg(
        short = 'R',
        long,
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        default_missing_value = "true",
        require_equals = true,
        value_parser = clap::value_parser!(bool),
        value_name = "BOOL",
        conflicts_with = "state",
        help = "Load the latest managed save state when the game starts"
    )]
    resume: Option<bool>,
}

#[derive(Subcommand)]
enum Command {
    #[command(
        about = "Manage configuration",
        visible_alias = "cfg",
        disable_help_subcommand = true,
        after_help = "Examples:
  asobou config list                         List supported keys and effective values
  asobou config edit                         Open the config in $VISUAL or $EDITOR
  asobou config get rewind.enabled           Print the effective rewind setting
  asobou config set rewind.buffer_size_mb 64 Override the rewind buffer size
  asobou config set display.fps 30            Set the terminal refresh rate
  asobou config set audio.muted true         Disable game audio by default
  asobou config unset rewind.buffer_size_mb  Restore the default rewind buffer size"
    )]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    #[command(
        about = "Manage libretro cores",
        visible_alias = "c",
        disable_help_subcommand = true,
        after_help = "Examples:
  asobou core list                List installed cores
  asobou core install mgba        Install the mGBA core
  asobou core update              Update every installed core
  asobou core remove mgba         Remove the mGBA core"
    )]
    Core {
        #[command(subcommand)]
        action: CoreAction,
    },
    #[command(
        about = "Manage save states",
        visible_alias = "s",
        disable_help_subcommand = true,
        after_help = "Examples:
  asobou state list                                      List every managed save state
  asobou state list 'Pokemon Emerald.gba'                Filter by ROM filename
  asobou state list 'Pokemon Emerald.gba' --core=mgba    Filter by ROM and core"
    )]
    State {
        #[command(subcommand)]
        action: StateAction,
    },
    #[command(
        about = "Download and play a Retrobrews homebrew game",
        visible_alias = "b",
        arg_required_else_help = true,
        after_help = "Downloads a supported Retrobrews homebrew ROM on first use and reuses it from the local cache thereafter.

Supported extensions: .gbc, .rom, .nes, .sms, .gba, .sfc, .d64, .tap.

Examples:
  asobou brew flappybird.nes
  asobou brew pacrun.gba --renderer=ascii
  asobou brew blt.sfc --core=snes9x"
    )]
    Brew {
        #[arg(help = "Homebrew ROM filename, including its extension")]
        game: String,
        #[command(flatten)]
        launch: LaunchArgs,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    #[command(
        about = "List supported configuration keys and effective values",
        visible_alias = "l"
    )]
    List,
    #[command(
        about = "Open the configuration file in an editor",
        visible_alias = "e"
    )]
    Edit,
    #[command(
        about = "Print the effective value of a configuration key",
        visible_alias = "g"
    )]
    Get { key: String },
    #[command(about = "Set a configuration override", visible_alias = "s")]
    Set {
        key: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    #[command(about = "Remove a configuration override", visible_alias = "u")]
    Unset { key: String },
}

#[derive(Subcommand)]
enum CoreAction {
    #[command(about = "List installed cores", visible_alias = "l")]
    List,
    #[command(
        about = "Install a core from buildbot.libretro.com",
        visible_alias = "i"
    )]
    Install { name: String },
    #[command(
        about = "Update one core or all installed cores from buildbot.libretro.com",
        visible_alias = "u"
    )]
    Update { name: Option<String> },
    #[command(about = "Remove an installed core", visible_alias = "rm")]
    Remove { name: String },
}

#[derive(Subcommand)]
enum StateAction {
    #[command(about = "List managed save states (filter by ROM and core)", visible_alias = "l")]
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

    match args.command {
        Some(Command::Config { action }) => {
            let result = match action {
                ConfigAction::List => commands::config::list(),
                ConfigAction::Edit => commands::config::edit(),
                ConfigAction::Get { key } => commands::config::get(&key),
                ConfigAction::Set { key, value } => commands::config::set(&key, &value),
                ConfigAction::Unset { key } => commands::config::unset(&key),
            };
            if let Err(error) = result {
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(Command::Core { action }) => {
            let settings = load_settings();
            let cores_dir = paths::cores_dir(settings.paths.data_dir.as_deref());
            match action {
                CoreAction::List => {
                    commands::core::list(&cores_dir);
                    return Ok(());
                }
                CoreAction::Install { name } => return commands::core::install(&name, &cores_dir),
                CoreAction::Update { name } => {
                    return commands::core::update(name.as_deref(), &cores_dir);
                }
                CoreAction::Remove { name } => return commands::core::remove(&name, &cores_dir),
            }
        }
        Some(Command::State { action }) => match action {
            StateAction::List { rom, core } => {
                let settings = load_settings();
                let states_dir = paths::states_dir(settings.paths.data_dir.as_deref());
                return commands::state::list(rom.as_deref(), core.as_deref(), &states_dir);
            }
        },
        Some(Command::Brew { game, launch }) => {
            let settings = load_settings();
            let cache_dir = paths::brew_cache_dir(settings.paths.cache_dir.as_deref());
            let rom = commands::brew::download(&game, &cache_dir).map_err(std::io::Error::other)?;
            return commands::run::run(run_config(launch, rom, settings));
        }
        None => {}
    }

    let Some(rom) = args.rom else {
        command.print_help()?;
        return Ok(());
    };

    commands::run::run(run_config(args.launch, rom, load_settings()))
}

fn load_settings() -> config::Settings {
    match config::load_settings() {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
}

fn run_config(launch: LaunchArgs, rom: PathBuf, settings: config::Settings) -> RunConfig {
    RunConfig {
        renderer: launch.renderer.unwrap_or(settings.display.renderer),
        core: launch.core,
        render_fps: launch.fps.unwrap_or(settings.display.fps),
        primary_screen: launch
            .primary_screen
            .unwrap_or(settings.display.primary_screen),
        muted: launch.muted.unwrap_or(settings.audio.muted),
        rom,
        cores_dir: paths::cores_dir(settings.paths.data_dir.as_deref()),
        states_dir: paths::states_dir(settings.paths.data_dir.as_deref()),
        input_bindings: settings.input_bindings,
        rewind: settings.rewind,
        status: settings.status,
        startup_state: launch.state,
        resume: launch.resume.unwrap_or(settings.state.resume),
        save_on_exit: settings.state.save_on_exit,
    }
}
