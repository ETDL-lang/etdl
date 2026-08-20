//! Ontology mapping for discovery candidates.

use serde::{Deserialize, Serialize};

/// How confidently a discovery candidate maps to a canonical ontology concept.
/// This is mapping quality, distinct from the ontology's own lifecycle status
/// (`Candidate/Reviewed/Accepted/...`) and from discovery confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MappingQuality {
    /// The candidate's concept is a canonical ontology id.
    Exact,
    /// A confident but heuristic mapping.
    Probable,
    /// Several ontology concepts could match.
    Ambiguous,
    /// No ontology concept matches; the candidate proposes a new concept.
    Unmapped,
    /// The mapped ontology concept is deprecated/merged; the alive id is given.
    Deprecated,
}

impl MappingQuality {
    pub fn label(self) -> &'static str {
        match self {
            MappingQuality::Exact => "exact",
            MappingQuality::Probable => "probable",
            MappingQuality::Ambiguous => "ambiguous",
            MappingQuality::Unmapped => "unmapped",
            MappingQuality::Deprecated => "deprecated",
        }
    }
}

/// A discovery candidate's mapping into the canonical ontology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OntologyMapping {
    /// The matched canonical ontology id, if any.
    pub canonical_id: Option<String>,
    /// The proposed new ontology concept (only when quality == Unmapped).
    pub proposed_concept: Option<String>,
    pub quality: MappingQuality,
    /// Confidence in the mapping, in [0, 1]. NOT a failure probability.
    pub confidence: f64,
    /// Why this mapping was chosen.
    pub evidence: Vec<String>,
}

impl OntologyMapping {
    pub fn unmapped(proposed: impl Into<String>) -> Self {
        OntologyMapping {
            canonical_id: None,
            proposed_concept: Some(proposed.into()),
            quality: MappingQuality::Unmapped,
            confidence: 0.0,
            evidence: Vec::new(),
        }
    }
}
