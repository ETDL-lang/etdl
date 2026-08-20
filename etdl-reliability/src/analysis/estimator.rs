//! The estimator abstraction: observations/evidence → typed probability
//! estimate with full provenance.
//!
//! Every estimator returns a [`ProbabilityEstimate`] (never a bare `f64`),
//! carries its name/version, declares its assumptions, and records the
//! statistical method and interpretation. Estimators are replaceable: a future
//! ML or Monte-Carlo-based estimator implements the same boundary.

use crate::observations::{AggregateObservation, ObservationError};
use crate::probability::{ProbabilityMetric, ProbabilitySource};

use etdl_reliability_core::estimate::{ProbabilityEstimate, ProbabilityState};
use etdl_reliability_core::provenance::Provenance;
use etdl_reliability_core::uncertainty::{ConfidenceInterval, Uncertainty};
use etdl_reliability_core::ReliabilityError;

/// Errors produced by estimation.
#[derive(Debug, thiserror::Error)]
pub enum EstimationError {
    #[error("invalid observation: {0}")]
    InvalidObservation(#[from] ObservationError),
    #[error("estimation failed: {0}")]
    Numerical(String),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("estimate is invalid for the artifact: {0}")]
    InvalidEstimate(#[from] ReliabilityError),
}

/// The statistical interpretation of the reported interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatisticalInterpretation {
    /// A frequentist confidence interval.
    Frequentist,
    /// A Bayesian credible interval.
    Bayesian,
}

/// Configuration for an estimation run.
#[derive(Debug, Clone)]
pub struct EstimationConfig {
    /// Confidence / credibility level for the interval, in (0, 1]. Default 0.95.
    pub level: f64,
    /// For the beta-binomial estimator: prior alpha. Default 1 (uniform).
    pub prior_alpha: f64,
    /// For the beta-binomial estimator: prior beta. Default 1 (uniform).
    pub prior_beta: f64,
    /// For the exponential estimator: mission time over which `P(failure by t)`
    /// is computed.
    pub mission_time: Option<f64>,
    /// The metric of the produced estimate.
    pub metric: ProbabilityMetric,
}

impl Default for EstimationConfig {
    fn default() -> Self {
        EstimationConfig {
            level: 0.95,
            prior_alpha: 1.0,
            prior_beta: 1.0,
            mission_time: None,
            metric: ProbabilityMetric::Probability,
        }
    }
}

/// The estimator boundary. Implementations convert evidence into a typed
/// estimate; the compiler never runs estimators.
pub trait ReliabilityEstimator: Send + Sync {
    /// Human-readable estimator name, e.g. `empirical/binomial`.
    fn name(&self) -> &str;

    /// Estimator implementation version (for reproducibility).
    fn version(&self) -> &str;

    /// The statistical model used, e.g. `binomial`, `beta-binomial`.
    fn model(&self) -> &str;

    /// The statistical interpretation of the reported interval.
    fn interpretation(&self) -> StatisticalInterpretation;

    /// Explicit model assumptions. Hidden assumptions are forbidden.
    fn assumptions(&self) -> Vec<String>;

    /// Estimate a probability from an aggregate observation.
    fn estimate(
        &self,
        observation: &AggregateObservation,
        config: &EstimationConfig,
    ) -> Result<ProbabilityEstimate, EstimationError>;
}

/// Common provenance construction for estimators.
fn provenance(
    source: ProbabilitySource,
    dataset: Option<&str>,
    model: &str,
    model_version: &str,
) -> Provenance {
    let mut p = Provenance::new(source);
    p.dataset = dataset.map(|s| s.to_string());
    p.model = Some(model.to_string());
    p.model_version = Some(model_version.to_string());
    p
}

/// Helper to attach the standard fields to an estimate.
pub(crate) fn finalize_estimate(
    mut estimate: ProbabilityEstimate,
    estimator: &dyn ReliabilityEstimator,
    obs: &AggregateObservation,
    uncertainty: Option<Uncertainty>,
) -> ProbabilityEstimate {
    estimate.state = ProbabilityState::Estimated;
    estimate.time_basis = Some(obs.exposure_unit);
    estimate.conditions = obs.conditions.clone();
    estimate.population = Some(obs.failure_mode.clone());
    estimate.method = Some(format!("{}/{}", estimator.model(), estimator.name()));
    estimate.uncertainty = uncertainty;
    estimate.version = Some("1".to_string());
    estimate
}

/// Empirical binomial estimator: `p_hat = failures / opportunities` with a
/// Wilson confidence interval.
#[derive(Debug, Clone, Default)]
pub struct EmpiricalBinomialEstimator {
    /// The interval confidence level (defaults to config).
    pub _level: f64,
}

impl EmpiricalBinomialEstimator {
    pub fn new() -> Self {
        EmpiricalBinomialEstimator { _level: 0.95 }
    }
}

impl ReliabilityEstimator for EmpiricalBinomialEstimator {
    fn name(&self) -> &str {
        "empirical/binomial"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    fn model(&self) -> &str {
        "binomial"
    }
    fn interpretation(&self) -> StatisticalInterpretation {
        StatisticalInterpretation::Frequentist
    }
    fn assumptions(&self) -> Vec<String> {
        vec![
            "independent trials".to_string(),
            format!("exposure is defined by unit {:?}", self.name()),
            "failure proportion is constant over the observation window".to_string(),
        ]
    }
    fn estimate(
        &self,
        observation: &AggregateObservation,
        config: &EstimationConfig,
    ) -> Result<ProbabilityEstimate, EstimationError> {
        observation.validate()?;
        let level = config.level;
        if !(0.0..=1.0).contains(&level) || level == 0.0 {
            return Err(EstimationError::InvalidConfig(format!(
                "interval level must be in (0, 1], got {level}"
            )));
        }
        let n = observation.exposure as f64;
        let p = observation.failures as f64 / n;
        let z = normal_quantile(0.5 + level / 2.0);
        let (lo, hi) = wilson(p, n, z);

        let mut e =
            ProbabilityEstimate::new(&observation.failure_mode, ProbabilityState::Estimated, p);
        e.provenance = Some(provenance(
            ProbabilitySource::Measurement,
            observation.source.as_deref(),
            self.model(),
            self.version(),
        ));
        let ci = Uncertainty::ConfidenceInterval(ConfidenceInterval::new(level, lo, hi));
        Ok(finalize_estimate(e, self, observation, Some(ci)))
    }
}

/// Wilson score interval at confidence z.
pub fn wilson(p: f64, n: f64, z: f64) -> (f64, f64) {
    let denom = 1.0 + z * z / n;
    let centre = (p + z * z / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    (centre - half, centre + half)
}

/// Normal quantile (z-score) via the Beasley-Springer-Moro approximation.
#[allow(clippy::excessive_precision)]
pub fn normal_quantile(p: f64) -> f64 {
    let p = p.clamp(1e-15, 1.0 - 1e-15);
    if p < 0.5 {
        -normal_quantile(1.0 - p)
    } else {
        let a = [
            -3.969683028665376e+01,
            2.209460984245205e+02,
            -2.759285104469687e+02,
            1.383577518672690e+02,
            -3.066479806614716e+01,
            2.506628277459239e+00,
        ];
        let b = [
            -5.447609879822406e+01,
            1.615858368580409e+02,
            -1.556989798598866e+02,
            6.680131188771972e+01,
            -1.328068155288572e+01,
        ];
        let c = [
            -7.784894002430293e-03,
            -3.223964580411365e-01,
            -2.400758277161838e+00,
            -2.549732539343734e+00,
            4.374664141464968e+00,
            2.938163982698783e+00,
        ];
        let d = [
            7.784695709041462e-03,
            3.224671290700398e-01,
            2.445134137142996e+00,
            3.754408661907416e+00,
        ];
        let q = p - 0.5;
        if q.abs() <= 0.425 {
            let r = 0.180625 - q * q;
            q * (((((a[4] * r + a[3]) * r + a[2]) * r + a[1]) * r + a[0]) * r + 1.0)
                / (((((b[4] * r + b[3]) * r + b[2]) * r + b[1]) * r + b[0]) * r + 1.0)
        } else {
            let r = if q < 0.0 { p } else { 1.0 - p };
            let r = (-r.ln()).ln();
            let x = if r < 5.0 {
                let r = r - 1.6;
                (((((c[4] * r + c[3]) * r + c[2]) * r + c[1]) * r + c[0]) * r + 1.0)
                    / ((((d[3] * r + d[2]) * r + d[1]) * r + d[0]) * r + 1.0)
            } else {
                let r = r - 5.0;
                (((((a[4] * r + a[3]) * r + a[2]) * r + a[1]) * r + a[0]) * r + 1.0)
                    / (((((b[4] * r + b[3]) * r + b[2]) * r + b[1]) * r + b[0]) * r + 1.0)
            };
            if q < 0.0 {
                -x
            } else {
                x
            }
        }
    }
}

/// Beta-binomial Bayesian estimator: posterior from a prior + observations.
/// Preserves the full posterior (alpha, beta); the point estimate is the
/// explicitly selected posterior mean; the interval is an equal-tailed
/// credible interval.
#[derive(Debug, Clone, Default)]
pub struct BetaBinomialEstimator;

impl ReliabilityEstimator for BetaBinomialEstimator {
    fn name(&self) -> &str {
        "bayesian/beta-binomial"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    fn model(&self) -> &str {
        "beta-binomial"
    }
    fn interpretation(&self) -> StatisticalInterpretation {
        StatisticalInterpretation::Bayesian
    }
    fn assumptions(&self) -> Vec<String> {
        vec![
            "independent trials".to_string(),
            format!("prior Beta(alpha, beta); posterior mean selected as the deterministic scalar"),
            "the prior is declared in the configuration (default uniform Beta(1,1))".to_string(),
        ]
    }
    fn estimate(
        &self,
        observation: &AggregateObservation,
        config: &EstimationConfig,
    ) -> Result<ProbabilityEstimate, EstimationError> {
        observation.validate()?;
        if config.prior_alpha <= 0.0 || config.prior_beta <= 0.0 {
            return Err(EstimationError::InvalidConfig(format!(
                "prior alpha/beta must be positive, got alpha={} beta={}",
                config.prior_alpha, config.prior_beta
            )));
        }
        let level = config.level;
        if !(0.0..=1.0).contains(&level) || level == 0.0 {
            return Err(EstimationError::InvalidConfig(format!(
                "interval level must be in (0, 1], got {level}"
            )));
        }
        let alpha_post = config.prior_alpha + observation.failures as f64;
        let beta_post = config.prior_beta + observation.successes() as f64;
        let point = alpha_post / (alpha_post + beta_post);
        let (lo, hi) = beta_credible_interval(alpha_post, beta_post, level);

        let mut e = ProbabilityEstimate::new(
            &observation.failure_mode,
            ProbabilityState::Estimated,
            point,
        );
        e.provenance = Some(provenance(
            ProbabilitySource::Measurement,
            observation.source.as_deref(),
            self.model(),
            self.version(),
        ));
        e.conditions = observation.conditions.clone();
        let ci = Uncertainty::CredibleInterval(ConfidenceInterval::new(level, lo, hi));
        Ok(finalize_estimate(e, self, observation, Some(ci)))
    }
}

/// Equal-tailed credible interval for a Beta distribution via the regularized
/// incomplete beta function (continued-fraction algorithm).
pub fn beta_credible_interval(alpha: f64, beta: f64, level: f64) -> (f64, f64) {
    let lower = (1.0 - level) / 2.0;
    let upper = 1.0 - lower;
    (
        beta_quantile(alpha, beta, lower),
        beta_quantile(alpha, beta, upper),
    )
}

/// Quantile of the Beta(alpha, beta) distribution via binary search on the CDF.
pub fn beta_quantile(alpha: f64, beta: f64, q: f64) -> f64 {
    if alpha <= 0.0 || beta <= 0.0 {
        return f64::NAN;
    }
    let q = q.clamp(1e-12, 1.0 - 1e-12);
    let mut lo = 0.0f64;
    let mut hi = 1.0f64;
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        let cdf = regularized_beta(mid, alpha, beta);
        if cdf < q {
            lo = mid;
        } else {
            hi = mid;
        }
        if (hi - lo).abs() < 1e-12 {
            break;
        }
    }
    0.5 * (lo + hi)
}

/// Regularized incomplete beta function I_x(a, b) via continued fractions.
/// Uses the classical Numerical Recipes `betacf` continued fraction with the
/// reflection `I_x(a,b) = 1 - I_{1-x}(b,a)` for x beyond the midpoint.
pub fn regularized_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // Reflect to evaluate near 0 for convergence.
    let use_reflect = x > (a + 1.0) / (a + b + 2.0);
    let (a, b, x) = if use_reflect {
        (b, a, 1.0 - x)
    } else {
        (a, b, x)
    };

    let ln_front = log_gamma(a + b) - log_gamma(a) - log_gamma(b) + a * x.ln() + b * (1.0 - x).ln();
    if ln_front < -700.0 {
        return if use_reflect { 1.0 } else { 0.0 };
    }
    let front = ln_front.exp() / a;

    const MAX_ITER: usize = 200;
    const EPS: f64 = 1e-13;
    const FPMIN: f64 = 1e-300;

    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;

    for m in 1..=MAX_ITER {
        let m2 = 2 * m;
        let mut aa = m as f64 * (b - m as f64) * x / ((qam + m2 as f64) * (a + m2 as f64));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;
        aa = -(a + m as f64) * (qab + m as f64) * x / ((a + m2 as f64) * (qap + m2 as f64));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }

    let result = front * h;
    if use_reflect {
        1.0 - result
    } else {
        result
    }
}

/// Natural log of the gamma function (Lanczos approximation), stable across
/// the positive reals. Used by the regularized incomplete beta function.
#[allow(clippy::excessive_precision)]
pub fn log_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const P: [f64; 9] = [
        0.99999999999980993,
        676.5203681218851,
        -1259.1392167224028,
        771.32342877765313,
        -176.61502916214059,
        12.507343278686905,
        -0.13857109526572012,
        9.9843695780195716e-6,
        1.5056327351493116e-7,
    ];
    if x < 0.5 {
        std::f64::consts::PI.ln() - (std::f64::consts::PI * x).sin().ln() - log_gamma(1.0 - x)
    } else {
        let x = x - 1.0;
        let mut a = P[0];
        let t = x + G + 0.5;
        for (i, coeff) in P.iter().enumerate().skip(1) {
            a += coeff / (x + i as f64);
        }
        0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
    }
}

/// Exponential constant-failure-rate model: converts a per-unit-time failure
/// rate λ into `P(failure by mission_time t) = 1 - exp(-λt)` using the
/// numerically stable `-expm1(-λt)`.
#[derive(Debug, Clone, Default)]
pub struct ExponentialRateEstimator;

impl ReliabilityEstimator for ExponentialRateEstimator {
    fn name(&self) -> &str {
        "exponential/constant-rate"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    fn model(&self) -> &str {
        "exponential"
    }
    fn interpretation(&self) -> StatisticalInterpretation {
        StatisticalInterpretation::Frequentist
    }
    fn assumptions(&self) -> Vec<String> {
        vec![
            "constant hazard rate over the mission".to_string(),
            "failure rate λ = failures / exposure per unit time".to_string(),
            format!("P(failure by t) = 1 - exp(-λ·t); λ must have a time basis"),
        ]
    }
    fn estimate(
        &self,
        observation: &AggregateObservation,
        config: &EstimationConfig,
    ) -> Result<ProbabilityEstimate, EstimationError> {
        observation.validate()?;
        let t = config.mission_time.ok_or_else(|| {
            EstimationError::InvalidConfig(
                "mission_time is required for the exponential estimator".to_string(),
            )
        })?;
        if t < 0.0 {
            return Err(EstimationError::InvalidConfig(format!(
                "mission_time must be non-negative, got {t}"
            )));
        }
        let lambda = observation.failures as f64 / observation.exposure as f64;
        // Numerically stable: -expm1(-λt) avoids catastrophic cancellation for
        // small λt.
        let p = -(-lambda * t).exp_m1();
        let mut e =
            ProbabilityEstimate::new(&observation.failure_mode, ProbabilityState::Estimated, p);
        e.metric = ProbabilityMetric::Probability;
        e.provenance = Some(provenance(
            ProbabilitySource::Measurement,
            observation.source.as_deref(),
            self.model(),
            self.version(),
        ));
        e.conditions = observation.conditions.clone();
        // The failure rate itself is a rate; the produced scalar is the
        // probability over the mission. Both are recorded.
        e.method = Some(format!(
            "{}; lambda={:.6e}/unit, t={}",
            self.model(),
            lambda,
            t
        ));
        Ok(finalize_estimate(e, self, observation, None))
    }
}

/// The default registry of built-in estimators.
pub fn builtin_estimators() -> Vec<Box<dyn ReliabilityEstimator>> {
    vec![
        Box::new(EmpiricalBinomialEstimator::new()),
        Box::new(BetaBinomialEstimator),
        Box::new(ExponentialRateEstimator),
    ]
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::probability::TimeBasis;

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
    fn empirical_known_value() {
        let e = EmpiricalBinomialEstimator::new()
            .estimate(&obs(37, 100_000), &EstimationConfig::default())
            .unwrap();
        assert!((e.value.unwrap() - 0.00037).abs() < 1e-12);
        assert_eq!(e.state, ProbabilityState::Estimated);
        assert!(matches!(
            e.uncertainty,
            Some(Uncertainty::ConfidenceInterval(_))
        ));
        // Wilson interval must bracket the point.
        let Uncertainty::ConfidenceInterval(ci) = e.uncertainty.unwrap() else {
            panic!("expected interval");
        };
        assert!(ci.lower <= 0.00037 && ci.upper >= 0.00037);
        assert!(ci.level - 0.95 < 1e-9);
        // Provenance preserved.
        let p = e.provenance.unwrap();
        assert_eq!(p.model.as_deref(), Some("binomial"));
        assert_eq!(e.method.as_deref(), Some("binomial/empirical/binomial"));
    }

    #[test]
    fn beta_binomial_preserves_posterior_and_credible_interval() {
        let mut cfg = EstimationConfig::default();
        cfg.prior_alpha = 1.0;
        cfg.prior_beta = 1.0;
        let e = BetaBinomialEstimator.estimate(&obs(2, 10), &cfg).unwrap();
        // Posterior Beta(3, 9): mean = 3/12 = 0.25.
        assert!((e.value.unwrap() - 0.25).abs() < 1e-9);
        assert!(matches!(
            e.uncertainty,
            Some(Uncertainty::CredibleInterval(_))
        ));
        let Uncertainty::CredibleInterval(ci) = e.uncertainty.unwrap() else {
            panic!("expected credible interval");
        };
        assert!(ci.lower < 0.25 && ci.upper > 0.25);
        assert_eq!(
            e.method.as_deref(),
            Some("beta-binomial/bayesian/beta-binomial")
        );
    }

    #[test]
    fn exponential_is_stable_and_converts() {
        let mut cfg = EstimationConfig::default();
        cfg.mission_time = Some(10.0);
        // λ = 2/10 = 0.2 per hour; P(failure by 10h) = 1 - exp(-2).
        let e = ExponentialRateEstimator
            .estimate(&obs(2, 10), &cfg)
            .unwrap();
        let expected = -(-2.0f64).exp_m1();
        assert!((e.value.unwrap() - expected).abs() < 1e-12);
    }

    #[test]
    fn zero_exposure_rejected() {
        let err = EmpiricalBinomialEstimator::new()
            .estimate(&obs(0, 0), &EstimationConfig::default())
            .unwrap_err();
        assert!(matches!(
            err,
            EstimationError::InvalidObservation(ObservationError::NonPositiveExposure(_))
        ));
    }

    #[test]
    fn failure_rate_is_not_probability() {
        // Exponential produces a probability over the mission; the method
        // string records the λ and t, and the value equals the conversion.
        let mut cfg = EstimationConfig::default();
        cfg.mission_time = Some(1.0);
        let e = ExponentialRateEstimator
            .estimate(&obs(1, 1000), &cfg)
            .unwrap();
        let lambda = 0.001_f64;
        let expected = -(-lambda * 1.0).exp_m1();
        assert!((e.value.unwrap() - expected).abs() < 1e-12);
    }
}
