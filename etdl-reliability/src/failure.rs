//! Failure modes and their lifecycle.

use serde::{Deserialize, Serialize};

/// A canonical failure-mode identifier, e.g. `failure.network.timeout`.
pub type FailureModeId = String;

/// Where a failure mode candidate came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureSource {
    Ontology,
    Discovery,
    Engineering,
    AiSuggestion,
    Unknown,
}

/// Lifecycle status of a failure mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureStatus {
    Candidate,
    Reviewed,
    Accepted,
    Rejected,
    Merged,
    Deprecated,
}

/// A failure mode. `id` is stable identity; probability/rates are knowledge
/// about it, never part of the id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FailureMode {
    pub id: FailureModeId,
    pub label: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub status: FailureStatus,
    pub source: FailureSource,
    pub causes: Vec<FailureModeId>,
    pub effects: Vec<FailureModeId>,
}

impl FailureMode {
    pub fn new(id: impl Into<String>) -> Self {
        FailureMode {
            id: id.into(),
            label: None,
            description: None,
            category: None,
            status: FailureStatus::Candidate,
            source: FailureSource::Unknown,
            causes: Vec::new(),
            effects: Vec::new(),
        }
    }
}
