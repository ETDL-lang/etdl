//! ETDL failure discovery.
//!
//! The discovery layer answers **"what failure modes are possible?"** by
//! analyzing source code. It produces **candidate** failure modes with
//! evidence, source locations, and ontology mapping; it NEVER produces
//! authoritative probabilities and never silently modifies the ontology.
//!
//! ```text
//! source code
//!     |
//!     v
//! discovery (deterministic, local, read-only)
//!     |
//!     v
//! candidate failure modes
//!     |
//!     v
//! ontology mapping (reviewed, not authoritative)
//!     |
//!     v
//! engineering review -> accepted failure mode -> reliability model
//! ```
//!
//! ## Core semantic distinction
//!
//! - **Discovered candidate** = static analysis suggests a failure is possible.
//! - **Estimated failure** = a statistical/reliability model assigns a
//!   probability or rate.
//! - **Observed failure** = something actually happened at runtime.
//!
//! Discovery confidence is **not** failure probability. A candidate with
//! `confidence = 0.92` is *not* claiming `P(failure) = 0.92`.
//!
//! ## Modules
//!
//! - [`candidate`] — [`DiscoveryCandidate`], classification, severity, evidence
//! - [`location`] — [`SourceLocation`], [`FunctionContext`]
//! - [`mapping`] — ontology mapping quality ([`MappingQuality`])
//! - [`report`] — [`DiscoveryReport`], schema, statistics, provenance
//! - [`config`] — [`DiscoveryConfig`]
//! - [`analyzer`] — [`SourceAnalyzer`] trait + registry
//! - [`rust`] — the Rust analyzer (`syn`-based)
//! - [`ontology`] — read-only ontology view
//! - [`identity`] — stable candidate identity
//! - [`source`] — project walking, hashing, project identity
//! - [`error`] — structured [`DiscoveryError`]
//! - [`bridge`] — discovery → reliability artifact bridge

pub mod analyzer;
pub mod bridge;
pub mod candidate;
pub mod config;
pub mod error;
pub mod identity;
pub mod location;
pub mod mapping;
pub mod ontology;
pub mod report;
pub mod rust;
pub mod source;

pub use analyzer::{AnalyzerRegistry, SourceAnalyzer};
pub use candidate::{
    CandidateStatus, DiscoveryCandidate, Evidence, FailureClassification, Severity,
};
pub use config::{DiscoveryConfig, OntologyPolicy};
pub use error::DiscoveryError;
pub use location::{FunctionContext, SourceLocation};
pub use mapping::{MappingQuality, OntologyMapping};
pub use ontology::OntologyView;
pub use report::{AnalyzerMetadata, DiscoveryReport, ReportStatistics, SourceIdentity};
