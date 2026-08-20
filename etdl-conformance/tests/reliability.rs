//! REL-* vectors: Reliability Supplement (`etdl-reliability-core` /
//! `etdl-reliability`) conformance. Covers task §14 (reliability
//! invariants: `0 <= R(t) <= 1`, failure probability within `[0,1]`) at the
//! estimate level — the non-time-indexed reliability estimate, as distinct
//! from Predictive Reliability's `R(t)` (see `predictive_reliability.rs`
//! for the time-indexed version of this invariant).

#![cfg(feature = "reliability")]

use etdl_conformance::vector::{ConformanceVector, Level};
use etdl_reliability_core::estimate::{ProbabilityEstimate, ProbabilityState};
use etdl_reliability_core::{ProbabilityMetric, ReliabilityError};

#[test]
fn rel_001_probability_like_estimate_out_of_range_is_rejected() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "REL-001",
        Level::Supplement,
        "docs/DIAGNOSTICS.md",
        "0 <= P(E) <= 1: a probability-like estimate outside [0,1] must be flagged, not clamped",
    );
    let too_high = ProbabilityEstimate::new("e", ProbabilityState::Estimated, 1.5);
    assert!(
        matches!(
            too_high.validate_value(),
            Some(ReliabilityError::OutOfRange(_))
        ),
        "{}: 1.5 must be rejected",
        VECTOR.id
    );
    let too_low = ProbabilityEstimate::new("e", ProbabilityState::Estimated, -0.1);
    assert!(
        matches!(
            too_low.validate_value(),
            Some(ReliabilityError::OutOfRange(_))
        ),
        "{}: -0.1 must be rejected",
        VECTOR.id
    );
}

#[test]
fn rel_002_non_finite_estimate_value_is_rejected() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "REL-002",
        Level::Supplement,
        "docs/DIAGNOSTICS.md",
        "NaN and infinite estimate values are always invalid, for any metric",
    );
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let estimate = ProbabilityEstimate::new("e", ProbabilityState::Estimated, v);
        assert!(
            matches!(
                estimate.validate_value(),
                Some(ReliabilityError::NonFiniteValue(_))
            ),
            "{}: {v} must be rejected",
            VECTOR.id
        );
    }
}

#[test]
fn rel_003_unknown_never_resolves_to_a_scalar() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "REL-003",
        Level::Supplement,
        "docs/DIAGNOSTICS.md",
        "an Unknown-state estimate must never resolve to a deterministic probability, \
         especially not silently to 0",
    );
    let unknown = ProbabilityEstimate::unknown("e");
    assert!(unknown.is_unknown(), "{}", VECTOR.id);
    assert!(
        matches!(
            unknown.resolved_probability(),
            Err(ReliabilityError::UnknownProbability(_))
        ),
        "{}: resolved_probability must error, not return 0.0",
        VECTOR.id
    );
}

#[test]
fn rel_004_rate_metrics_permit_values_above_one_but_not_negative() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "REL-004",
        Level::Supplement,
        "docs/DIAGNOSTICS.md",
        "a rate metric (e.g. FailureRate) is not bounded to [0,1] like a probability, but \
         must still be non-negative — do not impose non-repairable/probability-only \
         assumptions on every metric",
    );
    let mut high_rate = ProbabilityEstimate::new("e", ProbabilityState::Estimated, 3.5);
    high_rate.metric = ProbabilityMetric::FailureRate;
    assert!(
        high_rate.validate_value().is_none(),
        "{}: a rate of 3.5/hour is valid, not an out-of-[0,1]-range probability",
        VECTOR.id
    );

    let mut negative_rate = ProbabilityEstimate::new("e", ProbabilityState::Estimated, -0.5);
    negative_rate.metric = ProbabilityMetric::FailureRate;
    assert!(
        matches!(
            negative_rate.validate_value(),
            Some(ReliabilityError::OutOfRange(_))
        ),
        "{}: a negative rate must still be rejected",
        VECTOR.id
    );
}

#[test]
fn rel_005_non_probability_metric_does_not_implicitly_resolve() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "REL-005",
        Level::Supplement,
        "docs/DIAGNOSTICS.md",
        "resolved_probability() must refuse to convert a rate (or other non-probability-like \
         metric) into a probability implicitly",
    );
    let mut rate = ProbabilityEstimate::new("e", ProbabilityState::Estimated, 0.01);
    rate.metric = ProbabilityMetric::FailureRate;
    assert!(
        matches!(
            rate.resolved_probability(),
            Err(ReliabilityError::NonProbabilityMetric(_))
        ),
        "{}",
        VECTOR.id
    );
}
