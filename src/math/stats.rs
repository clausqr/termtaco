//! Running statistics over a fixed-size window of `f64` samples.
//!
//! Pure and self-contained: knows nothing about rendering or I/O. The render
//! loop pushes values in and asks for a [`Stats`] snapshot each frame.

use std::collections::VecDeque;

/// A bounded ring buffer of the most recent `cap` samples.
pub struct Window {
    buf: VecDeque<f64>,
    cap: usize,
}

/// A snapshot of the window's statistics. Cheap to copy and hand to a display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    /// Most recently pushed value.
    pub last: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    /// Population standard deviation (divided by n).
    pub stddev: f64,
    /// Number of samples currently in the window.
    pub count: usize,
}

impl Window {
    /// Create a window holding at most `cap` samples (clamped to at least 1).
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Window {
            buf: VecDeque::with_capacity(cap),
            cap,
        }
    }

    /// Push a sample, evicting the oldest once the window is full.
    ///
    /// Non-finite samples (NaN, ±∞) are dropped so the snapshot can never leak
    /// the min/max sentinels or poison the mean, keeping the invariant local
    /// to this module rather than relying on the caller to pre-filter.
    pub fn push(&mut self, v: f64) {
        if !v.is_finite() {
            return;
        }
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }

    /// Compute a fresh snapshot over the current contents.
    ///
    /// Two passes (mean, then variance) over the deque. At a window of 10000
    /// and ~30 fps this is well under a million ops/sec: negligible.
    /// Returns `None` while the window is empty.
    pub fn stats(&self) -> Option<Stats> {
        let n = self.buf.len();
        if n == 0 {
            return None;
        }

        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut sum = 0.0;
        for &x in &self.buf {
            if x < min {
                min = x;
            }
            if x > max {
                max = x;
            }
            sum += x;
        }
        let mean = sum / n as f64;

        let mut var_acc = 0.0;
        for &x in &self.buf {
            let d = x - mean;
            var_acc += d * d;
        }
        // Population standard deviation: σ = sqrt((1/n)·Σ(xᵢ−μ)²).
        let stddev = (var_acc / n as f64).sqrt();

        let last = *self.buf.back().unwrap();
        Some(Stats {
            last,
            min,
            max,
            mean,
            stddev,
            count: n,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn empty_window_has_no_stats() {
        let w = Window::new(8);
        assert!(w.stats().is_none());
    }

    #[test]
    fn evicts_oldest_at_capacity() {
        let mut w = Window::new(3);
        for v in [1.0, 2.0, 3.0, 4.0] {
            w.push(v);
        }
        let s = w.stats().unwrap();
        // Window holds [2, 3, 4]; the 1.0 was evicted.
        assert_eq!(s.count, 3);
        approx(s.min, 2.0);
        approx(s.max, 4.0);
        approx(s.last, 4.0);
    }

    #[test]
    fn computes_known_stats() {
        let mut w = Window::new(10);
        for v in [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0] {
            w.push(v);
        }
        let s = w.stats().unwrap();
        approx(s.mean, 5.0);
        // Population stddev of this classic set is exactly 2.0.
        approx(s.stddev, 2.0);
        approx(s.min, 2.0);
        approx(s.max, 9.0);
        approx(s.last, 9.0);
        assert_eq!(s.count, 8);
    }

    #[test]
    fn non_finite_samples_are_dropped() {
        let mut w = Window::new(8);
        w.push(2.0);
        w.push(f64::NAN);
        w.push(f64::INFINITY);
        w.push(4.0);
        let s = w.stats().unwrap();
        // Only the two finite samples count; no NaN/Inf leaked into the stats.
        assert_eq!(s.count, 2);
        approx(s.min, 2.0);
        approx(s.max, 4.0);
        approx(s.mean, 3.0);
        assert!(s.mean.is_finite() && s.stddev.is_finite());
    }

    #[test]
    fn all_non_finite_leaves_window_empty() {
        let mut w = Window::new(4);
        w.push(f64::NAN);
        w.push(f64::NEG_INFINITY);
        assert!(w.stats().is_none());
    }

    #[test]
    fn single_sample_has_zero_stddev() {
        let mut w = Window::new(4);
        w.push(42.0);
        let s = w.stats().unwrap();
        approx(s.mean, 42.0);
        approx(s.stddev, 0.0);
        approx(s.min, 42.0);
        approx(s.max, 42.0);
    }
}
