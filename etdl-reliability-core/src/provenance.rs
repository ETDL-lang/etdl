//! Probability provenance: where an estimate came from.

use serde::{Deserialize, Serialize};

use crate::probability::ProbabilitySource;

/// Provenance of a non-literal reliability estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source_type: ProbabilitySource,
    pub dataset: Option<String>,
    pub model: Option<String>,
    pub model_version: Option<String>,
    pub generated_at: Option<String>,
}

impl Provenance {
    pub fn new(source_type: ProbabilitySource) -> Self {
        Provenance {
            source_type,
            dataset: None,
            model: None,
            model_version: None,
            generated_at: None,
        }
    }

    /// Field names required for traceability given the source type. A
    /// `Literal` provenance needs nothing further; all other source types
    /// require at least one identifying attribute (`dataset`, `model`, or
    /// `model_version`).
    ///
    /// Returns a description of missing fields, or `None` if complete.
    pub fn missing_required_fields(&self) -> Option<String> {
        if self.source_type == ProbabilitySource::Literal {
            return None;
        }
        if self.dataset.is_some() || self.model.is_some() || self.model_version.is_some() {
            return None;
        }
        Some("at least one of dataset, model, model_version".to_string())
    }
}
