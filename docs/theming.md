# Theming

The dial's palette is plain white by default (red/yellow are reserved for the
overflow/stale alarms). Pick a built-in preset by name, no files needed:

```sh
./feed.sh sine | termtaco --theme nord
```

| Preset | |
| --- | --- |
| `bw` | plain white/gray, the default |
| `color` | a general colorful palette |
| `catppuccin-mocha` | soft pastels on a warm dark base |
| `dracula` | neon purple, pink, and green |
| `gruvbox` | warm retro, aqua arc, ember needle |
| `nord` | frosty, low-contrast arctic blues |
| `solarized-dark` | muted teal and amber on deep sea |
| `tokyo-night` | cool blues and violet |

For a fully custom palette, write `~/.config/termtaco/theme`: one
`field = color` line per dial element, e.g.

```
needle = yellow
arc = cyan
alarm = "#ff0055"
```

Colors are ANSI names (`red`, `light_blue`, `dark_gray`, underscores optional,
case-insensitive) or `#rrggbb` hex. Fields the file doesn't mention keep their
default, so a one-line file is a valid theme.

`--print-theme NAME` dumps any built-in preset as a starting point: this
works from a plain `cargo install termtaco` with nothing cloned, since the
presets are compiled into the binary:

```sh
mkdir -p ~/.config/termtaco
termtaco --print-theme nord > ~/.config/termtaco/theme
```

(Working from a checkout of this repo, [`themes/`](../themes/) has the same
files directly: `cp themes/nord.theme ~/.config/termtaco/theme`.)

`--theme` overrides the config file when both are given. An absent config
file, or a line with an unknown field or an unparsable color, falls back to
the default for that one field: nothing to set up for the default look, and
a typo can't break the dial.
