# termtaco

**A terminal tachometer.** Pipe a stream of numbers into `termtaco` and it draws
a live radial **speedometer gauge** in your terminal: a needle at the current
value, an arc scale with numbered graduations, window min/max/mean and a ±1σ
band, plus an overflow alarm and a staleness signal. A tiny **TUI/CLI dial** for
watching real-time metrics, rates, and any stream of floats at a glance.

[![CI](https://github.com/clausqr/termtaco/actions/workflows/ci.yml/badge.svg)](https://github.com/clausqr/termtaco/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/termtaco.svg)](https://crates.io/crates/termtaco)
[![docs.rs](https://docs.rs/termtaco/badge.svg)](https://docs.rs/termtaco)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

<p align="center">
  <img src="assets/demo.gif" alt="termtaco terminal speedometer gauge TUI demo: a live needle dial in the terminal" width="640">
</p>

`termtaco` reads one float per line from **stdin** (leniently: it grabs the
first number on each line), keeps running statistics over a sliding window, and
renders them as a 270° needle dial. It is a generic, host-side operator tool: it
computes its own stats, so it sits downstream of *any* producer (a log tail, a
benchmark, a packet counter, an ingest rate) without coupling to the source.
Think `pv` or `ttyplot`, but a speedometer.

## Install

```sh
cargo install termtaco
```

Or build from source (the project is pinned for Rust 1.72: ratatui 0.24,
crossterm 0.27, but newer toolchains work too):

```sh
cargo build --release
# binary at target/release/termtaco
```

## Usage

```sh
<producer> | termtaco [OPTIONS]
```

`termtaco` needs an interactive terminal for the display; pipe the data in and it
reads key events from the controlling tty.

### Options

| Option           | Description                                  | Default       |
| ---------------- | -------------------------------------------- | ------------- |
| `--window N`     | samples retained for the statistics window   | 200           |
| `--display NAME` | renderer to use                              | `speedometer` |
| `--parser SPEC`  | how to extract the value from each line (see below) | `first` |
| `--title TEXT`   | title shown at the top of the dial           | none          |
| `--border-label TEXT` | text in the dial's border               | none          |
| `--0`, `--zero`  | always keep 0 in the scale (e.g. a speedometer) | off        |
| `--min VALUE`    | fix the scale's lower bound instead of auto-scaling | auto      |
| `--max VALUE`    | fix the scale's upper bound instead of auto-scaling; a value past it stays capped and alarmed rather than rescaling | auto |
| `--fps N`        | refresh rate in frames per second            | 30            |
| `--stale-after SECS` | silence before the reading is flagged stale | 3         |
| `--overflow-hold SECS` | hold a capped reading this long before rescaling | 1   |
| `--kalman`       | smooth the needle/value with a Kalman filter (stats stay raw) | off |
| `--kalman-q Q`   | Kalman process noise variance per second     | 0.001         |
| `--kalman-r R`   | Kalman measurement noise variance            | 0.1           |
| `--max-decay SECS` | decay the max tick toward `max-decay-target × mean` once idle, instead of holding until it exits the window | off (holds) |
| `--max-decay-target M` | equilibrium multiplier of the mean for `--max-decay` | 2.0    |
| `--needle-inertia SECS` | give the needle mass: it lags the reading and settles over ~5× SECS | 0 (snaps) |
| `-h`, `--help`   | print help                                   |               |
| `--theme NAME`   | built-in color preset (see below)            | `bw`          |
| `--print-theme NAME` | print a preset's theme-file source to stdout, then exit |   |

Press `t` to cycle through the built-in presets live. Quit with `q`, `Esc`, or `Ctrl-C`.

### Theme

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
default, so a one-line file is a valid theme. `--print-theme NAME` dumps any
built-in preset as a starting point: this works from a plain `cargo install
termtaco` with nothing cloned, since the presets are compiled into the binary:

```sh
mkdir -p ~/.config/termtaco
termtaco --print-theme nord > ~/.config/termtaco/theme
```

(Working from a checkout of this repo, [`themes/`](themes/) has the same
files directly: `cp themes/nord.theme ~/.config/termtaco/theme`.)

`--theme` overrides the config file when both are given. An absent config
file, or a line with an unknown field or an unparsable color, falls back to
the default for that one field.

```sh
mkdir -p ~/.config/termtaco
cp themes/nord.theme ~/.config/termtaco/theme
```

An absent file, or a line with an unknown field or an unparsable color, falls
back to the default for that one field: nothing to set up for the default
look, and a typo can't break the dial.

### Parsers

By default termtaco uses the first number on each line, but some outputs put
the interesting value elsewhere. `--parser` picks the extraction strategy:

| Spec       | Value used                                                       |
| ---------- | ---------------------------------------------------------------- |
| `first`    | first number on the line (default)                               |
| `last`     | last number on the line                                          |
| `nth:N`    | N-th number on the line (1-based)                                |
| `key:NAME` | number after `NAME=` or `NAME:` (spaces around the separator ok) |
| `ping`     | round-trip time from `ping` output (the `time=` field)           |

Lines where the parser finds nothing are skipped, so noise lines (headers,
summaries) pass through harmlessly.

### Examples

```sh
# Live demo with the bundled feeder
./feed.sh sine | termtaco --title RATE --border-label termtaco

# Downstream of an existing text readout (first number per line is used)
my-rate-printer | termtaco --window 10000 --title "ingest/s"

# Network latency as a live dial: extract the RTT from each ping reply
ping 8.8.8.8 | termtaco --parser ping --title "ping ms" --0

# Same, but Kalman-smoothed: --kalman-r is the real per-sample noise
# variance, read straight off ping's own mdev (`rtt min/avg/max/mdev` in its
# summary line) squared, e.g. mdev=35.868ms -> r=35.868^2 ~= 1300. --kalman-q
# is picked so the filter's own implicit time constant (tau_kalman, see
# "Tuning: Kalman vs. needle inertia" below) is a handful of seconds, fast
# enough to track a real latency shift, slow enough to average out jitter.
ping 8.8.8.8 | termtaco --parser ping --title "ping ms" --0 --kalman \
    --kalman-q 0.5 --kalman-r 1300 --needle-inertia 0.5

# ROS 2 topic rate as a live dial. `ros2 topic hz` prints a multi-line block
# per sample, so keep only the "average rate" line (--line-buffered flushes
# each match immediately); the first number on it is the rate.
ros2 topic hz /odom | grep --line-buffered 'average rate' | termtaco --title "rate/s" --0

# A quick static sweep
seq 1 100 | awk '{print $1*0.7}' | termtaco --window 50
```

### Test feeder

`feed.sh` generates a stream for piping into termtaco. Modes:

| Mode     | What it produces                                            |
| -------- | ---------------------------------------------------------- |
| `sine`   | smooth oscillation, good for watching the needle           |
| `noisy`  | sine plus jitter, exercises the sigma band                 |
| `ramp`   | slow climb then reset, sweeps end to end and overflows     |
| `random` | uniform noise                                              |
| `rate`   | `average rate: N` lines, exercises the lenient parser      |
| `burst`  | emit for a few seconds then go quiet, looping (see below)  |

```sh
./feed.sh noisy 0.05 | termtaco
```

To watch the staleness behaviour, use `burst`: it feeds for `on` seconds then
stays quiet (with stdin held open) for `off` seconds, looping. With the default
`off` of 5 seconds, longer than the 3 second stale threshold (`--stale-after`),
the gauge goes STALE during each quiet window and recovers when the feed resumes.

```sh
./feed.sh burst | termtaco --title DEMO   # burst [delay] [on] [off]
```

## Tuning: Kalman vs. needle inertia

`--kalman-q`/`--kalman-r` and `--needle-inertia` are two independent smoothing
stages in series: Kalman is the *statistical* stage (estimates what the
signal actually is from noisy samples), the needle is the *mechanical* stage
(governs how fast the pointer can move to reflect that estimate; see the
module docs in `src/math/kalman.rs` and `src/math/needle.rs`). Tuned
separately with no way to relate them, it's easy for one stage's smoothing to
swamp the other without noticing.

### The filter has its own implicit time constant

The constant-velocity Kalman filter in `src/math/kalman.rs` is the classic
radar-tracking "alpha-beta" model (Kalata 1984): process noise `q`
(`--kalman-q`), discretized per step as a random acceleration impulse, and
measurement noise `r` (`--kalman-r`), a scalar. Its steady-state behavior
depends on `q`, `r`, and the sample interval `dt` (the time between incoming
measurements, not `--fps`) only through a single dimensionless number, the
tracking index:

    lambda = q * dt^4 / r

Two different `(q, r)` pairs with the same `lambda` converge to the exact
same steady-state Kalman gain `K0`, the fraction of each new measurement's
error the filter corrects toward (verified numerically to 10 decimal places:
`q=0.001, r=0.1` and `q=0.01, r=1.0` both give `lambda = 6.25e-8` and
`K0 = 0.0216340380`).

At steady state the filter's position update looks like an exponential
moving average, `estimate = prediction + K0 * (measurement - prediction)`,
which has an equivalent continuous decay time constant:

    tau_kalman = -dt / ln(1 - K0)

directly comparable to `--needle-inertia`'s own `tau` (same units, seconds; a
needle closes ~63% of the gap after `tau`, ~99% after `~5*tau`). `K0` has no
simple closed form (the general fixed point is a quartic), so the table below
was computed by iterating the same predict/update recursion `src/math/kalman.rs`
runs, until it converges:

| q      | r   | dt   | lambda   | K0       | tau_kalman |
| ------ | --- | ---- | -------- | -------- | ---------- |
| 0.001  | 1.0 | 0.05 | 6.25e-9  | 0.012341 | 4.03 s     |
| 0.001  | 0.1 | 0.05 | 6.25e-8  | 0.021634 | 2.29 s     |
| 0.01   | 1.0 | 0.05 | 6.25e-8  | 0.021634 | 2.29 s     |
| 0.001  | 1.0 | 1.0  | 1.00e-3  | 0.181822 | 4.98 s     |

(rows 2 and 3 have the same `lambda`, hence the identical `K0`/`tau_kalman`
despite different `q`/`r`.)

### Why it matters

Because the two stages are in series, the total lag before the dial reflects
a real change is roughly `tau_kalman + 5 * tau_needle`. With
`q=0.001, r=1.0, dt=0.05s`, a plausible choice when `r` is set to the raw
signal's real measurement variance (e.g. `./feed.sh gauss`, mean 0 / std 1,
so `--kalman-r 1.0` matches it exactly), `tau_kalman` is already ~4 seconds,
far larger than a typical `--needle-inertia 0.3`: essentially all the visible
smoothing is already coming from the Kalman stage, and the needle-inertia
setting is nearly invisible on top of it.

### Picking values

- Set `r` to something objective: the actual measurement-noise variance of
  the raw signal (`--kalman-r 1.0` for a standard-normal `std=1` source such
  as `./feed.sh gauss`).
- Use `--needle-inertia`'s `tau` purely for the pointer's visual feel (floaty
  vs. snappy), independent of how the Kalman filter happens to be tuned.
- To let the needle track the Kalman stage's own settling behavior directly,
  keep `tau` small, or `0` (the default), rather than stacking a second
  independent lag on top.
- To deliberately let the mechanical feel dominate regardless of Kalman
  tuning, set `tau` well above `tau_kalman` for your `q`/`r`/`dt`.

## Features

- **Radial 270° dial**, drawn as a true circle at any pane size (the cell aspect
  ratio is compensated, so it never renders as an ellipse).
- **Lenient input:** one float per line on stdin, taking the first number found
  on each line. It accepts both bare `33.7` and embedded `average rate: 33.746`.
- **Running stats:** min, max, mean and standard deviation over a sliding window.
- **Self-scaling:** the scale snaps to round numbers at the data magnitude (18
  rounds to 20, 49995 to 50000, 0.0023 to 0.0025), with numbered major ticks.
- Five fixed reference markers at min, quarter, mid, three-quarter and full scale.
- Optional title across the top of the dial.
- **Zero anchoring** (`--0`): always keep 0 in the scale, so a speedometer reads
  from 0 even when the data never gets near it.
- **Fixed scale** (`--min`/`--max`): pin either end of the scale instead of
  auto-scaling from the window; a fixed `--max` also disables the
  overflow-hold-then-rescale dance, since there's nowhere wider to rescale to.
- **Overflow alarm:** when a value runs past full scale the needle pins at the
  top, an LED lights, and the gauge holds there for one second before rescaling.
- **Staleness signal:** if the feed goes quiet for a few seconds a yellow STALE
  LED lights and the needle greys out, so a frozen needle is never mistaken for a
  live one.
- **Kalman smoothing** (`--kalman`): steadies the needle and value label on
  noisy feeds; the min/max/mean/stddev ticks keep tracking the raw samples.
- **Decaying max** (`--max-decay`): instead of holding rigidly until the peak
  sample ages out of the window, the max tick exponentially decays toward
  `max-decay-target × mean` once idle, snapping back up instantly on a fresh
  spike.
- **Raw sample tick:** a bold mark on the rim at the latest unfiltered
  sample, with smoothing on you see the filtered needle *and* where the last
  real measurement landed.
- **Needle inertia** (`--needle-inertia`): the needle becomes a
  critically-damped mass that settles toward the reading instead of
  teleporting, layered on top of (and independent of) `--kalman`: Kalman
  estimates what the signal is, inertia governs how fast the pointer can get
  there.
- White by default; red is reserved for the overflow alarm, yellow for stale.
- **No async runtime.** A stdin reader thread feeds the render loop over a channel.

## Reading the dial

- The needle points at the latest value.
- Numbered graduation ticks mark the scale; minor ticks subdivide each step.
- The five marks just outside the rim are fixed references at min, quarter, mid,
  three-quarter and full scale.
- The stat ticks annotate window min, max, mean and the ±1σ band.
- The bold tick on the rim is the latest raw sample; with `--kalman` and/or
  `--needle-inertia` the needle lags it, showing the filtered/damped reading.
- The big number under the hub is the current (filtered) value, not the
  lagging needle position.
- The LED on the lower right lights red on overflow; the needle and value turn
  red while the gauge is capped. The LED on the lower left lights yellow when the
  feed is stale, and the needle greys out.

## How it compares

Nothing off the shelf draws a *needle dial* with annotated min/max/σ marks:

- **ttyplot** and friends draw scrolling line plots over time, not an instrument.
- **gping** is a ping-specific line graph.
- **ratatui's `Gauge`** and rich/textual draw horizontal progress bars.

A dial reads instantaneous state at a glance (where the needle sits, how wide
the spread is, whether the value just ran off the top), which line plots and bars
don't surface as directly. That niche is why `termtaco` is a small standalone
tool rather than a flag on something else.

## Architecture

Three layers with a single coupling point, the `Display` trait.

```
src/
  math/      window stats, Kalman filter and needle dynamics (all pure)
  infra/     stdin reader thread, terminal lifecycle, ~30fps render loop
  display/   Display trait plus the themed speedometer renderer
```

- `math` knows nothing about rendering or I/O.
- `infra` drives any `Display` and knows nothing about what is drawn.
- `display` is the plugin boundary. Adding a renderer is a new file plus one line
  in `make()`. All colors live in one `Theme` struct.

## Toolchain notes

The host is Rust 1.72.1. Dependencies are pinned for that MSRV (`ratatui 0.24`,
`crossterm 0.27`, and `unicode-segmentation` held at `1.10.1` in the lockfile,
since newer releases require a much newer rustc). Build with the committed
`Cargo.lock`.

## License

[MIT](./LICENSE)
