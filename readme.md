# Asoby

## Overview

Play retro games directly in your terminal.

```sh
asoby 'Super Mario Bros.zip'
```

Asoby detects the system, ensures a suitable emulator is available, picks the best
renderer your terminal supports, and starts the game.

## Screenshots

Coming soon.

## Installation (WIP, following contents are not yet available)

- You do not necessarily need to install it if you just want a quick try.

  ```sh
  npx asoby
  ```

- From npm:

  ```sh
  npm install asoby -g
  ```

- From crates.io:

  ```sh
  cargo install asoby
  ```

## Configuration

Asoby loads the first applicable config path:

1. The exact path in `ASOBY_CONFIG`
2. `$XDG_CONFIG_HOME/asoby/config.toml`
3. `~/.config/asoby/config.toml` on Linux and macOS
4. `%APPDATA%\asoby\config.toml` on Windows

The config file is optional and an omitted setting keeps its default. Every
value in the example below is a default. Explicit command-line values override
configuration values.

```toml
[input]
up = "up"        # D-pad, arrow keys by default
down = "down"
left = "left"
right = "right"

a = "x"          # Libretro A button
b = "z"          # Libretro B button
x = "s"
y = "a"

start = "enter"
select = "rshift"
quit = "escape"  # Ctrl-C is always reserved for quitting
rewind = "r"     # Hold to rewind
save_state = "f2"  # Save a new state
load_state = "f4"  # Load the newest state

# Optional shoulder buttons, unbound by default:
# l = "q"
# r = "w"
# l2 = "e"
# r2 = "u"
# l3 = "t"
# r3 = "y"

[display]
renderer = "auto"       # auto, graphic, block, ascii, or debug
fps = 60                 # Maximum terminal refresh rate, from 1 to 240
primary_screen = false  # Use the primary buffer when true

[audio]
muted = false            # Disable game audio when true

[status]
enabled = true         # Show the on-screen keybinding status
gamepad = true         # Show gamepad inputs on the upper status line
controls = true        # Show save, load, rewind, and exit on the lower line

[rewind]
enabled = true          # Snapshot-based rewind
granularity = 2         # Frames between snapshots
buffer_size_mb = 20     # Memory cap for stored snapshots

[state]
save_on_exit = false    # Save a state when exiting cleanly
```

Rewind steps back while the rewind key is held. A higher `granularity` uses
less memory and CPU but rewinds in coarser steps; once `buffer_size_mb` is
reached the oldest snapshots are dropped. Disabling rewind frees the key for
other bindings and hides it from the on-screen status line.

The status text is centered and dimmed over the bottom of the rendered frame.
Set `status.enabled` to `false` to hide both lines, or toggle `gamepad` and
`controls` independently. Save and load notifications remain visible when the
keybinding status is hidden. Notifications are anchored to the bottom-left and
do not change the centered keybindings' position.

The `--primary-screen` and `--muted` options can be used without a value to
enable them, or with an equals-separated boolean to explicitly choose a value:
`--primary-screen`, `--primary-screen=false`, `--muted`, and `--muted=false`.

Each input takes one standalone key, and a key cannot be assigned to more than
one input. Key combinations such as `ctrl+x` are not supported.

### Default controls

| Key              | Control      |
| ---------------- | ------------ |
| Arrow keys       | D-pad        |
| Z / X            | B / A        |
| A / S            | Y / X        |
| Enter            | Start        |
| RShift           | Select       |
| F2               | Save state   |
| F4               | Load state   |
| Escape or Ctrl-C | Exit cleanly |

### Key names

Keys follow the RetroArch conventions. Printable characters are written
directly; the other supported names are:

```text
space  enter  escape  tab  backspace
up  down  left  right  home  end  pageup  pagedown  insert  del
f1 through f24
num0 through num9
period  comma  slash  minus  equals  leftbracket  backslash  rightbracket
backquote  quote  semicolon  tilde
capslock  numlock  print_screen  scroll_lock  pause  menu
shift  rshift  ctrl  rctrl  alt  ralt
left-super  right-super  left-hyper  right-hyper  left-meta  right-meta
iso-level-3-shift  iso-level-5-shift
```

`add` and `subtract` are the main-row `+` and `-` keys. Numeric keypad keys
use the `numpad-` prefix or their RetroArch names:

```text
numpad-0 through numpad-9        keypad0 through keypad9
numpad-decimal  kp_period        numpad-divide  divide
numpad-multiply  multiply        numpad-subtract  kp_minus
numpad-add  kp_plus              numpad-enter  kp_enter
numpad-equal  kp_equals          numpad-comma
numpad-up  numpad-down  numpad-left  numpad-right
numpad-home  numpad-end  numpad-page-up  numpad-page-down
numpad-insert  numpad-delete  numpad-begin
```

Navigation names identify the same physical keys as their numeric names, so
`numpad-end` and `numpad-1` are equivalent regardless of Num Lock state.

The legacy hyphenated spellings (`esc`, `page-up`, `page-down`, `delete`,
`caps-lock`, `num-lock`, `print-screen`, `scroll-lock`, `left-shift`,
`right-shift`, `left-control`, `right-control`, `left-alt`, `right-alt`)
remain accepted.

### Input behavior

Asoby requests enhanced keyboard reporting from terminals that support it,
providing distinct press, repeat, and release events. Traditional terminals,
and sessions through tmux, screen, or SSH, may only report presses and
operating-system auto-repeat. In that case Asoby uses a 140 ms repeat timeout
after a separate initial-hold grace period. This avoids a gap before
auto-repeat starts, but legacy protocols cannot distinguish a held key from a
released key with complete accuracy: a shorter timeout stutters and a longer
timeout can briefly continue movement after release.

Repeat delay, repeat rate, standalone modifier reporting, and simultaneous key
support vary across terminals and operating systems. Asoby preserves every
mapped event it receives and clears held input on focus loss, input errors,
shutdown, and startup. Standalone modifier and numeric keypad bindings require
enhanced keyboard reporting to be distinguished reliably.

## Save states

Press the save key to write a new timestamped state, and the load key to load
the newest state for the current ROM and core. Both actions fire once per key
press. Loading when no state exists is not an error and reports `No save state
found` in the status line. There are no slots, no state browser, and no
automatic pruning: every save creates a new file and Asoby never overwrites or
deletes states. Manage the files with normal filesystem tools.

States are stored under the platform data directory:

```text
<data-dir>/asoby/states/<core-name>/<rom-file-name>/
```

For example, on Linux with `XDG_DATA_HOME` unset, states for `fceumm` live in
`~/.local/share/asoby/states/fceumm/`:

```text
states/
└── fceumm/
    ├── Super Mario Bros.nes/
    │   ├── Super Mario Bros.nes.20260802T151205.903+0800.state
    │   └── Super Mario Bros.nes.20260802T154831.027+0800.state
    └── Contra.zip/
        └── Contra.zip.20260802T160011.412+0800.state
```

The directory and file names use the complete launched ROM filename, including
its extension, so two ROMs with the same name share a state directory. The core
name comes from the core library filename with the `_libretro` suffix and
platform extension removed, so different cores keep separate states.

Timestamps use the local timezone with the numeric offset, in the
filesystem-safe basic ISO 8601 form `YYYYMMDDTHHMMSS.sss+HHMM`
(e.g. `20260802T151205.903+0800`). When a generated filename already exists,
Asoby appends a deterministic counter (`-2`, `-3`, ...) instead of overwriting.
The newest state is selected by parsing these timestamps and comparing absolute
instants, so daylight-saving transitions and copied files with changed
modification times do not affect the result.

### Loading a specific state

Pass any state file to load it at startup, after the core initializes and the
ROM loads but before the first emulated frame:

```sh
asoby 'Super Mario Bros.nes' --state /path/to/super-mario.state
```

A missing, corrupt, incompatible, or unloadable explicitly requested state is a
fatal startup error. The file's embedded core and ROM identifiers must match
the running core and ROM, but its filename is ignored, so renamed or copied
files load fine. Without `--state`, Asoby starts the game normally and does not
automatically load the newest state. States saved after loading an explicit
file still go to the normal managed state directory.

### Save on exit

With `save_on_exit = true` in the `[state]` section, Asoby writes a new state
whenever the session ends cleanly, including Escape and Ctrl-C. It saves while
the ROM and core are still loaded, before core cleanup. Save-on-exit is skipped
when the ROM failed to load or the core does not support complete savestates.

### Listing states

The read-only `state list` command shows every managed state:

```sh
asoby state list
asoby state list 'Super Mario Bros.nes'
asoby state list 'Super Mario Bros.nes' --core fceumm
```

```text
GAME                   CORE     SAVED                         PATH
Super Mario Bros.nes   fceumm   2026-08-02 15:12:05 +08:00   /absolute/path/to/the/state/file
```

`GAME` is the complete launched ROM filename, `SAVED` is formatted in the
timestamp's recorded local offset, and `PATH` is absolute so it can be passed
directly to `rm`, `cp`, or `asoby <rom> --state <path>` (on Unix, paths under
the home directory are shown as `~/...`). Temporary files are skipped. Files that do not match the managed naming scheme, whose embedded
timestamp disagrees with their name, whose embedded core or ROM does not match
the enclosing directories, or that exceed 256 MiB are reported as malformed
with a reason. The command never resolves, installs, or downloads cores.

### Core support

Save states need the core's complete serialization support. When a core does
not provide it, runtime save/load, save-on-exit, and rewind are disabled with a
warning at startup. Loading a state during play preserves the rewind history
and the current session frame counter, so rewind can cross the load boundary.
A state supplied with `--state` at startup is treated as frame zero and rewind
capture starts from it.

## License

[MIT](license)
