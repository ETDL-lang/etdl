//! Per-input uncertainty for propagation.
//!
//! This module is the bridge between the **declared** uncertainty of a
//! probability estimate (`etdl_reliability_core::uncertainty::Uncertainty`,
//! the built-in representation) and the **sampling** model the propagation
//! engine needs. It deliberately does not introduce a second uncertainty
//! vocabulary: [`InputUncertainty::from_declared`] is the only supported way
//! to obtain one from an estimate, and every conversion either succeeds with a
//! documented sampling law or fails loudly with [`UnsupportedUncertainty`].
//!
//! ## What this module refuses to do
//!
//! - It never invents a distribution for a one-sided bound.
//! - It never silently substitutes the point value when the declared
//!   uncertainty cannot be sampled; the caller receives an error and the
//!   analysis emits a diagnostic.
//! - It never converts a confidence interval into a credible interval or vice
//!   versa. The distinction is carried through into the result as
//!   [`IntervalMeaning`] so the output can state what the input claimed.

use serde::{Deserialize, Serialize};

use etdl_reliability_core::distribution::DistributionKind;
use etdl_reliability_core::uncertainty::Uncertainty;

use super::sampling::Rng;

/// What a two-sided input interval claimed. Carried through propagation so the
/// result can state the provenance of its inputs without asserting that the
/// output inherits the same statistical meaning (it does not — see
/// [`super::monte_carlo::PropagationSemantics`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntervalMeaning {
    /// A frequentist confidence interval.
    Confidence,
    /// A Bayesian credible interval.
    Credible,
    /// A plain range with no coverage claim.
    PlausibleRange,
}

impl IntervalMeaning {
    pub fn label(self) -> &'static str {
        match self {
            IntervalMeaning::Confidence => "confidence-interval",
            IntervalMeaning::Credible => "credible-interval",
            IntervalMeaning::PlausibleRange => "plausible-range",
        }
    }
}

/// The sampling law used for one basic event (or common cause) during
/// uncertainty propagation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "law", rename_all = "kebab-case")]
pub enum InputUncertainty {
    /// No uncertainty: the value is held fixed at `value` in every sample.
    /// This is not "zero uncertainty is known to be true" — it is "this
    /// analysis propagates no uncertainty for this input".
    Deterministic { value: f64 },
    /// Uniform over `[lower, upper]`. Used for a declared plain interval,
    /// where no shape information exists. Uniform is the maximum-entropy
    /// choice on a bounded range and is recorded as an assumption.
    Uniform { lower: f64, upper: f64 },
    /// Beta(alpha, beta) — the natural conjugate law for a binomial failure
    /// probability. Exact, no truncation needed (support is already [0,1]).
    Beta { alpha: f64, beta: f64 },
    /// Normal(mean, sd), truncated to [0,1] by rejection-free clamping. The
    /// number of clamped samples is counted and reported.
    Normal { mean: f64, sd: f64 },
    /// LogNormal with the given parameters of the underlying normal, clamped
    /// to [0,1].
    LogNormal { log_mean: f64, log_sd: f64 },
    /// A Normal calibrated so that its central `level` interval equals
    /// `[lower, upper]`, clamped to [0,1].
    ///
    /// This is an **approximation** used when only interval endpoints are
    /// declared. It assumes symmetry and normality, which is a poor fit for
    /// probabilities near 0 or 1; a diagnostic is emitted when the calibrated
    /// interval would extend outside [0,1]. Prefer declaring a Beta.
    NormalFromInterval {
        lower: f64,
        upper: f64,
        level: f64,
        meaning: IntervalMeaning,
    },
}

/// Why a declared uncertainty could not be turned into a sampling law.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UnsupportedUncertainty {
    #[error("one-sided bounds ({0}) do not define a distribution and cannot be propagated; declare an interval or a distribution")]
    OneSidedBound(&'static str),
    #[error("distribution {0:?} is not supported by the propagation engine")]
    Distribution(DistributionKind),
    #[error("distribution {kind:?} has {got} parameters, expected {expected}")]
    DistributionParameters {
        kind: DistributionKind,
        expected: usize,
        got: usize,
    },
    #[error("invalid uncertainty bounds: [{lower}, {upper}]")]
    InvalidBounds { lower: f64, upper: f64 },
    #[error("invalid interval level {0}; expected a value in (0, 1)")]
    InvalidLevel(f64),
    #[error("invalid distribution parameter: {0}")]
    InvalidParameter(String),
}

impl InputUncertainty {
    /// Convert a declared [`Uncertainty`] into a sampling law.
    ///
    /// `point` is the estimate's point value, used only for the
    /// [`InputUncertainty::Deterministic`] fallback that the caller may choose
    /// after handling the error — this function itself never falls back.
    pub fn from_declared(
        uncertainty: &Uncertainty,
        point: f64,
    ) -> Result<Self, UnsupportedUncertainty> {
        let _ = point;
        match uncertainty {
            Uncertainty::ConfidenceInterval(ci) => Self::from_interval(
                ci.lower,
                ci.upper,
                ci.level,
                IntervalMeaning::Confidence,
            ),
            Uncertainty::CredibleInterval(ci) => {
                Self::from_interval(ci.lower, ci.upper, ci.level, IntervalMeaning::Credible)
            }
            Uncertainty::Interval(i) => {
                if !i.is_valid() {
                    return Err(UnsupportedUncertainty::InvalidBounds {
                        lower: i.lower,
                        upper: i.upper,
                    });
                }
                Ok(InputUncertainty::Uniform {
                    lower: i.lower,
                    upper: i.upper,
                })
            }
            Uncertainty::LowerBound(_) => {
                Err(UnsupportedUncertainty::OneSidedBound("lower-bound"))
            }
            Uncertainty::UpperBound(_) => {
                Err(UnsupportedUncertainty::OneSidedBound("upper-bound"))
            }
            Uncertainty::Distribution(d) => {
                if d.parameters.iter().any(|p| !p.is_finite()) {
                    return Err(UnsupportedUncertainty::InvalidParameter(
                        "non-finite distribution parameter".into(),
                    ));
                }
                let need = |n: usize| -> Result<(), UnsupportedUncertainty> {
                    if d.parameters.len() == n {
                        Ok(())
                    } else {
                        Err(UnsupportedUncertainty::DistributionParameters {
                            kind: d.name,
                            expected: n,
                            got: d.parameters.len(),
                        })
                    }
                };
                match d.name {
                    DistributionKind::Beta => {
                        need(2)?;
                        let (a, b) = (d.parameters[0], d.parameters[1]);
                        if a <= 0.0 || b <= 0.0 {
                            return Err(UnsupportedUncertainty::InvalidParameter(format!(
                                "Beta shape parameters must be positive (got {a}, {b})"
                            )));
                        }
                        Ok(InputUncertainty::Beta { alpha: a, beta: b })
                    }
                    DistributionKind::Normal => {
                        need(2)?;
                        let (m, s) = (d.parameters[0], d.parameters[1]);
                        if s <= 0.0 {
                            return Err(UnsupportedUncertainty::InvalidParameter(format!(
                                "Normal standard deviation must be positive (got {s})"
                            )));
                        }
                        Ok(InputUncertainty::Normal { mean: m, sd: s })
                    }
                    DistributionKind::LogNormal => {
                        need(2)?;
                        let (m, s) = (d.parameters[0], d.parameters[1]);
                        if s <= 0.0 {
                            return Err(UnsupportedUncertainty::InvalidParameter(format!(
                                "LogNormal sigma must be positive (got {s})"
                            )));
                        }
                        Ok(InputUncertainty::LogNormal {
                            log_mean: m,
                            log_sd: s,
                        })
                    }
                    other => Err(UnsupportedUncertainty::Distribution(other)),
                }
            }
        }
    }

    fn from_interval(
        lower: f64,
        upper: f64,
        level: f64,
        meaning: IntervalMeaning,
    ) -> Result<Self, UnsupportedUncertainty> {
        if !lower.is_finite() || !upper.is_finite() || lower > upper {
            return Err(UnsupportedUncertainty::InvalidBounds { lower, upper });
        }
        if !level.is_finite() || level <= 0.0 || level >= 1.0 {
            return Err(UnsupportedUncertainty::InvalidLevel(level));
        }
        Ok(InputUncertainty::NormalFromInterval {
            lower,
            upper,
            level,
            meaning,
        })
    }

    /// A fixed value with no propagated uncertainty.
    pub fn fixed(value: f64) -> Self {
        InputUncertainty::Deterministic { value }
    }

    /// Whether this law actually varies between samples.
    pub fn is_variable(&self) -> bool {
        match self {
            InputUncertainty::Deterministic { .. } => false,
            InputUncertainty::Uniform { lower, upper } => upper > lower,
            InputUncertainty::NormalFromInterval { lower, upper, .. } => upper > lower,
            _ => true,
        }
    }

    /// The distribution mean, where it is available in closed form. Used as
    /// the value to hold fixed when this input is frozen for the uncertainty
    /// contribution ranking.
    pub fn nominal(&self) -> Option<f64> {
        match self {
            InputUncertainty::Deterministic { value } => Some(*value),
            InputUncertainty::Uniform { lower, upper } => Some(0.5 * (lower + upper)),
            InputUncertainty::Beta { alpha, beta } => Some(alpha / (alpha + beta)),
            InputUncertainty::Normal { mean, .. } => Some(mean.clamp(0.0, 1.0)),
            InputUncertainty::LogNormal { log_mean, log_sd } => {
                Some((log_mean + 0.5 * log_sd * log_sd).exp().clamp(0.0, 1.0))
            }
            InputUncertainty::NormalFromInterval { lower, upper, .. } => {
                Some((0.5 * (lower + upper)).clamp(0.0, 1.0))
            }
        }
    }

    /// A short, stable description for provenance and reports.
    pub fn describe(&self) -> String {
        match self {
            InputUncertainty::Deterministic { value } => format!("deterministic({value:.6e})"),
            InputUncertainty::Uniform { lower, upper } => {
                format!("uniform[{lower:.6e}, {upper:.6e}]")
            }
            InputUncertainty::Beta { alpha, beta } => format!("beta({alpha}, {beta})"),
            InputUncertainty::Normal { mean, sd } => format!("normal({mean:.6e}, {sd:.6e})"),
            InputUncertainty::LogNormal { log_mean, log_sd } => {
                format!("lognormal({log_mean:.6e}, {log_sd:.6e})")
            }
            InputUncertainty::NormalFromInterval {
                lower,
                upper,
                level,
                meaning,
            } => format!(
                "normal-from-{}({:.0}%: [{:.6e}, {:.6e}])",
                meaning.label(),
                level * 100.0,
                lower,
                upper
            ),
        }
    }

    /// Draw one sample.
    ///
    /// Every law consumes a **fixed** number of underlying uniform draws for a
    /// given law kind, and every law draws at least once even when
    /// deterministic. This keeps random streams aligned across paired runs
    /// (common random numbers), which is what makes the uncertainty
    /// contribution ranking a like-for-like comparison rather than noise.
    ///
    /// Returns `(value, truncated)` where `truncated` is true when the raw
    /// draw fell outside [0, 1] and was clamped.
    pub fn sample(&self, rng: &mut Rng) -> (f64, bool) {
        match self {
            InputUncertainty::Deterministic { value } => {
                let _ = rng.next_f64(); // keep the stream aligned
                (*value, false)
            }
            InputUncertainty::Uniform { lower, upper } => {
                let v = rng.next_uniform(*lower, *upper);
                clamp_unit(v)
            }
            InputUncertainty::Beta { alpha, beta } => {
                let v = rng.next_beta(*alpha, *beta);
                if v.is_finite() {
                    (v, false)
                } else {
                    (alpha / (alpha + beta), true)
                }
            }
            InputUncertainty::Normal { mean, sd } => {
                let v = mean + sd * rng.next_normal();
                clamp_unit(v)
            }
            InputUncertainty::LogNormal { log_mean, log_sd } => {
                let v = (log_mean + log_sd * rng.next_normal()).exp();
                clamp_unit(v)
            }
            InputUncertainty::NormalFromInterval {
                lower,
                upper,
                level,
                ..
            } => {
                let z = crate::analysis::estimator::normal_quantile(0.5 + level / 2.0);
                let mid = 0.5 * (lower + upper);
                let half = 0.5 * (upper - lower);
                let sigma = if z > 0.0 { half / z } else { 0.0 };
                if sigma <= 0.0 {
                    let _ = rng.next_f64();
                    let _ = rng.next_f64();
                    return clamp_unit(mid);
                }
                clamp_unit(mid + sigma * rng.next_normal())
            }
        }
    }
}

fn clamp_unit(v: f64) -> (f64, bool) {
    if !v.is_finite() {
        return (0.0, true);
    }
    if v < 0.0 {
        (0.0, true)
    } else if v > 1.0 {
        (1.0, true)
    } else {
        (v, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use etdl_reliability_core::distribution::Distribution;
    use etdl_reliability_core::uncertainty::{ConfidenceInterval, Interval, LowerBound};

    #[test]
    fn confidence_and_credible_intervals_keep_their_meaning() {
        let ci = Uncertainty::ConfidenceInterval(ConfidenceInterval::new(0.95, 0.001, 0.003));
        let cr = Uncertainty::CredibleInterval(ConfidenceInterval::new(0.95, 0.001, 0.003));
        let a = InputUncertainty::from_declared(&ci, 0.002).unwrap();
        let b = InputUncertainty::from_declared(&cr, 0.002).unwrap();
        assert_ne!(a, b, "confidence and credible must not collapse into one law");
        assert!(a.describe().contains("confidence"));
        assert!(b.describe().contains("credible"));
    }

    #[test]
    fn one_sided_bounds_are_refused_not_guessed() {
        let u = Uncertainty::LowerBound(LowerBound { value: 0.001 });
        let err = InputUncertainty::from_declared(&u, 0.002).unwrap_err();
        assert!(matches!(err, UnsupportedUncertainty::OneSidedBound(_)));
    }

    #[test]
    fn unsupported_distribution_is_refused() {
        let u = Uncertainty::Distribution(Distribution {
            name: DistributionKind::Weibull,
            parameters: vec![1.0, 2.0],
            domain: None,
        });
        assert!(matches!(
            InputUncertainty::from_declared(&u, 0.1),
            Err(UnsupportedUncertainty::Distribution(
                DistributionKind::Weibull
            ))
        ));
    }

    #[test]
    fn plain_interval_becomes_uniform() {
        let u = Uncertainty::Interval(Interval::new(0.001, 0.003));
        let law = InputUncertainty::from_declared(&u, 0.002).unwrap();
        assert_eq!(
            law,
            InputUncertainty::Uniform {
                lower: 0.001,
                upper: 0.003
            }
        );
        assert_eq!(law.nominal(), Some(0.002));
    }

    #[test]
    fn beta_law_nominal_is_the_distribution_mean() {
        let law = InputUncertainty::Beta {
            alpha: 3.0,
            beta: 147.0,
        };
        assert!((law.nominal().unwrap() - 3.0 / 150.0).abs() < 1e-15);
    }

    #[test]
    fn deterministic_law_consumes_a_draw_to_keep_streams_aligned() {
        // Two laws, one fixed and one variable, must leave the RNG in the same
        // position so paired runs stay comparable.
        let fixed = InputUncertainty::fixed(0.01);
        let mut r1 = Rng::new(42);
        let (v, truncated) = fixed.sample(&mut r1);
        assert_eq!(v, 0.01);
        assert!(!truncated);
        let mut r2 = Rng::new(42);
        let _ = r2.next_f64();
        assert_eq!(r1.next_u64(), r2.next_u64());
    }

    #[test]
    fn samples_stay_in_unit_interval_and_report_truncation() {
        let law = InputUncertainty::Normal {
            mean: 0.999,
            sd: 0.01,
        };
        let mut rng = Rng::new(1);
        let mut truncations = 0;
        for _ in 0..5_000 {
            let (v, t) = law.sample(&mut rng);
            assert!((0.0..=1.0).contains(&v));
            if t {
                truncations += 1;
            }
        }
        assert!(truncations > 0, "truncation must be reported, not hidden");
    }

    #[test]
    fn interval_with_bad_bounds_is_rejected() {
        let u = Uncertainty::ConfidenceInterval(ConfidenceInterval::new(0.95, 0.4, 0.1));
        assert!(matches!(
            InputUncertainty::from_declared(&u, 0.2),
            Err(UnsupportedUncertainty::InvalidBounds { .. })
        ));
        let bad_level = Uncertainty::ConfidenceInterval(ConfidenceInterval::new(1.0, 0.1, 0.4));
        assert!(matches!(
            InputUncertainty::from_declared(&bad_level, 0.2),
            Err(UnsupportedUncertainty::InvalidLevel(_))
        ));
    }
}
