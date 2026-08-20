//! Numerical and pipeline tests for the evidence → estimation → artifact path.

#![allow(clippy::field_reassign_with_default)]

use etdl_reliability::analysis::{
    beta_credible_interval, beta_quantile, builtin_estimators, regularized_beta, wilson,
    EmpiricalBinomialEstimator, EstimationConfig, ExponentialRateEstimator, ReliabilityEstimator,
};
use etdl_reliability::observations::{AggregateObservation, ObservationError};
use etdl_reliability::probability::TimeBasis;
use etdl_reliability_core::estimate::{ProbabilityEstimate, ProbabilityState};
use etdl_reliability_core::uncertainty::Uncertainty;

fn obs(failures: u64, exposure: u64) -> AggregateObservation {
    AggregateObservation {
        id: None,
        failure_mode: "failure.gateway.timeout".into(),
        exposure,
        failures,
        exposure_unit: TimeBasis::PerRequest,
        conditions: vec!["production".into()],
        interval: None,
        source: Some("prod-obs".into()),
        version: Some("1".into()),
    }
}

// ---- known values ---------------------------------------------------------

#[test]
fn empirical_known_values() {
    let e = EmpiricalBinomialEstimator::new()
        .estimate(&obs(37, 100_000), &EstimationConfig::default())
        .unwrap();
    assert!((e.value.unwrap() - 0.00037).abs() < 1e-15);

    let e = EmpiricalBinomialEstimator::new()
        .estimate(&obs(12, 10_000), &EstimationConfig::default())
        .unwrap();
    assert!((e.value.unwrap() - 0.0012).abs() < 1e-15);

    let e = EmpiricalBinomialEstimator::new()
        .estimate(&obs(2, 720), &EstimationConfig::default())
        .unwrap();
    assert!((e.value.unwrap() - 2.0 / 720.0).abs() < 1e-15);
}

#[test]
fn wilson_known_values() {
    // Wilson interval for p=0.5, n=10 at 95% (z=1.96) is [0.2366, 0.7634].
    let (lo, hi) = wilson(0.5, 10.0, 1.96);
    assert!((lo - 0.2366).abs() < 0.001, "lo={lo}");
    assert!((hi - 0.7634).abs() < 0.001, "hi={hi}");
    // Bounds stay in [0,1].
    let (lo, hi) = wilson(0.0, 100.0, 1.96);
    assert!(lo >= 0.0 && hi <= 1.0);
}

#[test]
fn beta_quantile_known_values() {
    // Beta(2,2) median = 0.5.
    assert!((beta_quantile(2.0, 2.0, 0.5) - 0.5).abs() < 1e-3);
    // Beta(3,9) median ≈ 0.2358.
    assert!((beta_quantile(3.0, 9.0, 0.5) - 0.2358).abs() < 1e-3);
    // Regularized beta: I_{0.5}(1,1) = 0.5.
    assert!((regularized_beta(0.5, 1.0, 1.0) - 0.5).abs() < 1e-9);
}

#[test]
fn beta_credible_interval_brackets_point() {
    let (lo, hi) = beta_credible_interval(3.0, 9.0, 0.95);
    assert!(lo < 0.25 && hi > 0.25);
    assert!(lo >= 0.0 && hi <= 1.0 && lo <= hi);
}

#[test]
fn exponential_known_value() {
    let mut cfg = EstimationConfig::default();
    cfg.mission_time = Some(10.0);
    // λ = 2/10 per hour, t=10h → P = 1 - e^{-2} ≈ 0.864665.
    let e = ExponentialRateEstimator
        .estimate(&obs(2, 10), &cfg)
        .unwrap();
    let expected = -(-2.0f64).exp_m1();
    assert!((e.value.unwrap() - expected).abs() < 1e-12);
    assert!((e.value.unwrap() - 0.864665).abs() < 1e-4);
}

// ---- numerical edge cases -------------------------------------------------

#[test]
fn zero_failures_gives_zero_point_and_valid_interval() {
    let e = EmpiricalBinomialEstimator::new()
        .estimate(&obs(0, 1000), &EstimationConfig::default())
        .unwrap();
    assert_eq!(e.value, Some(0.0));
    let Uncertainty::ConfidenceInterval(ci) = e.uncertainty.unwrap() else {
        panic!("expected interval");
    };
    assert!(ci.lower >= 0.0 && ci.upper <= 1.0);
}

#[test]
fn all_failures_gives_one_point_and_valid_interval() {
    let e = EmpiricalBinomialEstimator::new()
        .estimate(&obs(1000, 1000), &EstimationConfig::default())
        .unwrap();
    assert!((e.value.unwrap() - 1.0).abs() < 1e-12);
    let Uncertainty::ConfidenceInterval(ci) = e.uncertainty.unwrap() else {
        panic!("expected interval");
    };
    assert!(ci.lower >= 0.0 && ci.upper <= 1.0);
}

#[test]
fn large_n_small_probability_stays_finite() {
    let e = EmpiricalBinomialEstimator::new()
        .estimate(&obs(1, 10_000_000), &EstimationConfig::default())
        .unwrap();
    let v = e.value.unwrap();
    assert!(v.is_finite() && v > 0.0 && v < 1.0);
    let Uncertainty::ConfidenceInterval(ci) = e.uncertainty.unwrap() else {
        panic!("expected interval");
    };
    assert!(ci.lower.is_finite() && ci.upper.is_finite());
    assert!((ci.lower - 1e-7).abs() < 1e-7);
}

#[test]
fn small_exposure_is_valid_but_zero_exposure_rejected() {
    assert!(EmpiricalBinomialEstimator::new()
        .estimate(&obs(0, 1), &EstimationConfig::default())
        .is_ok());
    let err = EmpiricalBinomialEstimator::new()
        .estimate(&obs(0, 0), &EstimationConfig::default())
        .unwrap_err();
    assert!(matches!(
        err,
        etdl_reliability::analysis::EstimationError::InvalidObservation(
            ObservationError::NonPositiveExposure(_)
        )
    ));
}

#[test]
fn extreme_lambda_and_large_mission_time() {
    let mut cfg = EstimationConfig::default();
    cfg.mission_time = Some(1e6);
    // λ = 1e-6 per unit, t = 1e6 → P ≈ 1 - e^{-1} ≈ 0.632 (not overflow).
    let e = ExponentialRateEstimator
        .estimate(&obs(1, 1_000_000), &cfg)
        .unwrap();
    let expected = -(-1.0f64).exp_m1();
    assert!((e.value.unwrap() - expected).abs() < 1e-9);
    // Huge mission time saturates at 1.
    cfg.mission_time = Some(1e30);
    let e = ExponentialRateEstimator
        .estimate(&obs(1, 1_000_000), &cfg)
        .unwrap();
    assert!((e.value.unwrap() - 1.0).abs() < 1e-9);
}

#[test]
fn probabilities_near_boundaries_are_clamped_by_wilson() {
    for (f, n) in [(0u64, 1000u64), (1, 1000), (999, 1000), (1000, 1000)] {
        let e = EmpiricalBinomialEstimator::new()
            .estimate(&obs(f, n), &EstimationConfig::default())
            .unwrap();
        let v = e.value.unwrap();
        assert!(v.is_finite() && (0.0..=1.0).contains(&v));
    }
}

// ---- estimator metadata ---------------------------------------------------

#[test]
fn estimator_declares_assumptions_and_version() {
    for est in builtin_estimators() {
        assert!(!est.name().is_empty());
        assert!(!est.version().is_empty());
        assert!(!est.model().is_empty());
        assert!(
            !est.assumptions().is_empty(),
            "{} must declare assumptions",
            est.name()
        );
    }
}

#[test]
fn estimate_preserves_full_metadata() {
    let e = EmpiricalBinomialEstimator::new()
        .estimate(&obs(37, 100_000), &EstimationConfig::default())
        .unwrap();
    assert_eq!(e.state, ProbabilityState::Estimated);
    assert_eq!(e.event, "failure.gateway.timeout");
    assert_eq!(e.conditions, vec!["production"]);
    assert_eq!(e.time_basis, Some(TimeBasis::PerRequest));
    assert_eq!(e.method.as_deref(), Some("binomial/empirical/binomial"));
    assert_eq!(
        e.provenance.as_ref().unwrap().model.as_deref(),
        Some("binomial")
    );
    assert_eq!(
        e.provenance.as_ref().unwrap().dataset.as_deref(),
        Some("prod-obs")
    );
}

#[test]
fn intervals_are_explicitly_frequentist_or_bayesian() {
    use etdl_reliability::analysis::StatisticalInterpretation;
    assert_eq!(
        EmpiricalBinomialEstimator::new().interpretation(),
        StatisticalInterpretation::Frequentist
    );
    assert_eq!(
        etdl_reliability::analysis::BetaBinomialEstimator.interpretation(),
        StatisticalInterpretation::Bayesian
    );
}

// ---- multiple conditions / versions / conflicts ---------------------------

fn est_with(fm: &str, cond: &str, value: f64, version: &str) -> ProbabilityEstimate {
    let mut e = etdl_reliability_core::artifact::declared(fm, value);
    e.conditions = vec![cond.to_string()];
    e.version = Some(version.to_string());
    e
}

#[test]
fn multi_condition_estimates_are_not_overwritten() {
    let mut a = etdl_reliability_core::artifact::ReliabilityArtifact::new("svc");
    a.add(est_with(
        "failure.gateway.timeout",
        "production",
        0.002,
        "1",
    ))
    .unwrap();
    a.add(est_with("failure.gateway.timeout", "high-load", 0.008, "1"))
        .unwrap();
    a.add(est_with(
        "failure.gateway.timeout",
        "region=us-east",
        0.001,
        "1",
    ))
    .unwrap();
    assert_eq!(a.all_for("failure.gateway.timeout").len(), 3);

    let prod = a
        .select("failure.gateway.timeout", &["production".to_string()], None)
        .unwrap();
    assert_eq!(prod.value, Some(0.002));
    let hi = a
        .select("failure.gateway.timeout", &["high-load".to_string()], None)
        .unwrap();
    assert_eq!(hi.value, Some(0.008));
    let us = a
        .select(
            "failure.gateway.timeout",
            &["region=us-east".to_string()],
            None,
        )
        .unwrap();
    assert_eq!(us.value, Some(0.001));
}

#[test]
fn multiple_versions_coexist_and_select_deterministically() {
    let mut a = etdl_reliability_core::artifact::ReliabilityArtifact::new("svc");
    let mut v1 = est_with("failure.x", "production", 0.01, "1");
    v1.conditions = vec!["production".into()];
    a.add(v1).unwrap();
    let mut v2 = est_with("failure.x", "production", 0.03, "2");
    v2.conditions = vec!["production".into()];
    a.add(v2).unwrap();
    assert_eq!(a.all_for("failure.x").len(), 2);

    use etdl_reliability::selection::{select_estimate, ConflictPolicy, EstimateKey};
    let key = EstimateKey {
        failure_mode: "failure.x".into(),
        metric: "Probability".into(),
        conditions: vec!["production".into()],
        population: None,
    };
    let e1 = select_estimate(
        &[("a", &a)],
        &key,
        &ConflictPolicy::ExplicitVersion("1".into()),
    )
    .unwrap();
    assert_eq!(e1.value, Some(0.01));
    let e2 = select_estimate(
        &[("a", &a)],
        &key,
        &ConflictPolicy::ExplicitVersion("2".into()),
    )
    .unwrap();
    assert_eq!(e2.value, Some(0.03));
}

#[test]
fn conflicting_artifacts_are_detected() {
    let mut a1 = etdl_reliability_core::artifact::ReliabilityArtifact::new("a1");
    a1.add(est_with("failure.x", "production", 0.01, "1"))
        .unwrap();
    let mut a2 = etdl_reliability_core::artifact::ReliabilityArtifact::new("a2");
    a2.add(est_with("failure.x", "production", 0.03, "1"))
        .unwrap();

    use etdl_reliability::selection::{
        detect_conflicts, select_estimate, ConflictPolicy, EstimateKey,
    };
    let conflicts = detect_conflicts(&[("a1", &a1), ("a2", &a2)], "failure.x");
    assert_eq!(conflicts.len(), 1);

    let key = EstimateKey {
        failure_mode: "failure.x".into(),
        metric: "Probability".into(),
        conditions: vec!["production".into()],
        population: None,
    };
    let err =
        select_estimate(&[("a1", &a1), ("a2", &a2)], &key, &ConflictPolicy::Error).unwrap_err();
    assert!(matches!(
        err,
        etdl_reliability::selection::SelectionError::Conflict(_)
    ));
}
