//! Predicted vs. observed: comparing a compiled reliability model against
//! runtime observations, without ever mutating the model.
//!
//! ```text
//! RELIABILITY ARTIFACT (predicted)     OBSERVATION DATASET (observed)
//!            \                                /
//!             \                              /
//!              --------- calibrate() --------
//!                          |
//!                 CalibrationResult
//!                          |
//!            engineer reviews, decides, and — only if they choose to —
//!            publishes a NEW reliability artifact
//! ```
//!
//! [`calibrate`] takes `&ReliabilityArtifact` and `&AggregateObservation` and
//! returns a new [`CalibrationResult`]. It never takes `&mut ReliabilityArtifact`
//! and there is no function anywhere in this module that could change a
//! probability estimate, a fault tree, generated code, or compiled binary.
//! The feedback loop is **observe -> analyze -> review -> publish a new
//! artifact -> rebuild**; nothing here closes that loop automatically.
//!
//! ## What "model drift" means here
//!
//! [`CalibrationResult::is_drift`] is true only for
//! [`CalibrationStatus::SignificantDeviation`] — which requires (a) predicted
//! and observed conditions/metric/time-basis to match ([`calibrate`] refuses
//! the comparison otherwise, see [`CalibrationStatus::UnsupportedComparison`]),
//! and (b) an exact binomial test to reject, at a strict significance level,
//! the null hypothesis that the observed failure rate equals the predicted
//! one. It never means merely `observed != predicted`: with enough
//! observations even a tiny, practically irrelevant difference is
//! "significant"; with too few, even a large difference is not. See
//! [`CalibrationStatus::InsufficientData`].

use serde::{Deserialize, Serialize};

use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::probability::{ProbabilityMetric, TimeBasis};

use crate::analysis::dependence::{AnalysisDiagnostic, ArtifactRef};
use crate::analysis::estimator::regularized_beta;
use crate::dataset::DatasetRef;
use crate::observations::{AggregateObservation, ObservationError, TimeInterval};

/// Version of the calibration-result schema.
pub const CALIBRATION_SCHEMA: &str = "etdl.reliability.calibration-result/1.0";
/// The statistical method used to compare predicted and observed rates.
pub const CALIBRATION_METHOD: &str = "binomial-two-sided-exact";
pub const CALIBRATION_METHOD_VERSION: &str = "1";

/// Stable diagnostic codes for calibration. Adding a code is a minor change;
/// changing the meaning of an existing code is not permitted.
pub mod code {
    /// The predicted estimate's metric is not probability-like (e.g. a rate);
    /// rate-based calibration is not implemented, so no statistical test ran.
    pub const UNSUPPORTED_METRIC: &str = "RC001";
    /// Predicted and observed conditions do not match; comparing them would
    /// silently ignore that they describe different circumstances.
    pub const CONDITION_MISMATCH: &str = "RC002";
    /// The predicted estimate declares a time basis that does not match the
    /// observation's exposure unit.
    pub const TIME_BASIS_MISMATCH: &str = "RC003";
    /// Exposure is below the configured minimum for a meaningful comparison.
    pub const INSUFFICIENT_DATA: &str = "RC004";
    /// The predicted probability is exactly zero, so a ratio is undefined.
    pub const UNDEFINED_RATIO: &str = "RC005";
}

/// The outcome of comparing a prediction to an observation. Never asserts
/// more than the statistical method supports; see the module docs for what
/// each status actually means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CalibrationStatus {
    /// The null hypothesis (observed rate equals predicted) was not
    /// rejected at the configured significance level.
    Consistent,
    /// Rejected at `alpha` but not at the stricter `strict_alpha`.
    PotentialDeviation,
    /// Rejected at `strict_alpha`. See [`CalibrationResult::is_drift`].
    SignificantDeviation,
    /// Exposure is below the configured minimum; a status was still computed
    /// but should not be treated as a confident claim.
    InsufficientData,
    /// Predicted and observed describe different circumstances (metric,
    /// conditions, or time basis do not match); no comparison was made.
    UnsupportedComparison,
}

/// Configuration for a calibration run. Every threshold is explicit and
/// reported back in [`CalibrationMethodInfo`] so a result never carries a
/// hidden default.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationConfig {
    /// Significance level for "potential deviation". Default `0.05`.
    pub alpha: f64,
    /// Stricter significance level for "significant deviation" (drift).
    /// Default `0.01`.
    pub strict_alpha: f64,
    /// Minimum exposure for [`CalibrationStatus::InsufficientData`] not to
    /// apply. Default `20`.
    pub min_exposure: u64,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        CalibrationConfig {
            alpha: 0.05,
            strict_alpha: 0.01,
            min_exposure: 20,
        }
    }
}

/// What the reliability artifact predicted, snapshotted at comparison time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictedSummary {
    pub estimate_id: String,
    pub value: f64,
    pub metric: ProbabilityMetric,
    pub time_basis: Option<TimeBasis>,
    pub conditions: Vec<String>,
}

/// What was observed, snapshotted at comparison time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedSummary {
    pub failures: u64,
    pub exposure: u64,
    pub exposure_unit: TimeBasis,
    pub conditions: Vec<String>,
    pub proportion: f64,
    #[serde(default)]
    pub interval: Option<TimeInterval>,
}

/// The statistical method used, made explicit so an engineer can judge
/// whether it applies. Never presented as a bare number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationMethodInfo {
    pub name: String,
    pub version: String,
    /// H0, stated in full so the p-value below has a defined meaning.
    pub null_hypothesis: String,
    pub alpha: f64,
    pub strict_alpha: f64,
    pub min_exposure: u64,
}

/// Everything needed to reproduce or audit the comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationProvenance {
    pub artifact: ArtifactRef,
    #[serde(default)]
    pub dataset_refs: Vec<DatasetRef>,
    #[serde(default)]
    pub source_observation_ids: Vec<String>,
    pub analyzer: String,
    pub analyzer_version: String,
    #[serde(default)]
    pub generated_at: Option<String>,
}

/// The result of comparing one prediction to one observation. Immutable: a
/// new comparison produces a new `CalibrationResult`, never an edit to this
/// one, and nothing here can be fed back into the artifact automatically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationResult {
    pub schema: String,
    pub event: String,
    pub predicted: PredictedSummary,
    pub observed: ObservedSummary,
    /// `exposure * predicted.value`; `None` when the comparison is
    /// unsupported.
    #[serde(default)]
    pub expected_failures: Option<f64>,
    /// `observed.proportion - predicted.value`; `None` when unsupported.
    #[serde(default)]
    pub difference: Option<f64>,
    /// `observed.proportion / predicted.value`; `None` when unsupported or
    /// when the predicted value is zero (see [`code::UNDEFINED_RATIO`]).
    #[serde(default)]
    pub ratio: Option<f64>,
    pub method: CalibrationMethodInfo,
    /// Two-sided exact binomial test p-value; `None` when unsupported.
    #[serde(default)]
    pub p_value: Option<f64>,
    pub status: CalibrationStatus,
    pub provenance: CalibrationProvenance,
    #[serde(default)]
    pub diagnostics: Vec<AnalysisDiagnostic>,
}

impl CalibrationResult {
    /// "Model drift": observed behaviour materially different from the
    /// declared model under comparable conditions. See the module docs.
    pub fn is_drift(&self) -> bool {
        self.status == CalibrationStatus::SignificantDeviation
    }

    /// A short human-readable rendering, in the spirit of the other
    /// analysis result `render()` methods in this crate.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Calibration: {}\n", self.event));
        out.push_str(&format!(
            "  predicted: {:.6} ({:?})\n",
            self.predicted.value, self.predicted.metric
        ));
        out.push_str(&format!(
            "  observed:  {}/{} = {:.6}\n",
            self.observed.failures, self.observed.exposure, self.observed.proportion
        ));
        if let Some(exp) = self.expected_failures {
            out.push_str(&format!(
                "  expected failures: {:.2} (observed: {})\n",
                exp, self.observed.failures
            ));
        }
        if let Some(p) = self.p_value {
            out.push_str(&format!(
                "  p-value: {:.6} ({})\n",
                p, self.method.null_hypothesis
            ));
        }
        out.push_str(&format!("  status: {:?}\n", self.status));
        for d in &self.diagnostics {
            out.push_str(&format!("  [{}] {}\n", d.code, d.message));
        }
        out
    }
}

/// Errors that prevent calibration from running at all (as opposed to
/// producing a result with [`CalibrationStatus::UnsupportedComparison`],
/// which is a normal, informative outcome, not an error).
#[derive(Debug, Clone, thiserror::Error)]
pub enum CalibrationError {
    #[error("no estimate found for event '{0}' in the artifact")]
    NoPrediction(String),
    #[error(
        "the estimate for event '{0}' is unknown (no deterministic value); cannot calibrate \
         against an unknown probability"
    )]
    UnknownPrediction(String),
    #[error("observed data is invalid: {0}")]
    InvalidObservation(#[from] ObservationError),
}

/// Compare an artifact's prediction for `event` against one observed
/// aggregate. Never mutates `artifact`. See the module docs for the overall
/// discipline this maintains.
pub fn calibrate(
    artifact: &ReliabilityArtifact,
    event: &str,
    observed: &AggregateObservation,
    dataset_refs: Vec<DatasetRef>,
    config: &CalibrationConfig,
) -> Result<CalibrationResult, CalibrationError> {
    observed.validate()?;

    let estimate = artifact
        .select(event, &observed.conditions, None)
        .ok_or_else(|| CalibrationError::NoPrediction(event.to_string()))?;
    if estimate.is_unknown() {
        return Err(CalibrationError::UnknownPrediction(event.to_string()));
    }
    let predicted_value = estimate.value.expect("checked not unknown above");

    let predicted = PredictedSummary {
        estimate_id: estimate.event.clone(),
        value: predicted_value,
        metric: estimate.metric,
        time_basis: estimate.time_basis,
        conditions: estimate.conditions.clone(),
    };
    let proportion = observed.failures as f64 / observed.exposure as f64;
    let observed_summary = ObservedSummary {
        failures: observed.failures,
        exposure: observed.exposure,
        exposure_unit: observed.exposure_unit,
        conditions: observed.conditions.clone(),
        proportion,
        interval: observed.interval.clone(),
    };

    let method = CalibrationMethodInfo {
        name: CALIBRATION_METHOD.to_string(),
        version: CALIBRATION_METHOD_VERSION.to_string(),
        null_hypothesis: format!(
            "H0: the true failure rate for '{event}' under the observed conditions equals the \
             predicted value {predicted_value:.6} (two-sided binomial test)"
        ),
        alpha: config.alpha,
        strict_alpha: config.strict_alpha,
        min_exposure: config.min_exposure,
    };

    let mut diagnostics: Vec<AnalysisDiagnostic> = Vec::new();
    let mut unsupported = false;

    if !predicted.metric.is_probability_like() {
        diagnostics.push(
            AnalysisDiagnostic::warning(
                code::UNSUPPORTED_METRIC,
                format!(
                    "metric {:?} is not probability-like; rate-based calibration (matching \
                     units, e.g. failures/hour) is not implemented, so predicted and observed \
                     were not statistically compared",
                    predicted.metric
                ),
            )
            .about(event),
        );
        unsupported = true;
    }

    let mut pred_conditions = predicted.conditions.clone();
    pred_conditions.sort();
    let mut obs_conditions = observed_summary.conditions.clone();
    obs_conditions.sort();
    if pred_conditions != obs_conditions {
        diagnostics.push(
            AnalysisDiagnostic::warning(
                code::CONDITION_MISMATCH,
                format!(
                    "predicted conditions {:?} do not match observed conditions {:?}; \
                     comparing them would attribute a difference to the model that may only \
                     reflect different circumstances",
                    pred_conditions, obs_conditions
                ),
            )
            .about(event),
        );
        unsupported = true;
    }

    if let Some(tb) = predicted.time_basis {
        if tb != observed_summary.exposure_unit {
            diagnostics.push(
                AnalysisDiagnostic::warning(
                    code::TIME_BASIS_MISMATCH,
                    format!(
                        "predicted time basis {} does not match observed exposure unit {}",
                        tb, observed_summary.exposure_unit
                    ),
                )
                .about(event),
            );
            unsupported = true;
        }
    }

    if unsupported {
        return Ok(CalibrationResult {
            schema: CALIBRATION_SCHEMA.to_string(),
            event: event.to_string(),
            predicted,
            observed: observed_summary,
            expected_failures: None,
            difference: None,
            ratio: None,
            method,
            p_value: None,
            status: CalibrationStatus::UnsupportedComparison,
            provenance: CalibrationProvenance {
                artifact: ArtifactRef::new(artifact.id.clone())
                    .with_version(artifact.version.clone().unwrap_or_default())
                    .with_role("prediction"),
                dataset_refs,
                source_observation_ids: observed.id.clone().into_iter().collect(),
                analyzer: "etdl-reliability".to_string(),
                analyzer_version: env!("CARGO_PKG_VERSION").to_string(),
                generated_at: None,
            },
            diagnostics,
        });
    }

    let expected_failures = observed.exposure as f64 * predicted_value;
    let difference = proportion - predicted_value;
    let ratio = if predicted_value > 0.0 {
        Some(proportion / predicted_value)
    } else {
        diagnostics.push(
            AnalysisDiagnostic::info(
                code::UNDEFINED_RATIO,
                "predicted probability is zero; observed/predicted ratio is undefined",
            )
            .about(event),
        );
        None
    };

    let p_value = binomial_test_two_sided(observed.failures, observed.exposure, predicted_value);

    let status = if observed.exposure < config.min_exposure {
        diagnostics.push(
            AnalysisDiagnostic::warning(
                code::INSUFFICIENT_DATA,
                format!(
                    "exposure {} is below the configured minimum {} for a confident \
                     comparison; the p-value below is reported but should not be treated as a \
                     reliable signal",
                    observed.exposure, config.min_exposure
                ),
            )
            .about(event),
        );
        CalibrationStatus::InsufficientData
    } else if p_value < config.strict_alpha {
        CalibrationStatus::SignificantDeviation
    } else if p_value < config.alpha {
        CalibrationStatus::PotentialDeviation
    } else {
        CalibrationStatus::Consistent
    };

    Ok(CalibrationResult {
        schema: CALIBRATION_SCHEMA.to_string(),
        event: event.to_string(),
        predicted,
        observed: observed_summary,
        expected_failures: Some(expected_failures),
        difference: Some(difference),
        ratio,
        method,
        p_value: Some(p_value),
        status,
        provenance: CalibrationProvenance {
            artifact: ArtifactRef::new(artifact.id.clone())
                .with_version(artifact.version.clone().unwrap_or_default())
                .with_role("prediction"),
            dataset_refs,
            source_observation_ids: observed.id.clone().into_iter().collect(),
            analyzer: "etdl-reliability".to_string(),
            analyzer_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: None,
        },
        diagnostics,
    })
}

/// Exact two-sided binomial test p-value for `k` failures out of `n` trials
/// against a null hypothesis probability `p0`.
///
/// Uses the regularized incomplete beta function identities
/// `P(X <= k) = I_{1-p0}(n-k, k+1)` and `P(X >= k) = I_{p0}(k, n-k+1)`
/// (the same exact machinery `analysis::estimator` uses for credible
/// intervals — no normal approximation), combining the two one-sided tail
/// probabilities via the standard doubling method `min(2*min(P_le, P_ge), 1)`.
pub fn binomial_test_two_sided(k: u64, n: u64, p0: f64) -> f64 {
    if n == 0 {
        return f64::NAN;
    }
    let p0 = p0.clamp(0.0, 1.0);
    if p0 <= 0.0 {
        return if k == 0 { 1.0 } else { 0.0 };
    }
    if p0 >= 1.0 {
        return if k == n { 1.0 } else { 0.0 };
    }
    let kf = k as f64;
    let nf = n as f64;
    let p_le = if k == n {
        1.0
    } else {
        regularized_beta(1.0 - p0, nf - kf, kf + 1.0)
    };
    let p_ge = if k == 0 {
        1.0
    } else {
        regularized_beta(p0, kf, nf - kf + 1.0)
    };
    (2.0 * p_le.min(p_ge)).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use etdl_reliability_core::artifact::declared;

    fn artifact_with(event: &str, value: f64) -> ReliabilityArtifact {
        let mut a = ReliabilityArtifact::new("svc");
        a.version = Some("1.0.0".to_string());
        let mut e = declared(event, value);
        e.time_basis = Some(TimeBasis::PerRequest);
        a.add(e).unwrap();
        a
    }

    fn agg(event: &str, failures: u64, exposure: u64) -> AggregateObservation {
        AggregateObservation {
            id: Some("agg-1".to_string()),
            failure_mode: event.to_string(),
            exposure,
            failures,
            exposure_unit: TimeBasis::PerRequest,
            conditions: vec![],
            interval: None,
            source: Some("prod".to_string()),
            version: None,
        }
    }

    #[test]
    fn binomial_test_matches_hand_derivation_n10_k0() {
        // n=10, k=0, p0=0.5: P(X<=0)=0.5^10, P(X>=0)=1, two-sided = 2*0.5^10.
        let p = binomial_test_two_sided(0, 10, 0.5);
        let expected = 2.0 * 0.5f64.powi(10);
        assert!((p - expected).abs() < 1e-12, "got {p}, expected {expected}");
    }

    #[test]
    fn binomial_test_matches_hand_derivation_symmetric_case() {
        // n=4, k=4, p0=0.5: P(X>=4)=0.5^4, P(X<=4)=1, two-sided = 2*0.5^4.
        let p = binomial_test_two_sided(4, 4, 0.5);
        let expected = 2.0 * 0.5f64.powi(4);
        assert!((p - expected).abs() < 1e-9, "got {p}, expected {expected}");
    }

    #[test]
    fn binomial_test_perfect_match_is_high_p_value() {
        // Observed exactly matches predicted proportion at large n: p-value
        // should be near 1 (no evidence against H0).
        let p = binomial_test_two_sided(50, 1000, 0.05);
        assert!(p > 0.9, "got {p}");
    }

    #[test]
    fn calibrate_consistent_case() {
        let artifact = artifact_with("failure.gateway.timeout", 0.0037);
        let observed = agg("failure.gateway.timeout", 370, 100_000);
        let result = calibrate(
            &artifact,
            "failure.gateway.timeout",
            &observed,
            vec![],
            &CalibrationConfig::default(),
        )
        .unwrap();
        assert_eq!(result.status, CalibrationStatus::Consistent);
        assert!(!result.is_drift());
        assert!((result.expected_failures.unwrap() - 370.0).abs() < 1e-9);
    }

    #[test]
    fn calibrate_significant_deviation_case() {
        // Predicted 0.001 (expected 10 failures) but observed 50/10000 =
        // 0.005: a 5x, well-attested difference at meaningful exposure.
        let artifact = artifact_with("failure.gateway.timeout", 0.001);
        let observed = agg("failure.gateway.timeout", 50, 10_000);
        let result = calibrate(
            &artifact,
            "failure.gateway.timeout",
            &observed,
            vec![],
            &CalibrationConfig::default(),
        )
        .unwrap();
        assert_eq!(result.status, CalibrationStatus::SignificantDeviation);
        assert!(result.is_drift());
    }

    #[test]
    fn calibrate_insufficient_data_case() {
        let artifact = artifact_with("failure.gateway.timeout", 0.5);
        let observed = agg("failure.gateway.timeout", 1, 3);
        let result = calibrate(
            &artifact,
            "failure.gateway.timeout",
            &observed,
            vec![],
            &CalibrationConfig::default(),
        )
        .unwrap();
        assert_eq!(result.status, CalibrationStatus::InsufficientData);
        // The number is still reported, just not asserted confidently.
        assert!(result.p_value.is_some());
    }

    #[test]
    fn calibrate_refuses_condition_mismatch() {
        let artifact = artifact_with("failure.gateway.timeout", 0.01);
        let mut observed = agg("failure.gateway.timeout", 5, 1000);
        observed.conditions = vec!["high-load".to_string()];
        let result = calibrate(
            &artifact,
            "failure.gateway.timeout",
            &observed,
            vec![],
            &CalibrationConfig::default(),
        )
        .unwrap();
        assert_eq!(result.status, CalibrationStatus::UnsupportedComparison);
        assert!(!result.is_drift());
        assert!(result.p_value.is_none());
    }

    #[test]
    fn calibrate_never_mutates_artifact() {
        let artifact = artifact_with("failure.gateway.timeout", 0.0024);
        let before = artifact.clone();
        let observed = agg("failure.gateway.timeout", 37, 10_000);
        let _ = calibrate(
            &artifact,
            "failure.gateway.timeout",
            &observed,
            vec![],
            &CalibrationConfig::default(),
        )
        .unwrap();
        assert_eq!(
            before.get("failure.gateway.timeout").unwrap().value,
            artifact.get("failure.gateway.timeout").unwrap().value
        );
    }

    #[test]
    fn calibrate_missing_prediction_is_explicit_error() {
        let artifact = ReliabilityArtifact::new("svc");
        let observed = agg("failure.unknown", 1, 100);
        let err = calibrate(
            &artifact,
            "failure.unknown",
            &observed,
            vec![],
            &CalibrationConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, CalibrationError::NoPrediction(_)));
    }
}
