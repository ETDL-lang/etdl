//! ETDL Reliability Ontology.
//!
//! The ontology answers **"what is this?"** with stable canonical identifiers.
//! It is deliberately separate from reliability *knowledge* ("how likely is
//! it?") and from *evidence* ("what happened?"):
//!
//! - Ontology identity: `failure.network.timeout` — stable.
//! - Reliability knowledge: `P = 0.0031` — mutable, versioned.
//! - Observations: immutable evidence.
//!
//! A new observation NEVER creates a new ontology identifier; it only updates
//! knowledge. Ontology refinement (e.g. splitting `failure.database.timeout`
//! into connection/query/lock timeouts) is versioned and traceable, and a
//! discovery engine never silently modifies the authoritative ontology.

pub mod mapping;
pub mod taxonomy;

pub use mapping::{MappingRule, MappingStatus, OntologyMapping};
pub use taxonomy::Taxonomy;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Lifecycle status of an ontology entry. A discovery engine never silently
/// moves an entry to `Accepted`; engineering review is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailureStatus {
    Candidate,
    Reviewed,
    Accepted,
    Rejected,
    Merged,
    Deprecated,
}

/// An ontology version: the ontology is versioned; knowledge is versioned
/// independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OntologyVersion {
    pub major: u64,
    pub minor: u64,
}

impl std::fmt::Display for OntologyVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A single ontology entry (a failure mode / cause / mechanism / effect).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OntologyEntry {
    pub id: String,
    pub kind: EntryKind,
    pub status: FailureStatus,
    /// Canonical id this entry was merged into (when status == Merged), or the
    /// replacement id (when Deprecated).
    pub replaced_by: Option<String>,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Event,
    Failure,
    FailureMode,
    Cause,
    Mechanism,
    Effect,
    Condition,
    Dependency,
    Resource,
    Barrier,
    Mitigation,
}

/// The authoritative ontology for a version: a map of canonical id -> entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ontology {
    pub version: OntologyVersion,
    pub entries: BTreeMap<String, OntologyEntry>,
}

impl Ontology {
    pub fn new(version: OntologyVersion) -> Self {
        Ontology {
            version,
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, entry: OntologyEntry) {
        self.entries.insert(entry.id.clone(), entry);
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    pub fn resolve(&self, id: &str) -> Option<&OntologyEntry> {
        self.entries.get(id)
    }

    /// Resolve an id through deprecation/merge to a live canonical id
    /// (owned, so it can be returned from a borrow-free context).
    pub fn resolve_alive(&self, id: &str) -> Option<String> {
        let mut current = id.to_string();
        let mut seen = std::collections::HashSet::new();
        loop {
            if !seen.insert(current.clone()) {
                return None; // cycle
            }
            let entry = self.entries.get(&current)?;
            match entry.status {
                FailureStatus::Deprecated | FailureStatus::Merged => {
                    current = entry.replaced_by.clone()?;
                }
                _ => return Some(current),
            }
        }
    }

    /// Deprecate `id` and point it at `replacement` (traceable, versioned).
    pub fn deprecate(&mut self, id: &str, replacement: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.status = FailureStatus::Deprecated;
            entry.replaced_by = Some(replacement.to_string());
        }
    }

    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    pub fn from_yaml(s: &str) -> Result<Self, String> {
        serde_yaml::from_str(s).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_ontology() -> Ontology {
        let mut o = Ontology::new(OntologyVersion { major: 1, minor: 0 });
        o.insert(OntologyEntry {
            id: "failure.network.timeout".into(),
            kind: EntryKind::FailureMode,
            status: FailureStatus::Accepted,
            replaced_by: None,
            aliases: vec!["TimeoutException".into()],
        });
        o.insert(OntologyEntry {
            id: "failure.network.connection_timeout".into(),
            kind: EntryKind::FailureMode,
            status: FailureStatus::Candidate,
            replaced_by: None,
            aliases: Vec::new(),
        });
        o
    }

    #[test]
    fn resolves_alive_ids() {
        let o = base_ontology();
        assert_eq!(
            o.resolve_alive("failure.network.timeout").as_deref(),
            Some("failure.network.timeout")
        );
        assert_eq!(o.resolve_alive("missing"), None);
    }

    #[test]
    fn deprecation_is_traceable() {
        let mut o = base_ontology();
        o.deprecate(
            "failure.network.connection_timeout",
            "failure.network.timeout",
        );
        assert_eq!(
            o.resolve_alive("failure.network.connection_timeout")
                .as_deref(),
            Some("failure.network.timeout")
        );
    }

    #[test]
    fn roundtrips_yaml() {
        let o = base_ontology();
        let s = serde_yaml::to_string(&o).unwrap();
        let o2 = Ontology::from_yaml(&s).unwrap();
        assert!(o2.contains("failure.network.timeout"));
    }
}
