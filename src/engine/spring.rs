//! Critically damped spring used for scroll animation.
//!
//! For a spring with angular frequency ω and critical damping, the
//! displacement from the target obeys x'' + 2ωx' + ω²x = 0, whose solution is
//!
//! ```text
//! x(t) = (c1 + c2·t)·e^(−ωt),   c1 = x0,   c2 = v0 + ω·x0
//! v(t) = (c2 − ω·(c1 + c2·t))·e^(−ωt)
//! ```
//!
//! It reaches the target as fast as possible without overshoot, and can be
//! retargeted mid-flight by starting a new spring from the current (x, v).

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub from: f64,
    pub to: f64,
    /// Initial velocity in units per second.
    pub velocity: f64,
    /// Angular frequency in rad/s. Settling time is roughly 6/ω.
    pub omega: f64,
}

impl Spring {
    pub fn new(from: f64, to: f64, velocity: f64, omega: f64) -> Self {
        Spring { from, to, velocity, omega: omega.max(1.0) }
    }

    /// Position and velocity `t` seconds after the start.
    pub fn sample(&self, t: f64) -> (f64, f64) {
        let t = t.max(0.0);
        let w = self.omega;
        let c1 = self.from - self.to;
        let c2 = self.velocity + w * c1;
        let e = (-w * t).exp();
        let x = (c1 + c2 * t) * e;
        let v = (c2 - w * (c1 + c2 * t)) * e;
        (self.to + x, v)
    }

    /// Settled: within half a pixel and moving slower than 5 px/s.
    pub fn done(&self, t: f64) -> bool {
        let (x, v) = self.sample(t);
        (x - self.to).abs() < 0.5 && v.abs() < 5.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_from_and_ends_at_to() {
        let s = Spring::new(0.0, 500.0, 0.0, 20.0);
        assert_eq!(s.sample(0.0), (0.0, 0.0));
        let (x, _) = s.sample(1.0);
        assert!((x - 500.0).abs() < 0.01);
        assert!(s.done(1.0));
        assert!(!s.done(0.05));
    }

    #[test]
    fn never_overshoots_from_rest() {
        let s = Spring::new(0.0, 100.0, 0.0, 25.0);
        let mut prev = 0.0;
        for i in 1..200 {
            let (x, _) = s.sample(i as f64 / 120.0);
            assert!(x <= 100.0 + 1e-9);
            assert!(x >= prev - 1e-9, "monotonic approach");
            prev = x;
        }
    }

    #[test]
    fn velocity_matches_derivative() {
        let s = Spring::new(10.0, -40.0, 300.0, 18.0);
        let t = 0.1;
        let h = 1e-6;
        let numeric = (s.sample(t + h).0 - s.sample(t - h).0) / (2.0 * h);
        assert!((numeric - s.sample(t).1).abs() < 1e-3);
        assert_eq!(s.sample(0.0).1, 300.0);
    }
}
