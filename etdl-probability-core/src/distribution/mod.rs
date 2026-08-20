//! Foundational probability distributions: [`bernoulli::Bernoulli`],
//! [`binomial::Binomial`], [`beta::Beta`], [`exponential::Exponential`],
//! [`normal::Normal`].
//!
//! Each distribution is a validated, immutable value (construction rejects
//! invalid parameters — no silent repair) exposing the mathematically
//! correct operations for its kind, under their correct names: `pmf` for
//! discrete distributions, `pdf` for continuous ones, `cdf` for all of
//! them, plus `mean`/`variance` and (where the math is cheap) `quantile`.
//! None of them expose `sample()` — see the crate-level docs' "Determinism"
//! section for why random sampling is out of scope here.
//!
//! This is a deliberately small, foundational set (matching
//! `docs/reference/standard-probability-library.md`'s "first
//! distributions" scope) — not a general statistics library. A hazard
//! rate / survival function / time-dependent distribution abstraction for
//! predictive reliability is future work; see the crate-level docs.

pub mod bernoulli;
pub mod beta;
pub mod binomial;
pub mod exponential;
pub mod normal;

pub use bernoulli::Bernoulli;
pub use beta::Beta;
pub use binomial::Binomial;
pub use exponential::Exponential;
pub use normal::Normal;
