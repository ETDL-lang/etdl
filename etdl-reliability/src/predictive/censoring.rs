//! Censored observation records.
//!
//! Deliberately minimal and purely additive: a data representation for
//! "the true failure time was not observed exactly," distinct from
//! [`crate::observations::AggregateObservation`] (unchanged — still the
//! type the binomial calibration pipeline in [`crate::calibration`]
//! consumes). No censored-data parameter estimation (MLE, Kaplan-Meier,
//! etc.) is implemented in 1.0 — that is explicitly deferred, matching the
//! task's own scope boundary. This module exists so censored data can at
//! least be *represented* and carried through provenance, without
//! pretending to fit it.

use serde::{Deserialize, Serialize};

/// How a single time-to-event record was censored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum CensoringKind {
    /// The event had not occurred by `time` (the unit under test was still
    /// operating/withdrawn at `time`); the true failure time is `>= time`.
    /// The most common case in reliability testing (e.g. "ran for 1000
    /// hours without failing").
    Right,
    /// The event was known to have occurred before `time`, but the exact
    /// time is unknown.
    Left,
    /// The event occurred sometime in `(lower, upper]`.
    Interval { lower: f64, upper: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum CensoredObservationError {
    #[error("censoring time {0} is not finite (NaN or infinity)")]
    NotFinite(f64),
    #[error("censoring time {0} is negative")]
    Negative(f64),
    #[error("interval censoring requires lower <= upper, got lower={lower}, upper={upper}")]
    InvalidInterval { lower: f64, upper: f64 },
}

/// One censored time-to-event record for a single unit/event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CensoredObservation {
    pub time: f64,
    pub censoring: CensoringKind,
}

impl CensoredObservation {
    pub fn right(time: f64) -> Result<Self, CensoredObservationError> {
        validate_time(time)?;
        Ok(CensoredObservation {
            time,
            censoring: CensoringKind::Right,
        })
    }

    pub fn left(time: f64) -> Result<Self, CensoredObservationError> {
        validate_time(time)?;
        Ok(CensoredObservation {
            time,
            censoring: CensoringKind::Left,
        })
    }

    /// `time` is stored as `upper`, matching the convention that
    /// `CensoredObservation.time` is always the latest known time bound
    /// for the record, regardless of censoring kind.
    pub fn interval(lower: f64, upper: f64) -> Result<Self, CensoredObservationError> {
        validate_time(lower)?;
        validate_time(upper)?;
        if lower > upper {
            return Err(CensoredObservationError::InvalidInterval { lower, upper });
        }
        Ok(CensoredObservation {
            time: upper,
            censoring: CensoringKind::Interval { lower, upper },
        })
    }
}

fn validate_time(time: f64) -> Result<(), CensoredObservationError> {
    if !time.is_finite() {
        return Err(CensoredObservationError::NotFinite(time));
    }
    if time < 0.0 {
        return Err(CensoredObservationError::Negative(time));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_censored_construction() {
        let obs = CensoredObservation::right(1000.0).unwrap();
        assert_eq!(obs.time, 1000.0);
        assert_eq!(obs.censoring, CensoringKind::Right);
    }

    #[test]
    fn interval_requires_lower_le_upper() {
        assert!(matches!(
            CensoredObservation::interval(10.0, 5.0),
            Err(CensoredObservationError::InvalidInterval { .. })
        ));
    }

    #[test]
    fn rejects_negative_and_non_finite() {
        assert!(matches!(
            CensoredObservation::right(-1.0),
            Err(CensoredObservationError::Negative(_))
        ));
        assert!(matches!(
            CensoredObservation::right(f64::NAN),
            Err(CensoredObservationError::NotFinite(_))
        ));
    }
}
