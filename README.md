# gauge

A tiny terminal speedometer for streaming values. Pipe numbers in on stdin and
`gauge` draws a live radial dial: a needle at the current value, an arc scale
with numbered graduations, annotations for the window min, max, mean and the
plus/minus one sigma band, and an overflow alarm.

It is a generic, host side operator tool. It computes its own running statistics
over a sliding window, so it sits downstream of any producer without coupling to
the data source.

```
              .-""""-.
         50  /    |    \
            |  TITLE    |
            |    O      |    O = needle hub
         0   \        /  100
              `-....-'
                17.00
```

## Why

Watching a live rate in a tmux pane usually means staring at a line of text:

```
average rate: 33.746
        min: 0.014s max: 0.102s std dev: 0.01405s window: 10000
```

A dial reads at a glance: where the needle sits, how wide the spread is, whether
the value just ran off the top of the scale. Nothing off the shelf draws a
needle dial with annotated min/max/sigma marks (ttyplot and friends draw
scrolling line plots, ratatui Gauge and rich/textual draw horizontal bars), so
this is a small standalone tool.

## Features

- Radial 270 degree dial, drawn as a true circle at any pane size (the cell
  aspect ratio is compensated, so it never renders as an ellipse).
- Lenient input: one float per line on stdin, taking the first number found on
  each line. It accepts both bare `33.7` and embedded `average rate: 33.746`.
- Running min, max, mean and standard deviation over a sliding window.
- Scale that snaps to round numbers at the data magnitude (18 rounds to 20,
  49995 to 50000, 0.0023 to 0.0025), with numbered major ticks.
- Five fixed reference markers at min, quarter, mid, three quarter and full
  scale.
- Optional title across the top of the dial.
- Optional zero anchoring (`--0`): always keep 0 in the scale, so a speedometer
  reads from 0 even when the data never gets near it.
- Overflow alarm: when a value runs past full scale the needle pins at the top,
  an LED lights, and the gauge holds there for one second before rescaling.
- Staleness signal: if the feed goes quiet for a few seconds the reading is
  flagged STALE and dimmed, so a frozen needle is never mistaken for a live one.
- White by default; red is reserved for the alarm, yellow for stale.
- No async runtime. A stdin reader thread feeds the render loop over a channel.

## Install

Requires a Rust toolchain. The project is pinned for Rust 1.72 (ratatui 0.24,
crossterm 0.27); newer toolchains work too.

```sh
cargo build --release
# binary at target/release/gauge
```

## Usage

```sh
<producer> | gauge [OPTIONS]
```

`gauge` needs an interactive terminal for the display; pipe the data in and it
reads key events from the controlling tty.

### Options

| Option           | Description                                  | Default       |
| ---------------- | -------------------------------------------- | ------------- |
| `--window N`     | samples retained for the statistics window   | 200           |
| `--display NAME` | renderer to use                              | `speedometer` |
| `--title TEXT`   | title shown at the top of the dial           | none          |
| `--0`, `--zero`  | always keep 0 in the scale (e.g. a speedometer) | off        |
| `-h`, `--help`   | print help                                   |               |

Quit with `q`, `Esc`, or `Ctrl-C`.

### Examples

```sh
# Live demo with the bundled feeder
./feed.sh sine | ./target/release/gauge --title RATE

# Downstream of an existing text readout (first number per line is used)
my-rate-printer | ./target/release/gauge --window 10000 --title "ingest/s"

# A quick static sweep
seq 1 100 | awk '{print $1*0.7}' | ./target/release/gauge --window 50
```

### Test feeder

`feed.sh` generates a stream for piping into the gauge. Modes:

| Mode     | What it produces                                            |
| -------- | ---------------------------------------------------------- |
| `sine`   | smooth oscillation, good for watching the needle           |
| `noisy`  | sine plus jitter, exercises the sigma band                 |
| `ramp`   | slow climb then reset, sweeps end to end and overflows     |
| `random` | uniform noise                                              |
| `rate`   | `average rate: N` lines, exercises the lenient parser      |
| `burst`  | emit for a few seconds then go quiet, looping (see below)  |

```sh
./feed.sh noisy 0.05 | ./target/release/gauge
```

To watch the staleness behaviour, use `burst`: it feeds for `on` seconds then
stays quiet (with stdin held open) for `off` seconds, looping. With the default
`off` of 5 seconds, longer than the 3 second stale threshold, the gauge goes
STALE during each quiet window and recovers when the feed resumes.

```sh
./feed.sh burst | ./target/release/gauge --title DEMO   # burst [delay] [on] [off]
```

## Reading the dial

- The needle points at the latest value.
- Numbered graduation ticks mark the scale; minor ticks subdivide each step.
- The five marks just outside the rim are fixed references at min, quarter, mid,
  three quarter and full scale.
- The stat ticks annotate window min, max, mean and the plus/minus one sigma
  band.
- The big number under the hub is the current value. The sample count sits in
  the top-right corner.
- The LED on the lower right lights red on overflow; the needle and value turn
  red while the gauge is capped.

## Architecture

Three layers with a single coupling point, the `Display` trait.

```
src/
  math/      ring buffer window plus running min/max/mean/stddev (pure)
  infra/     stdin reader thread, terminal lifecycle, ~30fps render loop
  display/   Display trait plus the themed speedometer renderer
```

- `math` knows nothing about rendering or I/O.
- `infra` drives any `Display` and knows nothing about what is drawn.
- `display` is the plugin boundary. Adding a renderer is a new file plus one
  line in `make()`. All colors live in one `Theme` struct.

## Toolchain notes

The host is Rust 1.72.1. Dependencies are pinned for that MSRV (`ratatui 0.24`,
`crossterm 0.27`, and `unicode-segmentation` held at `1.10.1` in the lockfile,
since newer releases require a much newer rustc). Build with the committed
`Cargo.lock`.
