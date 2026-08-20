//! Discovery → reliability bridge.
//!
//! This module connects a discovery report to the reliability ecosystem. It is
//! the seam where an engineer, after review, chooses to turn an accepted
//! candidate into a reliability estimate.
//!
//! **The bridge never invents a probability.** It only assembles an artifact
//! when the caller supplies an explicit value (from observations, a model, or
//! engineering judgment). Without a value, the bridge can still emit a
//! candidate-only artifact that is clearly marked as discovery output.

use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::estimate::{ProbabilityEstimate, ProbabilityState};
use etdl_reliability_core::probability::{ProbabilityMetric, ProbabilitySource, TimeBasis};

use crate::candidate::{CandidateStatus, DiscoveryCandidate};

/// A reliability estimate for one candidate, supplied externally.
///
/// The value MUST come from a reliability engineering process (observations,
/// a statistical model, engineering judgment). Discovery does not produce it.
#[derive(Debug, Clone, PartialEq)]
pub struct SuppliedEstimate {
    /// The candidate's stable id this estimate applies to.
    pub candidate_id: String,
    /// The deterministic probability or rate, supplied externally.
    pub value: f64,
    /// Human-readable note on how the value was obtained.
    pub basis: String,
    pub metric: ProbabilityMetric,
    pub time_basis: Option<TimeBasis>,
    pub source: ProbabilitySource,
}

impl SuppliedEstimate {
    /// Build a `ProbabilityEstimate` for a candidate (probability metric).
    pub fn to_estimate(&self, candidate: &DiscoveryCandidate) -> ProbabilityEstimate {
        let mut e =
            ProbabilityEstimate::new(&self.candidate_id, ProbabilityState::Estimated, self.value);
        e.population = Some(candidate.context.dotted_path());
        e.metric = self.metric;
        e.time_basis = self.time_basis;
        e.source = self.source.clone();
        e.method = Some(self.basis.clone());
        e
    }
}

/// The type of artifact produced by the bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeOutput {
    /// A candidate-only artifact (`failure-candidates.yaml`): discovery output,
    /// clearly NOT a reliability probability artifact.
    CandidateOnly,
    /// A reliability artifact (`.rprob`) with externally supplied estimates.
    ReliabilityArtifact,
}

/// Convert accepted, mapped candidates into a reliability artifact.
///
/// Only candidates with `status == Accepted` are included. Every estimate
/// must be supplied externally (`SuppliedEstimate`). This function never
/// derives a value from discovery confidence.
pub fn accepted_candidates_to_artifact(
    candidates: &[DiscoveryCandidate],
    estimates: &[SuppliedEstimate],
    artifact_id: impl Into<String>,
) -> Option<ReliabilityArtifact> {
    let accepted: Vec<&DiscoveryCandidate> = candidates
        .iter()
        .filter(|c| c.status == CandidateStatus::Accepted)
        .collect();
    if accepted.is_empty() || estimates.is_empty() {
        return None;
    }

    let mut artifact = ReliabilityArtifact::new(artifact_id);
    artifact.version = Some("1.0.0".to_string());
    for cand in &accepted {
        if let Some(est) = estimates.iter().find(|e| e.candidate_id == cand.id) {
            artifact.add(est.to_estimate(cand)).ok();
        }
    }
    Some(artifact)
}

/// Build a candidate-only artifact (discovery output, not reliability data).
///
/// This is a JSON-compatible structure identifying itself as discovery output
/// so it can never be mistaken for a `.rprob` probability artifact.
pub fn candidate_only_artifact(report: &crate::report::DiscoveryReport) -> serde_json::Value {
    serde_json::json!({
        "schema": "etdl.failure-discovery.candidates/1.0",
        "kind": "discovery-output",
        "note": "This file is DISCOVERY OUTPUT. confidence values are discovery confidence, NOT failure probabilities. It is not a reliability artifact.",
        "analyzer": report.analyzer,
        "candidates": report.candidates.iter().map(|c| {
            serde_json::json!({
                "id": c.id,
                "classification": format!("{:?}", c.classification),
                "location": c.location,
                "evidence": c.evidence.iter().map(|e| serde_json::json!({
                    "kind": e.kind,
                    "pattern": e.pattern,
                    "detail": e.detail,
                })).collect::<Vec<_>>(),
                "ontology": c.ontology,
                "confidence": c.confidence,
            })
        }).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::{FailureClassification, Severity};
    use crate::location::SourceLocation;
    use crate::mapping::{MappingQuality, OntologyMapping};

    fn sample_candidate(id: &str) -> DiscoveryCandidate {
        DiscoveryCandidate {
            id: id.to_string(),
            classification: FailureClassification::DependencyFailure,
            severity: Severity::Medium,
            location: SourceLocation::new("src/main.rs"),
            context: Default::default(),
            evidence: Vec::new(),
            ontology: OntologyMapping {
                canonical_id: Some("failure.network.timeout".into()),
                proposed_concept: None,
                quality: MappingQuality::Exact,
                confidence: 0.95,
                evidence: Vec::new(),
            },
            confidence: 0.9,
            possible: true,
            status: CandidateStatus::Accepted,
        }
    }

    #[test]
    fn artifact_requires_external_value() {
        let cand = sample_candidate("failure.dependency.timeout");
        // No supplied estimate -> no artifact.
        assert!(accepted_candidates_to_artifact(&[cand], &[], "test").is_none());
    }

    #[test]
    fn artifact_uses_supplied_value_not_confidence() {
        let cand = sample_candidate("failure.dependency.timeout");
        let est = SuppliedEstimate {
            candidate_id: "failure.dependency.timeout".into(),
            value: 0.001, // externally supplied
            basis: "observed 1 timeout in 1000 requests".into(),
            metric: ProbabilityMetric::Probability,
            time_basis: Some(TimeBasis::PerRequest),
            source: ProbabilitySource::Measurement,
        };
        let artifact = accepted_candidates_to_artifact(&[cand], &[est], "svc").unwrap();
        let est2 = artifact.get("failure.dependency.timeout").unwrap();
        assert_eq!(est2.value, Some(0.001));
        assert_eq!(est2.state, ProbabilityState::Estimated);
    }
}
