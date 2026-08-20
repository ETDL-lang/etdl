//! Immutable reliability observations (evidence).

use serde::{Deserialize, Serialize};

/// A runtime observation: immutable evidence of what happened.
///
/// Mirrors `etdl_core::observation::ReliabilityObservation` field-for-field
/// (the runtime and analysis layers are deliberately separate crates with no
/// dependency between them; JSON is the interchange format, not a shared
/// Rust type). Keep the two in sync so observations emitted by a generated
/// service round-trip through this crate without silently losing fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReliabilityObservation {
    pub id: String,
    pub event: String,
    pub timestamp: String,
    pub service: Option<String>,
    pub operation: Option<String>,
    pub environment: Option<String>,
    pub deployment: Option<String>,
    /// The software/model version that produced this observation.
    #[serde(default)]
    pub service_version: Option<String>,
    /// A stable reference to the compiled reliability artifact/build that
    /// generated this observation.
    #[serde(default)]
    pub build_ref: Option<String>,
    pub outcome: String,
    pub conditions: Vec<String>,
    pub duration_ms: Option<u64>,
    pub trace_id: Option<String>,
}

impl ReliabilityObservation {
    pub fn new(
        id: impl Into<String>,
        event: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        ReliabilityObservation {
            id: id.into(),
            event: event.into(),
            timestamp: timestamp.into(),
            service: None,
            operation: None,
            environment: None,
            deployment: None,
            service_version: None,
            build_ref: None,
            outcome: String::new(),
            conditions: Vec::new(),
            duration_ms: None,
            trace_id: None,
        }
    }

    pub fn outcome_failed(mut self) -> Self {
        self.outcome = "failed".to_string();
        self
    }

    pub fn outcome_succeeded(mut self) -> Self {
        self.outcome = "succeeded".to_string();
        self
    }
}

/// A failure observation: a specific occurrence of a failure mode.
pub type FailureObservation = ReliabilityObservation;

/// An event observation: a generic occurrence of an event.
pub type EventObservation = ReliabilityObservation;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_is_immutable_data() {
        let o = ReliabilityObservation::new("obs-1", "failure.db.timeout", "2026-08-16T12:00:00Z");
        assert_eq!(o.event, "failure.db.timeout");
        assert!(o.duration_ms.is_none());
    }
}
