//! [`Bernoulli`]: a single trial with success probability `p`.

use serde::{Deserialize, Serialize};

use crate::probability::{Probability, ProbabilityError};

/// `X ~ Bernoulli(p)`: `P(X=1) = p`, `P(X=0) = 1 - p`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bernoulli {
    p: Probability,
}

impl Bernoulli {
    pub fn new(p: Probability) -> Self {
        Bernoulli { p }
    }

    pub fn p(&self) -> Probability {
        self.p
    }

    /// The probability mass function: `P(X = k)` for `k in {0, 1}`.
    /// `None` for any other `k` — a Bernoulli variable has no other
    /// support.
    pub fn pmf(&self, k: u8) -> Option<Probability> {
        match k {
            1 => Some(self.p),
            0 => Some(Probability::new(1.0 - self.p.value()).expect("1-p in [0,1] since p is")),
            _ => None,
        }
    }

    /// `E[X] = p`.
    pub fn mean(&self) -> f64 {
        self.p.value()
    }

    /// `Var(X) = p(1-p)`.
    pub fn variance(&self) -> f64 {
        let p = self.p.value();
        p * (1.0 - p)
    }
}

/// Convenience: `Bernoulli::from_f64(0.3)` instead of constructing a
/// [`Probability`] first. Returns the same [`ProbabilityError`] `p` itself
/// would produce.
pub fn bernoulli(p: f64) -> Result<Bernoulli, ProbabilityError> {
    Ok(Bernoulli::new(Probability::new(p)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmf_known_values() {
        let b = bernoulli(0.3).unwrap();
        assert!((b.pmf(1).unwrap().value() - 0.3).abs() < 1e-12);
        assert!((b.pmf(0).unwrap().value() - 0.7).abs() < 1e-12);
        assert_eq!(b.pmf(2), None);
    }

    #[test]
    fn mean_and_variance_known_values() {
        let b = bernoulli(0.3).unwrap();
        assert!((b.mean() - 0.3).abs() < 1e-12);
        // Var = 0.3 * 0.7 = 0.21
        assert!((b.variance() - 0.21).abs() < 1e-12);
    }

    #[test]
    fn boundary_p_zero_and_one() {
        let never = bernoulli(0.0).unwrap();
        assert_eq!(never.pmf(1).unwrap().value(), 0.0);
        assert_eq!(never.pmf(0).unwrap().value(), 1.0);
        assert_eq!(never.variance(), 0.0);

        let always = bernoulli(1.0).unwrap();
        assert_eq!(always.pmf(1).unwrap().value(), 1.0);
        assert_eq!(always.pmf(0).unwrap().value(), 0.0);
        assert_eq!(always.variance(), 0.0);
    }

    #[test]
    fn invalid_p_is_rejected() {
        assert!(bernoulli(-0.1).is_err());
        assert!(bernoulli(1.1).is_err());
    }
}
