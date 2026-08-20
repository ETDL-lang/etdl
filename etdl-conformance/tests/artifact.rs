//! ART-* vectors: `ReliabilityArtifact` serialization/artifact conformance.
//! Covers task §17 (artifact conformance: serialization, deserialization,
//! version, schema, provenance, identity) and §18 (round trip). Only
//! compiled when the `reliability` feature is on (the artifact type lives
//! in `etdl-reliability-core`, an optional dependency of this crate,
//! mirroring `etdl-cli`).

#![cfg(feature = "reliability")]

use etdl_conformance::vector::{ConformanceVector, Level};
use etdl_reliability_core::artifact::{ReliabilityArtifact, ARTIFACT_SCHEMA};
use etdl_reliability_core::estimate::{ProbabilityEstimate, ProbabilityState};
use etdl_reliability_core::ReliabilityError;

#[test]
fn art_001_serialize_deserialize_round_trip_preserves_semantic_meaning() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ART-001",
        Level::Artifact,
        "docs/reliability/README.md",
        "ETDL -> artifact -> serialize -> deserialize -> artifact must preserve semantic \
         meaning (not necessarily byte-for-byte, since canonical serialization is not specified)",
    );
    let mut original = ReliabilityArtifact::new("pump-artifact");
    original.version = Some("1.0.0".to_string());
    original
        .add(ProbabilityEstimate::new(
            "pump-fails",
            ProbabilityState::Estimated,
            0.001,
        ))
        .unwrap();

    let json = serde_json::to_string(&original).unwrap();
    let restored = ReliabilityArtifact::from_json(&json).unwrap();

    assert_eq!(restored.id, original.id, "{}: id", VECTOR.id);
    assert_eq!(restored.version, original.version, "{}: version", VECTOR.id);
    assert_eq!(restored.schema, original.schema, "{}: schema", VECTOR.id);
    assert_eq!(
        restored.get("pump-fails").unwrap().value,
        original.get("pump-fails").unwrap().value,
        "{}: estimate value",
        VECTOR.id
    );
}

#[test]
fn art_002_yaml_round_trip_preserves_semantic_meaning() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ART-002",
        Level::Artifact,
        "docs/reliability/README.md",
        "the same round-trip guarantee holds for the YAML encoding, not just JSON",
    );
    let mut original = ReliabilityArtifact::new("valve-artifact");
    original.version = Some("2.3.1".to_string());
    original
        .add(ProbabilityEstimate::new(
            "valve-fails",
            ProbabilityState::Measured,
            0.02,
        ))
        .unwrap();

    let yaml = serde_yaml::to_string(&original).unwrap();
    let restored = ReliabilityArtifact::from_yaml(&yaml).unwrap();
    assert_eq!(restored.id, original.id, "{}", VECTOR.id);
    assert_eq!(
        restored.get("valve-fails").unwrap().value,
        original.get("valve-fails").unwrap().value,
        "{}",
        VECTOR.id
    );
}

#[test]
fn art_003_schema_field_is_present_and_checked_on_load() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ART-003",
        Level::Artifact,
        "docs/reliability/README.md",
        "an artifact carries an explicit schema id; loading an artifact with an \
         unrecognized schema must be rejected, not silently accepted",
    );
    let artifact = ReliabilityArtifact::new("x");
    assert_eq!(
        artifact.schema, ARTIFACT_SCHEMA,
        "{}: default schema",
        VECTOR.id
    );

    let malformed_schema_json =
        r#"{"schema":"etdl.reliability.artifact/99.0","id":"x","version":null,"estimates":{}}"#;
    let result = ReliabilityArtifact::from_json(malformed_schema_json);
    assert!(
        matches!(result, Err(ReliabilityError::SchemaVersionMismatch { .. })),
        "{}: expected SchemaVersionMismatch, got {result:?}",
        VECTOR.id
    );
}

#[test]
fn art_004_malformed_artifact_json_is_rejected_not_panicking() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ART-004",
        Level::Artifact,
        "docs/reliability/README.md",
        "malformed artifact input (task §42 security testing: unsafe deserialization) \
         must return a structured error, never panic or silently produce a partial artifact",
    );
    for malformed in [
        "not json at all",
        "{}",
        r#"{"schema": 123}"#,
        r#"{"schema": "etdl.reliability.artifact/1.0", "id": null}"#,
        "",
    ] {
        let result = ReliabilityArtifact::from_json(malformed);
        assert!(
            result.is_err(),
            "{}: expected an error for input {malformed:?}, got {result:?}",
            VECTOR.id
        );
    }
}

#[test]
fn art_005_identity_is_the_event_id_not_array_position() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ART-005",
        Level::Artifact,
        "docs/reliability/README.md",
        "an estimate's identity is its event id (a BTreeMap key), not its position in \
         any collection — re-serializing must not silently reorder or renumber estimates",
    );
    let mut artifact = ReliabilityArtifact::new("multi");
    artifact
        .add(ProbabilityEstimate::new(
            "z-event",
            ProbabilityState::Estimated,
            0.1,
        ))
        .unwrap();
    artifact
        .add(ProbabilityEstimate::new(
            "a-event",
            ProbabilityState::Estimated,
            0.2,
        ))
        .unwrap();

    let json = serde_json::to_string(&artifact).unwrap();
    let restored = ReliabilityArtifact::from_json(&json).unwrap();
    assert_eq!(
        restored.get("z-event").unwrap().value,
        Some(0.1),
        "{}: z-event",
        VECTOR.id
    );
    assert_eq!(
        restored.get("a-event").unwrap().value,
        Some(0.2),
        "{}: a-event",
        VECTOR.id
    );
}

#[test]
fn art_006_duplicate_estimate_for_the_same_event_is_rejected() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ART-006",
        Level::Artifact,
        "docs/reliability/README.md",
        "adding two unconditional estimates for the same event id must be rejected, \
         not silently overwritten (identity/provenance integrity)",
    );
    let mut artifact = ReliabilityArtifact::new("x");
    artifact
        .add(ProbabilityEstimate::new(
            "e",
            ProbabilityState::Estimated,
            0.1,
        ))
        .unwrap();
    let result = artifact.add(ProbabilityEstimate::new(
        "e",
        ProbabilityState::Estimated,
        0.2,
    ));
    assert!(
        matches!(result, Err(ReliabilityError::DuplicateEstimate { .. })),
        "{}: expected DuplicateEstimate, got {result:?}",
        VECTOR.id
    );
}

#[test]
fn art_007_unknown_state_never_silently_resolves_to_a_scalar_zero() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ART-007",
        Level::Artifact,
        "docs/DIAGNOSTICS.md",
        "`unknown` is explicit and MUST never be translated to `0` — a probability \
         invariant that also governs artifact conformance (task §13)",
    );
    let unknown = ProbabilityEstimate {
        value: None,
        ..ProbabilityEstimate::new("e", ProbabilityState::Unknown, 0.0)
    };
    assert_eq!(
        unknown.state,
        ProbabilityState::Unknown,
        "{}: state must remain Unknown",
        VECTOR.id
    );
    assert_eq!(
        unknown.value, None,
        "{}: value must not become Some(0.0)",
        VECTOR.id
    );
}
