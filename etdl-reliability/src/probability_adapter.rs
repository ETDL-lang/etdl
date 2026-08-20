//! Adapter: `std.probability` (`etdl-probability-core`) -> the reliability
//! domain's own [`ProbabilityEstimate`].
//!
//! This module is intentionally small. `etdl-probability-core::Probability`
//! is a validated `[0,1]` scalar with no provenance, uncertainty, method,
//! or conditions — the reliability domain's own `ProbabilityEstimate`
//! (state, metric, population, time basis, provenance, uncertainty,
//! version) remains completely unchanged and authoritative for anything
//! requiring that richer context. This adapter exists to prove the
//! dependency direction works end to end (`std.probability` beneath
//! reliability, never the reverse — see this crate's `Cargo.toml`, which
//! now depends on `etdl-probability-core`, and `etdl-probability-core`'s own
//! `Cargo.toml`, which depends on nothing reliability-specific), not to
//! replace any existing reliability estimation workflow.
//!
//! # Cross-validation, not a rewrite
//!
//! `etdl-probability-core` and `etdl-reliability::analysis::estimator` each
//! implement their own, independent `log_gamma`/`regularized_beta` (by
//! design — this crate must not depend on `etdl-probability-core` in a way
//! that would require moving or rewriting the existing, tested estimator
//! code, per this task's non-regression rule). The tests below assert the
//! two independent implementations agree on the same mathematical
//! questions to a documented tolerance, so a future consolidation (if ever
//! undertaken) has a correctness baseline — not to silently make one the
//! other.

use etdl_probability_core::Probability;
use etdl_reliability_core::estimate::{ProbabilityEstimate, ProbabilityState};

/// Build a reliability [`ProbabilityEstimate`] from a validated
/// `std.probability` [`Probability`]. The estimate's richer fields
/// (metric, population, time basis, conditions, source, method,
/// uncertainty, provenance, version, status) are left at their defaults,
/// exactly as [`ProbabilityEstimate::new`] already leaves them — this
/// function adds nothing beyond "the value came from a validated
/// `Probability`, not an unchecked `f64`".
pub fn estimate_from_probability(
    event: impl Into<String>,
    state: ProbabilityState,
    p: Probability,
) -> ProbabilityEstimate {
    ProbabilityEstimate::new(event, state, p.value())
}

/// The reverse direction: read an existing reliability estimate's resolved
/// value back out as a validated `std.probability` `Probability`, for code
/// that wants to hand it to `etdl-probability-core`'s composition functions
/// (`complement`, `independent_and`, ...). Fails exactly when
/// `estimate.resolved_probability()` already would (unknown state,
/// non-probability metric, out-of-range value) — this adapter does not
/// weaken that existing validation.
pub fn probability_from_estimate(
    estimate: &ProbabilityEstimate,
) -> Result<Probability, ProbabilityAdapterError> {
    let value = estimate
        .resolved_probability()
        .map_err(ProbabilityAdapterError::Reliability)?;
    Probability::new(value).map_err(ProbabilityAdapterError::Probability)
}

#[derive(Debug, thiserror::Error)]
pub enum ProbabilityAdapterError {
    #[error("estimate is not a resolvable probability: {0}")]
    Reliability(#[from] etdl_reliability_core::ReliabilityError),
    #[error("resolved value is not a valid std.probability Probability: {0}")]
    Probability(#[from] etdl_probability_core::ProbabilityError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use etdl_probability_core::distribution::{Beta, Binomial};
    use etdl_reliability_core::probability::ProbabilityMetric;

    #[test]
    fn estimate_from_probability_preserves_the_value() {
        let p = Probability::new(0.0037).unwrap();
        let e = estimate_from_probability("failure.gateway.timeout", ProbabilityState::Declared, p);
        assert_eq!(e.value, Some(0.0037));
        assert_eq!(e.metric, ProbabilityMetric::Probability);
    }

    #[test]
    fn round_trips_through_both_adapters() {
        let original = Probability::new(0.42).unwrap();
        let estimate =
            estimate_from_probability("x", ProbabilityState::Declared, original);
        let back = probability_from_estimate(&estimate).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn reverse_adapter_rejects_unknown_estimates_same_as_the_existing_api() {
        let unknown = ProbabilityEstimate::unknown("failure.x");
        assert!(matches!(
            probability_from_estimate(&unknown),
            Err(ProbabilityAdapterError::Reliability(_))
        ));
    }

    #[test]
    fn reverse_adapter_rejects_non_probability_metrics_same_as_the_existing_api() {
        let mut e = ProbabilityEstimate::new("x", ProbabilityState::Measured, 2.0);
        e.metric = ProbabilityMetric::FailureRate;
        assert!(matches!(
            probability_from_estimate(&e),
            Err(ProbabilityAdapterError::Reliability(_))
        ));
    }

    /// Cross-validation: `etdl-probability-core`'s independently-implemented
    /// Binomial CDF must agree with the exact binomial test math
    /// `etdl-reliability::calibration` already uses (also built on its own,
    /// independently-implemented `regularized_beta`) for the SAME inputs.
    #[test]
    fn probability_core_binomial_cdf_agrees_with_calibrations_own_math() {
        let n = 10_000u64;
        let k = 37u64;
        let p0 = 0.0024;

        let from_probability_core = Binomial::new(n, Probability::new(p0).unwrap())
            .unwrap()
            .cdf(k)
            .value();

        // The same P(X <= k) computed via etdl-reliability's own
        // independently-implemented regularized_beta, using the identity
        // documented in both crates: P(X<=k) = I_{1-p}(n-k, k+1).
        let from_reliability = crate::analysis::estimator::regularized_beta(
            1.0 - p0,
            (n - k) as f64,
            (k + 1) as f64,
        );

        assert!(
            (from_probability_core - from_reliability).abs() < 1e-9,
            "probability_core={from_probability_core} reliability={from_reliability}"
        );
    }

    /// Cross-validation: the Beta posterior mean `etdl-reliability`'s
    /// Beta-Binomial estimator relies on (posterior Beta(3,9) from a
    /// uniform prior and 2 failures in 10 trials -> mean 0.25, already
    /// asserted by that estimator's own test) matches
    /// `etdl-probability-core::distribution::Beta`'s independently
    /// implemented mean formula for the same parameters.
    #[test]
    fn probability_core_beta_mean_agrees_with_reliabilitys_beta_binomial_posterior() {
        let posterior = Beta::new(1.0 + 2.0, 1.0 + 8.0).unwrap();
        assert!((posterior.mean() - 0.25).abs() < 1e-12);
    }
}
