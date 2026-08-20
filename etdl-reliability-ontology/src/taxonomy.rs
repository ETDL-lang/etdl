//! Canonical failure taxonomy and the generic service failure-mode set.

use crate::{EntryKind, FailureStatus, Ontology, OntologyEntry, OntologyVersion};

/// The canonical high-level service/server failure taxonomy. These are
/// **identity** only — never universal probability values.
pub const GENERIC_SERVICE_FAILURES: &[&str] = &[
    "failure.compute.process_crash",
    "failure.compute.memory_exhaustion",
    "failure.compute.cpu_exhaustion",
    "failure.network.timeout",
    "failure.network.unreachable",
    "failure.network.dns_failure",
    "failure.network.connection_refused",
    "failure.database.unavailable",
    "failure.database.connection_timeout",
    "failure.database.query_timeout",
    "failure.database.constraint_failure",
    "failure.storage.capacity_exhaustion",
    "failure.storage.io_failure",
    "failure.messaging.publish_failure",
    "failure.messaging.consume_failure",
    "failure.configuration.invalid",
    "failure.configuration.missing",
    "failure.deployment.incompatible_version",
    "failure.deployment.failed",
    "failure.dependency.unavailable",
    "failure.dependency.timeout",
    "failure.runtime.unhandled_error",
    "failure.runtime.cancellation",
];

/// Build the default generic-service ontology at version 1.0, with every entry
/// marked `accepted` (it is the authoritative set, not a discovery candidate).
pub fn generic_service_ontology() -> Ontology {
    let mut o = Ontology::new(OntologyVersion { major: 1, minor: 0 });
    for id in GENERIC_SERVICE_FAILURES {
        let (kind, domain) = classify(id);
        o.insert(OntologyEntry {
            id: id.to_string(),
            kind,
            status: FailureStatus::Accepted,
            replaced_by: None,
            aliases: vec![format!(
                "canonical:{}:{}",
                domain,
                id.rsplit('.').next().unwrap_or(id)
            )],
        });
    }
    o
}

/// Classify a canonical id into an entry kind and a domain label.
pub fn classify(id: &str) -> (EntryKind, &'static str) {
    if id.starts_with("failure.compute.") {
        (EntryKind::FailureMode, "compute")
    } else if id.starts_with("failure.network.") {
        (EntryKind::FailureMode, "network")
    } else if id.starts_with("failure.database.") {
        (EntryKind::FailureMode, "database")
    } else if id.starts_with("failure.storage.") {
        (EntryKind::FailureMode, "storage")
    } else if id.starts_with("failure.messaging.") {
        (EntryKind::FailureMode, "messaging")
    } else if id.starts_with("failure.configuration.") {
        (EntryKind::FailureMode, "configuration")
    } else if id.starts_with("failure.deployment.") {
        (EntryKind::FailureMode, "deployment")
    } else if id.starts_with("failure.dependency.") {
        (EntryKind::FailureMode, "dependency")
    } else if id.starts_with("failure.runtime.") {
        (EntryKind::FailureMode, "runtime")
    } else {
        (EntryKind::Failure, "other")
    }
}

/// A taxonomy of all canonical ids (id -> kind).
pub type Taxonomy = std::collections::BTreeMap<String, EntryKind>;

impl From<&Ontology> for Taxonomy {
    fn from(o: &Ontology) -> Self {
        o.entries
            .iter()
            .map(|(id, e)| (id.clone(), e.kind))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_ontology_has_no_duplicates() {
        let o = generic_service_ontology();
        assert_eq!(o.entries.len(), GENERIC_SERVICE_FAILURES.len());
    }

    #[test]
    fn taxonomy_classification() {
        assert_eq!(classify("failure.network.timeout").1, "network");
        assert_eq!(classify("failure.database.unavailable").1, "database");
    }
}
