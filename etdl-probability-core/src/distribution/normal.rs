//! [`Normal`]: the Gaussian distribution.

use serde::{Deserialize, Serialize};

use crate::numerics::{normal_cdf, normal_quantile};

/// `X ~ Normal(mu, sigma)`, `sigma > 0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Normal {
    mu: f64,
    sigma: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum NormalError {
    #[error("Normal requires sigma > 0, got sigma={0}")]
    NonPositiveSigma(f64),
    #[error("Normal parameter {0} is not finite (NaN or infinity)")]
    NotFinite(f64),
}

impl Normal {
    pub fn new(mu: f64, sigma: f64) -> Result<Self, NormalError> {
        if !mu.is_finite() {
            return Err(NormalError::NotFinite(mu));
        }
        if !sigma.is_finite() {
            return Err(NormalError::NotFinite(sigma));
        }
        if sigma <= 0.0 {
            return Err(NormalError::NonPositiveSigma(sigma));
        }
        Ok(Normal { mu, sigma })
    }

    pub fn mu(&self) -> f64 {
        self.mu
    }

    pub fn sigma(&self) -> f64 {
        self.sigma
    }

    /// The probability density function at `x`.
    pub fn pdf(&self, x: f64) -> f64 {
        let z = (x - self.mu) / self.sigma;
        (-0.5 * z * z).exp() / (self.sigma * (2.0 * std::f64::consts::PI).sqrt())
    }

    /// `P(X <= x)`, via the standard normal CDF ([`crate::numerics::normal_cdf`])
    /// evaluated at the standardized `z = (x - mu) / sigma`. Inherits that
    /// function's documented ~1.5e-7 absolute-error bound.
    pub fn cdf(&self, x: f64) -> f64 {
        normal_cdf((x - self.mu) / self.sigma)
    }

    /// The quantile function: `mu + sigma * standard_normal_quantile(q)`.
    pub fn quantile(&self, q: f64) -> f64 {
        self.mu + self.sigma * normal_quantile(q)
    }

    /// `E[X] = mu`.
    pub fn mean(&self) -> f64 {
        self.mu
    }

    /// `Var(X) = sigma^2`.
    pub fn variance(&self) -> f64 {
        self.sigma * self.sigma
    }
}

pub fn normal(mu: f64, sigma: f64) -> Result<Normal, NormalError> {
    Normal::new(mu, sigma)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_positive_sigma() {
        assert_eq!(normal(0.0, 0.0), Err(NormalError::NonPositiveSigma(0.0)));
        assert_eq!(normal(0.0, -1.0), Err(NormalError::NonPositiveSigma(-1.0)));
    }

    #[test]
    fn standard_normal_cdf_known_values() {
        let z = normal(0.0, 1.0).unwrap();
        assert!((z.cdf(0.0) - 0.5).abs() < 1e-9);
        assert!((z.cdf(1.96) - 0.9750021048517795).abs() < 1e-6);
    }

    #[test]
    fn cdf_shifts_and_scales_correctly() {
        // Normal(10, 2) at x=12 is standard normal at z=1.
        let n = normal(10.0, 2.0).unwrap();
        let z = normal(0.0, 1.0).unwrap();
        assert!((n.cdf(12.0) - z.cdf(1.0)).abs() < 1e-9);
    }

    #[test]
    fn pdf_peak_at_mean() {
        let n = normal(5.0, 1.0).unwrap();
        let at_mean = n.pdf(5.0);
        let away = n.pdf(6.0);
        assert!(at_mean > away);
        // Known peak value for sigma=1: 1/sqrt(2*pi) ~= 0.3989422804
        assert!((at_mean - 0.3989422804014327).abs() < 1e-9);
    }

    #[test]
    fn mean_and_variance_known_values() {
        let n = normal(3.0, 4.0).unwrap();
        assert_eq!(n.mean(), 3.0);
        assert_eq!(n.variance(), 16.0);
    }

    #[test]
    fn quantile_inverts_cdf() {
        let n = normal(2.0, 3.0).unwrap();
        for q in [0.1, 0.25, 0.5, 0.75, 0.9] {
            let x = n.quantile(q);
            assert!((n.cdf(x) - q).abs() < 1e-5, "q={q}");
        }
    }

    #[test]
    fn symmetric_around_mean() {
        let n = normal(0.0, 1.0).unwrap();
        for d in [0.5, 1.0, 2.0] {
            assert!((n.cdf(d) + n.cdf(-d) - 1.0).abs() < 1e-9);
        }
    }
}
