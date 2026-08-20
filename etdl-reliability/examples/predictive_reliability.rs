//! `cargo run -p etdl-reliability --example predictive_reliability`
//!
//! Three small, self-contained demonstrations of Predictive Reliability:
//! (1) mission reliability from a constant-hazard model, (2) a Weibull
//! aging model showing hazard growth over life, and (3) the
//! predict -> observe -> calibrate -> new artifact -> new prediction
//! loop, making the "never mutate, always publish a new artifact"
//! discipline concrete.

use etdl_reliability::predictive::calibration_adapter::exponential_model_from_artifact;
use etdl_reliability::predictive::models::{ExponentialModel, TimeToFailureModel, WeibullModel};
use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::estimate::ProbabilityEstimate;
use etdl_reliability_core::{ProbabilityMetric, ProbabilityState};

fn main() {
    mission_reliability();
    println!();
    weibull_aging();
    println!();
    calibration_loop();
}

/// (1) A pump with a constant failure rate of 0.001/hour. What is its
/// reliability over a 100-hour mission?
fn mission_reliability() {
    println!("-- mission reliability (exponential) --");
    let pump = ExponentialModel::new(0.001).unwrap();
    let t = 100.0;
    println!(
        "  lambda = {} /hour, mission time = {} hours",
        pump.lambda(),
        t
    );
    println!("  R(t) = S(t) = {:.6}", pump.survival(t));
    println!("  F(t) = {:.6}", pump.failure_probability(t));
    println!("  h(t) = {:.6} (constant)", pump.hazard(t));
    println!("  mean life = {:.1} hours", pump.mean().unwrap());
    println!("  median life = {:.1} hours", pump.quantile(0.5).unwrap());
}

/// (2) A bearing that wears out: Weibull shape=2.5 (increasing hazard).
/// Compare hazard early vs. late in life — something the exponential model
/// cannot represent at all.
fn weibull_aging() {
    println!("-- aging model (weibull, shape=2.5) --");
    let bearing = WeibullModel::new(2.5, 5000.0).unwrap();
    for t in [100.0, 1000.0, 4000.0, 8000.0] {
        println!(
            "  t={t:>6.0}h  S(t)={:.6}  h(t)={:.6}",
            bearing.survival(t),
            bearing.hazard(t)
        );
    }
    println!(
        "  hazard grows monotonically with age here; a constant-rate \
         model would report the same h(t) at every row above, silently \
         hiding the wear-out"
    );
}

/// (3) The calibration loop discipline: a prediction made from an original
/// artifact, an observation that suggests drift, and a *new* artifact
/// published after review — the original is never touched.
fn calibration_loop() {
    println!("-- calibration loop (predict -> observe -> review -> republish) --");

    let mut artifact_v1 = ReliabilityArtifact::new("pump-artifact");
    artifact_v1.version = Some("1.0.0".to_string());
    let mut estimate = ProbabilityEstimate::new("pump-fails", ProbabilityState::Estimated, 0.001);
    estimate.metric = ProbabilityMetric::FailureRate;
    artifact_v1
        .estimates
        .insert("pump-fails".to_string(), estimate);

    let (model_v1, _provenance) =
        exponential_model_from_artifact(&artifact_v1, "pump-fails").unwrap();
    println!(
        "  artifact v{}: predicted F(100h) = {:.6}",
        artifact_v1.version.as_deref().unwrap(),
        model_v1.failure_probability(100.0)
    );

    println!("  ... field data comes in showing more failures than predicted ...");
    println!(
        "  ... an engineer reviews it and publishes a NEW artifact (never \
         edits artifact v1) ..."
    );

    let mut artifact_v2 = ReliabilityArtifact::new("pump-artifact");
    artifact_v2.version = Some("1.1.0".to_string());
    let mut revised = ProbabilityEstimate::new("pump-fails", ProbabilityState::Estimated, 0.004);
    revised.metric = ProbabilityMetric::FailureRate;
    artifact_v2
        .estimates
        .insert("pump-fails".to_string(), revised);

    let (model_v2, _provenance) =
        exponential_model_from_artifact(&artifact_v2, "pump-fails").unwrap();
    println!(
        "  artifact v{}: predicted F(100h) = {:.6}",
        artifact_v2.version.as_deref().unwrap(),
        model_v2.failure_probability(100.0)
    );

    println!(
        "  artifact v1 is unchanged: predicted F(100h) is still {:.6}",
        model_v1.failure_probability(100.0)
    );
}
