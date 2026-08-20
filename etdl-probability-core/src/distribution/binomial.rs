//! [`Binomial`]: the number of successes in `n` independent Bernoulli(p)
//! trials.

use serde::{Deserialize, Serialize};

use crate::numerics::{log_gamma, regularized_beta};
use crate::probability::{Probability, ProbabilityError};

/// `X ~ Binomial(n, p)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Binomial {
    n: u64,
    p: Probability,
}

/// A problem constructing a [`Binomial`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum BinomialError {
    #[error("Binomial requires n >= 1, got n={0}")]
    NIsZero(u64),
    #[error("invalid p: {0}")]
    InvalidP(#[from] ProbabilityError),
}

impl Binomial {
    pub fn new(n: u64, p: Probability) -> Result<Self, BinomialError> {
        if n == 0 {
            return Err(BinomialError::NIsZero(n));
        }
        Ok(Binomial { n, p })
    }

    pub fn n(&self) -> u64 {
        self.n
    }

    pub fn p(&self) -> Probability {
        self.p
    }

    /// The probability mass function `P(X = k)`, computed in log-space
    /// (`ln C(n,k) + k*ln(p) + (n-k)*ln(1-p)`, via [`log_gamma`] for the
    /// binomial coefficient) to avoid overflow for large `n` — a naive
    /// `n! / (k! (n-k)!)` computation overflows `u64`/`f64` well before `n`
    /// reaches typical fault-tree exposure counts (millions of requests).
    /// Returns `Probability(0)` for `k > n` (outside the support, not an
    /// error).
    pub fn pmf(&self, k: u64) -> Probability {
        if k > self.n {
            return Probability::IMPOSSIBLE;
        }
        let p = self.p.value();
        if p == 0.0 {
            return if k == 0 {
                Probability::CERTAIN
            } else {
                Probability::IMPOSSIBLE
            };
        }
        if p == 1.0 {
            return if k == self.n {
                Probability::CERTAIN
            } else {
                Probability::IMPOSSIBLE
            };
        }
        let n = self.n as f64;
        let kf = k as f64;
        let ln_coeff = log_gamma(n + 1.0) - log_gamma(kf + 1.0) - log_gamma(n - kf + 1.0);
        let ln_pmf = ln_coeff + kf * p.ln() + (n - kf) * (1.0 - p).ln();
        Probability::new(ln_pmf.exp()).unwrap_or(Probability::IMPOSSIBLE)
    }

    /// The CDF `P(X <= k)`, via the exact identity
    /// `P(X <= k) = I_{1-p}(n-k, k+1)` (the regularized incomplete beta
    /// function) — the same identity `etdl-reliability`'s calibration
    /// module uses for its own, independently implemented, binomial test.
    pub fn cdf(&self, k: u64) -> Probability {
        if k >= self.n {
            return Probability::CERTAIN;
        }
        let p = self.p.value();
        if p == 0.0 {
            return Probability::CERTAIN; // X is always 0 <= k
        }
        if p == 1.0 {
            return Probability::IMPOSSIBLE; // X is always n > k
        }
        let n = self.n as f64;
        let kf = k as f64;
        let value = regularized_beta(1.0 - p, n - kf, kf + 1.0);
        Probability::new(value.clamp(0.0, 1.0)).unwrap_or(Probability::IMPOSSIBLE)
    }

    /// `E[X] = n*p`.
    pub fn mean(&self) -> f64 {
        self.n as f64 * self.p.value()
    }

    /// `Var(X) = n*p*(1-p)`.
    pub fn variance(&self) -> f64 {
        let p = self.p.value();
        self.n as f64 * p * (1.0 - p)
    }
}

pub fn binomial(n: u64, p: f64) -> Result<Binomial, BinomialError> {
    Binomial::new(n, Probability::new(p)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_n_zero() {
        assert_eq!(binomial(0, 0.5), Err(BinomialError::NIsZero(0)));
    }

    #[test]
    fn rejects_invalid_p() {
        assert!(binomial(10, -0.1).is_err());
        assert!(binomial(10, 1.5).is_err());
    }

    #[test]
    fn pmf_known_values_n10_p05() {
        // Binomial(10, 0.5): P(X=5) = C(10,5)*0.5^10 = 252/1024 = 0.24609375
        let b = binomial(10, 0.5).unwrap();
        assert!((b.pmf(5).value() - 0.24609375).abs() < 1e-9);
        // P(X=0) = 0.5^10 = 0.0009765625
        assert!((b.pmf(0).value() - 0.0009765625).abs() < 1e-9);
        // P(X=10) = 0.5^10
        assert!((b.pmf(10).value() - 0.0009765625).abs() < 1e-9);
        // Outside support.
        assert_eq!(b.pmf(11).value(), 0.0);
    }

    #[test]
    fn pmf_sums_to_one() {
        let b = binomial(15, 0.37).unwrap();
        let total: f64 = (0..=15).map(|k| b.pmf(k).value()).sum();
        assert!((total - 1.0).abs() < 1e-9, "got {total}");
    }

    #[test]
    fn cdf_matches_manual_sum_of_pmf() {
        let b = binomial(20, 0.3).unwrap();
        for k in 0..=20 {
            let manual: f64 = (0..=k).map(|i| b.pmf(i).value()).sum();
            let cdf = b.cdf(k).value();
            assert!((manual - cdf).abs() < 1e-6, "k={k} manual={manual} cdf={cdf}");
        }
    }

    #[test]
    fn cdf_boundary() {
        let b = binomial(10, 0.5).unwrap();
        assert_eq!(b.cdf(10).value(), 1.0);
        assert_eq!(b.cdf(999).value(), 1.0); // beyond n is still certain
    }

    #[test]
    fn mean_and_variance_known_values() {
        // Binomial(100, 0.2): mean=20, var=100*0.2*0.8=16
        let b = binomial(100, 0.2).unwrap();
        assert!((b.mean() - 20.0).abs() < 1e-9);
        assert!((b.variance() - 16.0).abs() < 1e-9);
    }

    #[test]
    fn large_n_does_not_overflow() {
        // The exact scenario that breaks a naive n!/(k!(n-k)!) computation.
        let b = binomial(1_000_000, 0.00037).unwrap();
        let p37 = b.pmf(370);
        assert!(p37.value().is_finite());
        assert!(p37.value() > 0.0);
        assert!((b.mean() - 370.0).abs() < 1e-6);
    }

    #[test]
    fn degenerate_p_zero_and_one() {
        let never = binomial(5, 0.0).unwrap();
        assert_eq!(never.pmf(0).value(), 1.0);
        assert_eq!(never.pmf(1).value(), 0.0);
        assert_eq!(never.cdf(0).value(), 1.0);

        let always = binomial(5, 1.0).unwrap();
        assert_eq!(always.pmf(5).value(), 1.0);
        assert_eq!(always.pmf(4).value(), 0.0);
        assert_eq!(always.cdf(4).value(), 0.0);
        assert_eq!(always.cdf(5).value(), 1.0);
    }
}
