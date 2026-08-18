//! Constant-velocity Kalman filter for smoothing a noisy 1-D signal.
//!
//! State is `[position, velocity]`. [`Kalman::predict`] advances the estimate
//! along its current velocity with no new data; the render loop calls this
//! every frame so the needle moves smoothly between measurements instead of
//! sitting frozen until the next one arrives. [`Kalman::update`] additionally
//! corrects the estimate when a real measurement lands. Both take the
//! caller-supplied elapsed time so the filter itself stays free of wall-clock
//! reads: the render loop and the stdin reader advance at different,
//! irregular rates (e.g. a 60fps display against ~1Hz ping replies).
//!
//! [`Kalman::new_adaptive`] builds a filter that additionally re-estimates
//! its own process noise `q` online, driven by the normalized innovation
//! squared (NIS): `nu^2 / s`, where `nu` is the innovation and `s` its
//! variance. For a 1-D measurement a consistent filter has NIS averaging ~1;
//! a windowed mean well above 1 means the filter is too confident (the
//! tracked signal is maneuvering faster than `q` accounts for), well below 1
//! means it's too loose (fitting noise). See [`Kalman::adapt_q`] for the
//! update rule.

use std::collections::VecDeque;
use std::time::Duration;

/// Recursive constant-velocity Kalman filter. `q` is the process noise
/// (variance of the acceleration driving the velocity's drift, per second);
/// `r` is the measurement noise variance (how noisy each raw sample is). A
/// smaller `q`/`r` ratio smooths harder and extrapolates more conservatively;
/// a larger one tracks the raw signal (and its swings) more closely.
pub struct Kalman {
    q: f64,
    r: f64,
    pos: f64,
    vel: f64,
    // Covariance P = [[p00, p01], [p01, p11]] (symmetric).
    p00: f64,
    p01: f64,
    p11: f64,
    seeded: bool,
    adaptive: Option<AdaptiveQ>,
}

/// NIS-windowed online adaptation of `q`. Only present on filters built with
/// [`Kalman::new_adaptive`]; a plain [`Kalman::new`] never allocates this.
struct AdaptiveQ {
    nis: VecDeque<f64>,
    window: usize,
    q_min: f64,
    q_max: f64,
    gain: f64,
}

/// Windowed mean NIS is left alone inside this band: a 1-D measurement's NIS
/// averages ~1 for a consistent filter, so a mean this close to 1 isn't
/// worth reacting to (it would just chase sampling noise in the NIS
/// statistic itself).
const ADAPT_DEADBAND: (f64, f64) = (0.9, 1.1);

/// A [`Kalman`] filter's runtime knobs, bundled so callers thread one value
/// through instead of a long positional list. `adaptive: false` means a plain
/// fixed-`q` filter; `window`/`q_min`/`q_max`/`gain` are only meaningful when
/// it's `true`. See [`Kalman::new`] and [`Kalman::new_adaptive`] for what each
/// field does.
#[derive(Clone, Copy, Debug)]
pub struct KalmanTuning {
    pub q: f64,
    pub r: f64,
    pub adaptive: bool,
    pub window: usize,
    pub q_min: f64,
    pub q_max: f64,
    pub gain: f64,
}

impl Kalman {
    pub fn new(q: f64, r: f64) -> Self {
        Kalman { q, r, pos: 0.0, vel: 0.0, p00: 1.0, p01: 0.0, p11: 1.0, seeded: false, adaptive: None }
    }

    /// Like [`new`](Self::new), but `q` is only the *initial* process noise:
    /// after each `window` measurements, if the mean normalized-innovation-
    /// squared (NIS) strays outside [`ADAPT_DEADBAND`], `q` is nudged in log
    /// space toward the value that would have made the filter consistent,
    /// clamped to `[q_min, q_max]`. `gain` is the step size on `log q` per
    /// adaptation (0.05 = smooth, 0.2 = fast); `q_min` should be no larger
    /// than the process noise of the quietest motion expected, or the filter
    /// will react sluggishly right as a maneuver begins.
    pub fn new_adaptive(q0: f64, r: f64, window: usize, q_min: f64, q_max: f64, gain: f64) -> Self {
        let mut k = Kalman::new(q0, r);
        k.adaptive = Some(AdaptiveQ { nis: VecDeque::with_capacity(window), window, q_min, q_max, gain });
        k
    }

    /// [`new`](Self::new) or [`new_adaptive`](Self::new_adaptive), picked by
    /// `tuning.adaptive`, from one bundled set of knobs instead of a long
    /// positional argument list.
    pub fn from_tuning(tuning: &KalmanTuning) -> Self {
        if tuning.adaptive {
            Kalman::new_adaptive(tuning.q, tuning.r, tuning.window, tuning.q_min, tuning.q_max, tuning.gain)
        } else {
            Kalman::new(tuning.q, tuning.r)
        }
    }

    /// The filter's current process noise: the value passed to `new`,
    /// or its latest online-adapted value on a [`new_adaptive`](Self::new_adaptive) filter.
    #[cfg(test)]
    fn q(&self) -> f64 {
        self.q
    }

    /// Advance the estimate by `dt` with no new measurement, returning the
    /// predicted position. Meant to be called every render frame so the
    /// needle keeps moving (along the current velocity estimate) between
    /// measurements rather than sitting still until the next one arrives.
    /// A no-op (returns the seed value) until the first [`update`](Self::update).
    pub fn predict(&mut self, dt: Duration) -> f64 {
        if self.seeded {
            self.advance(dt.as_secs_f64());
        }
        self.pos
    }

    /// Advance by `dt` (same as [`predict`](Self::predict)), then correct the
    /// estimate with a new raw measurement `z`. Returns the corrected
    /// position. The first call seeds the estimate with `z` and zero
    /// velocity, so there is no startup lag on the very first sample.
    pub fn update(&mut self, z: f64, dt: Duration) -> f64 {
        if !self.seeded {
            self.pos = z;
            self.vel = 0.0;
            self.p00 = 1.0;
            self.p01 = 0.0;
            self.p11 = 1.0;
            self.seeded = true;
            return z;
        }
        self.advance(dt.as_secs_f64());

        // Correct: H = [1, 0] (only position is measured), so the
        // innovation is just the raw sample minus the predicted position.
        let y = z - self.pos;
        let s = self.p00 + self.r;
        let k0 = self.p00 / s;
        let k1 = self.p01 / s;

        self.pos += k0 * y;
        self.vel += k1 * y;

        // P' = (I - K H) P_pred, specialized for H = [1, 0].
        let p00 = (1.0 - k0) * self.p00;
        let p01 = (1.0 - k0) * self.p01;
        let p11 = self.p11 - k1 * self.p01;
        self.p00 = p00;
        self.p01 = p01;
        self.p11 = p11;

        if self.adaptive.is_some() {
            self.adapt_q(y * y / s);
        }

        self.pos
    }

    /// Fold one measurement's normalized innovation squared (`nu^2 / s`)
    /// into the adaptation window and, once it's full, nudge `q` toward
    /// consistency. A no-op on a non-adaptive filter (checked by the caller)
    /// or while the window is still filling: warm-up keeps the prior `q`
    /// rather than reacting to a partial, noisier mean.
    fn adapt_q(&mut self, nis: f64) {
        let a = self.adaptive.as_mut().expect("adapt_q called on a non-adaptive filter");
        if a.nis.len() == a.window {
            a.nis.pop_front();
        }
        a.nis.push_back(nis);
        if a.nis.len() < a.window {
            return;
        }

        let mean_nis = a.nis.iter().sum::<f64>() / a.window as f64;
        let (lo, hi) = ADAPT_DEADBAND;
        if (lo..=hi).contains(&mean_nis) {
            return;
        }
        // Move log q toward log(mean_nis) (the multiplier that would have
        // made the mean NIS exactly 1), bounded so a single outlier window
        // can't blow q up or down in one step.
        let step = mean_nis.ln().clamp(-1.0, 1.0);
        self.q = (self.q * (a.gain * step).exp()).clamp(a.q_min, a.q_max);
    }

    /// Standard deviation of the position estimate: `sqrt(p00)`, the
    /// diagonal of the covariance matrix that tracks position uncertainty.
    /// Shrinks as consistent measurements arrive, grows on `predict` and
    /// after a gap, so it doubles as a live "how much do I trust this
    /// estimate" readout in the same units as the tracked value. `0.0`
    /// before the first [`update`](Self::update) seeds the filter.
    pub fn uncertainty(&self) -> f64 {
        if self.seeded {
            self.p00.sqrt()
        } else {
            0.0
        }
    }

    /// Predict-only state transition: constant-velocity motion (`F = [[1,
    /// dt], [0, 1]]`), plus a discretized white-noise-acceleration process
    /// noise term scaled by the elapsed time: a longer gap since the last
    /// advance (predict or update) allows more drift, so the filter readily
    /// jumps to a new measurement after a gap instead of clinging to a stale
    /// trajectory.
    fn advance(&mut self, dt: f64) {
        self.pos += self.vel * dt;
        // Velocity is unchanged by prediction alone (no control input).

        // P' = F P F^T.
        let p00 = self.p00 + dt * (2.0 * self.p01 + dt * self.p11);
        let p01 = self.p01 + dt * self.p11;
        let p11 = self.p11;

        let dt2 = dt * dt;
        let dt3 = dt2 * dt;
        let dt4 = dt2 * dt2;
        self.p00 = p00 + self.q * dt4 / 4.0;
        self.p01 = p01 + self.q * dt3 / 2.0;
        self.p11 = p11 + self.q * dt2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(100);

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn first_sample_is_returned_unchanged() {
        let mut k = Kalman::new(0.001, 0.1);
        assert_eq!(k.update(42.0, TICK), 42.0);
    }

    #[test]
    fn predict_before_any_update_is_a_no_op() {
        let mut k = Kalman::new(0.001, 0.1);
        assert_eq!(k.predict(TICK), 0.0);
    }

    #[test]
    fn hand_computed_two_updates_then_predict() {
        // q=0 (no process noise) makes the arithmetic exact.
        let mut k = Kalman::new(0.0, 1.0);
        assert_eq!(k.update(0.0, Duration::ZERO), 0.0); // seeds pos=0, vel=0, P=I

        // Second measurement one second later: p00_pred=2, p01_pred=1,
        // p11_pred=1 (P propagated with dt=1, q=0); S=p00+r=3; k0=2/3, k1=1/3.
        // pos = 0 + 2/3*3 = 2; vel = 0 + 1/3*3 = 1.
        let x1 = k.update(3.0, Duration::from_secs(1));
        approx(x1, 2.0);

        // No new measurement: predicting one second ahead extrapolates along
        // the fitted velocity (1.0/s): this is exactly what lets the needle
        // keep moving between measurements.
        let predicted = k.predict(Duration::from_secs(1));
        approx(predicted, 3.0);
    }

    #[test]
    fn predict_with_zero_dt_holds_position() {
        let mut k = Kalman::new(0.0, 1.0);
        k.update(0.0, Duration::ZERO);
        k.update(3.0, Duration::from_secs(1)); // now has vel=1.0
        approx(k.predict(Duration::ZERO), 2.0); // unchanged: pos += vel*0
    }

    #[test]
    fn converges_toward_a_constant_input() {
        let mut k = Kalman::new(0.001, 0.1);
        let mut last = k.update(10.0, TICK);
        for _ in 0..200 {
            last = k.update(10.0, TICK);
        }
        assert!((last - 10.0).abs() < 1e-6, "expected convergence to 10.0, got {last}");
    }

    #[test]
    fn smooths_a_noisy_step() {
        // Alternating +/- noise around 5.0; the filtered output should end
        // up much closer to 5.0 than the raw swings ever get.
        let mut k = Kalman::new(0.001, 1.0);
        let mut last = 0.0;
        for i in 0..100 {
            let noisy = if i % 2 == 0 { 6.0 } else { 4.0 };
            last = k.update(noisy, TICK);
        }
        assert!((last - 5.0).abs() < 0.5, "expected near 5.0, got {last}");
    }

    #[test]
    fn longer_gap_trusts_the_new_measurement_more() {
        // Same q/r/measurement, but a much longer dt before the correction:
        // predicted uncertainty (and hence the Kalman gain) grows with the
        // gap, pulling the estimate further toward the raw sample.
        let mut short = Kalman::new(0.5, 1.0);
        short.update(0.0, Duration::from_secs(1));
        let short_x1 = short.update(10.0, Duration::from_secs(1));

        let mut long = Kalman::new(0.5, 1.0);
        long.update(0.0, Duration::from_secs(1));
        let long_x1 = long.update(10.0, Duration::from_secs(30));

        assert!(long_x1 > short_x1, "a longer gap should trust the new sample more: {long_x1} vs {short_x1}");
    }

    #[test]
    fn uncertainty_is_zero_before_seeding() {
        let k = Kalman::new(0.001, 0.1);
        assert_eq!(k.uncertainty(), 0.0);
    }

    #[test]
    fn uncertainty_shrinks_as_consistent_measurements_arrive() {
        let mut k = Kalman::new(0.001, 1.0);
        k.update(5.0, TICK);
        let after_first = k.uncertainty();
        let mut last = after_first;
        for _ in 0..20 {
            k.update(5.0, TICK);
            last = k.uncertainty();
        }
        assert!(last < after_first, "uncertainty should shrink as the estimate converges: {after_first} -> {last}");
    }

    #[test]
    fn uncertainty_grows_across_a_predict_only_gap() {
        let mut k = Kalman::new(0.5, 1.0);
        k.update(0.0, Duration::ZERO);
        for _ in 0..20 {
            k.update(0.0, TICK);
        }
        let settled = k.uncertainty();
        k.predict(Duration::from_secs(10));
        assert!(k.uncertainty() > settled, "a long gap with no measurement should widen the uncertainty");
    }

    #[test]
    fn non_adaptive_filter_never_changes_q() {
        let mut k = Kalman::new(0.001, 1.0);
        for i in 0..100 {
            let noisy = if i % 2 == 0 { 6.0 } else { 4.0 };
            k.update(noisy, TICK);
        }
        assert_eq!(k.q(), 0.001);
    }

    #[test]
    fn adaptive_q_holds_steady_during_warm_up() {
        let mut k = Kalman::new_adaptive(0.01, 1.0, 5, 1e-6, 1e6, 0.1);
        k.update(5.0, TICK); // seeds, no NIS sample yet
        for _ in 0..3 {
            // fewer than `window` (5) samples: still warming up
            k.update(5.0, TICK);
        }
        assert_eq!(k.q(), 0.01, "q shouldn't move before the NIS window fills");
    }

    #[test]
    fn adaptive_q_grows_when_innovations_run_persistently_large() {
        // A model mismatch (constant-velocity can't track wide swings) keeps
        // the NIS well above 1, which should push q up from its initial value.
        let mut k = Kalman::new_adaptive(0.001, 1.0, 5, 1e-6, 1e6, 0.1);
        k.update(0.0, TICK); // seed
        for i in 0..20 {
            let z = if i % 2 == 0 { 1000.0 } else { -1000.0 };
            k.update(z, TICK);
        }
        assert!(k.q() > 0.001, "persistently large NIS should grow q, got {}", k.q());
    }

    #[test]
    fn adaptive_q_shrinks_when_innovations_run_persistently_small() {
        // A constant, noise-free measurement keeps the innovation near zero,
        // so the NIS stays well below 1, which should pull q down toward q_min.
        let mut k = Kalman::new_adaptive(0.5, 1.0, 5, 1e-6, 1e6, 0.1);
        k.update(5.0, TICK); // seed
        for _ in 0..20 {
            k.update(5.0, TICK);
        }
        assert!(k.q() < 0.5, "persistently small NIS should shrink q, got {}", k.q());
    }

    #[test]
    fn adaptive_q_is_clamped_to_q_max() {
        let mut k = Kalman::new_adaptive(0.001, 1.0, 5, 1e-6, 0.002, 0.1);
        k.update(0.0, TICK); // seed
        for i in 0..200 {
            let z = if i % 2 == 0 { 1000.0 } else { -1000.0 };
            k.update(z, TICK);
        }
        assert!(k.q() <= 0.002, "q should never exceed q_max, got {}", k.q());
    }

    #[test]
    fn adaptive_q_is_clamped_to_q_min() {
        let mut k = Kalman::new_adaptive(0.5, 1.0, 5, 0.4, 1e6, 0.1);
        k.update(5.0, TICK); // seed
        for _ in 0..200 {
            k.update(5.0, TICK);
        }
        assert!(k.q() >= 0.4, "q should never drop below q_min, got {}", k.q());
    }
}
