//! Cross-frame animation state: the overflow-hold-then-rescale machine, the
//! decaying-max peak hold, and the needle-inertia pointer.
//!
//! Each exposes a `step(..., now: Instant)` that advances by exactly one
//! frame — taking `now` explicitly (rather than reading
//! `std::time::Instant::now()` internally) is what makes the timed
//! transitions here unit-testable deterministically, the same way
//! `math::kalman` and `math::needle` take `dt` explicitly rather than
//! reading the clock themselves.

use std::time::{Duration, Instant};

use crate::math::needle::Needle;
use crate::math::stats::Stats;

/// Default for how long the gauge stays pinned at full scale (overflow LED lit)
/// before it rescales to fit a value that has run past the top. Overridable via
/// `--overflow-hold`. `pub` so `main.rs` can cite this as the single source of
/// truth for the CLI's own default rather than duplicating the literal.
pub const DEFAULT_OVERFLOW_HOLD: Duration = Duration::from_secs(1);

/// Default equilibrium multiplier of the mean the decaying max settles
/// toward. Overridable via `--max-decay-target`. `pub` for the same reason
/// as `DEFAULT_OVERFLOW_HOLD`.
pub const DEFAULT_MAX_DECAY_TARGET: f64 = 2.0;

/// Held scale plus the overflow-hold timer: when the live value runs past
/// the top of the currently displayed scale, the scale holds (needle capped,
/// LED lit) for a configurable duration before rescaling to fit.
#[derive(Default)]
pub(super) struct ScaleState {
    pub(super) current: Option<(f64, f64, f64)>,
    pub(super) overflow_since: Option<Instant>,
}

impl ScaleState {
    /// Advance by one frame: `target` is the scale that would fit the
    /// current window, `value` is the value being checked against the
    /// displayed top, `hold` is the configured overflow-hold duration.
    /// Returns the scale to draw this frame and whether it's currently in
    /// overflow (capped, not yet rescaled).
    pub(super) fn step(
        &mut self,
        target: (f64, f64, f64),
        value: f64,
        hold: Duration,
        now: Instant,
    ) -> ((f64, f64, f64), bool) {
        let (_, cur_hi, _) = *self.current.get_or_insert(target);

        let overflow = if value > cur_hi {
            let since = *self.overflow_since.get_or_insert(now);
            if now.duration_since(since) >= hold {
                self.current = Some(target); // held long enough → rescale to fit
                self.overflow_since = None;
                false
            } else {
                true // still pinned at full scale
            }
        } else {
            // In range: track the target (handles shrinking / downward moves).
            self.overflow_since = None;
            self.current = Some(target);
            false
        };

        (self.current.unwrap(), overflow)
    }
}

/// Peak-hold-with-decay for the max tick: held rigidly at the window's raw
/// max by default, or exponentially decayed toward `target_mult × mean` once
/// armed via `--max-decay`, snapping back up instantly on a fresh extreme.
#[derive(Default)]
pub(super) struct MaxDecay {
    pub(super) value: Option<f64>,
    pub(super) last_tick: Option<Instant>,
}

impl MaxDecay {
    pub(super) fn step(&mut self, stats: &Stats, tau: Option<Duration>, target_mult: f64, now: Instant) -> f64 {
        let tau = match tau {
            Some(tau) => tau,
            None => return stats.max,
        };
        let dt = self.last_tick.map(|t| now.duration_since(t)).unwrap_or_default();
        self.last_tick = Some(now);
        let prev = self.value.unwrap_or(stats.max);
        let target = stats.mean * target_mult;
        let decayed = decay_toward(prev, stats.max, target, dt, tau);
        self.value = Some(decayed);
        decayed
    }
}

/// Peak-hold-with-decay's pure core: snaps up instantly to a fresh extreme,
/// otherwise exponentially decays `prev` toward `target` with time constant
/// `tau` over elapsed time `dt`. Never overshoots past `target`.
fn decay_toward(prev: f64, raw_max: f64, target: f64, dt: Duration, tau: Duration) -> f64 {
    if raw_max >= prev {
        return raw_max;
    }
    // Floor `tau` so a caller-supplied (or defaulted) zero can't divide by
    // zero or collapse to an instant snap.
    const MIN_TAU_SECS: f64 = 1e-6;
    let tau_secs = tau.as_secs_f64().max(MIN_TAU_SECS);
    let alpha = (-dt.as_secs_f64() / tau_secs).exp();
    target + (prev - target) * alpha
}

/// Physical needle dynamics: with `--needle-inertia` the needle is a damped
/// mass chasing the reading instead of snapping to it.
#[derive(Default)]
pub(super) struct NeedleState {
    pub(super) pointer: Option<Needle>,
    pub(super) last_tick: Option<Instant>,
}

impl NeedleState {
    pub(super) fn step(&mut self, target: f64, now: Instant) -> f64 {
        match &mut self.pointer {
            Some(n) => {
                let dt = self.last_tick.map(|t| now.duration_since(t)).unwrap_or_default();
                self.last_tick = Some(now);
                n.step(target, dt)
            }
            None => target,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    fn stats(last: f64, min: f64, max: f64) -> Stats {
        Stats { last, min, max, mean: (min + max) / 2.0, stddev: 1.0, count: 200 }
    }

    fn stats_with_mean(min: f64, max: f64, mean: f64) -> Stats {
        Stats { last: max, min, max, mean, stddev: 1.0, count: 200 }
    }

    #[test]
    fn decay_snaps_up_on_fresh_extreme() {
        let d = Duration::from_secs(5);
        // A new raw max at or above the previous displayed max wins outright,
        // regardless of elapsed time.
        approx(decay_toward(10.0, 12.0, 5.0, d, d), 12.0);
        approx(decay_toward(10.0, 10.0, 5.0, d, d), 10.0);
    }

    #[test]
    fn decay_approaches_but_never_crosses_target() {
        let tau = Duration::from_secs(10);
        let mut v = 20.0;
        for _ in 0..50 {
            v = decay_toward(v, 0.0, 8.0, Duration::from_secs(2), tau);
            assert!(v > 8.0, "decay should approach the target from above without crossing it, got {v}");
        }
        assert!(v - 8.0 < 0.1, "expected near-convergence to the target, got {v}");
    }

    #[test]
    fn decay_no_time_elapsed_holds_steady() {
        let tau = Duration::from_secs(10);
        approx(decay_toward(20.0, 0.0, 8.0, Duration::ZERO, tau), 20.0);
    }

    #[test]
    fn overflow_holds_then_rescales_after_hold() {
        let mut s = ScaleState::default();
        let hold = Duration::from_secs(1);
        let t0 = Instant::now();
        // Establish an initial scale of (30, 45, 5).
        let (scale0, overflow0) = s.step((30.0, 45.0, 5.0), 33.0, hold, t0);
        assert_eq!(scale0, (30.0, 45.0, 5.0));
        assert!(!overflow0);

        // First overflowing frame arms the timer at t1 — `hold` is measured
        // from here, not from t0.
        let t1 = t0 + Duration::from_millis(10);
        let (scale1, overflow1) = s.step((60.0, 99.0, 10.0), 99.0, hold, t1);
        assert_eq!(scale1, (30.0, 45.0, 5.0), "scale should still be held");
        assert!(overflow1);

        // Just short of `hold` since the timer armed: still capped.
        let (scale2, overflow2) = s.step((60.0, 99.0, 10.0), 99.0, hold, t1 + hold - Duration::from_millis(1));
        assert_eq!(scale2, (30.0, 45.0, 5.0), "scale should still be held");
        assert!(overflow2);

        // Held long enough since the timer armed: rescales to fit.
        let (scale3, overflow3) = s.step((60.0, 99.0, 10.0), 99.0, hold, t1 + hold);
        assert_eq!(scale3, (60.0, 99.0, 10.0));
        assert!(!overflow3);
    }

    #[test]
    fn overflow_timer_resets_when_value_returns_in_range() {
        let mut s = ScaleState::default();
        let hold = Duration::from_secs(1);
        let t0 = Instant::now();
        s.step((30.0, 45.0, 5.0), 33.0, hold, t0);
        let (_, overflow1) = s.step((30.0, 45.0, 5.0), 99.0, hold, t0 + Duration::from_millis(500));
        assert!(overflow1);

        // Back in range before the hold expires: clears immediately, and the
        // scale tracks the (possibly shrunk) target rather than staying held.
        let (scale2, overflow2) = s.step((20.0, 40.0, 5.0), 33.0, hold, t0 + Duration::from_millis(600));
        assert!(!overflow2);
        assert_eq!(scale2, (20.0, 40.0, 5.0));

        // A fresh overflow after returning in range re-arms the timer from
        // scratch rather than reusing the old (already-elapsed) one.
        let (_, overflow3) = s.step(
            (60.0, 99.0, 10.0),
            99.0,
            hold,
            t0 + Duration::from_millis(600) + hold - Duration::from_millis(1),
        );
        assert!(overflow3, "timer should have restarted, not still be counting from the first overflow");
    }

    #[test]
    fn overflow_hold_zero_rescales_on_the_first_overflowing_frame() {
        let mut s = ScaleState::default();
        let t0 = Instant::now();
        s.step((30.0, 45.0, 5.0), 33.0, Duration::ZERO, t0);
        let (scale, overflow) = s.step((60.0, 99.0, 10.0), 99.0, Duration::ZERO, t0);
        assert_eq!(scale, (60.0, 99.0, 10.0));
        assert!(!overflow, "zero hold should rescale immediately rather than ever showing capped");
    }

    #[test]
    fn overflow_arms_the_timer_on_the_first_overflowing_frame() {
        let mut s = ScaleState::default();
        let hold = Duration::from_secs(1);
        let t0 = Instant::now();
        s.step((30.0, 45.0, 5.0), 33.0, hold, t0);
        assert!(s.overflow_since.is_none());
        let t1 = t0 + Duration::from_millis(10);
        s.step((30.0, 45.0, 5.0), 99.0, hold, t1);
        assert_eq!(s.overflow_since, Some(t1), "timer should start at the first overflowing frame, not before");
    }

    #[test]
    fn max_decay_disabled_returns_the_raw_max() {
        let mut d = MaxDecay::default();
        let v = d.step(&stats(5.0, 0.0, 10.0), None, 2.0, Instant::now());
        assert_eq!(v, 10.0);
        assert!(d.value.is_none(), "state shouldn't be touched while disabled");
    }

    #[test]
    fn max_decay_first_frame_holds_at_the_raw_max() {
        let mut d = MaxDecay::default();
        let v = d.step(&stats(5.0, 0.0, 10.0), Some(Duration::from_secs(5)), 2.0, Instant::now());
        assert_eq!(v, 10.0, "first frame has no prior dt, so it holds at the raw max");
    }

    #[test]
    fn max_decay_advances_over_synthetic_elapsed_time() {
        let mut d = MaxDecay::default();
        let tau = Duration::from_secs(10);
        let t0 = Instant::now();
        // First frame: window max is 10 -> holds at the raw max (nothing to
        // decay from yet).
        let v0 = d.step(&stats_with_mean(0.0, 10.0, 5.0), Some(tau), 1.0, t0);
        assert_eq!(v0, 10.0);

        // The peak has since aged out of the window (raw max dropped to 6,
        // below the previously displayed 10, mean unchanged): should decay
        // partway toward target=mean=5, not snap straight down to 6.
        let v1 = d.step(&stats_with_mean(0.0, 6.0, 5.0), Some(tau), 1.0, t0 + Duration::from_secs(2));
        assert!(v1 < 10.0 && v1 > 5.0, "expected partial decay toward the mean, got {v1}");
    }

    #[test]
    fn max_decay_snaps_back_up_through_the_state_machine() {
        let mut d = MaxDecay::default();
        let tau = Duration::from_secs(10);
        let t0 = Instant::now();
        d.step(&stats_with_mean(0.0, 10.0, 5.0), Some(tau), 1.0, t0);
        let decayed = d.step(&stats_with_mean(0.0, 6.0, 5.0), Some(tau), 1.0, t0 + Duration::from_secs(5));
        assert!(decayed < 10.0);
        // A fresh, higher extreme snaps back up instantly.
        let v = d.step(&stats_with_mean(0.0, 20.0, 5.0), Some(tau), 1.0, t0 + Duration::from_secs(6));
        assert_eq!(v, 20.0);
    }

    #[test]
    fn needle_state_snaps_when_inertia_is_off() {
        let mut n = NeedleState::default();
        assert_eq!(n.step(42.0, Instant::now()), 42.0);
    }

    #[test]
    fn needle_state_lags_when_on() {
        let mut n = NeedleState { pointer: Some(Needle::new(Duration::from_secs(1))), last_tick: None };
        let t0 = Instant::now();
        assert_eq!(n.step(0.0, t0), 0.0); // seeds at the first target
        let moved = n.step(10.0, t0 + Duration::from_millis(100));
        assert!(moved > 0.0 && moved < 10.0, "should lag partway toward the new target, got {moved}");
    }
}
