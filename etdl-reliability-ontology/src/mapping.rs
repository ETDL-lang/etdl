//! Mapping discovered candidates to canonical ontology identifiers.

use serde::{Deserialize, Serialize};

use crate::Ontology;

/// Status of a candidate-to-ontology mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MappingStatus {
    Candidate,
    Reviewed,
    Accepted,
    Rejected,
}

/// A single mapping rule: a discovered token/exception name -> canonical id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingRule {
    /// The discovered token (e.g. exception name, error string, symbol).
    pub pattern: String,
    /// Whether `pattern` is matched as a substring or exact.
    pub exact: bool,
    pub canonical_id: String,
}

/// A mapping attempt with confidence and evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OntologyMapping {
    pub candidate: String,
    pub canonical_id: Option<String>,
    pub confidence: f64,
    pub status: MappingStatus,
    pub evidence: Vec<String>,
}

/// A set of mapping rules used to map discoveries to the ontology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MappingTable {
    pub rules: Vec<MappingRule>,
}

impl MappingTable {
    pub fn new() -> Self {
        MappingTable { rules: Vec::new() }
    }

    pub fn add(&mut self, rule: MappingRule) {
        self.rules.push(rule);
    }

    /// Map a discovered token to a canonical id, returning the best match.
    pub fn map(&self, token: &str) -> Option<OntologyMapping> {
        let mut best: Option<OntologyMapping> = None;
        for rule in &self.rules {
            let matched = if rule.exact {
                token == rule.pattern
            } else {
                token.to_lowercase().contains(&rule.pattern.to_lowercase())
            };
            if matched {
                let confidence = if rule.exact { 0.99 } else { 0.8 };
                let candidate = OntologyMapping {
                    candidate: token.to_string(),
                    canonical_id: Some(rule.canonical_id.clone()),
                    confidence,
                    status: MappingStatus::Candidate,
                    evidence: vec![format!("matched rule '{}'", rule.pattern)],
                };
                if best
                    .as_ref()
                    .is_none_or(|b: &OntologyMapping| candidate.confidence > b.confidence)
                {
                    best = Some(candidate);
                }
            }
        }
        best
    }

    /// Map and validate against an ontology: unresolved or invalid ids become
    /// unresolved candidates (never silently authoritative).
    pub fn map_into_ontology(&self, token: &str, ontology: &Ontology) -> OntologyMapping {
        let mut m = self.map(token).unwrap_or(OntologyMapping {
            candidate: token.to_string(),
            canonical_id: None,
            confidence: 0.0,
            status: MappingStatus::Candidate,
            evidence: Vec::new(),
        });
        if let Some(id) = &m.canonical_id {
            match ontology.resolve_alive(id) {
                Some(alive) if &alive == id => {
                    m.status = MappingStatus::Reviewed;
                }
                Some(alive) => {
                    m.canonical_id = Some(alive);
                    m.status = MappingStatus::Reviewed;
                }
                None => {
                    m.canonical_id = None;
                    m.status = MappingStatus::Candidate;
                }
            }
        }
        m
    }
}

impl Default for MappingTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> MappingTable {
        let mut t = MappingTable::new();
        t.add(MappingRule {
            pattern: "TimeoutException".into(),
            exact: false,
            canonical_id: "failure.network.timeout".into(),
        });
        t.add(MappingRule {
            pattern: "ConnectionRefused".into(),
            exact: false,
            canonical_id: "failure.network.connection_refused".into(),
        });
        t
    }

    #[test]
    fn maps_token() {
        let t = table();
        let m = t.map("java.net.SocketTimeoutException").unwrap();
        assert_eq!(m.canonical_id.as_deref(), Some("failure.network.timeout"));
    }

    #[test]
    fn unresolved_stays_candidate() {
        let t = table();
        let m = t.map("SomeUnknownError");
        assert!(m.is_none());
    }

    #[test]
    fn ontology_validation() {
        let o = crate::taxonomy::generic_service_ontology();
        let t = table();
        let m = t.map_into_ontology("SocketTimeoutException", &o);
        assert_eq!(m.canonical_id.as_deref(), Some("failure.network.timeout"));
        assert_eq!(m.status, MappingStatus::Reviewed);
    }
}
