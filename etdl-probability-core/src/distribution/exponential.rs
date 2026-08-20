//! [`Exponential`]: the constant-hazard-rate failure-time distribution —
//! the same model `etdl-reliability`'s `ExponentialRateEstimator` already
//! implements for its own workflow. This crate provides the generic
//! distribution math (pdf/cdf/mean/variance/quantile); it does not own or
//! replace the reliability estimator.

use serde::{Deserialize, Serialize};

/// `X ~ Exponential(lambda)`, `lambda > 0`. `lambda` is a rate (see
/// [`crate::Rate`] for how a caller should carry its unit, e.g.
/// "per hour") — this type stores only the bare scalar, exactly as
/// `etdl-reliability`'s own exponential model does, and does not silently
/// attach or assume a unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Exponential {
    lambda: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum ExponentialError {
    #[error("Exponential requires lambda > 0, got lambda={0}")]
    NonPositiveLambda(f64),
    #[error("Exponential lambda {0} is not finite (NaN or infinity)")]
    NotFinite(f64),
}

impl Exponential {
    pub fn new(lambda: f64) -> Result<Self, ExponentialError> {
        if !lambda.is_finite() {
            return Err(ExponentialError::NotFinite(lambda));
        }
        if lambda <= 0.0 {
            return Err(ExponentialError::NonPositiveLambda(lambda));
        }
        Ok(Exponential { lambda })
    }

    pub fn lambda(&self) -> f64 {
        self.lambda
    }

    /// The probability density function at `x >= 0`; `0` for `x < 0`.
    pub fn pdf(&self, x: f64) -> f64 {
        if x < 0.0 {
            return 0.0;
        }
        self.lambda * (-self.lambda * x).exp()
    }

    /// `P(X <= x) = 1 - exp(-lambda*x)` for `x >= 0`, computed via the
    /// numerically stable `-expm1(-lambda*x)` (avoids catastrophic
    /// cancellation for small `lambda*x`, the same technique
    /// `etdl-reliability`'s own exponential estimator already uses).
    pub fn cdf(&self, x: f64) -> f64 {
        if x < 0.0 {
            return 0.0;
        }
        -(-self.lambda * x).exp_m1()
    }

    /// The survival function `P(X > x) = 1 - CDF(x) = exp(-lambda*x)`,
    /// named per its correct statistical term (not folded into `cdf` under
    /// a generic name) — the foundation a future predictive-reliability
    /// hazard/survival abstraction would build on without changing this
    /// type. See the crate-level docs.
    pub fn survival(&self, x: f64) -> f64 {
        if x < 0.0 {
            return 1.0;
        }
        (-self.lambda * x).exp()
    }

    /// The quantile function: `-ln(1-q) / lambda` for `q in [0, 1)`.
    pub fn quantile(&self, q: f64) -> f64 {
        -(1.0 - q).ln() / self.lambda
    }

    /// `E[X] = 1 / lambda`.
    pub fn mean(&self) -> f64 {
        1.0 / self.lambda
    }

    /// `Var(X) = 1 / lambda^2`.
    pub fn variance(&self) -> f64 {
        1.0 / (self.lambda * self.lambda)
    }
}

pub fn exponential(lambda: f64) -> Result<Exponential, ExponentialError> {
    Exponential::new(lambda)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_positive_lambda() {
        assert_eq!(
            exponential(0.0),
            Err(ExponentialError::NonPositiveLambda(0.0))
        );
        assert_eq!(
            exponential(-1.0),
            Err(ExponentialError::NonPositiveLambda(-1.0))
        );
    }

    #[test]
    fn cdf_known_value() {
        // lambda=0.2, t=10: P = 1 - exp(-2) = 0.8646647...
        let e = exponential(0.2).unwrap();
        let expected = 1.0 - (-2.0f64).exp();
        assert!((e.cdf(10.0) - expected).abs() < 1e-12);
    }

    #[test]
    fn survival_plus_cdf_is_one() {
        let e = exponential(0.001).unwrap();
        for x in [0.0, 1.0, 100.0, 1000.0] {
            assert!((e.cdf(x) + e.survival(x) - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn cdf_zero_at_x_zero() {
        let e = exponential(0.5).unwrap();
        assert_eq!(e.cdf(0.0), 0.0);
        assert_eq!(e.survival(0.0), 1.0);
    }

    #[test]
    fn cdf_negative_x_is_zero() {
        let e = exponential(0.5).unwrap();
        assert_eq!(e.cdf(-1.0), 0.0);
    }

    #[test]
    fn mean_and_variance_known_values() {
        let e = exponential(0.25).unwrap();
        assert!((e.mean() - 4.0).abs() < 1e-12);
        assert!((e.variance() - 16.0).abs() < 1e-12);
    }

    #[test]
    fn quantile_inverts_cdf() {
        let e = exponential(0.03).unwrap();
        for q in [0.1, 0.5, 0.9, 0.99] {
            let x = e.quantile(q);
            assert!((e.cdf(x) - q).abs() < 1e-9, "q={q}");
        }
    }

    #[test]
    fn stable_for_very_small_lambda_times_t() {
        // A naive `1.0 - (-lambda*x).exp()` loses precision here; the
        // expm1-based implementation should not.
        let e = exponential(1e-9).unwrap();
        let p = e.cdf(1.0);
        assert!(p > 0.0, "expected a tiny but nonzero probability, got {p}");
        assert!((p - 1e-9).abs() < 1e-15);
    }
}
