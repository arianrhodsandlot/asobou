# Asoby

## Overview

Asoby is a command-line emulator that plays retro games in the terminal:

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
value in the example below is a default:

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

# Optional shoulder buttons, unbound by default:
# l = "q"
# r = "w"
# l2 = "e"
# r2 = "u"
# l3 = "t"
# r3 = "y"

[rewind]
enabled = true          # Snapshot-based rewind
granularity = 2         # Frames between snapshots
buffer_size_mb = 20     # Memory cap for stored snapshots
```

Rewind steps back while the rewind key is held. A higher `granularity` uses
less memory and CPU but rewinds in coarser steps; once `buffer_size_mb` is
reached the oldest snapshots are dropped. Disabling rewind frees the key for
other bindings and hides it from the on-screen status line.

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

## License

[MIT](license)
