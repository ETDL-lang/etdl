//! Discovery candidates: the failure modes discovery may have found.

use serde::{Deserialize, Serialize};

use crate::location::{FunctionContext, SourceLocation};
use crate::mapping::OntologyMapping;

/// The life-cycle status of a discovery candidate. Discovery only ever
/// produces `Candidate`; engineering review moves it forward. Historical
/// reports are immutable — a new analysis produces a new report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateStatus {
    Candidate,
    Accepted,
    Rejected,
    Ignored,
    Mapped,
}

/// A coarse failure classification. This is a *classification*, not a
/// probability and not an ontology id (though it usually maps to one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureClassification {
    ApplicationFailure,
    DependencyFailure,
    DataFailure,
    ValidationFailure,
    TimeoutFailure,
    ResourceFailure,
    ConcurrencyFailure,
    ConfigurationFailure,
    SerializationFailure,
    IoFailure,
    UnknownFailure,
}

impl FailureClassification {
    pub fn label(self) -> &'static str {
        match self {
            FailureClassification::ApplicationFailure => "application",
            FailureClassification::DependencyFailure => "dependency",
            FailureClassification::DataFailure => "data",
            FailureClassification::ValidationFailure => "validation",
            FailureClassification::TimeoutFailure => "timeout",
            FailureClassification::ResourceFailure => "resource",
            FailureClassification::ConcurrencyFailure => "concurrency",
            FailureClassification::ConfigurationFailure => "configuration",
            FailureClassification::SerializationFailure => "serialization",
            FailureClassification::IoFailure => "io",
            FailureClassification::UnknownFailure => "unknown",
        }
    }
}

/// Severity is an engineering classification, NOT a probability and NOT a
/// risk score (severity × probability is forbidden until a probability exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// Structured evidence explaining WHY a candidate was discovered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// The kind of evidence, e.g. `source-pattern`, `api-detection`,
    /// `error-type-detection`.
    pub kind: String,
    /// The concrete source pattern, e.g. `unwrap()` or `?` or `panic!`.
    pub pattern: String,
    /// Free-form human-readable explanation.
    pub detail: String,
    /// The source line text (trimmed) for quick review.
    pub line_text: Option<String>,
}

impl Evidence {
    pub fn new(
        kind: impl Into<String>,
        pattern: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Evidence {
            kind: kind.into(),
            pattern: pattern.into(),
            detail: detail.into(),
            line_text: None,
        }
    }
}

/// A possible failure mechanism discovered by static analysis.
///
/// A candidate is **possible**, never proven: discovery establishes that a
/// failure mode *could* occur at this location given the analysis evidence.
/// `confidence` is the confidence that the discovery/classification is
/// correct — it is NOT a failure probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryCandidate {
    /// Stable identity across source movement (see `crate::identity`).
    pub id: String,
    pub classification: FailureClassification,
    pub severity: Severity,
    pub location: SourceLocation,
    pub context: FunctionContext,
    pub evidence: Vec<Evidence>,
    pub ontology: OntologyMapping,
    /// Confidence in the discovery itself, in [0, 1]. NOT a probability.
    pub confidence: f64,
    /// True when the candidate is only *possible* (the default).
    pub possible: bool,
    pub status: CandidateStatus,
}
