//! ETDL Conformance, Verification & Validation 1.0.
//!
//! Answers "how do we know an ETDL implementation actually implements the
//! ETDL specification and supplements correctly?" — by defining explicit
//! conformance [`vector::Level`]s, normative [`vector::ConformanceVector`]s,
//! an [`reference`] oracle independent of the implementation's own math,
//! and a machine-readable [`manifest::ConformanceManifest`] +
//! [`report`] the CLI's `etdl conformance` subcommand consumes.
//!
//! # This extends, it does not replace
//!
//! `conformance/conformance.rs` (Level 0/1: parser + compiler + fault-tree
//! probability, 12 cases, wired into `etdl-compiler/tests/
//! conformance_test.rs`) already existed before this crate and is
//! unchanged. This crate adds Levels 2-7 — standard library, supplements
//! (Generic Tree Event, Reliability, Predictive Reliability, Runtime
//! Feedback & Calibration), artifact/serialization, runtime behavior, and
//! WASM — plus the manifest/report/dependency-graph infrastructure none of
//! the existing suites provide. See `docs/reference/
//! conformance-framework.md` for the full methodology and
//! `docs/CONFORMANCE.md` for how the two relate.
//!
//! # No self-certification loop
//!
//! [`reference`] is coded independently of every crate under test — see
//! its module docs. A conformance vector that only re-asserts what the
//! implementation itself computed would prove nothing; every numerical
//! vector in this crate's `tests/` compares the implementation's output to
//! either a value computed by [`reference`] or a hand-derived textbook
//! constant, never to a second call into the same formula.

pub mod depgraph;
pub mod levels;
pub mod manifest;
pub mod reference;
pub mod report;
pub mod vector;

/// The conformance *suite's own* version — distinct from the ETDL language
/// version, the workspace crate version, and any individual supplement's
/// version. Bumped when vectors are added, changed, or deprecated; see
/// `docs/reference/conformance-framework.md#versioning`.
pub const CONFORMANCE_SUITE_VERSION: &str = "1.0.0";

/// The ETDL core language version this suite targets (matches `etdl:
/// "1.x.y"` in a document's `etdl` field and the E-100 diagnostic's
/// accepted range).
pub const ETDL_LANGUAGE_VERSION: &str = "1.0.0";
