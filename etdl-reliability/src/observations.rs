//! Aggregate observations: machine-readable evidence of failures against an
//! explicit exposure.
//!
//! An [`AggregateObservation`] answers the question "out of N opportunities,
//! how many failed?" with a defined exposure unit, conditions, and time
//! interval. It is the input to the estimators. It is distinct from the
//! per-event [`ReliabilityObservation`] model, which records individual
//! occurrences.

use serde::{Deserialize, Serialize};

use crate::observation::ReliabilityObservation;
use crate::probability::TimeBasis;

/// A single aggregated observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregateObservation {
    /// Stable identity of this observation record, independent of its
    /// position in any collection. `None` for legacy/ad-hoc observations
    /// built directly for one-off estimation; an [`crate::dataset::ObservationDataset`]
    /// requires every member to carry one (array position is never identity).
    #[serde(default)]
    pub id: Option<String>,
    /// The failure-mode identity (stable, ontology-aligned).
    pub failure_mode: String,
    /// Number of opportunities / trials / exposure count.
    pub exposure: u64,
    /// Number of failures observed.
    pub failures: u64,
    /// What the exposure denominator represents (per-request, per-hour, ...).
    #[serde(default)]
    pub exposure_unit: TimeBasis,
    /// Conditions under which the observation was made (e.g. `production`,
    /// `high-load`, `region=us-east`).
    #[serde(default)]
    pub conditions: Vec<String>,
    /// Optional ISO-8601 time interval `[start, end]` during which the
    /// observation was collected.
    pub interval: Option<TimeInterval>,
    /// Where the observation came from (dataset / source name).
    #[serde(default)]
    pub source: Option<String>,
    /// Version of this observation dataset.
    #[serde(default)]
    pub version: Option<String>,
}

/// A half-open time interval with ISO-8601 string bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeInterval {
    pub start: String,
    pub end: String,
}

impl TimeInterval {
    /// Structural validation: non-empty bounds, and (for RFC 3339 timestamps,
    /// which sort lexically) `start` no later than `end`. Non-RFC-3339
    /// strings are accepted as long as non-empty; this is a guard against
    /// obviously malformed intervals, not a full calendar parser.
    pub fn validate(&self) -> Result<(), ObservationError> {
        if self.start.trim().is_empty() || self.end.trim().is_empty() {
            return Err(ObservationError::InvalidInterval {
                start: self.start.clone(),
                end: self.end.clone(),
            });
        }
        let looks_rfc3339 = |s: &str| s.len() >= 10 && s.as_bytes()[4] == b'-';
        if looks_rfc3339(&self.start) && looks_rfc3339(&self.end) && self.start > self.end {
            return Err(ObservationError::InvalidInterval {
                start: self.start.clone(),
                end: self.end.clone(),
            });
        }
        Ok(())
    }
}

/// A problem found while validating an observation. Validation never silently
/// repairs invalid data; it reports.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ObservationError {
    #[error("failure mode identity is required")]
    MissingFailureMode,
    #[error("exposure must be positive, got {0}")]
    NonPositiveExposure(u64),
    #[error("failures must be <= exposure, got failures={failures} exposure={exposure}")]
    FailuresExceedExposure { failures: u64, exposure: u64 },
    #[error("failures must not be negative, got {0}")]
    NegativeFailures(i64),
    #[error("invalid interval bounds: start='{start}' end='{end}'")]
    InvalidInterval { start: String, end: String },
}

impl AggregateObservation {
    /// Validate the observation. Rejects zero exposure, failures > exposure,
    /// negative failures, missing identity, and malformed intervals.
    pub fn validate(&self) -> Result<(), ObservationError> {
        if self.failure_mode.trim().is_empty() {
            return Err(ObservationError::MissingFailureMode);
        }
        if self.exposure == 0 {
            return Err(ObservationError::NonPositiveExposure(self.exposure));
        }
        if self.failures > self.exposure {
            return Err(ObservationError::FailuresExceedExposure {
                failures: self.failures,
                exposure: self.exposure,
            });
        }
        if let Some(iv) = &self.interval {
            iv.validate()?;
        }
        Ok(())
    }

    /// Number of successful opportunities (exposure - failures).
    pub fn successes(&self) -> u64 {
        self.exposure - self.failures
    }

    /// Empirical failure proportion `failures / exposure`, if exposure > 0.
    pub fn empirical_proportion(&self) -> Option<f64> {
        if self.exposure == 0 {
            None
        } else {
            Some(self.failures as f64 / self.exposure as f64)
        }
    }

    /// Convert into a per-event [`ReliabilityObservation`] for the aggregate
    /// (a single synthetic observation carrying the counts).
    pub fn to_observation(&self, id: &str) -> ReliabilityObservation {
        let mut o = ReliabilityObservation::new(id, &self.failure_mode, "");
        o.outcome = format!("{} failures / {} exposure", self.failures, self.exposure);
        o.conditions = self.conditions.clone();
        o.environment = self.source.clone();
        o
    }
}

/// A set of observations for one or more failure modes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObservationSet {
    pub observations: Vec<AggregateObservation>,
}

impl ObservationSet {
    pub fn new() -> Self {
        ObservationSet {
            observations: Vec::new(),
        }
    }

    /// Validate every observation; returns the first error found.
    pub fn validate(&self) -> Result<(), ObservationError> {
        for o in &self.observations {
            o.validate()?;
        }
        Ok(())
    }

    /// Aggregate observations for a single failure mode across the whole set.
    /// Sums exposure and failures (only meaningful when all share the same
    /// exposure unit and conditions).
    pub fn total_for(&self, failure_mode: &str) -> Option<(u64, u64)> {
        let mut exposure = 0u64;
        let mut failures = 0u64;
        let mut found = false;
        for o in &self.observations {
            if o.failure_mode == failure_mode {
                exposure += o.exposure;
                failures += o.failures;
                found = true;
            }
        }
        if found {
            Some((exposure, failures))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(failures: u64, exposure: u64) -> AggregateObservation {
        AggregateObservation {
            id: None,
            failure_mode: "failure.gateway.timeout".into(),
            exposure,
            failures,
            exposure_unit: TimeBasis::PerRequest,
            conditions: vec!["production".into()],
            interval: None,
            source: Some("prod-obs".into()),
            version: Some("1".into()),
        }
    }

    #[test]
    fn validates_well_formed() {
        assert!(obs(37, 100_000).validate().is_ok());
    }

    #[test]
    fn rejects_zero_exposure() {
        assert!(matches!(
            obs(0, 0).validate(),
            Err(ObservationError::NonPositiveExposure(_))
        ));
    }

    #[test]
    fn rejects_failures_exceeding_exposure() {
        assert!(matches!(
            obs(5, 3).validate(),
            Err(ObservationError::FailuresExceedExposure { .. })
        ));
    }

    #[test]
    fn rejects_missing_failure_mode() {
        let mut o = obs(1, 10);
        o.failure_mode = "".into();
        assert!(matches!(
            o.validate(),
            Err(ObservationError::MissingFailureMode)
        ));
    }

    #[test]
    fn empirical_proportion_and_successes() {
        let o = obs(37, 100_000);
        assert_eq!(o.successes(), 99_963);
        assert!((o.empirical_proportion().unwrap() - 0.00037).abs() < 1e-12);
    }

    #[test]
    fn observation_set_totals() {
        let mut set = ObservationSet::new();
        set.observations.push(obs(37, 100_000));
        set.observations.push(obs(3, 10_000));
        let (exp, fail) = set.total_for("failure.gateway.timeout").unwrap();
        assert_eq!((exp, fail), (110_000, 40));
        assert!(set.total_for("failure.missing").is_none());
    }
}
