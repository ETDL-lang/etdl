//! ETDL Reliability — built-in layer for the Reliability Supplement
//! (`etdl.reliability`).
//!
//! This is the **built-in** reliability crate: the small, deterministic subset
//! the ETDL compiler depends on to understand and compile a reliability
//! reference. It contains:
//!
//! - [`probability`] — metrics (probability / failure rate / ...), time bases,
//!   sources
//! - [`estimate`] — probability estimates, explicit `unknown` state
//! - [`provenance`] — where an estimate came from
//! - [`uncertainty`], [`distribution`] — representation of uncertainty
//! - [`artifact`] — versioned `.rprob` artifacts, deterministic resolution, the
//!   provider seam ([`artifact::ProbabilityProvider`])
//! - [`validation`] — structural and semantic validation of estimates/artifacts
//!
//! The crate deliberately contains **no statistical algorithms**, no numerical
//! simulation, no network clients, no filesystem access, and no runtime
//! service coupling. It is WASM-compatible and dependency-light so the normal
//! ETDL compiler stays lightweight.
//!
//! The richer reliability engineering (estimation, Bayesian analysis,
//! distributions algorithms, Monte Carlo, evidence/observations, ontology,
//! discovery) lives in optional crates:
//!
//! - `etdl-reliability` — richer domain model + analysis
//! - `etdl-reliability-ontology` — canonical reliability concepts
//! - `etdl-failure-discovery` — source analysis producing failure candidates
//!
//! The invariant maintained across the ecosystem:
//!
//! - **Ontology** — what is this? (canonical IDs)
//! - **Evidence** — what happened? (observations)
//! - **Reliability model** — how likely is it? (estimates, here)
//! - **Analysis** — what does the model imply? (optional `etdl-reliability`)
//! - **ETDL** — how does the system behave? (the compiler, elsewhere)

pub mod artifact;
pub mod distribution;
pub mod estimate;
pub mod probability;
pub mod provenance;
pub mod uncertainty;
pub mod validation;

/// The version of this built-in reliability crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use artifact::{ArtifactResolver, ResolutionContext, ResolvedProbability};
pub use estimate::{ProbabilityEstimate, ProbabilityState, ReliabilityError};
pub use probability::{ProbabilityMetric, ProbabilitySource, TimeBasis};
pub use provenance::Provenance;
pub use uncertainty::{ConfidenceInterval, Interval, Uncertainty, UncertaintyKind};
