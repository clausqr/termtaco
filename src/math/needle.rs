//! Needle dynamics: a critically-damped mass-and-spring pointer that chases a
//! moving target.
//!
//! This is the *mechanical* stage layered on top of the *statistical* Kalman
//! stage (see [`crate::math::kalman`]): Kalman estimates what the underlying
//! signal probably is; this governs how fast the rendered pointer can
//! physically get there, the way a real moving-coil meter's needle has mass
//! and can't teleport to a new reading. Pure and self-contained, like the
//! rest of `math` — no wall-clock reads; the caller supplies elapsed time.

use std::time::Duration;

/// Critically-damped needle chasing a target value. `tau` is the settling
/// time constant: after `tau` the needle has closed ~26% of the remaining
/// gap, after `5·tau` ~96%. Critical damping (rather than under- or
/// over-damped) is what a real gauge needle is built for: it closes the gap
/// as fast as possible without ringing.
pub struct Needle {
    tau: Duration,
    pos: f64,
    vel: f64,
    seeded: bool,
}

impl Needle {
    pub fn new(tau: Duration) -> Self {
        Needle { tau, pos: 0.0, vel: 0.0, seeded: false }
    }

    /// Advance the needle by `dt` toward `target`, returning its new
    /// position. The first call seeds the needle at `target` with zero
    /// velocity, so the dial doesn't sweep up from 0 on the first frame.
    pub fn step(&mut self, target: f64, dt: Duration) -> f64 {
        if !self.seeded {
            self.pos = target;
            self.vel = 0.0;
            self.seeded = true;
            return self.pos;
        }
        let (pos, vel) = settle(self.pos, self.vel, target, dt, self.tau);
        self.pos = pos;
        self.vel = vel;
        self.pos
    }
}

/// One exact step of the critically-damped second-order response
/// `ẍ = ω²(target − x) − 2ω·ẋ` (damping ratio ζ = 1, ω = 1/tau). Solved in
/// closed form rather than integrated, so it's an exact flow map — stepping
/// by `tau` twice gives the same result as stepping by `2·tau` once,
/// independent of how finely the caller subdivides `dt` (i.e. independent of
/// frame rate). A gap of many time constants is snapped straight to the
/// target: the needle would have settled by then, and it keeps the
/// exponential term out of denormal territory.
fn settle(pos: f64, vel: f64, target: f64, dt: Duration, tau: Duration) -> (f64, f64) {
    // Floor `tau` so a caller-supplied (or defaulted) zero can't divide by
    // zero or collapse to an instant snap.
    const MIN_TAU_SECS: f64 = 1e-6;
    // A gap this many time constants long has fully settled (see doc
    // comment above); snapping avoids the exponential term underflowing.
    const SNAP_AFTER_TAU_MULTIPLES: f64 = 20.0;
    let tau_secs = tau.as_secs_f64().max(MIN_TAU_SECS);
    let t = dt.as_secs_f64();
    if t >= SNAP_AFTER_TAU_MULTIPLES * tau_secs {
        return (target, 0.0);
    }
    let w = 1.0 / tau_secs;
    let e0 = pos - target;
    let b = vel + w * e0;
    let decay = (-w * t).exp();
    let e = (e0 + b * t) * decay;
    let v = (b - w * (e0 + b * t)) * decay;
    (target + e, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn first_step_seeds_at_target() {
        let mut n = Needle::new(Duration::from_secs(1));
        assert_eq!(n.step(42.0, Duration::from_millis(100)), 42.0);
    }

    #[test]
    fn zero_dt_holds_position() {
        let mut n = Needle::new(Duration::from_secs(1));
        n.step(0.0, Duration::from_millis(100)); // seed at 0
        let moved = n.step(10.0, Duration::from_millis(100));
        assert_eq!(n.step(10.0, Duration::ZERO), moved);
    }

    #[test]
    fn hand_computed_critically_damped_step() {
        // tau=1s: seed at 0 (vel=0), then step toward 1.0 over 1s.
        // e0=-1, b=vel+w*e0=0+1*(-1)=-1, decay=exp(-1).
        // e(1) = (e0 + b*1)*decay = (-1 + -1)*exp(-1) = -2/e.
        // pos = target + e = 1 - 2/e.
        let mut n = Needle::new(Duration::from_secs(1));
        n.step(0.0, Duration::from_secs(1)); // seed
        let pos = n.step(1.0, Duration::from_secs(1));
        approx(pos, 1.0 - 2.0 / std::f64::consts::E);
    }

    #[test]
    fn two_small_steps_equal_one_big_step() {
        let tau = Duration::from_millis(300);
        let mut a = Needle::new(tau);
        a.step(0.0, Duration::from_millis(10)); // seed
        a.step(5.0, Duration::from_millis(10));
        let combined = a.step(5.0, Duration::from_millis(200));

        let mut b = Needle::new(tau);
        b.step(0.0, Duration::from_millis(10)); // seed
        b.step(5.0, Duration::from_millis(10));
        let split_first = b.step(5.0, Duration::from_millis(100));
        let split_second = b.step(5.0, Duration::from_millis(100));
        assert!((combined - split_second).abs() < 1e-9, "flow map should be dt-subdivision independent");
        let _ = split_first;
    }

    #[test]
    fn never_overshoots_from_rest() {
        let mut n = Needle::new(Duration::from_millis(500));
        n.step(0.0, Duration::from_millis(10)); // seed at rest
        let mut last = 0.0;
        for _ in 0..200 {
            let p = n.step(10.0, Duration::from_millis(10));
            assert!(p >= last - 1e-9, "should climb monotonically from rest, got {p} after {last}");
            assert!(p <= 10.0 + 1e-9, "critically damped from rest should never overshoot, got {p}");
            last = p;
        }
    }

    #[test]
    fn settles_within_five_time_constants() {
        let mut n = Needle::new(Duration::from_secs(1));
        n.step(0.0, Duration::from_millis(1)); // seed at rest
        let pos = n.step(1.0, Duration::from_secs(5));
        // Analytic: e(5tau) = (e0 + b*5tau)*exp(-5) = -6*exp(-5) starting from rest.
        let expected = 1.0 - 6.0 * (-5.0_f64).exp();
        approx(pos, expected);
        assert!(pos > 0.95, "expected within 5% of target after 5tau, got {pos}");
    }

    #[test]
    fn larger_inertia_lags_more() {
        let mut light = Needle::new(Duration::from_millis(100));
        light.step(0.0, Duration::from_millis(1));
        let light_pos = light.step(10.0, Duration::from_millis(50));

        let mut heavy = Needle::new(Duration::from_secs(1));
        heavy.step(0.0, Duration::from_millis(1));
        let heavy_pos = heavy.step(10.0, Duration::from_millis(50));

        assert!(heavy_pos < light_pos, "a larger tau should lag further behind: {heavy_pos} vs {light_pos}");
    }

    #[test]
    fn long_gap_snaps_to_target() {
        let mut n = Needle::new(Duration::from_millis(100));
        n.step(0.0, Duration::from_millis(1));
        let pos = n.step(50.0, Duration::from_secs(1000));
        assert_eq!(pos, 50.0);
    }

    #[test]
    fn tracks_a_ramp_with_bounded_lag() {
        // Steady-state lag of a critically damped tracker following a ramp of
        // slope s is 2*s*tau. Drive it finely and check convergence.
        let tau_secs = 0.2;
        let slope = 5.0; // units/sec
        let mut n = Needle::new(Duration::from_secs_f64(tau_secs));
        let dt = Duration::from_millis(5);
        let mut t = 0.0;
        n.step(0.0, dt);
        let mut pos = 0.0;
        for _ in 0..4000 {
            t += dt.as_secs_f64();
            pos = n.step(slope * t, dt);
        }
        let lag = slope * t - pos;
        let expected_lag = 2.0 * slope * tau_secs;
        assert!((lag - expected_lag).abs() / expected_lag < 0.1, "expected lag near {expected_lag}, got {lag}");
    }
}
