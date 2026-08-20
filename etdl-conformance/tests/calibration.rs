//! CAL-* vectors: Runtime Feedback & Calibration conformance. Covers task
//! §36 (deterministic calibration test vectors: fixed artifact + fixed
//! observations + fixed configuration -> expected calibrated result) and
//! §35 (no mutation of historical artifacts).
//!
//! The p-value vectors compare `etdl-reliability::calibration::
//! binomial_test_two_sided`'s output to [`etdl_conformance::reference::
//! binomial_test_two_sided`] — coded via direct PMF summation, an
//! independent algorithm from the implementation's regularized-incomplete-
//! beta approach (see that module's docs for why this is a legitimate,
//! non-circular oracle for the same "doubling method" statistical
//! definition, not a different test).

#![cfg(feature = "reliability")]

use etdl_conformance::reference;
use etdl_conformance::vector::{ConformanceVector, Level};
use etdl_reliability::calibration::{
    binomial_test_two_sided, calibrate, CalibrationConfig, CalibrationStatus,
};
use etdl_reliability::observations::AggregateObservation;
use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::estimate::{ProbabilityEstimate, ProbabilityState};
use etdl_reliability_core::TimeBasis;

#[test]
fn cal_001_binomial_test_matches_the_independent_reference_across_a_grid() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "CAL-001",
        Level::Supplement,
        "docs/reliability/runtime-feedback-calibration.md",
        "binomial_test_two_sided's p-value matches an independently-coded direct-summation \
         binomial test across a grid of (k, n, p0)",
    );
    for (k, n, p0) in [
        (5u64, 50u64, 0.1),
        (10, 100, 0.1),
        (25, 100, 0.2),
        (0, 30, 0.05),
        (30, 30, 0.9),
        (15, 20, 0.5),
    ] {
        let implementation = binomial_test_two_sided(k, n, p0);
        let oracle = reference::binomial_test_two_sided(k, n, p0);
        assert!(
            (implementation - oracle).abs() < 1e-6,
            "{}: k={k} n={n} p0={p0}: implementation={implementation} oracle={oracle}",
            VECTOR.id
        );
    }
}

/// A fully deterministic calibration vector, per task §36: fixed artifact
/// (predicted probability 0.10), fixed observation (15 failures / 100
/// trials, proportion 0.15), fixed default `CalibrationConfig`. No
/// randomness is involved, so no seed is needed — the vector itself is the
/// fixed input/expected-output pair.
#[test]
fn cal_002_deterministic_calibration_vector_predicted_010_observed_015_of_100() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "CAL-002",
        Level::Runtime,
        "docs/reliability/runtime-feedback-calibration.md",
        "given predicted P=0.10 and 15/100 observed failures under default \
         CalibrationConfig, the expected_failures, difference, ratio, and p-value are \
         exactly the closed-form/oracle values",
    );

    let mut artifact = ReliabilityArtifact::new("svc");
    artifact.version = Some("1.0.0".to_string());
    let mut estimate = ProbabilityEstimate::new("pump-fails", ProbabilityState::Declared, 0.10);
    estimate.time_basis = Some(TimeBasis::PerRequest);
    artifact.add(estimate).unwrap();

    let observed = AggregateObservation {
        id: Some("obs-cal-002".to_string()),
        failure_mode: "pump-fails".to_string(),
        exposure: 100,
        failures: 15,
        exposure_unit: TimeBasis::PerRequest,
        conditions: vec![],
        interval: None,
        source: Some("conformance-vector".to_string()),
        version: None,
    };

    let result = calibrate(
        &artifact,
        "pump-fails",
        &observed,
        vec![],
        &CalibrationConfig::default(),
    )
    .unwrap();

    assert_eq!(
        result.expected_failures,
        Some(10.0),
        "{}: expected_failures = 100 * 0.10",
        VECTOR.id
    );
    let difference = result.difference.expect("supported comparison");
    assert!(
        (difference - 0.05).abs() < 1e-12,
        "{}: difference = 0.15 - 0.10 = 0.05, got {difference}",
        VECTOR.id
    );
    let ratio = result.ratio.expect("supported comparison");
    assert!(
        (ratio - 1.5).abs() < 1e-9,
        "{}: ratio = 0.15 / 0.10 = 1.5, got {ratio}",
        VECTOR.id
    );

    let p_value = result.p_value.expect("supported comparison");
    let oracle_p = reference::binomial_test_two_sided(15, 100, 0.10);
    assert!(
        (p_value - oracle_p).abs() < 1e-6,
        "{}: p_value={p_value} oracle={oracle_p}",
        VECTOR.id
    );

    // At default alpha=0.05/strict_alpha=0.01, this specific deviation
    // (15/100 vs 10% predicted) is expected to land at PotentialDeviation
    // or Consistent, not SignificantDeviation — asserted against the
    // p-value itself rather than hardcoding the enum, so this vector
    // stays correct if the config's thresholds ever change deliberately.
    let config = CalibrationConfig::default();
    if oracle_p < config.strict_alpha {
        assert_eq!(
            result.status,
            CalibrationStatus::SignificantDeviation,
            "{}",
            VECTOR.id
        );
    } else if oracle_p < config.alpha {
        assert_eq!(
            result.status,
            CalibrationStatus::PotentialDeviation,
            "{}",
            VECTOR.id
        );
    } else {
        assert_eq!(
            result.status,
            CalibrationStatus::Consistent,
            "{}",
            VECTOR.id
        );
    }
}

#[test]
fn cal_003_calibration_never_mutates_the_input_artifact() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "CAL-003",
        Level::Runtime,
        "docs/reliability/runtime-feedback-calibration.md",
        "calibrate() takes the artifact by shared reference and must not mutate it \
         (task §35/§19: never mutating historical artifacts)",
    );
    let mut artifact = ReliabilityArtifact::new("svc");
    artifact.version = Some("1.0.0".to_string());
    let mut estimate = ProbabilityEstimate::new("e", ProbabilityState::Declared, 0.2);
    estimate.time_basis = Some(TimeBasis::PerRequest);
    artifact.add(estimate).unwrap();

    let before = serde_json::to_string(&artifact).unwrap();

    let observed = AggregateObservation {
        id: Some("obs".to_string()),
        failure_mode: "e".to_string(),
        exposure: 50,
        failures: 20,
        exposure_unit: TimeBasis::PerRequest,
        conditions: vec![],
        interval: None,
        source: None,
        version: None,
    };
    let _ = calibrate(
        &artifact,
        "e",
        &observed,
        vec![],
        &CalibrationConfig::default(),
    )
    .unwrap();

    let after = serde_json::to_string(&artifact).unwrap();
    assert_eq!(
        before, after,
        "{}: artifact must be byte-for-byte unchanged",
        VECTOR.id
    );
}

#[test]
fn cal_004_insufficient_exposure_below_minimum_is_flagged() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "CAL-004",
        Level::Runtime,
        "docs/reliability/runtime-feedback-calibration.md",
        "exposure below CalibrationConfig::min_exposure (default 20) is flagged \
         InsufficientData, not silently treated as a confident result",
    );
    let mut artifact = ReliabilityArtifact::new("svc");
    artifact.version = Some("1.0.0".to_string());
    let mut estimate = ProbabilityEstimate::new("e", ProbabilityState::Declared, 0.5);
    estimate.time_basis = Some(TimeBasis::PerRequest);
    artifact.add(estimate).unwrap();

    let observed = AggregateObservation {
        id: Some("obs".to_string()),
        failure_mode: "e".to_string(),
        exposure: 5,
        failures: 2,
        exposure_unit: TimeBasis::PerRequest,
        conditions: vec![],
        interval: None,
        source: None,
        version: None,
    };
    let result = calibrate(
        &artifact,
        "e",
        &observed,
        vec![],
        &CalibrationConfig::default(),
    )
    .unwrap();
    assert_eq!(
        result.status,
        CalibrationStatus::InsufficientData,
        "{}",
        VECTOR.id
    );
}
