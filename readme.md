# Asoby

Run retro games in the terminal.

It's still in development.

## Controls

| Key | Control |
| --- | --- |
| Arrow keys | D-pad |
| Z / X | B / A |
| A / S | Y / X |
| Enter | Start |
| Right Shift or Backspace | Select |
| Q, Esc, or Ctrl-C | Exit cleanly |
| Ctrl-R | Clear all held buttons |

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
shutdown, and startup. Use Ctrl-R to clear input manually if a terminal does not
report focus loss and a button becomes stuck.

The terminal input layer owns persistent per-key and per-button state. The
Libretro FFI callback only validates the requested port, device, index, and
standard joypad button ID before reading an atomic bitmask. Terminal polling and
state mutation remain outside the callback, so no lock is held while a core runs
or a frame renders.

## License

[MIT](license)
