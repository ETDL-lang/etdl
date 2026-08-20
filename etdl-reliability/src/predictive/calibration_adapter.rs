//! Bridges an existing, already-calibrated [`ReliabilityArtifact`] into a
//! [`TimeToFailureModel`], so Predictive Reliability *consumes* the
//! existing estimation/calibration pipeline rather than building a second
//! one.
//!
//! This module reads; it never writes. It does not touch
//! [`crate::calibration`] or [`crate::dataset`] at all — the calibration
//! loop (`observe -> analyze -> review -> publish new artifact -> rebuild`,
//! see `docs/reliability/runtime-feedback-calibration.md`) is entirely
//! unmodified. A caller who wants a prediction reflecting fresh runtime
//! evidence must run that loop to produce a *new* artifact and then call
//! this adapter again on the new artifact — this module offers no
//! shortcut, by design.

use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::{ProbabilityMetric, ProbabilityState};

use crate::analysis::dependence::ArtifactRef;
use crate::predictive::models::{ExponentialModel, ExponentialModelError};
use crate::predictive::PredictiveProvenance;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CalibrationAdapterError {
    #[error("artifact has no estimate for event {0:?}")]
    UnknownEvent(String),
    #[error(
        "estimate for event {event:?} has metric {metric:?}, expected FailureRate \
         (a predictive model cannot be built from a bare probability without a time basis)"
    )]
    WrongMetric {
        event: String,
        metric: ProbabilityMetric,
    },
    #[error("estimate for event {0:?} has no value (state is {1:?})")]
    NoValue(String, ProbabilityState),
    #[error(transparent)]
    InvalidRate(#[from] ExponentialModelError),
}

/// Reads the estimate for `event` out of `artifact` and, if it is a
/// `FailureRate`-metric estimate with a value, constructs an
/// [`ExponentialModel`] whose `lambda` is that rate — plus a
/// [`PredictiveProvenance`] recording exactly which artifact/estimate it
/// came from.
///
/// This is the only supported way in 1.0 to go from "an estimate" to "a
/// predictive model": constant-hazard estimates only. Building a Weibull
/// model from an artifact would require shape information the estimation
/// pipeline does not currently produce (see the crate's final report for
/// this documented gap) — constructing a `WeibullModel` directly from
/// literal parameters remains available via `WeibullModel::new`.
pub fn exponential_model_from_artifact(
    artifact: &ReliabilityArtifact,
    event: &str,
) -> Result<(ExponentialModel, PredictiveProvenance), CalibrationAdapterError> {
    let estimate = artifact
        .estimates
        .get(event)
        .ok_or_else(|| CalibrationAdapterError::UnknownEvent(event.to_string()))?;

    if estimate.metric != ProbabilityMetric::FailureRate {
        return Err(CalibrationAdapterError::WrongMetric {
            event: event.to_string(),
            metric: estimate.metric,
        });
    }

    let value = estimate
        .value
        .ok_or_else(|| CalibrationAdapterError::NoValue(event.to_string(), estimate.state))?;

    let model = ExponentialModel::new(value)?;

    let mut artifact_ref = ArtifactRef::new(artifact.id.clone());
    artifact_ref.version = artifact.version.clone();
    artifact_ref.schema = Some(artifact.schema.clone());
    artifact_ref.role = Some("predictive-model-source".to_string());

    let mut provenance = PredictiveProvenance::new();
    provenance.source_artifact = Some(artifact_ref);
    provenance.source_estimate = Some(event.to_string());

    Ok((model, provenance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use etdl_reliability_core::estimate::ProbabilityEstimate;

    fn artifact_with(
        event: &str,
        metric: ProbabilityMetric,
        value: Option<f64>,
    ) -> ReliabilityArtifact {
        let mut artifact = ReliabilityArtifact::new("test-artifact");
        artifact.version = Some("1.0.0".to_string());
        let state = if value.is_some() {
            ProbabilityState::Estimated
        } else {
            ProbabilityState::Unknown
        };
        let mut estimate = ProbabilityEstimate::new(event, state, value.unwrap_or(0.0));
        estimate.value = value;
        estimate.metric = metric;
        artifact.estimates.insert(event.to_string(), estimate);
        artifact
    }

    #[test]
    fn builds_model_from_failure_rate_estimate() {
        let artifact = artifact_with("pump-fails", ProbabilityMetric::FailureRate, Some(0.001));
        let (model, provenance) = exponential_model_from_artifact(&artifact, "pump-fails").unwrap();
        assert!((model.lambda() - 0.001).abs() < 1e-15);
        assert_eq!(provenance.source_estimate.as_deref(), Some("pump-fails"));
        assert_eq!(provenance.source_artifact.unwrap().id, "test-artifact");
    }

    #[test]
    fn rejects_unknown_event() {
        let artifact = artifact_with("pump-fails", ProbabilityMetric::FailureRate, Some(0.001));
        assert!(matches!(
            exponential_model_from_artifact(&artifact, "valve-fails"),
            Err(CalibrationAdapterError::UnknownEvent(_))
        ));
    }

    #[test]
    fn rejects_wrong_metric() {
        let artifact = artifact_with("pump-fails", ProbabilityMetric::Probability, Some(0.1));
        assert!(matches!(
            exponential_model_from_artifact(&artifact, "pump-fails"),
            Err(CalibrationAdapterError::WrongMetric { .. })
        ));
    }

    #[test]
    fn rejects_missing_value() {
        let artifact = artifact_with("pump-fails", ProbabilityMetric::FailureRate, None);
        assert!(matches!(
            exponential_model_from_artifact(&artifact, "pump-fails"),
            Err(CalibrationAdapterError::NoValue(_, _))
        ));
    }
}
