//! Ontology mapping tests for discovery candidates.

use etdl_failure_discovery::mapping::{MappingQuality, OntologyMapping};
use etdl_failure_discovery::ontology::OntologyView;
use etdl_failure_discovery::rust::patterns::{pattern_mapping, RustPattern};

/// Build an ontology view where one concept is deprecated into another, to
/// exercise the Deprecated mapping quality without mutating the shared ontology.
fn ontology_with_deprecated() -> OntologyView {
    let mut o = etdl_reliability_ontology::taxonomy::generic_service_ontology();
    o.deprecate("failure.network.timeout", "failure.dependency.timeout");
    OntologyView::from_ontology(o)
}

#[test]
fn exact_mapping_for_existing_concept() {
    let view = OntologyView::generic_service();
    let m = pattern_mapping(RustPattern::Timeout, &view);
    assert_eq!(m.quality, MappingQuality::Exact);
    assert_eq!(m.canonical_id.as_deref(), Some("failure.network.timeout"));
}

#[test]
fn unmapped_proposes_new_concept() {
    let view = OntologyView::generic_service();
    let m = pattern_mapping(RustPattern::CustomError, &view);
    assert_eq!(m.quality, MappingQuality::Unmapped);
    assert!(m.proposed_concept.is_some());
    assert!(m.canonical_id.is_none());
}

#[test]
fn deprecated_ontology_maps_to_alive() {
    let view = ontology_with_deprecated();
    // Timeout's default id is deprecated into failure.dependency.timeout.
    let m = pattern_mapping(RustPattern::Timeout, &view);
    assert_eq!(m.quality, MappingQuality::Deprecated);
    assert_eq!(
        m.canonical_id.as_deref(),
        Some("failure.dependency.timeout")
    );
    assert!(
        m.evidence.iter().any(|e| e.contains("deprecated")),
        "evidence should explain the deprecation, got {:?}",
        m.evidence
    );
}

#[test]
fn merged_ontology_concept_resolves_alive() {
    // Simulate a merged concept by deprecating into another.
    let view = ontology_with_deprecated();
    let (alive, deprecated) = view.resolve("failure.network.timeout").unwrap();
    assert!(deprecated);
    assert_eq!(alive, "failure.dependency.timeout");
}

#[test]
fn unmapped_constructor_marks_proposed() {
    let m = OntologyMapping::unmapped("failure.application.new_concept");
    assert_eq!(m.quality, MappingQuality::Unmapped);
    assert_eq!(
        m.proposed_concept.as_deref(),
        Some("failure.application.new_concept")
    );
}

#[test]
fn ambiguous_mapping_is_supported() {
    // The mapping model supports Ambiguous; verify it round-trips.
    let m = OntologyMapping {
        canonical_id: None,
        proposed_concept: None,
        quality: MappingQuality::Ambiguous,
        confidence: 0.5,
        evidence: vec!["multiple candidates".into()],
    };
    let s = serde_json::to_string(&m).unwrap();
    let back: OntologyMapping = serde_json::from_str(&s).unwrap();
    assert_eq!(back.quality, MappingQuality::Ambiguous);
}
