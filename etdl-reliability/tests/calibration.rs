//! End-to-end tests for runtime feedback & calibration, matching the worked
//! example in `examples/reliability-runtime-feedback/`. Every number here is
//! reproduced in that example's README so the documentation stays honest.

use etdl_reliability::calibration::{calibrate, CalibrationConfig, CalibrationStatus};
use etdl_reliability::dataset::{aggregate_across, ObservationDataset};
use etdl_reliability_core::artifact::declared;
use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::probability::TimeBasis;

const EVENT: &str = "failure.gateway.timeout";

fn artifact(value: f64) -> ReliabilityArtifact {
    let mut a = ReliabilityArtifact::new("payment-gateway");
    let mut e = declared(EVENT, value);
    e.time_basis = Some(TimeBasis::PerRequest);
    a.add(e).unwrap();
    a
}

fn week_dataset(id: &str, failures: u64, exposure: u64) -> ObservationDataset {
    let mut ds = ObservationDataset::new(id, "1");
    ds.observations.push(etdl_reliability::observations::AggregateObservation {
        id: Some(format!("{id}-obs-1")),
        failure_mode: EVENT.to_string(),
        exposure,
        failures,
        exposure_unit: TimeBasis::PerRequest,
        conditions: vec![],
        interval: None,
        source: Some("gateway-service JSONL export".to_string()),
        version: None,
    });
    ds
}

#[test]
fn stale_artifact_is_flagged_as_drift() {
    let stale = artifact(0.01);
    let week1 = week_dataset("prod-us-east-week-1", 110, 50_000);
    let week2 = week_dataset("prod-us-east-week-2", 120, 50_000);

    let aggregated = aggregate_across(&[&week1, &week2], EVENT).unwrap();
    assert_eq!(aggregated.observation.exposure, 100_000);
    assert_eq!(aggregated.observation.failures, 230);

    let result = calibrate(
        &stale,
        EVENT,
        &aggregated.observation,
        aggregated.provenance.source_datasets.clone(),
        &CalibrationConfig::default(),
    )
    .unwrap();

    assert_eq!(result.status, CalibrationStatus::SignificantDeviation);
    assert!(result.is_drift());
    assert!((result.expected_failures.unwrap() - 1000.0).abs() < 1e-9);
    // Comparing the same observed data against a stale prediction gives an
    // overwhelmingly small p-value: this is not sampling noise.
    assert!(result.p_value.unwrap() < 1e-100);
}

#[test]
fn current_artifact_is_a_borderline_potential_deviation() {
    let current = artifact(0.002);
    let week1 = week_dataset("prod-us-east-week-1", 110, 50_000);
    let week2 = week_dataset("prod-us-east-week-2", 120, 50_000);

    let aggregated = aggregate_across(&[&week1, &week2], EVENT).unwrap();
    let result = calibrate(
        &current,
        EVENT,
        &aggregated.observation,
        aggregated.provenance.source_datasets.clone(),
        &CalibrationConfig::default(),
    )
    .unwrap();

    assert_eq!(result.status, CalibrationStatus::PotentialDeviation);
    assert!(!result.is_drift());
    assert!((result.expected_failures.unwrap() - 200.0).abs() < 1e-9);
    let p = result.p_value.unwrap();
    assert!((p - 0.040245227645656585).abs() < 1e-9, "got {p}");
}

#[test]
fn same_observed_data_different_verdicts_depending_on_prediction() {
    // The point of the worked example: identical observed data yields two
    // different, individually correct verdicts depending on which
    // prediction it is compared against. Neither is a tool bug.
    let week1 = week_dataset("prod-us-east-week-1", 110, 50_000);
    let week2 = week_dataset("prod-us-east-week-2", 120, 50_000);
    let aggregated = aggregate_across(&[&week1, &week2], EVENT).unwrap();

    let stale_result = calibrate(
        &artifact(0.01),
        EVENT,
        &aggregated.observation,
        vec![],
        &CalibrationConfig::default(),
    )
    .unwrap();
    let current_result = calibrate(
        &artifact(0.002),
        EVENT,
        &aggregated.observation,
        vec![],
        &CalibrationConfig::default(),
    )
    .unwrap();

    assert_ne!(stale_result.status, current_result.status);
    assert_eq!(
        stale_result.observed.proportion,
        current_result.observed.proportion
    );
}

#[test]
fn calibration_never_mutates_the_artifact_files_on_disk_equivalent() {
    // No `&mut ReliabilityArtifact` path exists anywhere in `calibrate`; this
    // is enforced by the type system (the function signature only accepts
    // `&ReliabilityArtifact`), verified here by comparing before/after.
    let a = artifact(0.01);
    let before = serde_json::to_string(&a).unwrap();
    let week1 = week_dataset("prod-us-east-week-1", 110, 50_000);
    let aggregated = aggregate_across(&[&week1], EVENT).unwrap();
    let _ = calibrate(
        &a,
        EVENT,
        &aggregated.observation,
        vec![],
        &CalibrationConfig::default(),
    )
    .unwrap();
    let after = serde_json::to_string(&a).unwrap();
    assert_eq!(before, after);
}
