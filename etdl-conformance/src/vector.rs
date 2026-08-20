//! The normative test-vector schema shared by every conformance suite in
//! this crate.
//!
//! A [`ConformanceVector`] identifies *what* is being checked and *why* —
//! it carries no input/expected-output payload of its own, because those
//! vary too widely across domains (ETDL source text, a numeric formula, an
//! artifact JSON document) to usefully force into one generic field. Each
//! `#[test]` function owns its own case data and asserts against it
//! directly; the vector is metadata attached to that test for reporting,
//! traceability, and stability purposes — see `docs/reference/
//! conformance-framework.md` for the full methodology.

use serde::{Deserialize, Serialize};

/// Conformance levels, matching the layering this workspace already has
/// (parser -> compiler -> stdlib -> supplements -> artifacts -> runtime ->
/// WASM), not an invented hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// Concrete syntax: does it parse? (`etdl-parser`)
    Syntax = 0,
    /// Semantic/compiler conformance: validation diagnostics, fault-tree
    /// resolution, type checking. (`etdl-compiler`)
    Semantic = 1,
    /// Standard-library conformance: `libraries:` import resolution,
    /// built-in `std.*` modules. (`etdl-compiler::stdlib`,
    /// `etdl-probability-core`)
    StandardLibrary = 2,
    /// Supplement conformance: Generic Tree Event, Reliability, Predictive
    /// Reliability, Runtime Feedback & Calibration.
    Supplement = 3,
    /// Artifact/serialization conformance: `ReliabilityArtifact` and
    /// friends round-trip, preserve schema/version/provenance.
    Artifact = 4,
    /// Runtime behavioral conformance: `BranchMonitor`, observation
    /// emission, calibration against fixed vectors.
    Runtime = 5,
    /// WASM conformance: the WASM-safe subset builds and behaves
    /// identically to the native path for the operations it exposes.
    Wasm = 6,
    /// Full ETDL implementation conformance: every lower level holds
    /// together for at least one complete, realistic scenario.
    Full = 7,
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = *self as u8;
        let name = match self {
            Level::Syntax => "syntax",
            Level::Semantic => "semantic",
            Level::StandardLibrary => "standard-library",
            Level::Supplement => "supplement",
            Level::Artifact => "artifact",
            Level::Runtime => "runtime",
            Level::Wasm => "wasm",
            Level::Full => "full",
        };
        write!(f, "L{n}-{name}")
    }
}

/// Whether a vector is currently normative. Mirrors the existing
/// `docs/DIAGNOSTICS.md` discipline of "codes are stable within a MAJOR
/// version; new codes are added, never reused" — vectors follow the same
/// rule: a `Deprecated` vector is kept (for backward-compatibility
/// testing, see §19/20) rather than deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VectorStatus {
    Active,
    Experimental,
    Deprecated,
}

/// A structured diagnostic category, for classifying *negative* test
/// expectations without requiring exact diagnostic text (per this task's
/// own instruction: "Do not require exact diagnostic text unless the
/// specification says it is normative"). These map loosely onto the
/// existing `E-1xx`/`V-1xx..V-5xx`/`W-4xx`/`RA0xx`/`RC0xx` code families
/// documented in `docs/DIAGNOSTICS.md` — this enum classifies, it does not
/// replace or duplicate that code registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCategory {
    Syntax,
    Type,
    Module,
    Import,
    Unit,
    Probability,
    Reliability,
    Tree,
    Artifact,
    Configuration,
}

/// A normative conformance test vector: identity and traceability metadata
/// for one `#[test]`. See module docs for why it carries no payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConformanceVector {
    /// Stable ID, e.g. `"PRED-001"`. Never reused for a different
    /// requirement once published (same discipline as diagnostic codes).
    pub id: &'static str,
    pub level: Level,
    /// Where the normative requirement comes from: a specification
    /// section, or (for supplements the core spec does not define) the
    /// reference doc section that serves as that supplement's own
    /// authority, e.g. `"docs/reference/predictive-reliability-
    /// supplement.md#exponential-model"`.
    pub spec_ref: &'static str,
    /// The semantic requirement being checked, in one sentence.
    pub requirement: &'static str,
    /// The conformance-suite version this vector was introduced in.
    pub version: &'static str,
    pub status: VectorStatus,
}

impl ConformanceVector {
    pub const fn new(
        id: &'static str,
        level: Level,
        spec_ref: &'static str,
        requirement: &'static str,
    ) -> Self {
        ConformanceVector {
            id,
            level,
            spec_ref,
            requirement,
            version: crate::CONFORMANCE_SUITE_VERSION,
            status: VectorStatus::Active,
        }
    }
}
