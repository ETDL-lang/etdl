//! Probability estimates and their state.

use serde::{Deserialize, Serialize};

use crate::probability::{ProbabilityMetric, ProbabilitySource, TimeBasis};
use crate::provenance::Provenance;
use crate::uncertainty::Uncertainty;

/// The state of a probability value. `unknown` is explicit and MUST never be
/// translated to `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProbabilityState {
    Declared,
    Assumed,
    Measured,
    Estimated,
    Predicted,
    Inferred,
    Imported,
    Unknown,
}

/// A formal probability estimate with provenance, uncertainty, and conditions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbabilityEstimate {
    /// Stable identity of what this estimate describes (e.g. a failure-mode id).
    pub event: String,
    pub state: ProbabilityState,
    /// The value. `None` when the state is `Unknown` or the estimate is
    /// uncertainty-only.
    pub value: Option<f64>,
    #[serde(default)]
    pub metric: ProbabilityMetric,
    #[serde(default)]
    pub population: Option<String>,
    #[serde(default)]
    pub time_basis: Option<TimeBasis>,
    #[serde(default)]
    pub conditions: Vec<String>,
    #[serde(default)]
    pub source: ProbabilitySource,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub uncertainty: Option<Uncertainty>,
    #[serde(default)]
    pub provenance: Option<Provenance>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

impl ProbabilityEstimate {
    pub fn new(event: impl Into<String>, state: ProbabilityState, value: f64) -> Self {
        ProbabilityEstimate {
            event: event.into(),
            state,
            value: Some(value),
            metric: ProbabilityMetric::Probability,
            population: None,
            time_basis: None,
            conditions: Vec::new(),
            source: ProbabilitySource::Literal,
            method: None,
            uncertainty: None,
            provenance: None,
            version: None,
            status: None,
        }
    }

    /// A declared estimate with an explicit `unknown` state and no value.
    pub fn unknown(event: impl Into<String>) -> Self {
        ProbabilityEstimate {
            event: event.into(),
            state: ProbabilityState::Unknown,
            value: None,
            metric: ProbabilityMetric::Probability,
            population: None,
            time_basis: None,
            conditions: Vec::new(),
            source: ProbabilitySource::Unknown,
            method: None,
            uncertainty: None,
            provenance: None,
            version: None,
            status: None,
        }
    }

    pub fn is_unknown(&self) -> bool {
        self.state == ProbabilityState::Unknown || self.value.is_none()
    }

    /// A canonical, deterministic key identifying this estimate's context.
    ///
    /// Distinguishes estimates for the same failure mode under different
    /// conditions, populations, metrics, or time bases, so they are never
    /// silently merged. The key is: `event::metric::time_basis::population::c1,c2::version`.
    pub fn key(&self) -> String {
        let mut conditions = self.conditions.clone();
        conditions.sort();
        conditions.dedup();
        format!(
            "{}::{:?}::{}::{}::{}::{}",
            self.event,
            self.metric,
            self.time_basis.map(|t| t.to_string()).unwrap_or_default(),
            self.population.clone().unwrap_or_default(),
            conditions.join(","),
            self.version.clone().unwrap_or_default(),
        )
    }

    /// Validate that the value, if present, is a finite number appropriate for
    /// the estimate's metric. NaN and infinities are always invalid.
    ///
    /// Returns `None` when valid, or a description of the first problem found.
    pub fn validate_value(&self) -> Option<ReliabilityError> {
        let v = self.value?;
        if !v.is_finite() {
            return Some(ReliabilityError::NonFiniteValue(v));
        }
        if self.metric.is_probability_like() && !(0.0..=1.0).contains(&v) {
            return Some(ReliabilityError::OutOfRange(v));
        }
        if !self.metric.is_probability_like() && v < 0.0 {
            return Some(ReliabilityError::OutOfRange(v));
        }
        None
    }

    /// Resolve to a deterministic scalar probability.
    ///
    /// Returns an error when the estimate is unknown or the metric is not
    /// probability-like (conversion between metrics is not done implicitly).
    pub fn resolved_probability(&self) -> Result<f64, ReliabilityError> {
        if self.is_unknown() {
            return Err(ReliabilityError::UnknownProbability(self.event.clone()));
        }
        let v = self
            .value
            .ok_or_else(|| ReliabilityError::UnknownProbability(self.event.clone()))?;
        if !self.metric.is_probability_like() {
            return Err(ReliabilityError::NonProbabilityMetric(self.metric));
        }
        if !v.is_finite() {
            return Err(ReliabilityError::NonFiniteValue(v));
        }
        if !(0.0..=1.0).contains(&v) {
            return Err(ReliabilityError::OutOfRange(v));
        }
        Ok(v)
    }
}

/// Reliability-layer errors. These map to ETDL diagnostics in the compiler
/// rather than replacing the ETDL diagnostic system.
#[derive(Debug, thiserror::Error)]
pub enum ReliabilityError {
    #[error("probability for event '{0}' is unknown and was not set to zero")]
    UnknownProbability(String),
    #[error("metric {0:?} is not probability-like; conversion is not implicit")]
    NonProbabilityMetric(ProbabilityMetric),
    #[error("probability value {0} is outside [0,1]")]
    OutOfRange(f64),
    #[error("probability value {0} is not finite (NaN or infinity)")]
    NonFiniteValue(f64),
    #[error("malformed reliability artifact: {0}")]
    MalformedArtifact(String),
    #[error("external source '{0}' could not be resolved")]
    UnresolvedSource(String),
    #[error("artifact schema version mismatch: found {found}, expected {expected}")]
    SchemaVersionMismatch { found: String, expected: String },
    #[error(
        "duplicate estimate for event '{event}' with canonical key '{key}'; refusing to overwrite"
    )]
    DuplicateEstimate { event: String, key: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_resolves() {
        let e = ProbabilityEstimate::new("failure.db.timeout", ProbabilityState::Declared, 0.008);
        assert_eq!(e.resolved_probability().unwrap(), 0.008);
    }

    #[test]
    fn unknown_never_resolves_to_zero() {
        let e = ProbabilityEstimate::unknown("failure.x");
        assert!(e.is_unknown());
        assert!(e.resolved_probability().is_err());
    }

    #[test]
    fn non_probability_metric_rejected() {
        let mut e = ProbabilityEstimate::new("failure.x", ProbabilityState::Measured, 2.0);
        e.metric = ProbabilityMetric::FailureRate;
        assert!(e.resolved_probability().is_err());
    }

    #[test]
    fn out_of_range_rejected() {
        let e = ProbabilityEstimate::new("failure.x", ProbabilityState::Declared, 1.5);
        assert!(e.resolved_probability().is_err());
    }

    #[test]
    fn nan_rejected() {
        let e = ProbabilityEstimate::new("failure.x", ProbabilityState::Declared, f64::NAN);
        assert!(matches!(
            e.resolved_probability(),
            Err(ReliabilityError::NonFiniteValue(_))
        ));
        assert!(e.validate_value().is_some());
    }

    #[test]
    fn infinity_rejected() {
        for v in [f64::INFINITY, f64::NEG_INFINITY] {
            let e = ProbabilityEstimate::new("failure.x", ProbabilityState::Declared, v);
            assert!(matches!(
                e.resolved_probability(),
                Err(ReliabilityError::NonFiniteValue(_))
            ));
        }
    }

    #[test]
    fn validate_value_accepts_finite_in_range() {
        let e = ProbabilityEstimate::new("failure.x", ProbabilityState::Declared, 0.5);
        assert!(e.validate_value().is_none());
        assert_eq!(e.resolved_probability().unwrap(), 0.5);
    }

    #[test]
    fn validate_value_allows_non_negative_rates() {
        let mut e = ProbabilityEstimate::new("failure.x", ProbabilityState::Measured, 2.0);
        e.metric = ProbabilityMetric::FailureRate;
        assert!(e.validate_value().is_none());
        // Negative rates are nonsensical for any metric.
        e.value = Some(-1.0);
        assert!(e.validate_value().is_some());
        // ...but rates are never usable as probabilities.
        assert!(e.resolved_probability().is_err());
    }
}
