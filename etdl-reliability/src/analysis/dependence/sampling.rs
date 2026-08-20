//! Deterministic sampling primitives for uncertainty propagation.
//!
//! Everything here is seeded and platform-independent: no operating-system
//! entropy, no floating point from external sources, no threads. Two runs with
//! the same seed, sample count, sampler version and draw order produce
//! bit-identical streams.
//!
//! The sampler identity is reported in every analysis result so a run can be
//! reproduced or audited later:
//!
//! - algorithm: [`SAMPLER_ALGORITHM`]
//! - version: [`SAMPLER_VERSION`]
//!
//! Changing the algorithm or the draw order of any sampler REQUIRES bumping
//! [`SAMPLER_VERSION`]; results produced by different sampler versions are not
//! expected to be bit-identical and must not be compared as if they were.

/// The pseudo-random number generator used for all propagation sampling.
pub const SAMPLER_ALGORITHM: &str = "xorshift64star";

/// The sampler implementation version. Bump when the algorithm or any draw
/// order changes.
pub const SAMPLER_VERSION: &str = "1";

/// A deterministic xorshift64* PRNG.
///
/// Period 2^64 − 1. Adequate for reliability uncertainty propagation at the
/// sample counts used here (10^3–10^7); it is **not** a cryptographic RNG and
/// must never be used for security purposes.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // 0 is a valid seed for the caller but is a degenerate xorshift state,
        // so it is remapped deterministically.
        Rng {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1) with 53 bits of resolution.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [lower, upper].
    pub fn next_uniform(&mut self, lower: f64, upper: f64) -> f64 {
        lower + (upper - lower) * self.next_f64()
    }

    /// Standard normal via Box–Muller (the second variate is discarded so the
    /// draw order stays one-normal-per-two-uniforms, which keeps streams
    /// comparable across runs).
    pub fn next_normal(&mut self) -> f64 {
        let u1 = (self.next_f64()).max(1e-12);
        let u2 = self.next_f64();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = 2.0 * std::f64::consts::PI * u2;
        r * theta.cos()
    }

    /// Gamma(shape, scale = 1) via Marsaglia–Tsang (2000).
    ///
    /// For `shape < 1` the standard boost `Gamma(a) = Gamma(a+1) · U^(1/a)`
    /// identity is used. Returns `f64::NAN` for a non-positive or non-finite
    /// shape rather than looping.
    pub fn next_gamma(&mut self, shape: f64) -> f64 {
        if !shape.is_finite() || shape <= 0.0 {
            return f64::NAN;
        }
        if shape < 1.0 {
            let g = self.next_gamma(shape + 1.0);
            let u = self.next_f64().max(f64::MIN_POSITIVE);
            return g * u.powf(1.0 / shape);
        }
        let d = shape - 1.0 / 3.0;
        let c = 1.0 / (9.0 * d).sqrt();
        // Bounded rejection loop: acceptance probability is > 0.95 for all
        // shape >= 1, so the cap is defensive only.
        for _ in 0..10_000 {
            let x = self.next_normal();
            let v = 1.0 + c * x;
            if v <= 0.0 {
                continue;
            }
            let v3 = v * v * v;
            let u = self.next_f64();
            if u < 1.0 - 0.0331 * x * x * x * x {
                return d * v3;
            }
            if u.ln() < 0.5 * x * x + d * (1.0 - v3 + v3.ln()) {
                return d * v3;
            }
        }
        // Fall back to the distribution mean rather than looping forever.
        shape
    }

    /// Beta(alpha, beta) as `X / (X + Y)` with `X ~ Gamma(alpha)`,
    /// `Y ~ Gamma(beta)`.
    ///
    /// This is exact (not an approximation) and is well behaved for the very
    /// asymmetric shapes reliability work produces, e.g. `Beta(1, 1_000_000)`.
    pub fn next_beta(&mut self, alpha: f64, beta: f64) -> f64 {
        let x = self.next_gamma(alpha);
        let y = self.next_gamma(beta);
        if !x.is_finite() || !y.is_finite() {
            return f64::NAN;
        }
        let s = x + y;
        if s <= 0.0 {
            return 0.0;
        }
        x / s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seed_different_stream() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        let sa: Vec<u64> = (0..10).map(|_| a.next_u64()).collect();
        let sb: Vec<u64> = (0..10).map(|_| b.next_u64()).collect();
        assert_ne!(sa, sb);
    }

    #[test]
    fn uniform_in_unit_interval() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            let v = r.next_f64();
            assert!((0.0..1.0).contains(&v), "got {v}");
        }
    }

    #[test]
    fn normal_moments_are_reasonable() {
        let mut r = Rng::new(3);
        let n = 200_000;
        let mut sum = 0.0;
        let mut sq = 0.0;
        for _ in 0..n {
            let v = r.next_normal();
            sum += v;
            sq += v * v;
        }
        let mean = sum / n as f64;
        let var = sq / n as f64 - mean * mean;
        // Standard error of the mean is 1/sqrt(n) ~ 0.0022; 5 SE is a tight,
        // non-vacuous bound.
        assert!(mean.abs() < 0.012, "mean {mean}");
        assert!((var - 1.0).abs() < 0.02, "var {var}");
    }

    #[test]
    fn gamma_mean_matches_shape() {
        // E[Gamma(k, 1)] = k, Var = k.
        for shape in [0.5f64, 1.0, 2.5, 30.0] {
            let mut r = Rng::new(11);
            let n = 100_000;
            let mut sum = 0.0;
            for _ in 0..n {
                sum += r.next_gamma(shape);
            }
            let mean = sum / n as f64;
            let se = (shape / n as f64).sqrt();
            assert!(
                (mean - shape).abs() < 6.0 * se + 1e-3,
                "shape {shape}: mean {mean}"
            );
        }
    }

    #[test]
    fn beta_mean_and_variance_match_closed_form() {
        // E[Beta(a,b)] = a/(a+b); Var = ab / ((a+b)^2 (a+b+1)).
        let cases = [(2.0f64, 5.0f64), (1.0, 999.0), (3.0, 150.0)];
        for (a, b) in cases {
            let mut r = Rng::new(23);
            let n = 200_000;
            let mut sum = 0.0;
            let mut sq = 0.0;
            for _ in 0..n {
                let v = r.next_beta(a, b);
                assert!((0.0..=1.0).contains(&v), "beta sample out of range: {v}");
                sum += v;
                sq += v * v;
            }
            let mean = sum / n as f64;
            let var = sq / n as f64 - mean * mean;
            let em = a / (a + b);
            let ev = a * b / ((a + b) * (a + b) * (a + b + 1.0));
            let se = (ev / n as f64).sqrt();
            assert!(
                (mean - em).abs() < 6.0 * se,
                "Beta({a},{b}) mean {mean} vs {em}"
            );
            assert!(
                (var - ev).abs() < 0.1 * ev,
                "Beta({a},{b}) var {var} vs {ev}"
            );
        }
    }

    #[test]
    fn beta_handles_rare_event_shapes() {
        // Beta(1, 1e6): mean ~ 1e-6. Verifies no underflow to zero.
        let mut r = Rng::new(5);
        let n = 50_000;
        let mut sum = 0.0;
        let mut nonzero = 0;
        for _ in 0..n {
            let v = r.next_beta(1.0, 1_000_000.0);
            assert!((0.0..=1.0).contains(&v));
            if v > 0.0 {
                nonzero += 1;
            }
            sum += v;
        }
        let mean = sum / n as f64;
        assert!(nonzero > n / 2, "too many zero samples: {nonzero}/{n}");
        assert!(
            (mean - 1e-6).abs() < 2e-7,
            "rare-event Beta mean {mean} vs 1e-6"
        );
    }
}
