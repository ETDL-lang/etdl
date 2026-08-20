//! Time-to-failure models: the analytical core of Predictive Reliability.
//!
//! [`TimeToFailureModel`] is intentionally small and closed-form only — no
//! sampling, no numerical integration. Every method must be computable
//! directly from the model's parameters. This is what keeps Predictive
//! Reliability from becoming "a new monolithic engine": each model is a
//! self-contained set of formulas, and everything else in this crate
//! (calibration adapter, tree integration, [`super::PredictiveResult`]
//! construction) is built entirely on top of this trait, never around it.

use std::collections::BTreeMap;

use crate::predictive::ModelDescriptor;

/// Common behavior of a time-to-failure distribution, expressed purely in
/// terms of the standard reliability functions. All methods take `t >= 0`
/// (mission time in the model's own unit — see [`super::MissionTime`]) and
/// are total functions: they never panic on any finite non-negative `t`,
/// including `t = 0` and very large `t`.
pub trait TimeToFailureModel {
    /// `S(t) = P(T > t)`, monotonically non-increasing, `S(0) = 1`.
    fn survival(&self, t: f64) -> f64;

    /// `h(t)`, the instantaneous hazard rate at `t`. Not a probability.
    fn hazard(&self, t: f64) -> f64;

    /// `H(t) = -ln(S(t))`, the cumulative hazard. Implementations should
    /// prefer a closed form over `-S(t).ln()` where one exists, since the
    /// closed form remains accurate as `S(t) -> 0` (where `ln` of a
    /// near-zero float loses precision) — see each model's own doc comment.
    fn cumulative_hazard(&self, t: f64) -> f64;

    /// `f(t) = h(t) * S(t)`, the failure-time density. The default
    /// implementation is exact for any model that defines `hazard` and
    /// `survival` consistently; models may override only if a more
    /// numerically stable closed form exists.
    fn density(&self, t: f64) -> f64 {
        self.hazard(t) * self.survival(t)
    }

    /// `F(t) = 1 - S(t)`, the failure probability by time `t`.
    fn failure_probability(&self, t: f64) -> f64 {
        1.0 - self.survival(t)
    }

    /// Mean time to failure, `E[T]`, if finite and defined for this model's
    /// parameters.
    fn mean(&self) -> Option<f64>;

    /// The time `t` at which `S(t) = 1 - q` for `q` in `(0, 1)` — e.g.
    /// `quantile(0.5)` is the median life. Returns `None` if `q` is not in
    /// `(0, 1)`.
    fn quantile(&self, q: f64) -> Option<f64>;

    /// A descriptor identifying this model's family, parameters, and
    /// assumptions, for embedding in a [`super::PredictiveResult`].
    fn descriptor(&self) -> ModelDescriptor;
}

/// Constant-failure-rate (exponential) model: `h(t) = lambda` for all
/// `t >= 0`. Thin wrapper over
/// [`etdl_probability_core::distribution::Exponential`] — this model does
/// not reimplement exponential math, it reuses `std.probability`'s
/// distribution directly (`survival(t) = 1 - exponential.cdf(t)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialModel {
    inner: etdl_probability_core::distribution::Exponential,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum ExponentialModelError {
    #[error("invalid failure rate: {0}")]
    InvalidRate(#[from] etdl_probability_core::distribution::exponential::ExponentialError),
}

impl ExponentialModel {
    /// `lambda` is the constant failure rate, in inverse time units
    /// matching the mission time this model will be evaluated against
    /// (e.g. failures/hour if `t` is given in hours).
    pub fn new(lambda: f64) -> Result<Self, ExponentialModelError> {
        Ok(ExponentialModel {
            inner: etdl_probability_core::distribution::Exponential::new(lambda)?,
        })
    }

    pub fn lambda(&self) -> f64 {
        self.inner.lambda()
    }
}

impl TimeToFailureModel for ExponentialModel {
    fn survival(&self, t: f64) -> f64 {
        if t < 0.0 {
            return 1.0;
        }
        self.inner.survival(t)
    }

    fn hazard(&self, _t: f64) -> f64 {
        self.inner.lambda()
    }

    fn cumulative_hazard(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        self.inner.lambda() * t
    }

    fn density(&self, t: f64) -> f64 {
        if t < 0.0 {
            return 0.0;
        }
        self.inner.pdf(t)
    }

    fn mean(&self) -> Option<f64> {
        Some(self.inner.mean())
    }

    fn quantile(&self, q: f64) -> Option<f64> {
        if !(q > 0.0 && q < 1.0) {
            return None;
        }
        Some(self.inner.quantile(q))
    }

    fn descriptor(&self) -> ModelDescriptor {
        let mut parameters = BTreeMap::new();
        parameters.insert("lambda".to_string(), self.inner.lambda());
        ModelDescriptor {
            family: "exponential".to_string(),
            parameters,
            assumptions: vec![
                "constant hazard".to_string(),
                "non-repairable (time to first failure)".to_string(),
            ],
            valid_range: None,
        }
    }
}

/// Weibull model: shape `k` and scale `lambda`. Not present in
/// `std.probability` (which stays domain-neutral and covers only
/// Bernoulli/Binomial/Beta/Exponential/Normal) — this is a genuinely new
/// implementation, scoped to time-to-failure use.
///
/// Formulas (standard two-parameter Weibull):
/// - `S(t) = exp(-(t/lambda)^k)`
/// - `h(t) = (k/lambda) * (t/lambda)^(k-1)`
/// - `H(t) = (t/lambda)^k`
/// - `f(t) = h(t) * S(t)`
/// - `mean = lambda * Gamma(1 + 1/k)`
///
/// `k < 1`: decreasing hazard (infant mortality / burn-in). `k = 1`:
/// constant hazard (reduces to the exponential model with `lambda' =
/// 1/lambda`). `k > 1`: increasing hazard (wear-out/aging) — this
/// distinction is exactly why the task exists: the exponential model
/// cannot represent aging, and forcing it to would be a silent modeling
/// error rather than an explicit one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeibullModel {
    shape: f64,
    scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum WeibullModelError {
    #[error("weibull shape must be finite and > 0, got {0}")]
    InvalidShape(f64),
    #[error("weibull scale must be finite and > 0, got {0}")]
    InvalidScale(f64),
}

impl WeibullModel {
    pub fn new(shape: f64, scale: f64) -> Result<Self, WeibullModelError> {
        if !shape.is_finite() || shape <= 0.0 {
            return Err(WeibullModelError::InvalidShape(shape));
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(WeibullModelError::InvalidScale(scale));
        }
        Ok(WeibullModel { shape, scale })
    }

    pub fn shape(&self) -> f64 {
        self.shape
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }
}

impl TimeToFailureModel for WeibullModel {
    fn survival(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 1.0;
        }
        (-self.cumulative_hazard(t)).exp()
    }

    fn hazard(&self, t: f64) -> f64 {
        if t <= 0.0 {
            // At t=0: shape<1 -> +infinity, shape=1 -> k/lambda, shape>1 -> 0.
            return if self.shape < 1.0 {
                f64::INFINITY
            } else if self.shape > 1.0 {
                0.0
            } else {
                1.0 / self.scale
            };
        }
        (self.shape / self.scale) * (t / self.scale).powf(self.shape - 1.0)
    }

    fn cumulative_hazard(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        (t / self.scale).powf(self.shape)
    }

    fn density(&self, t: f64) -> f64 {
        if t < 0.0 {
            return 0.0;
        }
        if t == 0.0 {
            return if self.shape < 1.0 {
                f64::INFINITY
            } else if self.shape == 1.0 {
                1.0 / self.scale
            } else {
                0.0
            };
        }
        self.hazard(t) * self.survival(t)
    }

    fn mean(&self) -> Option<f64> {
        Some(self.scale * gamma_function(1.0 + 1.0 / self.shape))
    }

    fn quantile(&self, q: f64) -> Option<f64> {
        if !(q > 0.0 && q < 1.0) {
            return None;
        }
        // S(t) = 1-q => (t/lambda)^k = -ln(1-q) => t = lambda * (-ln(1-q))^(1/k)
        Some(self.scale * (-(1.0 - q).ln()).powf(1.0 / self.shape))
    }

    fn descriptor(&self) -> ModelDescriptor {
        let mut parameters = BTreeMap::new();
        parameters.insert("shape".to_string(), self.shape);
        parameters.insert("scale".to_string(), self.scale);
        let mut assumptions = vec!["non-repairable (time to first failure)".to_string()];
        assumptions.push(if self.shape < 1.0 {
            "decreasing hazard (infant mortality)".to_string()
        } else if self.shape > 1.0 {
            "increasing hazard (wear-out/aging)".to_string()
        } else {
            "constant hazard (shape = 1, equivalent to exponential)".to_string()
        });
        ModelDescriptor {
            family: "weibull".to_string(),
            parameters,
            assumptions,
            valid_range: None,
        }
    }
}

/// `Gamma(x)` via the Lanczos approximation, independently reimplemented
/// here (not shared with `etdl_probability_core::numerics::log_gamma`,
/// which is a private module and unreachable outside that crate — see
/// `docs/reference/predictive-reliability-supplement.md` for why this is a
/// deliberate "fresh reimplementation, not a shared dependency" choice,
/// consistent with the same pattern already used between
/// `etdl-reliability`'s estimator and `etdl-probability-core`'s numerics).
/// Cross-validated against known values in tests (e.g. `Gamma(1) = 1`,
/// `Gamma(0.5) = sqrt(pi)`, `Gamma(1+1/2) for exponential-equivalent
/// shape=1 reduces to Gamma(2) = 1`).
fn gamma_function(x: f64) -> f64 {
    const G: f64 = 7.0;
    const COEFFICIENTS: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_1,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];

    if x < 0.5 {
        // Reflection formula: Gamma(x) * Gamma(1-x) = pi / sin(pi x)
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma_function(1.0 - x))
    } else {
        let x = x - 1.0;
        let mut a = COEFFICIENTS[0];
        let t = x + G + 0.5;
        for (i, coeff) in COEFFICIENTS.iter().enumerate().skip(1) {
            a += coeff / (x + i as f64);
        }
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
    }
}

#[cfg(test)]
mod gamma_tests {
    use super::gamma_function;

    #[test]
    fn gamma_known_values() {
        assert!((gamma_function(1.0) - 1.0).abs() < 1e-9);
        assert!((gamma_function(2.0) - 1.0).abs() < 1e-9);
        assert!((gamma_function(3.0) - 2.0).abs() < 1e-9);
        assert!((gamma_function(0.5) - std::f64::consts::PI.sqrt()).abs() < 1e-9);
    }
}
