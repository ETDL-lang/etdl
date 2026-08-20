//! ETDL Predictive Reliability Supplement 1.0.
//!
//! Extends "what is the estimated probability?" (the existing reliability
//! engine, unchanged) to "what is the predicted probability/reliability
//! under specified conditions, over a specified future time/exposure?"
//!
//! ```text
//! ETDL Core
//!    |
//! std.probability (etdl-probability-core)
//!    |
//! [std.reliability facade — not yet built; etdl-reliability plays this
//!  role today. This module builds on the EXISTING reliability engine
//!  directly, not a facade that doesn't exist yet. See the crate-level
//!  docs and docs/reference/predictive-reliability-supplement.md.]
//!    |
//! Predictive Reliability     <- this module
//!    |
//! ReliabilityArtifact / predictive metadata on top of it
//! ```
//!
//! ```text
//! Generic Tree Event (etdl-tree-core)
//!    |
//! Reliability interpretation (crate::tree_adapter, UNCHANGED)
//!    |
//! Predictive Reliability (crate::predictive::tree, reuses tree_adapter
//!    as-is — no new tree-composition logic)
//! ```
//!
//! # Prediction vs. estimation vs. observation
//!
//! - An **estimate** (`etdl_reliability_core::estimate::ProbabilityEstimate`,
//!   unchanged) is inference about an existing quantity from available
//!   evidence — no time horizon.
//! - An **observation** (`crate::observations::AggregateObservation`,
//!   unchanged) is a record of what happened.
//! - A **prediction** ([`PredictiveResult`]) is an expected future outcome
//!   *over a specified time/exposure interval*, computed from a
//!   [`models::TimeToFailureModel`] whose parameters typically originate
//!   from an estimate. These are never collapsed into one type.
//!
//! # Determinism
//!
//! Every function here is closed-form/analytical and deterministic — no
//! sampling anywhere in this module. Monte Carlo / Bayesian posterior
//! predictive simulation is explicitly out of scope for 1.0 (it would
//! reuse `crate::analysis::dependence::monte_carlo`'s existing seeded
//! sampler, not a new one) — see
//! `docs/reference/predictive-reliability-supplement.md`.

pub mod calibration_adapter;
pub mod censoring;
pub mod models;
pub mod tree;

use serde::{Deserialize, Serialize};

use crate::analysis::dependence::ArtifactRef;

/// Schema identity for the Predictive Reliability Supplement.
pub const PREDICTIVE_SCHEMA: &str = "etdl.predictive-reliability/1.0";

/// An explicit mission/exposure duration. Mirrors [`crate::observations::AggregateObservation`]'s
/// discipline of an explicit exposure basis: a prediction never silently
/// assumes a time horizon. `unit` is a free-text label (the same
/// `std.units` deferral documented in `docs/reference/standard-library.md`
/// applies here — there is no checked unit type in ETDL yet, so
/// dimensional consistency between a rate's unit and a mission time's unit
/// is the caller's responsibility, stated explicitly rather than silently
/// assumed compatible).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionTime {
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum MissionTimeError {
    #[error("mission time {0} is not finite (NaN or infinity)")]
    NotFinite(f64),
    #[error("mission time {0} is negative")]
    Negative(f64),
}

impl MissionTime {
    pub fn new(value: f64, unit: impl Into<String>) -> Result<Self, MissionTimeError> {
        if !value.is_finite() {
            return Err(MissionTimeError::NotFinite(value));
        }
        if value < 0.0 {
            return Err(MissionTimeError::Negative(value));
        }
        Ok(MissionTime {
            value,
            unit: unit.into(),
        })
    }
}

/// Which predictive quantity a [`PredictiveResult`] reports. Kept as a
/// closed, named set (never a bare unlabeled `f64`) precisely so hazard is
/// never confused with failure probability, and survival is never confused
/// with reliability unless the domain interpretation explicitly says so
/// (`Reliability` and `Survival` are reported as the SAME formula, `S(t)`,
/// under two different names, because for a non-repairable system they are
/// the same quantity by definition — see
/// `docs/reference/predictive-reliability-supplement.md` §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PredictiveQuantity {
    /// `S(t) = P(T > t)`.
    Survival,
    /// `R(t) = P(T > t)`, the reliability-domain reading of `S(t)` for a
    /// non-repairable system — mathematically identical to `Survival`,
    /// named separately so a result can state which interpretation was
    /// intended without inventing a second formula.
    Reliability,
    /// `F(t) = P(T <= t) = 1 - S(t)`.
    FailureProbability,
    /// `h(t)`: instantaneous failure rate given survival to `t`. NOT a
    /// probability — never in `[0, 1]` in general (e.g. Weibull with
    /// shape > 1 has unbounded hazard as `t -> infinity`).
    Hazard,
    /// `H(t) = -ln(S(t))`, the integral of hazard from 0 to `t`.
    CumulativeHazard,
    /// `f(t)`, the failure-time density (continuous models only).
    Density,
}

/// Identifies the model that produced a [`PredictiveResult`]: which family,
/// its parameters, and the assumptions it makes. Never hidden — every
/// prediction carries this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDescriptor {
    /// e.g. `"exponential"`, `"weibull"`.
    pub family: String,
    pub parameters: std::collections::BTreeMap<String, f64>,
    /// Explicit model assumptions (e.g. `"constant hazard"`,
    /// `"non-repairable"`, `"independent trials"`) — stated, never implied.
    pub assumptions: Vec<String>,
    /// The time range this model is asserted valid for, if declared
    /// (`(lower, upper)`, same unit as the mission time it is evaluated
    /// against). `None` means no validity bound was declared — this is
    /// itself informative (never invented by this crate).
    #[serde(default)]
    pub valid_range: Option<(f64, f64)>,
}

/// Where a prediction's parameters came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictiveProvenance {
    /// The `ReliabilityArtifact`/estimate this model's parameters were
    /// read from, if any (see `calibration_adapter`). `None` for a model
    /// constructed directly from literal parameters (e.g. in a worked
    /// example) — a plain mathematical prediction needs no artificial
    /// provenance, matching the same principle already applied to
    /// `etdl-probability-core::Probability`.
    #[serde(default)]
    pub source_artifact: Option<ArtifactRef>,
    #[serde(default)]
    pub source_estimate: Option<String>,
    pub analyzer: String,
    pub analyzer_version: String,
    #[serde(default)]
    pub generated_at: Option<String>,
}

impl PredictiveProvenance {
    pub fn new() -> Self {
        PredictiveProvenance {
            source_artifact: None,
            source_estimate: None,
            analyzer: "etdl-reliability::predictive".to_string(),
            analyzer_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: None,
        }
    }
}

impl Default for PredictiveProvenance {
    fn default() -> Self {
        Self::new()
    }
}

/// The result of a predictive query: one [`PredictiveQuantity`], for one
/// event, at one mission time, under one model, with explicit conditions
/// and extrapolation status. This is a distinct type from
/// `ProbabilityEstimate` — a prediction always carries a time horizon; an
/// estimate never does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictiveResult {
    pub schema: String,
    pub event: String,
    pub quantity: PredictiveQuantity,
    pub value: f64,
    pub time: MissionTime,
    pub model: ModelDescriptor,
    /// Operating conditions this prediction was made under (e.g.
    /// `"normal"`, `"high-load"`) — free-text, mirroring the same
    /// `conditions: Vec<String>` convention `ProbabilityEstimate` and
    /// `AggregateObservation` already use. Never inferred; carried through
    /// only if the caller supplied them.
    #[serde(default)]
    pub conditions: Vec<String>,
    /// `true` when `time` falls outside `model.valid_range` (only ever
    /// computed when a range was actually declared — see
    /// [`ModelDescriptor::valid_range`]; `false` when no range was
    /// declared, since "not observed to be invalid" is not the same claim
    /// as "confirmed within range").
    pub extrapolated: bool,
    pub provenance: PredictiveProvenance,
}

impl PredictiveResult {
    /// Builds a [`PredictiveResult`], computing `extrapolated` from
    /// `model.valid_range` (see [`ModelDescriptor::valid_range`]) rather
    /// than requiring every call site to duplicate that comparison. When no
    /// range was declared, `extrapolated` is `false` — "no declared range"
    /// is not evidence of validity, but this function never invents a
    /// range to compare against, so it cannot claim extrapolation either.
    pub fn new(
        event: impl Into<String>,
        quantity: PredictiveQuantity,
        value: f64,
        time: MissionTime,
        model: ModelDescriptor,
        conditions: Vec<String>,
        provenance: PredictiveProvenance,
    ) -> Self {
        let extrapolated = match model.valid_range {
            Some((lower, upper)) => time.value < lower || time.value > upper,
            None => false,
        };
        PredictiveResult {
            schema: PREDICTIVE_SCHEMA.to_string(),
            event: event.into(),
            quantity,
            value,
            time,
            model,
            conditions,
            extrapolated,
            provenance,
        }
    }
}
