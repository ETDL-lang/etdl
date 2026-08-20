//! Simple, well-defined statistical estimators.
//!
//! These are the reference estimators for the analysis path. They are optional;
//! the deterministic ETDL compilation path does not use them.

use crate::evidence::Evidence;

/// Empirical estimate of a failure probability from evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct EmpiricalEstimate {
    pub failures: u64,
    pub total: u64,
    pub point: Option<f64>,
    /// Wilson-score 95% confidence interval (or None if no observations).
    pub lower: Option<f64>,
    pub upper: Option<f64>,
}

impl EmpiricalEstimate {
    pub fn from_evidence(evidence: &Evidence) -> Self {
        let total = evidence.total();
        if total == 0 {
            return EmpiricalEstimate {
                failures: evidence.failures,
                total,
                point: None,
                lower: None,
                upper: None,
            };
        }
        let p = evidence.failures as f64 / total as f64;
        let (lower, upper) = wilson(p, total as f64, 1.96);
        EmpiricalEstimate {
            failures: evidence.failures,
            total,
            point: Some(p),
            lower: Some(lower),
            upper: Some(upper),
        }
    }
}

/// Wilson score interval (normal approximation at z = 1.96 for 95%).
pub fn wilson(p: f64, n: f64, z: f64) -> (f64, f64) {
    let denom = 1.0 + z * z / n;
    let centre = (p + z * z / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    (centre - half, centre + half)
}

/// Beta-Binomial Bayesian estimate: posterior from a prior + observations.
#[derive(Debug, Clone, PartialEq)]
pub struct BetaBinomial {
    pub alpha_prior: f64,
    pub beta_prior: f64,
    pub observations: u64,
    pub failures: u64,
    pub alpha_posterior: f64,
    pub beta_posterior: f64,
    pub point: f64,
    /// Equal-tailed 95% credible interval (beta quantile approximation).
    pub lower: f64,
    pub upper: f64,
}

impl BetaBinomial {
    /// Uniform prior by default: Beta(1,1).
    pub fn uniform() -> Self {
        Self::new(1.0, 1.0)
    }

    pub fn new(alpha_prior: f64, beta_prior: f64) -> Self {
        BetaBinomial {
            alpha_prior,
            beta_prior,
            observations: 0,
            failures: 0,
            alpha_posterior: alpha_prior,
            beta_posterior: beta_prior,
            point: alpha_prior / (alpha_prior + beta_prior),
            lower: 0.0,
            upper: 1.0,
        }
    }

    pub fn add_observation(&mut self, failed: bool) {
        self.observations += 1;
        if failed {
            self.failures += 1;
            self.alpha_posterior += 1.0;
        } else {
            self.beta_posterior += 1.0;
        }
        self.update();
    }

    pub fn update(&mut self) {
        self.point = self.alpha_posterior / (self.alpha_posterior + self.beta_posterior);
        // Approximate equal-tailed 95% credible interval via normal
        // approximation to the posterior beta.
        let v = (self.alpha_posterior * self.beta_posterior)
            / ((self.alpha_posterior + self.beta_posterior).powi(2)
                * (self.alpha_posterior + self.beta_posterior + 1.0));
        let sd = v.sqrt();
        self.lower = (self.point - 1.96 * sd).max(0.0);
        self.upper = (self.point + 1.96 * sd).min(1.0);
    }
}

/// Exponential constant-failure-rate model: `P = 1 - e^(-λt)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExponentialFailure {
    pub failure_rate: f64,
    pub mission_time: f64,
}

impl ExponentialFailure {
    pub fn probability(&self) -> f64 {
        1.0 - (-self.failure_rate * self.mission_time).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empirical_wilson() {
        let mut e = Evidence::new("failure.x");
        for _ in 0..5 {
            let _ = e.append(
                crate::observation::ReliabilityObservation::new("o", "failure.x", "t")
                    .outcome_failed(),
            );
        }
        for _ in 0..95 {
            let _ = e.append(
                crate::observation::ReliabilityObservation::new("o", "failure.x", "t")
                    .outcome_succeeded(),
            );
        }
        let est = EmpiricalEstimate::from_evidence(&e);
        assert!((est.point.unwrap() - 0.05).abs() < 1e-9);
        assert!(est.lower.unwrap() < 0.05 && est.upper.unwrap() > 0.05);
    }

    #[test]
    fn beta_binomial_updates() {
        let mut bb = BetaBinomial::uniform();
        for _ in 0..2 {
            bb.add_observation(true);
        }
        for _ in 0..8 {
            bb.add_observation(false);
        }
        // Posterior Beta(3,9): mean = 3/12 = 0.25
        assert!((bb.point - 0.25).abs() < 1e-9);
        assert!(bb.lower < bb.point && bb.upper > bb.point);
    }

    #[test]
    fn exponential_model() {
        let m = ExponentialFailure {
            failure_rate: 0.1,
            mission_time: 10.0,
        };
        assert!((m.probability() - (1.0 - (-1.0f64).exp())).abs() < 1e-9);
    }
}
