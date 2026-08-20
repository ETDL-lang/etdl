//! Engineering review of discovery candidates.
//!
//! Discovery is advisory. An engineer decides whether to accept, reject, remap,
//! or ignore each candidate. This module records that decision as an immutable,
//! auditable [`ReviewRecord`] and links an accepted candidate to a stable
//! [`ReviewedFailureMode`].
//!
//! Review records are versioned and never mutated: a new review produces a new
//! record. They do not require personally identifying information.

use serde::{Deserialize, Serialize};

use crate::failure::{FailureModeId, FailureSource, FailureStatus};

/// The engineer's decision on a discovery candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewStatus {
    Accepted,
    Rejected,
    Ignored,
    /// Accepted but mapped to a different ontology concept than the discovery
    /// suggested.
    Remapped,
}

impl ReviewStatus {
    pub fn label(self) -> &'static str {
        match self {
            ReviewStatus::Accepted => "accepted",
            ReviewStatus::Rejected => "rejected",
            ReviewStatus::Ignored => "ignored",
            ReviewStatus::Remapped => "remapped",
        }
    }
}

/// An immutable, auditable record of one engineering review decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRecord {
    /// The candidate id being reviewed (stable concept identity).
    pub candidate_id: String,
    /// The discovery report id that produced the candidate (if known).
    pub candidate_report_id: Option<String>,
    pub status: ReviewStatus,
    /// Free-text reviewer label (optional; not required to be personal).
    pub reviewer: Option<String>,
    /// Explicitly recorded review time, if the engineer records it.
    pub reviewed_at: Option<String>,
    /// The ontology concept selected for the accepted failure mode.
    pub selected_ontology_id: Option<FailureModeId>,
    /// Engineer rationale.
    pub rationale: Option<String>,
    /// Engineering assumptions (e.g. "assumes timeout path dominates").
    #[serde(default)]
    pub assumptions: Vec<String>,
    /// Free-form notes.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Version of this review record.
    pub version: String,
}

impl ReviewRecord {
    pub fn new(candidate_id: impl Into<String>, status: ReviewStatus) -> Self {
        ReviewRecord {
            candidate_id: candidate_id.into(),
            candidate_report_id: None,
            status,
            reviewer: None,
            reviewed_at: None,
            selected_ontology_id: None,
            rationale: None,
            assumptions: Vec::new(),
            notes: Vec::new(),
            version: "1".to_string(),
        }
    }
}

/// A failure mode that has been accepted (or remapped) through engineering
/// review, with the traceability needed to feed estimation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewedFailureMode {
    /// Stable failure-mode identity (survives source movement).
    pub failure_mode_id: FailureModeId,
    pub label: Option<String>,
    pub description: Option<String>,
    pub source: FailureSource,
    pub status: FailureStatus,
    /// The review that produced this acceptance.
    pub review: ReviewRecord,
    /// Source location evidence (human-readable; line numbers are evidence,
    /// not identity).
    pub source_location: Option<String>,
    /// Discovery evidence strings (what static analysis found).
    #[serde(default)]
    pub discovery_evidence: Vec<String>,
    /// The ontology version used to map this failure mode (if known).
    pub ontology_version: Option<String>,
}

impl ReviewedFailureMode {
    /// Create an accepted failure mode from a review record.
    pub fn from_review(review: ReviewRecord) -> Self {
        let failure_mode_id = review
            .selected_ontology_id
            .clone()
            .unwrap_or_else(|| review.candidate_id.clone());
        ReviewedFailureMode {
            status: if matches!(
                review.status,
                ReviewStatus::Accepted | ReviewStatus::Remapped
            ) {
                FailureStatus::Accepted
            } else {
                FailureStatus::Candidate
            },
            failure_mode_id,
            label: None,
            description: None,
            source: FailureSource::Discovery,
            review,
            source_location: None,
            discovery_evidence: Vec::new(),
            ontology_version: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_review_creates_accepted_failure_mode() {
        let mut review = ReviewRecord::new("failure.gateway.timeout", ReviewStatus::Accepted);
        review.selected_ontology_id = Some("failure.dependency.timeout".into());
        review.rationale = Some("matches production timeout path".into());
        let fm = ReviewedFailureMode::from_review(review);
        assert_eq!(fm.status, FailureStatus::Accepted);
        assert_eq!(fm.failure_mode_id, "failure.dependency.timeout");
        assert_eq!(fm.source, FailureSource::Discovery);
    }

    #[test]
    fn review_records_are_immutable_and_versioned() {
        let r1 = ReviewRecord::new("failure.x", ReviewStatus::Accepted);
        assert_eq!(r1.version, "1");
        // A new review is a new record.
        let r2 = ReviewRecord::new("failure.x", ReviewStatus::Rejected);
        assert_ne!(r1, r2);
    }

    #[test]
    fn rejected_review_keeps_candidate_status() {
        let review = ReviewRecord::new("failure.x", ReviewStatus::Rejected);
        let fm = ReviewedFailureMode::from_review(review);
        assert_eq!(fm.status, FailureStatus::Candidate);
    }
}
