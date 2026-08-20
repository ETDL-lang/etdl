//! Uncertainty representations.

use serde::{Deserialize, Serialize};

use crate::distribution::Distribution;

/// Uncertainty associated with an estimate.
///
/// Internally tagged on `kind` so the same representation works in YAML and
/// JSON and distinguishes confidence vs credible intervals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Uncertainty {
    ConfidenceInterval(ConfidenceInterval),
    CredibleInterval(ConfidenceInterval),
    Distribution(Distribution),
    /// A plain numeric range with **no** statistical interpretation: the value
    /// is asserted to lie between `lower` and `upper`, with no coverage claim
    /// and no distributional shape. Used for engineering bounds and vendor
    /// min/max data. It is NOT a confidence interval and MUST NOT be reported
    /// as one.
    Interval(Interval),
    LowerBound(LowerBound),
    UpperBound(UpperBound),
}

/// A plain interval: a range with no statistical interpretation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interval {
    pub lower: f64,
    pub upper: f64,
}

impl Interval {
    pub fn new(lower: f64, upper: f64) -> Self {
        Interval { lower, upper }
    }

    pub fn is_valid(&self) -> bool {
        self.lower.is_finite() && self.upper.is_finite() && self.lower <= self.upper
    }
}

/// A single-sided lower bound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LowerBound {
    pub value: f64,
}

/// A single-sided upper bound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpperBound {
    pub value: f64,
}

/// A confidence or credible interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceInterval {
    pub level: f64,
    pub lower: f64,
    pub upper: f64,
}

impl ConfidenceInterval {
    pub fn new(level: f64, lower: f64, upper: f64) -> Self {
        ConfidenceInterval {
            level,
            lower,
            upper,
        }
    }

    /// All bounds must be finite; `level` must be in (0, 1]; `lower <= upper`.
    pub fn is_valid(&self) -> bool {
        [self.level, self.lower, self.upper]
            .iter()
            .all(|v| v.is_finite())
            && (0.0..=1.0).contains(&self.level)
            && self.lower <= self.upper
    }
}

impl Uncertainty {
    /// The representation kind.
    pub fn kind(&self) -> UncertaintyKind {
        match self {
            Uncertainty::ConfidenceInterval(_) => UncertaintyKind::ConfidenceInterval,
            Uncertainty::CredibleInterval(_) => UncertaintyKind::CredibleInterval,
            Uncertainty::Distribution(_) => UncertaintyKind::Distribution,
            Uncertainty::Interval(_) => UncertaintyKind::Interval,
            Uncertainty::LowerBound(_) => UncertaintyKind::LowerBound,
            Uncertainty::UpperBound(_) => UncertaintyKind::UpperBound,
        }
    }

    /// The stated confidence/credibility level, when the representation has
    /// one. A plain [`Uncertainty::Interval`] has **no** level: returning
    /// `None` here is the type system refusing to invent a coverage claim.
    pub fn level(&self) -> Option<f64> {
        match self {
            Uncertainty::ConfidenceInterval(ci) | Uncertainty::CredibleInterval(ci) => {
                Some(ci.level)
            }
            _ => None,
        }
    }

    /// The two-sided numeric bounds, when the representation has both.
    pub fn bounds(&self) -> Option<(f64, f64)> {
        match self {
            Uncertainty::ConfidenceInterval(ci) | Uncertainty::CredibleInterval(ci) => {
                Some((ci.lower, ci.upper))
            }
            Uncertainty::Interval(i) => Some((i.lower, i.upper)),
            Uncertainty::Distribution(d) => d.domain,
            _ => None,
        }
    }

    /// Validate the numeric contents of every uncertainty representation.
    /// Returns `None` when valid, or the first problem found.
    pub fn validate(&self) -> Option<UncertaintyError> {
        match self {
            Uncertainty::ConfidenceInterval(ci) | Uncertainty::CredibleInterval(ci) => {
                if !ci.is_valid() {
                    return Some(UncertaintyError::InvalidInterval {
                        level: ci.level,
                        lower: ci.lower,
                        upper: ci.upper,
                    });
                }
            }
            Uncertainty::Distribution(d) => {
                if d.parameters.iter().any(|p| !p.is_finite()) {
                    return Some(UncertaintyError::NonFiniteParameter);
                }
            }
            Uncertainty::Interval(i) => {
                if !i.is_valid() {
                    return Some(UncertaintyError::InvalidRange {
                        lower: i.lower,
                        upper: i.upper,
                    });
                }
            }
            Uncertainty::LowerBound(b) => {
                if !b.value.is_finite() {
                    return Some(UncertaintyError::NonFiniteParameter);
                }
            }
            Uncertainty::UpperBound(b) => {
                if !b.value.is_finite() {
                    return Some(UncertaintyError::NonFiniteParameter);
                }
            }
        }
        None
    }
}

/// Problems found while validating an uncertainty representation.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum UncertaintyError {
    #[error("interval must be finite with level in [0,1] and lower <= upper (got level {level}, [{lower}, {upper}])")]
    InvalidInterval { level: f64, lower: f64, upper: f64 },
    #[error("uncertainty bound/parameter is not finite (NaN or infinity)")]
    NonFiniteParameter,
    #[error("interval must be finite with lower <= upper (got [{lower}, {upper}])")]
    InvalidRange { lower: f64, upper: f64 },
}

/// The kind of uncertainty representation, as a value (for reporting and
/// provenance without matching on the enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UncertaintyKind {
    ConfidenceInterval,
    CredibleInterval,
    Distribution,
    Interval,
    LowerBound,
    UpperBound,
}

impl UncertaintyKind {
    pub fn label(self) -> &'static str {
        match self {
            UncertaintyKind::ConfidenceInterval => "confidence-interval",
            UncertaintyKind::CredibleInterval => "credible-interval",
            UncertaintyKind::Distribution => "distribution",
            UncertaintyKind::Interval => "interval",
            UncertaintyKind::LowerBound => "lower-bound",
            UncertaintyKind::UpperBound => "upper-bound",
        }
    }

    /// A one-sentence statement of what this representation means. These are
    /// deliberately NOT interchangeable: a frequentist confidence interval and
    /// a Bayesian credible interval make different claims about the same
    /// numbers.
    pub fn interpretation(self) -> &'static str {
        match self {
            UncertaintyKind::ConfidenceInterval => {
                "frequentist confidence interval: over repeated sampling, intervals \
                 constructed this way cover the true value with the stated level"
            }
            UncertaintyKind::CredibleInterval => {
                "Bayesian credible interval: given the prior and the observed data, \
                 the posterior probability that the value lies in this interval is \
                 the stated level"
            }
            UncertaintyKind::Distribution => {
                "a parametric distribution over the value; quantiles follow from the \
                 distribution, not from a coverage claim"
            }
            UncertaintyKind::Interval => {
                "a plain range with no coverage claim and no distributional shape; \
                 not a confidence interval"
            }
            UncertaintyKind::LowerBound => "a one-sided lower bound on the value",
            UncertaintyKind::UpperBound => "a one-sided upper bound on the value",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_validity() {
        assert!(ConfidenceInterval::new(0.95, 0.001, 0.003).is_valid());
        assert!(!ConfidenceInterval::new(1.5, 0.0, 0.5).is_valid());
        assert!(!ConfidenceInterval::new(0.95, 0.3, 0.1).is_valid());
    }

    #[test]
    fn interval_rejects_non_finite() {
        assert!(!ConfidenceInterval::new(f64::NAN, 0.0, 1.0).is_valid());
        assert!(!ConfidenceInterval::new(0.95, f64::NAN, 0.5).is_valid());
        assert!(!ConfidenceInterval::new(0.95, 0.0, f64::INFINITY).is_valid());
        assert!(!ConfidenceInterval::new(0.95, f64::NEG_INFINITY, 0.5).is_valid());
    }

    #[test]
    fn plain_interval_makes_no_coverage_claim() {
        let u = Uncertainty::Interval(Interval::new(0.001, 0.004));
        assert!(u.validate().is_none());
        assert_eq!(u.kind(), UncertaintyKind::Interval);
        // A plain interval has no level: the type refuses to invent one.
        assert_eq!(u.level(), None);
        assert_eq!(u.bounds(), Some((0.001, 0.004)));
        assert!(Uncertainty::Interval(Interval::new(0.5, 0.1))
            .validate()
            .is_some());
        assert!(Uncertainty::Interval(Interval::new(f64::NAN, 0.1))
            .validate()
            .is_some());
    }

    #[test]
    fn confidence_and_credible_are_not_interchangeable() {
        let ci = Uncertainty::ConfidenceInterval(ConfidenceInterval::new(0.95, 0.1, 0.2));
        let cr = Uncertainty::CredibleInterval(ConfidenceInterval::new(0.95, 0.1, 0.2));
        // Same numbers, different meaning; the kind and interpretation differ.
        assert_eq!(ci.bounds(), cr.bounds());
        assert_eq!(ci.level(), cr.level());
        assert_ne!(ci.kind(), cr.kind());
        assert_ne!(ci.kind().interpretation(), cr.kind().interpretation());
        assert_ne!(ci, cr);
    }

    #[test]
    fn uncertainty_validate() {
        use crate::distribution::{Distribution, DistributionKind};
        assert!(
            Uncertainty::ConfidenceInterval(ConfidenceInterval::new(0.95, 0.0, 0.5))
                .validate()
                .is_none()
        );
        assert!(
            Uncertainty::ConfidenceInterval(ConfidenceInterval::new(f64::NAN, 0.0, 0.5))
                .validate()
                .is_some()
        );
        assert!(Uncertainty::Distribution(Distribution {
            name: DistributionKind::Beta,
            parameters: vec![2.0, f64::NAN],
            domain: None,
        })
        .validate()
        .is_some());
        assert!(Uncertainty::LowerBound(crate::uncertainty::LowerBound {
            value: f64::INFINITY,
        })
        .validate()
        .is_some());
    }
}
