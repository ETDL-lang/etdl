//! ETDL Reliability — richer reliability engineering library for the
//! Reliability Supplement (`etdl.reliability`).
//!
//! This crate is the **optional** richer layer on top of the built-in
//! [`etdl_reliability_core`] crate. It re-exports the built-in types
//! (probability estimates, artifacts, resolution) and adds:
//!
//! - [`analysis`] — statistical estimation (empirical / Wilson, Beta-Binomial
//!   Bayesian, exponential model), sensitivity / importance
//! - [`evidence`], [`observation`] — immutable runtime observations
//! - [`failure`], [`dependency`] — failure modes and dependencies
//!
//! The compiler does **not** depend on this crate; it depends only on
//! `etdl-reliability-core`. This crate is for engineers performing analysis and
//! producing reliability artifacts that the ordinary ETDL compiler then
//! consumes.
//!
//! The invariant maintained across the ecosystem:
//!
//! - **Ontology** — what is this? ([`failure`] IDs; the ontology crate)
//! - **Evidence** — what happened? ([`observation`], [`evidence`])
//! - **Reliability model** — how likely is it? (built-in estimates)
//! - **Analysis** — what does the model imply? ([`analysis`])
//! - **ETDL** — how does the system behave? (the compiler, elsewhere)

pub mod analysis;
pub mod calibration;
pub mod dataset;
pub mod dependency;
pub mod evidence;
pub mod failure;
pub mod observation;
pub mod observations;
pub mod predictive;
pub mod probability_adapter;
pub mod review;
pub mod selection;
pub mod trace;
pub mod tree_adapter;

// Re-export the built-in layer so `etdl_reliability::ProbabilityEstimate`
// etc. continue to resolve (the compiler only depends on the core crate).
pub use etdl_reliability_core::ProbabilityState;
pub use etdl_reliability_core::{
    artifact, distribution, estimate, probability, provenance, uncertainty, validation,
};
pub use etdl_reliability_core::{
    ArtifactResolver, ConfidenceInterval, ProbabilityEstimate, ProbabilityMetric,
    ProbabilitySource, Provenance, ReliabilityError, ResolutionContext, ResolvedProbability,
    TimeBasis, Uncertainty,
};
pub use failure::{FailureMode, FailureModeId, FailureSource};
pub use observation::{EventObservation, FailureObservation, ReliabilityObservation};
pub use review::{ReviewRecord, ReviewStatus, ReviewedFailureMode};
pub use trace::{trace_from_estimate, EstimateTrace};
