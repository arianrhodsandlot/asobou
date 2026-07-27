mod audio;
mod commands;
mod cores;
mod input;
mod libretro;
mod render;

use clap::Parser;
use commands::run::RunConfig;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "asoby", about = "Retro game emulator for the terminal")]
struct Args {
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
        help = "Path to a libretro core (.dylib/.so/.dll)"
    )]
    core: Option<PathBuf>,

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

    #[arg(help = "Path to the ROM file to load")]
    rom: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
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
    };
    commands::run::run(config)
}
