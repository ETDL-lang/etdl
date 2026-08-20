//! Ontology integration: a thin, read-only view over the canonical ontology.
//!
//! Discovery only ever **reads** the ontology. It never creates, edits, or
//! deprecates ontology definitions — that is engineering governance. It may
//! *propose* new concepts (as `proposed_concept` on an `Unmapped` mapping),
//! which a human must approve.

use etdl_reliability_ontology::Ontology;

/// A read-only view over an `Ontology`.
#[derive(Debug, Clone)]
pub struct OntologyView {
    inner: Ontology,
}

impl OntologyView {
    /// Build a view over the generic service ontology (v1.0, all `accepted`).
    pub fn generic_service() -> Self {
        OntologyView {
            inner: etdl_reliability_ontology::taxonomy::generic_service_ontology(),
        }
    }

    pub fn from_ontology(inner: Ontology) -> Self {
        OntologyView { inner }
    }

    /// Resolve `id` to its alive canonical id. Returns `(alive, was_deprecated)`:
    /// `was_deprecated` is true when the id was deprecated/merged and had to be
    /// resolved forward.
    pub fn resolve(&self, id: &str) -> Option<(String, bool)> {
        if !self.inner.contains(id) {
            return None;
        }
        let entry = self.inner.resolve(id)?;
        let alive = self.inner.resolve_alive(id)?;
        let was_deprecated = entry.replaced_by.is_some() && alive != id;
        Some((alive, was_deprecated))
    }

    /// Whether `id` exists in the ontology at all.
    pub fn contains(&self, id: &str) -> bool {
        self.inner.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_alive_and_deprecated() {
        let view = OntologyView::generic_service();
        let (alive, dep) = view.resolve("failure.network.timeout").unwrap();
        assert_eq!(alive, "failure.network.timeout");
        assert!(!dep);
        assert!(view.resolve("failure.nonexistent").is_none());
    }
}
