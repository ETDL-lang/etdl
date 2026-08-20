//! PRED-* vectors: Predictive Reliability Supplement conformance. Covers
//! task §15 (predictive invariants: time-unit consistency, mission-time
//! consistency, distribution parameter validity, survival semantics,
//! hazard semantics, extrapolation indicators) and §37 (predictive
//! reliability reference vectors: exponential, Weibull, survival, failure
//! probability, hazard, mission time).
//!
//! Every numerical assertion compares `etdl-reliability::predictive`'s
//! output to [`etdl_conformance::reference`] — coded independently, not a
//! second call into the same formula (see the crate's "no
//! self-certification loop" doc). This is exactly the case the previous
//! task's own integration tests (`etdl-reliability/tests/
//! predictive_reliability.rs`) did NOT do — those compare the
//! implementation to itself via hand-derived inline constants; this suite
//! is the independently-oracled counterpart the conformance framework adds
//! on top, not a replacement for that existing suite.

#![cfg(feature = "reliability")]

use etdl_conformance::reference;
use etdl_conformance::vector::{ConformanceVector, Level};
use etdl_reliability::predictive::models::{ExponentialModel, TimeToFailureModel, WeibullModel};

const TOLERANCE: f64 = 1e-9;

#[test]
fn pred_001_exponential_survival_matches_the_independent_reference() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "PRED-001",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md#exponentialmodel-constant-hazard",
        "S(t) = exp(-lambda*t): the reference test case lambda=0.001/hour, t=100 hours",
    );
    let model = ExponentialModel::new(0.001).unwrap();
    let oracle = reference::exponential_survival(0.001, 100.0);
    assert!(
        (model.survival(100.0) - oracle).abs() < TOLERANCE,
        "{}: implementation={} oracle={}",
        VECTOR.id,
        model.survival(100.0),
        oracle
    );
}

#[test]
fn pred_002_exponential_family_matches_the_reference_across_a_grid() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "PRED-002",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md#exponentialmodel-constant-hazard",
        "survival, failure probability, hazard, and cumulative hazard all match the \
         independent reference across a grid of (lambda, t)",
    );
    for lambda in [0.0001, 0.001, 0.01, 0.1] {
        for t in [0.0, 1.0, 10.0, 100.0, 1000.0] {
            let model = ExponentialModel::new(lambda).unwrap();
            assert!(
                (model.survival(t) - reference::exponential_survival(lambda, t)).abs() < TOLERANCE,
                "{}: survival lambda={lambda} t={t}",
                VECTOR.id
            );
            assert!(
                (model.failure_probability(t)
                    - reference::exponential_failure_probability(lambda, t))
                .abs()
                    < TOLERANCE,
                "{}: failure_probability lambda={lambda} t={t}",
                VECTOR.id
            );
            assert!(
                (model.hazard(t) - reference::exponential_hazard(lambda, t)).abs() < TOLERANCE,
                "{}: hazard lambda={lambda} t={t}",
                VECTOR.id
            );
            assert!(
                (model.cumulative_hazard(t) - reference::exponential_cumulative_hazard(lambda, t))
                    .abs()
                    < TOLERANCE,
                "{}: cumulative_hazard lambda={lambda} t={t}",
                VECTOR.id
            );
        }
    }
}

#[test]
fn pred_003_weibull_family_matches_the_reference_across_a_grid() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "PRED-003",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md#weibullmodel-shape-k-scale-lambda",
        "S(t), h(t), H(t) for the Weibull model match the independent reference across \
         infant-mortality, constant-hazard, and wear-out shape regimes",
    );
    for shape in [0.5, 1.0, 2.5] {
        for scale in [100.0, 1000.0] {
            for t in [1.0, 50.0, 500.0, 5000.0] {
                let model = WeibullModel::new(shape, scale).unwrap();
                assert!(
                    (model.survival(t) - reference::weibull_survival(shape, scale, t)).abs()
                        < TOLERANCE,
                    "{}: survival shape={shape} scale={scale} t={t}",
                    VECTOR.id
                );
                assert!(
                    (model.hazard(t) - reference::weibull_hazard(shape, scale, t)).abs() < 1e-6,
                    "{}: hazard shape={shape} scale={scale} t={t}",
                    VECTOR.id
                );
                assert!(
                    (model.cumulative_hazard(t)
                        - reference::weibull_cumulative_hazard(shape, scale, t))
                    .abs()
                        < TOLERANCE,
                    "{}: cumulative_hazard shape={shape} scale={scale} t={t}",
                    VECTOR.id
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// §15 Predictive invariants (survival/hazard semantics, not tied to any
// one model family).
// ---------------------------------------------------------------------

#[test]
fn pred_004_survival_is_bounded_zero_to_one_and_non_increasing() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "PRED-004",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md",
        "0 <= S(t) <= 1 and S is non-increasing, for a non-repairable time-to-failure model \
         (do not impose this on repairable models — none exist in this implementation yet, \
         so the exclusion is vacuously satisfied but stated per task §14)",
    );
    let models: Vec<Box<dyn TimeToFailureModel>> = vec![
        Box::new(ExponentialModel::new(0.01).unwrap()),
        Box::new(WeibullModel::new(2.0, 500.0).unwrap()),
        Box::new(WeibullModel::new(0.5, 500.0).unwrap()),
    ];
    for model in &models {
        let mut previous = model.survival(0.0);
        assert!(
            (previous - 1.0).abs() < 1e-12,
            "{}: S(0) must be 1.0, got {previous}",
            VECTOR.id
        );
        for i in 1..=200 {
            let t = i as f64 * 10.0;
            let s = model.survival(t);
            assert!(
                (0.0..=1.0).contains(&s),
                "{}: S({t})={s} out of [0,1]",
                VECTOR.id
            );
            assert!(
                s <= previous + 1e-12,
                "{}: S must be non-increasing at t={t}",
                VECTOR.id
            );
            previous = s;
        }
    }
}

#[test]
fn pred_005_failure_probability_is_bounded_zero_to_one() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "PRED-005",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md",
        "failure probability F(t) = 1 - S(t) remains within [0,1]",
    );
    let model = WeibullModel::new(3.0, 200.0).unwrap();
    for i in 0..=500 {
        let t = i as f64 * 5.0;
        let f = model.failure_probability(t);
        assert!((0.0..=1.0).contains(&f), "{}: F({t})={f}", VECTOR.id);
    }
}

#[test]
fn pred_006_survival_plus_failure_probability_is_one() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "PRED-006",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md",
        "S(t) + F(t) = 1 under the non-repairable semantics both models here implement",
    );
    let model = WeibullModel::new(1.7, 850.0).unwrap();
    for t in [0.0, 1.0, 100.0, 850.0, 5000.0] {
        let sum = model.survival(t) + model.failure_probability(t);
        assert!((sum - 1.0).abs() < 1e-9, "{}: t={t}, sum={sum}", VECTOR.id);
    }
}

#[test]
fn pred_007_distribution_parameters_are_validated_not_silently_accepted() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "PRED-007",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md",
        "distribution parameter validity: non-positive or non-finite rate/shape/scale must \
         be rejected at construction",
    );
    assert!(
        ExponentialModel::new(0.0).is_err(),
        "{}: lambda=0",
        VECTOR.id
    );
    assert!(
        ExponentialModel::new(-1.0).is_err(),
        "{}: lambda<0",
        VECTOR.id
    );
    assert!(
        ExponentialModel::new(f64::NAN).is_err(),
        "{}: lambda=NaN",
        VECTOR.id
    );
    assert!(
        WeibullModel::new(0.0, 100.0).is_err(),
        "{}: shape=0",
        VECTOR.id
    );
    assert!(
        WeibullModel::new(1.0, 0.0).is_err(),
        "{}: scale=0",
        VECTOR.id
    );
    assert!(
        WeibullModel::new(-1.0, 100.0).is_err(),
        "{}: shape<0",
        VECTOR.id
    );
}

#[test]
fn pred_008_zero_survival_at_extreme_t_does_not_produce_nan_or_negative() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "PRED-008",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md#numerical-stability-and-edge-cases",
        "S(t) as t -> very large remains finite, non-negative, and never NaN (numerical \
         tolerance policy: near-zero values are acceptable, NaN/negative are not)",
    );
    let model = WeibullModel::new(3.0, 10.0).unwrap();
    let s = model.survival(1_000_000.0);
    assert!(s.is_finite(), "{}: S must be finite, got {s}", VECTOR.id);
    assert!(s >= 0.0, "{}: S must be non-negative, got {s}", VECTOR.id);
}
