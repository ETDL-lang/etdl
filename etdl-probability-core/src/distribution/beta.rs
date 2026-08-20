//! [`Beta`]: a continuous distribution on `[0, 1]`, the conjugate prior for
//! a binomial proportion (the same role it plays in
//! `etdl-reliability`'s Beta-Binomial calibration — this crate provides the
//! generic distribution math; the reliability domain's Bayesian estimation
//! workflow remains its own, unchanged).

use serde::{Deserialize, Serialize};

use crate::numerics::{beta_quantile, log_gamma, regularized_beta};

/// `X ~ Beta(alpha, beta)`, `alpha > 0`, `beta > 0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Beta {
    alpha: f64,
    beta: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum BetaError {
    #[error("Beta requires alpha > 0, got alpha={0}")]
    NonPositiveAlpha(f64),
    #[error("Beta requires beta > 0, got beta={0}")]
    NonPositiveBeta(f64),
    #[error("Beta parameter {0} is not finite (NaN or infinity)")]
    NotFinite(f64),
}

impl Beta {
    pub fn new(alpha: f64, beta: f64) -> Result<Self, BetaError> {
        if !alpha.is_finite() {
            return Err(BetaError::NotFinite(alpha));
        }
        if !beta.is_finite() {
            return Err(BetaError::NotFinite(beta));
        }
        if alpha <= 0.0 {
            return Err(BetaError::NonPositiveAlpha(alpha));
        }
        if beta <= 0.0 {
            return Err(BetaError::NonPositiveBeta(beta));
        }
        Ok(Beta { alpha, beta })
    }

    pub fn alpha(&self) -> f64 {
        self.alpha
    }

    pub fn beta(&self) -> f64 {
        self.beta
    }

    /// The probability density function at `x`. `0` outside `[0, 1]`.
    pub fn pdf(&self, x: f64) -> f64 {
        if !(0.0..=1.0).contains(&x) {
            return 0.0;
        }
        // Degenerate density at the boundary when alpha or beta < 1 (the
        // density is unbounded there); pdf() reports the limiting behavior
        // honestly rather than a spurious finite number.
        if x == 0.0 {
            return if self.alpha < 1.0 {
                f64::INFINITY
            } else if self.alpha == 1.0 {
                self.beta // pdf(0) = beta * (1-0)^(beta-1) / B(1,beta) = beta
            } else {
                0.0
            };
        }
        if x == 1.0 {
            return if self.beta < 1.0 {
                f64::INFINITY
            } else if self.beta == 1.0 {
                self.alpha
            } else {
                0.0
            };
        }
        let ln_beta_fn = log_gamma(self.alpha) + log_gamma(self.beta) - log_gamma(self.alpha + self.beta);
        let ln_pdf = (self.alpha - 1.0) * x.ln() + (self.beta - 1.0) * (1.0 - x).ln() - ln_beta_fn;
        ln_pdf.exp()
    }

    /// The CDF `P(X <= x) = I_x(alpha, beta)`.
    pub fn cdf(&self, x: f64) -> f64 {
        regularized_beta(x.clamp(0.0, 1.0), self.alpha, self.beta)
    }

    /// The quantile function (inverse CDF) at `q in (0, 1)`, via bisection
    /// on [`cdf`]. See `docs/reference/standard-probability-library.md` for
    /// the numerical tolerance this carries (bisected to ~1e-14 in the
    /// bisection parameter, which does not imply that precision in `q`
    /// itself for extreme shape parameters).
    pub fn quantile(&self, q: f64) -> f64 {
        beta_quantile(self.alpha, self.beta, q)
    }

    /// `E[X] = alpha / (alpha + beta)`.
    pub fn mean(&self) -> f64 {
        self.alpha / (self.alpha + self.beta)
    }

    /// `Var(X) = alpha*beta / ((alpha+beta)^2 * (alpha+beta+1))`.
    pub fn variance(&self) -> f64 {
        let sum = self.alpha + self.beta;
        (self.alpha * self.beta) / (sum * sum * (sum + 1.0))
    }
}

pub fn beta(alpha: f64, beta_param: f64) -> Result<Beta, BetaError> {
    Beta::new(alpha, beta_param)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_positive_parameters() {
        assert_eq!(beta(-1.0, 2.0), Err(BetaError::NonPositiveAlpha(-1.0)));
        assert_eq!(beta(2.0, 0.0), Err(BetaError::NonPositiveBeta(0.0)));
    }

    #[test]
    fn mean_known_value() {
        // Beta(2, 8): mean = 2/10 = 0.2
        let b = beta(2.0, 8.0).unwrap();
        assert!((b.mean() - 0.2).abs() < 1e-12);
    }

    #[test]
    fn variance_known_value() {
        // Beta(2, 8): var = 2*8 / (10^2 * 11) = 16/1100 = 0.014545...
        let b = beta(2.0, 8.0).unwrap();
        assert!((b.variance() - 16.0 / 1100.0).abs() < 1e-9);
    }

    #[test]
    fn uniform_special_case_beta_1_1() {
        // Beta(1,1) is Uniform(0,1): pdf=1 everywhere in [0,1], cdf=x, mean=0.5.
        let b = beta(1.0, 1.0).unwrap();
        for x in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!((b.pdf(x) - 1.0).abs() < 1e-9, "pdf({x})");
            assert!((b.cdf(x) - x).abs() < 1e-9, "cdf({x})");
        }
        assert!((b.mean() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn pdf_zero_outside_support() {
        let b = beta(2.0, 2.0).unwrap();
        assert_eq!(b.pdf(-0.1), 0.0);
        assert_eq!(b.pdf(1.1), 0.0);
    }

    #[test]
    fn quantile_inverts_cdf() {
        let b = beta(3.0, 5.0).unwrap();
        for q in [0.1, 0.25, 0.5, 0.75, 0.9] {
            let x = b.quantile(q);
            assert!((b.cdf(x) - q).abs() < 1e-6, "q={q}");
        }
    }

    #[test]
    fn posterior_mean_matches_beta_binomial_convention() {
        // The exact scenario etdl-reliability's Beta-Binomial estimator
        // uses: uniform prior Beta(1,1), 2 failures in 10 trials ->
        // posterior Beta(3, 9), mean = 3/12 = 0.25 (matches
        // etdl-reliability's own test for the same inputs).
        let posterior = beta(1.0 + 2.0, 1.0 + 8.0).unwrap();
        assert!((posterior.mean() - 0.25).abs() < 1e-9);
    }
}
