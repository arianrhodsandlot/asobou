# Asoby

Run retro games in the terminal.

It's still in development.

## Renderers

Asoby selects graphics mode when the terminal supports the Kitty graphics
protocol and otherwise uses colored half blocks. Select a mode explicitly with
`--renderer graphic`, `--renderer block`, `--renderer ascii`, or
`--renderer debug`.

## Controls

| Key | Control |
| --- | --- |
| Arrow keys | D-pad |
| Z / X | B / A |
| A / S | Y / X |
| Enter | Start |
| Backspace | Select |
| Esc or Ctrl-C | Exit cleanly |

Ctrl-C is always reserved for exiting Asoby. Every other binding can be changed
in `config.toml`:

```toml
[input]
up = "up"
down = "down"
left = "left"
right = "right"

a = "x"
b = "z"
x = "s"
y = "a"

start = "enter"
select = "backspace"
quit = "esc"
rewind = "r"
```

Rewind keeps compressed snapshots of the emulated state and steps back while
the rewind key is held. It costs up to 20 MB of memory plus some CPU, so it can
be tuned or switched off entirely:

```toml
[rewind]
enabled = true
granularity = 2
buffer_size_mb = 20
```

`granularity` is the number of frames between snapshots: higher values use less
memory and CPU but rewind in coarser steps. `buffer_size_mb` caps the memory
spent on stored snapshots; when the cap is reached the oldest snapshots are
dropped. When rewind is disabled the rewind key is freed for other bindings
and is hidden from the on-screen status line.

Asoby loads the first applicable config path:

1. The exact path in `ASOBY_CONFIG`
2. `$XDG_CONFIG_HOME/asoby/config.toml`
3. `~/.config/asoby/config.toml` on Linux and macOS
4. `%APPDATA%\asoby\config.toml` on Windows

The config file is optional. An omitted setting retains its default. Each input
accepts one standalone key, and a key cannot be assigned to more than one
input. Key combinations such as `ctrl+x` are not supported.

Printable characters are written directly. Other supported names are:

```text
space
backspace  enter  tab  esc
up  down  left  right
home  end  page-up  page-down
insert  delete
f1 through f24
caps-lock  scroll-lock  num-lock  print-screen  pause  menu
left-shift  right-shift
left-control  right-control
left-alt  right-alt
left-super  right-super
left-hyper  right-hyper
left-meta  right-meta
iso-level-3-shift  iso-level-5-shift
```

Numeric keypad keys use the `numpad-` prefix:

```text
numpad-0 through numpad-9
numpad-decimal  numpad-divide  numpad-multiply
numpad-subtract  numpad-add  numpad-enter
numpad-equal  numpad-comma
numpad-up  numpad-down  numpad-left  numpad-right
numpad-home  numpad-end  numpad-page-up  numpad-page-down
numpad-insert  numpad-delete  numpad-begin
```

Navigation names identify the same physical keys as their numeric names, so
`numpad-end` and `numpad-1` are equivalent regardless of Num Lock state.

The remaining standard Libretro buttons can be assigned with the optional
`l`, `r`, `l2`, `r2`, `l3`, and `r3` settings.

Asoby requests enhanced keyboard reporting from terminals that support it. This
provides distinct press, repeat, and release events. Traditional terminals, and
sessions passing through tmux, screen, or SSH, may only report presses and
operating-system auto-repeat. In that case Asoby uses a 140 ms repeat timeout
after a separate initial-hold grace period. This avoids a gap before auto-repeat
starts, but legacy protocols cannot distinguish a held key from a released key
with complete accuracy. A shorter timeout can stutter and a longer timeout can
briefly continue movement after release.

Repeat delay, repeat rate, standalone modifier reporting, and simultaneous key
support vary across terminals and operating systems. Asoby preserves every
mapped event it receives and clears held input on focus loss, input errors,
shutdown, and startup. Standalone modifier and numeric keypad bindings require
enhanced keyboard reporting to be distinguished reliably.

The terminal input layer owns persistent per-key and per-button state. The
Libretro FFI callback only validates the requested port, device, index, and
standard joypad button ID before reading an atomic bitmask. Terminal polling and
state mutation remain outside the callback, so no lock is held while a core runs
or a frame renders.

## License

[MIT](license)
