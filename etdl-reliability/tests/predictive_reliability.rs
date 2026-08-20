//! Integration tests for the Predictive Reliability Supplement.
//!
//! Covers the task's own "Reference Test" requirement (constant-rate
//! survival at a known mission time), Weibull edge cases, quantiles/mean
//! lifetime, the extrapolation flag, censoring construction, tree +
//! predictive composition, and — most importantly — the full
//! predict -> observe -> calibrate -> new artifact -> new prediction loop,
//! asserting the *original* artifact and its predictions are never
//! mutated by any step.

use std::collections::BTreeMap;

use etdl_reliability::calibration::{calibrate, CalibrationConfig, CalibrationStatus};
use etdl_reliability::observations::AggregateObservation;
use etdl_reliability::predictive::calibration_adapter::exponential_model_from_artifact;
use etdl_reliability::predictive::censoring::{CensoredObservation, CensoringKind};
use etdl_reliability::predictive::models::{ExponentialModel, TimeToFailureModel, WeibullModel};
use etdl_reliability::predictive::tree::evaluate_failure_probability_at;
use etdl_reliability::predictive::{
    MissionTime, ModelDescriptor, PredictiveProvenance, PredictiveQuantity, PredictiveResult,
};
use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::estimate::ProbabilityEstimate;
use etdl_reliability_core::{ProbabilityMetric, ProbabilityState};
use etdl_tree_core::{GateKind, Tree, TreeNode};

// ---------------------------------------------------------------------
// Reference test (task's own acceptance criterion): lambda = 0.001/hour,
// t = 100 hours => R(t) = exp(-0.1).
// ---------------------------------------------------------------------

#[test]
fn exponential_reference_test_lambda_0_001_t_100() {
    let model = ExponentialModel::new(0.001).unwrap();
    let expected_survival = (-0.1f64).exp();

    assert!((model.survival(100.0) - expected_survival).abs() < 1e-12);
    assert!((model.failure_probability(100.0) - (1.0 - expected_survival)).abs() < 1e-12);
    assert!((model.hazard(100.0) - 0.001).abs() < 1e-15);
    assert!((model.cumulative_hazard(100.0) - 0.1).abs() < 1e-12);
    assert!((model.mean().unwrap() - 1000.0).abs() < 1e-9);
}

#[test]
fn exponential_survival_at_zero_is_one() {
    let model = ExponentialModel::new(0.05).unwrap();
    assert_eq!(model.survival(0.0), 1.0);
    assert_eq!(model.cumulative_hazard(0.0), 0.0);
    assert_eq!(model.failure_probability(0.0), 0.0);
}

#[test]
fn exponential_quantile_inverts_survival() {
    let model = ExponentialModel::new(0.01).unwrap();
    for q in [0.1, 0.5, 0.9, 0.99] {
        let t = model.quantile(q).unwrap();
        assert!((model.failure_probability(t) - q).abs() < 1e-9, "q={q}");
    }
}

// ---------------------------------------------------------------------
// Weibull: aging (shape > 1), infant mortality (shape < 1), and the
// shape == 1 equivalence to a constant-rate exponential.
// ---------------------------------------------------------------------

#[test]
fn weibull_shape_one_matches_exponential() {
    let lambda = 0.002;
    let weibull = WeibullModel::new(1.0, 1.0 / lambda).unwrap();
    let exponential = ExponentialModel::new(lambda).unwrap();

    for t in [0.0, 1.0, 50.0, 500.0, 5000.0] {
        assert!(
            (weibull.survival(t) - exponential.survival(t)).abs() < 1e-9,
            "t={t}"
        );
        assert!(
            (weibull.hazard(t.max(0.001)) - exponential.hazard(t)).abs() < 1e-6,
            "t={t}"
        );
    }
    assert!((weibull.mean().unwrap() - exponential.mean().unwrap()).abs() < 1e-6);
}

#[test]
fn weibull_aging_hazard_increases_with_shape_above_one() {
    let model = WeibullModel::new(2.5, 1000.0).unwrap();
    let h_early = model.hazard(10.0);
    let h_late = model.hazard(900.0);
    assert!(
        h_late > h_early,
        "expected increasing hazard for shape > 1: h(10)={h_early}, h(900)={h_late}"
    );
}

#[test]
fn weibull_infant_mortality_hazard_decreases_with_shape_below_one() {
    let model = WeibullModel::new(0.5, 1000.0).unwrap();
    let h_early = model.hazard(10.0);
    let h_late = model.hazard(900.0);
    assert!(
        h_late < h_early,
        "expected decreasing hazard for shape < 1: h(10)={h_early}, h(900)={h_late}"
    );
}

#[test]
fn weibull_zero_survival_at_large_t_does_not_panic_or_go_negative() {
    let model = WeibullModel::new(3.0, 10.0).unwrap();
    let s = model.survival(1_000_000.0);
    assert!(s >= 0.0 && s.is_finite());
    assert!(s < 1e-30);
}

#[test]
fn weibull_quantile_inverts_survival() {
    let model = WeibullModel::new(1.8, 300.0).unwrap();
    for q in [0.1, 0.5, 0.9] {
        let t = model.quantile(q).unwrap();
        assert!((model.failure_probability(t) - q).abs() < 1e-6, "q={q}");
    }
}

#[test]
fn quantile_rejects_out_of_range_q() {
    let model = ExponentialModel::new(0.01).unwrap();
    assert!(model.quantile(0.0).is_none());
    assert!(model.quantile(1.0).is_none());
    assert!(model.quantile(-0.1).is_none());
}

// ---------------------------------------------------------------------
// PredictiveResult: extrapolation flag.
// ---------------------------------------------------------------------

#[test]
fn extrapolation_flag_is_set_only_when_a_range_was_declared_and_exceeded() {
    let model = ExponentialModel::new(0.001).unwrap();
    let mut descriptor = model.descriptor();
    descriptor.valid_range = Some((0.0, 500.0));

    let within = PredictiveResult::new(
        "pump-fails",
        PredictiveQuantity::FailureProbability,
        model.failure_probability(100.0),
        MissionTime::new(100.0, "hour").unwrap(),
        descriptor.clone(),
        vec![],
        PredictiveProvenance::new(),
    );
    assert!(!within.extrapolated);

    let beyond = PredictiveResult::new(
        "pump-fails",
        PredictiveQuantity::FailureProbability,
        model.failure_probability(1000.0),
        MissionTime::new(1000.0, "hour").unwrap(),
        descriptor,
        vec![],
        PredictiveProvenance::new(),
    );
    assert!(beyond.extrapolated);

    let no_range_declared = PredictiveResult::new(
        "pump-fails",
        PredictiveQuantity::FailureProbability,
        model.failure_probability(1_000_000.0),
        MissionTime::new(1_000_000.0, "hour").unwrap(),
        ModelDescriptor {
            family: "exponential".to_string(),
            parameters: BTreeMap::new(),
            assumptions: vec![],
            valid_range: None,
        },
        vec![],
        PredictiveProvenance::new(),
    );
    assert!(
        !no_range_declared.extrapolated,
        "no declared range must not be treated as evidence of extrapolation"
    );
}

// ---------------------------------------------------------------------
// Censoring: construction only (no fitting in 1.0).
// ---------------------------------------------------------------------

#[test]
fn censored_observations_round_trip_through_serde() {
    let right = CensoredObservation::right(1000.0).unwrap();
    let json = serde_json::to_string(&right).unwrap();
    let back: CensoredObservation = serde_json::from_str(&json).unwrap();
    assert_eq!(right, back);

    let interval = CensoredObservation::interval(10.0, 20.0).unwrap();
    assert_eq!(interval.time, 20.0);
    assert_eq!(
        interval.censoring,
        CensoringKind::Interval {
            lower: 10.0,
            upper: 20.0
        }
    );
}

// ---------------------------------------------------------------------
// Tree + predictive composition (reuses tree_adapter unchanged).
// ---------------------------------------------------------------------

#[test]
fn tree_of_time_to_failure_models_and_gate() {
    let pump = ExponentialModel::new(0.0005).unwrap();
    let valve = WeibullModel::new(2.0, 2000.0).unwrap();

    let tree = Tree::new("pump-valve", "1", "Top")
        .with_node("Pump", TreeNode::leaf())
        .with_node("Valve", TreeNode::leaf())
        .with_node(
            "Top",
            TreeNode::gate(GateKind::And, vec!["Pump".to_string(), "Valve".to_string()]),
        );

    let leaf_models: BTreeMap<String, &dyn TimeToFailureModel> = BTreeMap::from([
        ("Pump".to_string(), &pump as &dyn TimeToFailureModel),
        ("Valve".to_string(), &valve as &dyn TimeToFailureModel),
    ]);

    let result = evaluate_failure_probability_at(&tree, &leaf_models, 500.0).unwrap();

    let expected = pump.failure_probability(500.0) * valve.failure_probability(500.0);
    assert!((result.value() - expected).abs() < 1e-9);
}

// ---------------------------------------------------------------------
// Full loop: predict -> observe -> calibrate -> new artifact -> new
// prediction. The original artifact, model, and prediction must never be
// mutated by any step.
// ---------------------------------------------------------------------

#[test]
fn calibration_loop_never_mutates_the_original_artifact_or_prediction() {
    // 1. Original calibrated artifact: failure rate 0.001/hour.
    let mut original_artifact = ReliabilityArtifact::new("pump-artifact");
    original_artifact.version = Some("1.0.0".to_string());
    let estimate = ProbabilityEstimate::new("pump-fails", ProbabilityState::Estimated, 0.001);
    let mut estimate = estimate;
    estimate.metric = ProbabilityMetric::FailureRate;
    original_artifact
        .estimates
        .insert("pump-fails".to_string(), estimate);

    let original_artifact_snapshot_json = serde_json::to_string(&original_artifact).unwrap();

    // 2. Build a predictive model from the original artifact and predict
    //    at t=100h.
    let (original_model, provenance) =
        exponential_model_from_artifact(&original_artifact, "pump-fails").unwrap();
    let original_prediction = PredictiveResult::new(
        "pump-fails",
        PredictiveQuantity::FailureProbability,
        original_model.failure_probability(100.0),
        MissionTime::new(100.0, "hour").unwrap(),
        original_model.descriptor(),
        vec![],
        provenance,
    );
    let original_prediction_snapshot = original_prediction.clone();

    // 3. Observe: many more failures happened than the model predicted
    //    (simulating a genuine drift).
    let observed = AggregateObservation {
        id: Some("obs-1".to_string()),
        failure_mode: "pump-fails".to_string(),
        exposure: 1000,
        failures: 40,
        exposure_unit: etdl_reliability_core::TimeBasis::PerHour,
        conditions: vec![],
        interval: None,
        source: Some("field-data".to_string()),
        version: None,
    };

    // 4. Calibrate against the ORIGINAL artifact. This must not mutate it.
    let calibration = calibrate(
        &original_artifact,
        "pump-fails",
        &observed,
        vec![],
        &CalibrationConfig::default(),
    )
    .unwrap();

    // FailureRate is not probability-like, so today's binomial calibration
    // reports this comparison as unsupported rather than guessing at a
    // rate-based statistical test — an honest, documented gap, not a
    // silent wrong answer.
    assert_eq!(calibration.status, CalibrationStatus::UnsupportedComparison);
    assert_eq!(
        serde_json::to_string(&original_artifact).unwrap(),
        original_artifact_snapshot_json
    );

    // 5. A human reviews the drift out-of-band and publishes a NEW
    //    artifact with a revised rate. This is a brand-new artifact, not
    //    an edit — the discipline this whole crate exists to preserve.
    let mut revised_artifact = ReliabilityArtifact::new("pump-artifact");
    revised_artifact.version = Some("1.1.0".to_string());
    let mut revised_estimate =
        ProbabilityEstimate::new("pump-fails", ProbabilityState::Estimated, 0.04);
    revised_estimate.metric = ProbabilityMetric::FailureRate;
    revised_artifact
        .estimates
        .insert("pump-fails".to_string(), revised_estimate);

    let (revised_model, _revised_provenance) =
        exponential_model_from_artifact(&revised_artifact, "pump-fails").unwrap();
    let revised_prediction_value = revised_model.failure_probability(100.0);

    // 6. The original artifact, model, and prediction are byte-for-byte
    //    unchanged; the new prediction is a materially different value.
    assert_eq!(
        serde_json::to_string(&original_artifact).unwrap(),
        original_artifact_snapshot_json
    );
    assert_eq!(original_prediction, original_prediction_snapshot);
    assert!((original_model.lambda() - 0.001).abs() < 1e-15);
    assert!(
        (revised_prediction_value - original_prediction.value).abs() > 0.1,
        "expected the revised prediction to materially differ from the original"
    );
}
