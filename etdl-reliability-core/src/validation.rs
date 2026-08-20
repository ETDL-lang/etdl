//! Reliability-domain validation helpers (value range, artifact schema, metric
//! semantics).

use crate::artifact::ReliabilityArtifact;
use crate::estimate::ProbabilityEstimate;
use crate::ReliabilityError;

/// Validate that an estimate is usable as a deterministic scalar probability.
pub fn validate_estimate(estimate: &ProbabilityEstimate) -> Result<(), ReliabilityError> {
    estimate.resolved_probability().map(|_| ())
}

/// A single structured problem found while validating an artifact.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ArtifactIssue {
    #[error("artifact has no id")]
    MissingId,
    #[error("artifact has no version; a version is required for provenance")]
    MissingVersion,
    #[error("artifact declares no estimates")]
    EmptyEstimates,
    #[error("estimate '{0}' has a duplicate key (event id '{1}')")]
    DuplicateEstimate(String, String),
    #[error("estimate '{0}' must be finite: {1}")]
    NonFiniteEstimate(String, String),
    #[error("estimate '{0}' metric '{1:?}' requires a numeric value")]
    MetricRequiresValue(String, crate::probability::ProbabilityMetric),
    #[error("estimate '{0}' metric '{1:?}' must not carry a probability value; metric and value conflict")]
    MetricValueConflict(String, crate::probability::ProbabilityMetric),
    #[error("estimate '{0}' carries an unknown state with a value; unknown must have no value")]
    UnknownWithValue(String),
    #[error(
        "estimate '{0}' metric '{1:?}' requires a time_basis (it is a rate or time-based metric)"
    )]
    RateRequiresTimeBasis(String, crate::probability::ProbabilityMetric),
    #[error("estimate '{0}' has invalid uncertainty: {1}")]
    InvalidUncertainty(String, String),
    #[error("estimate '{0}' has invalid distribution: {1}")]
    InvalidDistribution(String, String),
    #[error("estimate '{0}' has missing provenance fields: {1}")]
    IncompleteProvenance(String, String),
}

/// Validate an artifact structurally and semantically.
///
/// This is stricter than `validate_estimate` (which only checks the
/// deterministic scalar path): it checks id/version presence, duplicate
/// estimate ids, per-metric value rules, uncertainty, and distributions.
/// The result is the list of problems found; an empty list means valid.
pub fn validate_artifact_issues(artifact: &ReliabilityArtifact) -> Vec<ArtifactIssue> {
    let mut issues = Vec::new();

    if artifact.id.trim().is_empty() {
        issues.push(ArtifactIssue::MissingId);
    }
    if artifact.version.is_none() {
        issues.push(ArtifactIssue::MissingVersion);
    }
    if artifact.estimates.is_empty() {
        issues.push(ArtifactIssue::EmptyEstimates);
    }

    // Detect duplicate estimate ids (an id mapping to more than one event).
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (key, est) in &artifact.estimates {
        if let Some(prev) = seen.insert(est.event.clone(), key.clone()) {
            if prev != *key {
                issues.push(ArtifactIssue::DuplicateEstimate(
                    key.clone(),
                    est.event.clone(),
                ));
            }
        }
    }

    for estimate in artifact.estimates.values() {
        issues.extend(validate_estimate_semantics(estimate));
    }

    issues
}

/// Validate one estimate's internal consistency (independent of the artifact).
pub fn validate_estimate_semantics(estimate: &ProbabilityEstimate) -> Vec<ArtifactIssue> {
    let mut issues = Vec::new();
    let id = estimate.event.clone();

    if estimate.state == crate::estimate::ProbabilityState::Unknown && estimate.value.is_some() {
        issues.push(ArtifactIssue::UnknownWithValue(id.clone()));
    }

    if estimate.metric.requires_time_basis() && estimate.time_basis.is_none() {
        issues.push(ArtifactIssue::RateRequiresTimeBasis(
            id.clone(),
            estimate.metric,
        ));
    }

    if let Some(v) = estimate.value {
        if !v.is_finite() {
            issues.push(ArtifactIssue::NonFiniteEstimate(id.clone(), v.to_string()));
        } else {
            match estimate.metric {
                crate::probability::ProbabilityMetric::Probability
                | crate::probability::ProbabilityMetric::Availability => {
                    if !(0.0..=1.0).contains(&v) {
                        issues.push(ArtifactIssue::MetricRequiresValue(
                            id.clone(),
                            estimate.metric,
                        ));
                    }
                }
                _ => {
                    if v < 0.0 {
                        issues.push(ArtifactIssue::MetricRequiresValue(
                            id.clone(),
                            estimate.metric,
                        ));
                    }
                }
            }
        }
    } else if estimate.state != crate::estimate::ProbabilityState::Unknown {
        // A non-unknown estimate must have a value. (Only the Unknown state
        // legitimately carries no value.)
        issues.push(ArtifactIssue::MetricRequiresValue(
            id.clone(),
            estimate.metric,
        ));
    }

    if let Some(uncertainty) = &estimate.uncertainty {
        if let Some(err) = uncertainty.validate() {
            issues.push(ArtifactIssue::InvalidUncertainty(
                id.clone(),
                err.to_string(),
            ));
        }
        if let crate::uncertainty::Uncertainty::Distribution(d) = uncertainty {
            if let Some(err) = d.validate() {
                issues.push(ArtifactIssue::InvalidDistribution(
                    id.clone(),
                    err.to_string(),
                ));
            }
        }
    }

    if let Some(provenance) = &estimate.provenance {
        if let Some(missing) = provenance.missing_required_fields() {
            issues.push(ArtifactIssue::IncompleteProvenance(id.clone(), missing));
        }
    }

    issues
}

/// Validate an artifact: schema and every estimate.
pub fn validate_artifact(artifact: &ReliabilityArtifact) -> Result<(), ReliabilityError> {
    let issues = validate_artifact_issues(artifact);
    if let Some(first) = issues.first() {
        return Err(ReliabilityError::MalformedArtifact(first.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::declared;
    use crate::estimate::{ProbabilityEstimate, ProbabilityState};

    #[test]
    fn artifact_validates() {
        let mut a = ReliabilityArtifact::new("x");
        a.version = Some("1.0.0".into());
        a.add(declared("e", 0.01)).unwrap();
        assert!(validate_artifact(&a).is_ok());
        assert!(validate_artifact_issues(&a).is_empty());
    }

    #[test]
    fn missing_version_is_flagged() {
        let mut a = ReliabilityArtifact::new("x");
        a.add(declared("e", 0.01)).unwrap();
        let issues = validate_artifact_issues(&a);
        assert!(issues.contains(&ArtifactIssue::MissingVersion));
    }

    #[test]
    fn empty_artifact_flagged() {
        let a = ReliabilityArtifact::new("x");
        let issues = validate_artifact_issues(&a);
        assert!(issues.contains(&ArtifactIssue::EmptyEstimates));
        assert!(issues.contains(&ArtifactIssue::MissingVersion));
    }

    #[test]
    fn non_finite_estimate_flagged() {
        let mut a = ReliabilityArtifact::new("x");
        a.version = Some("1.0.0".into());
        a.add(declared("e", f64::NAN)).unwrap();
        let issues = validate_artifact_issues(&a);
        assert!(issues
            .iter()
            .any(|i| matches!(i, ArtifactIssue::NonFiniteEstimate(_, _))));
    }

    #[test]
    fn duplicate_estimate_id_flagged() {
        let mut a = ReliabilityArtifact::new("x");
        a.version = Some("1.0.0".into());
        let mut e1 = declared("e", 0.01);
        let mut e2 = declared("e", 0.02);
        e1.event = "e".into();
        e2.event = "e".into();
        // Two different keys mapping to the same event id.
        a.estimates.insert("e".into(), e1);
        a.estimates.insert("e-alias".into(), e2);
        let issues = validate_artifact_issues(&a);
        assert!(issues
            .iter()
            .any(|i| matches!(i, ArtifactIssue::DuplicateEstimate(_, _))));
    }

    #[test]
    fn unknown_with_value_flagged() {
        let mut e = ProbabilityEstimate::new("e", ProbabilityState::Unknown, 0.5);
        e.value = Some(0.5);
        let issues = validate_estimate_semantics(&e);
        assert!(issues.contains(&ArtifactIssue::UnknownWithValue("e".into())));
    }

    #[test]
    fn metric_requires_value_flagged() {
        // A measured estimate without a value is invalid.
        let mut e = ProbabilityEstimate::new("e", ProbabilityState::Measured, 0.1);
        e.value = None;
        let issues = validate_estimate_semantics(&e);
        assert!(issues
            .iter()
            .any(|i| matches!(i, ArtifactIssue::MetricRequiresValue(_, _))));
    }

    #[test]
    fn rate_metric_requires_time_basis() {
        let mut e = ProbabilityEstimate::new("e", ProbabilityState::Measured, 0.0001);
        e.metric = crate::probability::ProbabilityMetric::FailureRate;
        // No time_basis -> flagged.
        assert!(issues_has_rate_issue(&e));
        // With a time basis -> fine.
        e.time_basis = Some(crate::probability::TimeBasis::PerHour);
        assert!(!issues_has_rate_issue(&e));
    }

    fn issues_has_rate_issue(e: &ProbabilityEstimate) -> bool {
        validate_estimate_semantics(e)
            .iter()
            .any(|i| matches!(i, ArtifactIssue::RateRequiresTimeBasis(_, _)))
    }
}
