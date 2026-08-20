//! ETDL Standard Probability Library — native layer (`std.probability`'s
//! Rust API).
//!
//! This crate is domain-neutral mathematical foundation: [`Probability`],
//! [`Rate`], and a small set of foundational [`distribution`]s (Bernoulli,
//! Binomial, Beta, Exponential, Normal), plus explicit composition
//! operations (complement, independent AND/OR, conditional probability,
//! Bayes' rule).
//!
//! # Where this sits
//!
//! ```text
//! ETDL Core
//!    |
//! ETDL Standard Library
//!    |
//! std.probability          <- this crate is the native layer beneath it
//!    |
//! Domain Libraries          (reliability, safety, security, risk, ...)
//!    |
//! User Models
//! ```
//!
//! **This crate has zero dependency on any reliability crate** — that is
//! not a convention, it is enforced by this crate's own `Cargo.toml`
//! dependency list. `etdl-reliability`/`etdl-reliability-core` may (and, via
//! a small adapter, do) depend on this crate; this crate must never depend
//! on them. See `docs/reference/standard-probability-library.md`.
//!
//! # Why this is a Rust crate, not ETDL source
//!
//! ETDL is a declarative YAML document format with no general expression or
//! function-call syntax (the only embedded mini-language, ECEL, exists
//! solely to parse barrier branch conditions — it has no arithmetic, no
//! function definitions, and cannot compute `complement(p)` or a Binomial
//! PMF). The mathematical operations this crate provides are therefore
//! genuinely native/compiled — there is no honest way to express "compute
//! the complement of a referenced probability" as ETDL YAML. What *is*
//! expressible in pure ETDL — reusable, named probability **constants** —
//! lives in `etdl-compiler/stdlib/probability/lib.etdl` (`std.probability`), resolved
//! through the existing library-import mechanism exactly like
//! `std.events`/`std.logic`. This crate is the computational counterpart a
//! future Tree Event Supplement, or a Rust-implemented domain library, links
//! against directly.
//!
//! # Determinism
//!
//! Every function in this crate is a pure, deterministic mathematical
//! evaluation. There is no random sampling anywhere in this crate — Monte
//! Carlo / sampling belongs to an optional statistics layer (the
//! reliability crate already has one, `analysis::dependence::sampling`, for
//! its own dependency-aware propagation) and is explicitly out of scope
//! here. See `docs/reference/standard-probability-library.md`'s
//! "Built-in vs. optional" section for the rationale.

pub mod distribution;
mod numerics;
pub mod probability;
pub mod rate;

pub use probability::{
    bayes, complement, conditional, independent_and, independent_and_n, independent_or,
    independent_or_n, mutually_exclusive_or, Probability, ProbabilityError,
};
pub use rate::{Rate, RateError};

/// Schema identity for this crate's version of the standard probability
/// library — distinct from the ETDL language version, from crate/Cargo
/// versions, and from `etdl.stdlib/1.0` (the stdlib package schema); see
/// `docs/reference/standard-library.md`'s versioning-axis table.
pub const STD_PROBABILITY_SCHEMA: &str = "etdl.stdlib.probability/1.0";
