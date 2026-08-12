//! Sample ingestion, optional Kalman smoothing, and staleness bookkeeping.
//!
//! [`Feed`] owns everything the render loop needs to decide *what* to draw
//! and *when* to repaint, and nothing that needs a terminal, so the
//! decision logic (should we repaint, is the Kalman filter allowed to
//! extrapolate right now) is unit-testable against synthetic values and
//! `Instant`s, without a real tty or a real clock. The public `drain`/`tick`
//! read the wall clock once and forward it to a private `_at` twin that does
//! the actual work; tests call the `_at` twins directly with synthetic
//! `Instant`s.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::math::kalman::Kalman;
use crate::math::stats::{Stats, Window};

pub struct Feed {
    window: Window,
    kalman: Option<Kalman>,
    /// The filter's own clock: the instant of its last predict-or-update
    /// advance, distinct from `last_sample` (raw data arrival, used for
    /// staleness) since the filter also advances on ticks with no new data.
    kalman_last: Option<Instant>,
    last_smoothed: Option<f64>,
    last_sample: Option<Instant>,
    stale: bool,
    stale_after: Duration,
    stdin_closed: bool,
}

impl Feed {
    pub fn new(window: usize, stale_after: Duration, kalman: Option<Kalman>) -> Self {
        Feed {
            window: Window::new(window),
            kalman,
            kalman_last: None,
            last_smoothed: None,
            last_sample: None,
            stale: false,
            stale_after,
            stdin_closed: false,
        }
    }

    /// Drain every pending sample, folding each through the Kalman filter
    /// (if enabled). Returns whether anything changed (a sample landed, or
    /// stdin just closed), which the caller should OR into its dirty flag.
    pub fn drain(&mut self, rx: &Receiver<f64>) -> bool {
        self.drain_at(rx, Instant::now())
    }

    fn drain_at(&mut self, rx: &Receiver<f64>, now: Instant) -> bool {
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok(v) => {
                    if let Some(k) = &mut self.kalman {
                        let dt = self.kalman_last.map(|t| now.duration_since(t)).unwrap_or_default();
                        self.last_smoothed = Some(k.update(v, dt));
                        self.kalman_last = Some(now);
                    } else {
                        self.last_smoothed = Some(v);
                    }
                    self.window.push(v);
                    self.last_sample = Some(now);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // stdin reached EOF. Remember it so the placeholder can
                    // stop claiming we're still waiting, but only report a
                    // change the first time.
                    if !self.stdin_closed {
                        self.stdin_closed = true;
                        changed = true;
                    }
                    break;
                }
            }
        }
        changed
    }

    /// One pass with no new data: advance the filter's predict-only
    /// extrapolation (skipped while stale, see the comment inside), apply
    /// the animate repaint policy, and re-evaluate staleness. Returns
    /// whether a repaint is warranted, which the caller should OR into its
    /// dirty flag.
    pub fn tick(&mut self, animate: bool) -> bool {
        self.tick_at(animate, Instant::now())
    }

    fn tick_at(&mut self, animate: bool, now: Instant) -> bool {
        let mut dirty = false;

        // With no new measurement this pass, still advance the filter's
        // estimate along its fitted velocity so the needle keeps moving
        // smoothly between measurements, rather than sitting frozen until
        // the next one arrives. Skipped once the feed is stale:
        // extrapolating a dead feed's last known velocity forever would let
        // the estimate drift without bound while the STALE LED is lit.
        // Freezing `kalman_last` here also means the eventual real
        // measurement arrives with the whole gap as its `dt`, so the
        // filter's own dt-scaled process noise (see `math::kalman`) makes it
        // trust that fresh sample almost completely instead of dragging in
        // the stale trajectory.
        if !self.stale {
            if let Some(k) = &mut self.kalman {
                let dt = self.kalman_last.map(|t| now.duration_since(t)).unwrap_or_default();
                let predicted = k.predict(dt);
                self.kalman_last = Some(now);
                if self.last_sample.is_some() {
                    self.last_smoothed = Some(predicted);
                }
            }
        }

        // Some effects (Kalman extrapolation, needle inertia in the display
        // layer) keep moving between measurements, so keep repainting once
        // data has started flowing, same as a genuinely live feed.
        if animate && self.last_sample.is_some() {
            dirty = true;
        }

        // Re-evaluate staleness; repaint once when it flips so a dead feed
        // doesn't masquerade as a live, steady reading.
        let now_stale = self.last_sample.is_some_and(|t| now.duration_since(t) >= self.stale_after);
        if now_stale != self.stale {
            self.stale = now_stale;
            dirty = true;
        }

        dirty
    }

    /// The window's raw statistics paired with the smoothed reading (the Kalman
    /// estimate, or `stats.last` when smoothing is disabled), or `None` while no
    /// sample has landed yet.
    ///
    /// Returned alongside rather than folded into `Stats`: the smoothing is this
    /// module's, not `Window`'s, and `Window` has no way to compute it.
    pub fn snapshot(&self) -> Option<(Stats, f64)> {
        let stats = self.window.stats()?;
        let smoothed = self.last_smoothed.unwrap_or(stats.last);
        Some((stats, smoothed))
    }

    /// The Kalman filter's current position uncertainty (a standard
    /// deviation, in the same units as the tracked value), or `None` when
    /// `--kalman` is off. Read alongside [`snapshot`](Self::snapshot) so a
    /// renderer can show the estimate together with how much to trust it.
    pub fn kalman_uncertainty(&self) -> Option<f64> {
        self.kalman.as_ref().map(Kalman::uncertainty)
    }

    pub fn stale(&self) -> bool {
        self.stale
    }

    pub fn stdin_closed(&self) -> bool {
        self.stdin_closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn feed(kalman: bool) -> Feed {
        Feed::new(200, Duration::from_secs(3), kalman.then(|| Kalman::new(0.001, 0.1)))
    }

    #[test]
    fn drain_pushes_and_reports_change() {
        let (tx, rx) = mpsc::channel();
        tx.send(1.0).unwrap();
        tx.send(2.0).unwrap();
        let mut f = feed(false);
        assert!(f.drain_at(&rx, Instant::now()));
        assert_eq!(f.snapshot().unwrap().0.last, 2.0);
    }

    #[test]
    fn drain_on_empty_channel_reports_no_change() {
        let (_tx, rx) = mpsc::channel::<f64>();
        let mut f = feed(false);
        assert!(!f.drain_at(&rx, Instant::now()));
    }

    #[test]
    fn drain_reports_stdin_closed_exactly_once() {
        let (tx, rx) = mpsc::channel::<f64>();
        drop(tx);
        let mut f = feed(false);
        let now = Instant::now();
        assert!(f.drain_at(&rx, now), "first drain after disconnect should report a change");
        assert!(f.stdin_closed());
        assert!(!f.drain_at(&rx, now), "second drain shouldn't re-report the same closure");
        assert!(f.stdin_closed());
    }

    #[test]
    fn snapshot_is_none_until_the_first_sample() {
        let f = feed(false);
        assert!(f.snapshot().is_none());
    }

    #[test]
    fn snapshot_smoothed_equals_last_without_kalman() {
        let (tx, rx) = mpsc::channel();
        tx.send(5.0).unwrap();
        let mut f = feed(false);
        f.drain_at(&rx, Instant::now());
        let (stats, smoothed) = f.snapshot().unwrap();
        assert_eq!(smoothed, stats.last);
    }

    #[test]
    fn kalman_uncertainty_is_none_when_disabled() {
        let (tx, rx) = mpsc::channel();
        tx.send(5.0).unwrap();
        let mut f = feed(false);
        f.drain_at(&rx, Instant::now());
        assert_eq!(f.kalman_uncertainty(), None);
    }

    #[test]
    fn kalman_uncertainty_is_some_when_enabled() {
        let (tx, rx) = mpsc::channel();
        tx.send(5.0).unwrap();
        let mut f = feed(true);
        assert_eq!(f.kalman_uncertainty(), Some(0.0), "unseeded filter reports zero uncertainty, not none");
        f.drain_at(&rx, Instant::now());
        assert!(f.kalman_uncertainty().unwrap() > 0.0, "a seeded filter should report a real uncertainty");
    }

    #[test]
    fn snapshot_smoothed_diverges_with_kalman() {
        let (tx, rx) = mpsc::channel();
        let mut f = feed(true);
        let t0 = Instant::now();
        tx.send(0.0).unwrap();
        f.drain_at(&rx, t0); // seeds the filter; smoothed == raw on the first sample
        tx.send(100.0).unwrap();
        f.drain_at(&rx, t0 + Duration::from_millis(100));
        let (stats, smoothed) = f.snapshot().unwrap();
        assert_eq!(stats.last, 100.0);
        assert!(smoothed < 100.0, "kalman should smooth away from the raw spike");
    }

    #[test]
    fn tick_does_not_extrapolate_while_stale() {
        let (tx, rx) = mpsc::channel();
        let mut f = feed(true);
        let t0 = Instant::now();
        tx.send(0.0).unwrap();
        f.drain_at(&rx, t0);
        tx.send(10.0).unwrap();
        f.drain_at(&rx, t0 + Duration::from_millis(100)); // establishes a velocity estimate

        // Advance in small (frame-sized) steps up to and past stale_after
        // (3s), the way the real render loop calls tick() every frame, so
        // staleness is detected promptly rather than in one large jump.
        let step = Duration::from_millis(16);
        let mut t = t0 + Duration::from_millis(100);
        while !f.stale() {
            t += step;
            f.tick_at(false, t);
        }
        let frozen = f.snapshot().unwrap().1;

        // Many more ticks pass while stale; without the staleness gate the
        // estimate would keep drifting along the fitted velocity forever.
        for _ in 0..50 {
            t += step;
            f.tick_at(false, t);
        }
        assert_eq!(
            f.snapshot().unwrap().1,
            frozen,
            "estimate should freeze once stale, not keep extrapolating"
        );
    }

    #[test]
    fn stale_gap_lands_wholly_in_the_next_measurement_dt() {
        // After a stale gap, kalman_last stays frozen (tick_at skips the
        // predict-and-advance step while stale), so the next real sample
        // sees the *entire* gap as its dt, which is what makes the filter
        // trust it almost completely rather than dragging in a stale
        // trajectory (covered at the Kalman level by
        // `longer_gap_trusts_the_new_measurement_more`; this test pins that
        // the app layer actually wires the freeze correctly).
        let (tx, rx) = mpsc::channel();
        let mut f = feed(true);
        let t0 = Instant::now();
        tx.send(0.0).unwrap();
        f.drain_at(&rx, t0);
        tx.send(100.0).unwrap();
        f.drain_at(&rx, t0 + Duration::from_millis(100));

        let step = Duration::from_millis(16);
        let mut t = t0 + Duration::from_millis(100);
        while !f.stale() {
            t += step;
            f.tick_at(false, t);
        }

        // A long silent gap passes...
        let t_resume = t + Duration::from_secs(60);
        tx.send(-100.0).unwrap();
        f.drain_at(&rx, t_resume);

        // ...and the filter should trust the fresh sample almost completely
        // rather than a velocity trajectory extrapolated from minutes ago.
        let (_, smoothed) = f.snapshot().unwrap();
        assert!(
            (smoothed - (-100.0)).abs() < 1.0,
            "expected near-immediate trust of the fresh sample, got {smoothed}"
        );
    }

    #[test]
    fn tick_flips_stale_at_the_deadline_and_marks_dirty_once() {
        let (tx, rx) = mpsc::channel();
        let mut f = feed(false);
        let t0 = Instant::now();
        tx.send(1.0).unwrap();
        f.drain_at(&rx, t0);

        assert!(!f.tick_at(false, t0 + Duration::from_secs(1)), "well within stale_after, no flip");
        assert!(!f.stale());

        assert!(f.tick_at(false, t0 + Duration::from_secs(3)), "at the deadline, should flip and report dirty");
        assert!(f.stale());

        assert!(!f.tick_at(false, t0 + Duration::from_secs(4)), "already stale, no further flip");
    }

    #[test]
    fn stale_clears_when_a_fresh_sample_arrives() {
        let (tx, rx) = mpsc::channel();
        let mut f = feed(false);
        let t0 = Instant::now();
        tx.send(1.0).unwrap();
        f.drain_at(&rx, t0);
        f.tick_at(false, t0 + Duration::from_secs(5));
        assert!(f.stale());

        tx.send(2.0).unwrap();
        let t_resume = t0 + Duration::from_secs(5) + Duration::from_millis(1);
        f.drain_at(&rx, t_resume);
        f.tick_at(false, t_resume);
        assert!(!f.stale());
    }

    #[test]
    fn animate_marks_dirty_only_after_the_first_sample() {
        let mut f = feed(false);
        let t0 = Instant::now();
        assert!(!f.tick_at(true, t0), "no data yet, animate shouldn't force a repaint");

        let (tx, rx) = mpsc::channel();
        tx.send(1.0).unwrap();
        f.drain_at(&rx, t0);
        assert!(f.tick_at(true, t0 + Duration::from_millis(10)), "once seeded, animate forces a repaint every tick");
    }
}
