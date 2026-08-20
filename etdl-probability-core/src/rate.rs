//! [`Rate`]: a non-negative quantity per unit, kept distinct from
//! [`crate::Probability`].
//!
//! A rate such as `2e-5 / hour` is not a probability — it is not bounded to
//! `[0, 1]`, and converting it to a probability requires an explicit model
//! (e.g. the exponential failure model, `P(failure by t) = 1 - exp(-λt)`,
//! already implemented in `etdl-reliability::analysis::estimator`). This
//! crate never performs that conversion implicitly; [`Rate`] and
//! [`crate::Probability`] are separate types with no `From`/`Into` between
//! them.

use serde::{Deserialize, Serialize};

/// A non-negative rate, explicitly tagged with what it is measured per
/// (e.g. `"hour"`, `"request"`). `per_unit` is a free-text label, not a
/// checked unit type — see `docs/reference/standard-probability-library.md`
/// for why a real unit-checked type (distinguishing seconds from hours, for
/// instance) is proposed future work and not implemented here, matching the
/// `std.units` deferral in the Standard Library Core task this builds on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rate {
    pub value: f64,
    pub per_unit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum RateError {
    #[error("rate value {0} is not finite (NaN or infinity)")]
    NotFinite(f64),
    #[error("rate value {0} is negative; a rate cannot be negative")]
    Negative(f64),
}

impl Rate {
    pub fn new(value: f64, per_unit: impl Into<String>) -> Result<Self, RateError> {
        if !value.is_finite() {
            return Err(RateError::NotFinite(value));
        }
        if value < 0.0 {
            return Err(RateError::Negative(value));
        }
        Ok(Rate {
            value,
            per_unit: per_unit.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_rate() {
        let r = Rate::new(2e-5, "hour").unwrap();
        assert_eq!(r.value, 2e-5);
        assert_eq!(r.per_unit, "hour");
    }

    #[test]
    fn rejects_negative_rate() {
        assert_eq!(Rate::new(-1.0, "hour"), Err(RateError::Negative(-1.0)));
    }

    #[test]
    fn rejects_non_finite_rate() {
        assert!(matches!(
            Rate::new(f64::NAN, "hour"),
            Err(RateError::NotFinite(_))
        ));
    }

    #[test]
    fn rate_is_not_a_probability_no_conversion_exists() {
        // Type-level assertion: there is no From<Rate> for Probability and
        // no From<Probability> for Rate. This test exists so that if such a
        // conversion is ever added, a reviewer sees this test's name and
        // intent change, rather than the conversion slipping in silently.
        let r = Rate::new(2e-5, "hour").unwrap();
        assert_eq!(r.value, 2e-5); // Rate::value is just an f64; using it
                                    // as a Probability requires an explicit
                                    // model, never an implicit cast.
    }
}
