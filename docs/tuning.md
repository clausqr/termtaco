# Tuning: Kalman vs. needle inertia

`--kalman-q`/`--kalman-r` and `--needle-inertia` are two independent smoothing
stages in series: Kalman is the *statistical* stage (estimates what the
signal actually is from noisy samples), the needle is the *mechanical* stage
(governs how fast the pointer can move to reflect that estimate; see the
module docs in `src/math/kalman.rs` and `src/math/needle.rs`). Tuned
separately with no way to relate them, it's easy for one stage's smoothing to
swamp the other without noticing.

## The filter has its own implicit time constant

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

## Why it matters

Because the two stages are in series, the total lag before the dial reflects
a real change is roughly `tau_kalman + 5 * tau_needle`. With
`q=0.001, r=1.0, dt=0.05s`, a plausible choice when `r` is set to the raw
signal's real measurement variance (e.g. `./feed.sh gauss`, mean 0 / std 1,
so `--kalman-r 1.0` matches it exactly), `tau_kalman` is already ~4 seconds,
far larger than a typical `--needle-inertia 0.3`: essentially all the visible
smoothing is already coming from the Kalman stage, and the needle-inertia
setting is nearly invisible on top of it.

## Picking values

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

## Adaptive `q` (`--kalman-adaptive`)

A single fixed `q` is a bet on one motion regime: tuned quiet, it lags behind
a real maneuver; tuned fast, it's needlessly jumpy at rest. `--kalman-adaptive`
re-estimates `q` online instead, driven by the normalized innovation squared
(NIS): `nu^2 / s`, the squared innovation over its predicted variance, which
should average ~1 for a consistent filter. Every `--kalman-adaptive-window`
measurements, if the windowed mean NIS drifts outside `[0.9, 1.1]`, `q` is
nudged in log space toward the value that would have made it consistent
(`--kalman-adaptive-gain` sets the step size), clamped to
`[--kalman-q-min, --kalman-q-max]`.

`--kalman-q-min` matters most: set it no higher than the process noise of the
quietest motion you expect, or the filter starts each quiet stretch stiffer
than it should and reacts sluggishly right as the next maneuver begins.
