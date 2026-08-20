//! Seeded Monte Carlo uncertainty propagation.
//!
//! # What this computes
//!
//! Given a fault tree whose basic events carry **uncertainty about their
//! probabilities**, propagation answers: *how uncertain is the top-event
//! probability?* It does this by repeatedly drawing a full set of basic-event
//! probabilities from their declared uncertainty laws, evaluating the
//! dependency-aware fault tree for each draw, and describing the resulting
//! distribution of top-event probabilities.
//!
//! ```text
//! for s in 1..=N:
//!     for each basic event i:  q_i^(s) ~ L_i        (declared law)
//!     P_top^(s) = evaluate(tree, model, q^(s))      (exact, dependency-aware)
//! report: mean, median, quantiles of { P_top^(s) }
//! ```
//!
//! # What the output interval means
//!
//! The reported `[lower, upper]` are **empirical quantiles of the propagated
//! distribution of the top-event probability**. They are not a confidence
//! interval for an estimator, not a guaranteed range, and not a bound. Two
//! separate approximations sit between the inputs and this interval:
//!
//! 1. Monte Carlo error, which shrinks as `1/sqrt(N)` and is reported as the
//!    standard error and quantile stability in [`ConvergenceInfo`].
//! 2. Whatever the declared input laws got wrong. Propagation cannot repair a
//!    misspecified input.
//!
//! When *every* input law is a Bayesian credible representation, the output
//! interval is interpretable as a credible interval for the top event. When
//! inputs mix statistical interpretations, the output has no clean single
//! interpretation; the mix is recorded in the result rather than papered over.
//!
//! # Independence of parameter uncertainty
//!
//! Uncertainty in different basic events is sampled **independently**, even
//! when the events themselves are dependent through a common cause. Event
//! dependence and parameter-uncertainty correlation are different things: the
//! former is handled exactly by the evaluator, the latter is a documented
//! limitation that produces a
//! `CORRELATED_UNCERTAINTY_NOT_MODELLED` diagnostic.
//!
//! Monte Carlo never runs during ordinary deterministic compilation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::diagnostics::{code, AnalysisDiagnostic, Diagnostics};
use super::evaluate::DependencyEvaluator;
use super::input_uncertainty::{InputUncertainty, IntervalMeaning};
use super::model::{DependencyModel, FaultTreeSpec, ModelError};
use super::sampling::{Rng, SAMPLER_ALGORITHM, SAMPLER_VERSION};

/// The propagation method identifier recorded in results.
pub const PROPAGATION_METHOD: &str = "monte-carlo-propagation";

/// The propagation method version. Bump when the estimator, quantile
/// definition, or draw order changes.
pub const PROPAGATION_METHOD_VERSION: &str = "1";

/// The sample count used when a caller supplies none. Whenever this default is
/// applied it is reported via a `DEFAULT_APPLIED` diagnostic; it is never
/// applied silently.
pub const DEFAULT_SAMPLES: usize = 10_000;

/// Sample count below which quantile estimates are flagged as unreliable.
pub const MIN_RELIABLE_SAMPLES: usize = 1_000;

/// Number of sequential batches used for the quantile-stability check.
pub const CONVERGENCE_BATCHES: usize = 5;

/// Configuration for a Monte Carlo run.
#[derive(Debug, Clone, PartialEq)]
pub struct MonteCarloConfig {
    /// Number of samples. Must be greater than zero.
    pub samples: usize,
    /// Explicit seed for reproducibility.
    pub seed: u64,
    /// Central interval level for `lower`/`upper` (e.g. 0.95).
    pub level: f64,
}

impl Default for MonteCarloConfig {
    fn default() -> Self {
        MonteCarloConfig {
            samples: DEFAULT_SAMPLES,
            seed: 42,
            level: 0.95,
        }
    }
}

impl MonteCarloConfig {
    /// Validate the configuration. Sample count must be positive and the level
    /// must be a proper interior probability.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.samples == 0 {
            return Err(ModelError::Unsupported(
                "Monte Carlo sample count must be greater than zero".into(),
            ));
        }
        if !self.level.is_finite() || self.level <= 0.0 || self.level >= 1.0 {
            return Err(ModelError::Unsupported(format!(
                "Monte Carlo interval level {} is invalid; expected a value in (0, 1)",
                self.level
            )));
        }
        Ok(())
    }
}

/// How the reported output interval should be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PropagationSemantics {
    /// Every input law was a Bayesian credible representation, so the output
    /// quantiles are interpretable as a credible interval for the top event.
    PropagatedCredible,
    /// Every input law was calibrated to a frequentist confidence interval.
    /// The output is the propagated distribution of the top event; it is NOT a
    /// confidence interval with the stated coverage, because propagating
    /// endpoint-calibrated intervals through a nonlinear function does not
    /// preserve frequentist coverage.
    PropagatedFrequentistInputs,
    /// Inputs mixed statistical interpretations, or used shape-only laws with
    /// no coverage claim. The output is an empirical quantile interval of the
    /// propagated distribution and nothing more.
    PropagatedMixed,
    /// No input varied; there was nothing to propagate.
    NoPropagatedUncertainty,
}

impl PropagationSemantics {
    pub fn label(self) -> &'static str {
        match self {
            PropagationSemantics::PropagatedCredible => "propagated-credible-interval",
            PropagationSemantics::PropagatedFrequentistInputs => {
                "propagated-quantile-interval-from-confidence-inputs"
            }
            PropagationSemantics::PropagatedMixed => "propagated-quantile-interval",
            PropagationSemantics::NoPropagatedUncertainty => "no-propagated-uncertainty",
        }
    }

    /// A full sentence stating what the interval does and does not claim.
    pub fn interpretation(self) -> &'static str {
        match self {
            PropagationSemantics::PropagatedCredible => {
                "empirical quantiles of the propagated posterior for the top-event \
                 probability; interpretable as a Bayesian credible interval given the \
                 declared input posteriors and the model structure"
            }
            PropagationSemantics::PropagatedFrequentistInputs => {
                "empirical quantiles of the propagated distribution obtained by sampling \
                 laws calibrated to frequentist confidence intervals; this is NOT a \
                 confidence interval for the top event and carries no coverage guarantee"
            }
            PropagationSemantics::PropagatedMixed => {
                "empirical quantiles of the propagated distribution of the top-event \
                 probability; not a confidence interval, not a credible interval, and \
                 not a guaranteed range"
            }
            PropagationSemantics::NoPropagatedUncertainty => {
                "no input declared propagatable uncertainty, so this is the point \
                 estimate repeated; the absence of an interval is a modelling gap, not \
                 evidence of certainty"
            }
        }
    }
}

/// Convergence information for a propagation run.
///
/// These are **stability indicators**, not a proof of convergence. A run is
/// never described as converged merely because a fixed number of samples was
/// executed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvergenceInfo {
    /// Number of batches used for the stability check.
    pub batches: usize,
    /// Standard error of the reported mean: `sd / sqrt(N)`.
    pub standard_error: f64,
    /// `standard_error / |mean|`, or `NaN` when the mean is zero.
    pub relative_standard_error: f64,
    /// Relative spread of the lower quantile across batches.
    pub lower_quantile_spread: f64,
    /// Relative spread of the upper quantile across batches.
    pub upper_quantile_spread: f64,
    /// The exact rule applied to set `stable`.
    pub criterion: String,
    /// Whether every stability indicator met its threshold.
    pub stable: bool,
}

/// The empirical result of a Monte Carlo propagation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub samples: usize,
    pub seed: u64,
    /// Central interval level (e.g. 0.95) for `lower`/`upper`.
    pub level: f64,
    pub mean: f64,
    pub median: f64,
    pub lower: f64,
    pub upper: f64,
    pub min: f64,
    pub max: f64,
    /// Sample standard deviation of the propagated top-event probability.
    pub std_dev: f64,
    /// Additional quantiles, keyed by probability rendered to four decimals.
    #[serde(default)]
    pub quantiles: BTreeMap<String, f64>,
    pub method: String,
    pub method_version: String,
    pub sampler: String,
    pub sampler_version: String,
    /// What the interval means.
    pub semantics: PropagationSemantics,
    /// The same, as a sentence.
    pub interpretation: String,
    /// How many inputs actually varied between samples.
    pub variable_inputs: usize,
    /// How many individual input draws were clamped into [0, 1].
    pub truncated_samples: usize,
    pub convergence: ConvergenceInfo,
}

/// Quantile of a sorted slice using linear interpolation between order
/// statistics (the R type-7 / NumPy default definition).
fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return f64::NAN;
    }
    if n == 1 {
        return sorted[0];
    }
    let h = (n as f64 - 1.0) * p.clamp(0.0, 1.0);
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    if lo == hi {
        return sorted[lo.min(n - 1)];
    }
    let frac = h - lo as f64;
    sorted[lo] + (sorted[hi.min(n - 1)] - sorted[lo]) * frac
}

fn relative_spread(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut sum = 0.0;
    for &v in values {
        lo = lo.min(v);
        hi = hi.max(v);
        sum += v;
    }
    let mean = sum / values.len() as f64;
    if mean.abs() <= f64::MIN_POSITIVE {
        return if (hi - lo).abs() <= f64::MIN_POSITIVE {
            0.0
        } else {
            f64::INFINITY
        };
    }
    (hi - lo) / mean.abs()
}

impl MonteCarloResult {
    /// Build a result from raw samples.
    ///
    /// `variable_inputs` is how many input laws actually varied; `truncated`
    /// is how many individual draws were clamped into [0, 1].
    pub fn from_samples(
        mut values: Vec<f64>,
        config: &MonteCarloConfig,
        semantics: PropagationSemantics,
        variable_inputs: usize,
        truncated: usize,
    ) -> Self {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = values.len();
        let tail = (1.0 - config.level) / 2.0;

        let mean = if n == 0 {
            f64::NAN
        } else {
            values.iter().sum::<f64>() / n as f64
        };
        let var = if n > 1 {
            values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n as f64 - 1.0)
        } else {
            0.0
        };
        let std_dev = var.max(0.0).sqrt();
        let standard_error = if n > 0 {
            std_dev / (n as f64).sqrt()
        } else {
            f64::NAN
        };
        let relative_standard_error = if mean.abs() > 0.0 {
            standard_error / mean.abs()
        } else {
            f64::NAN
        };

        let mut quantiles = BTreeMap::new();
        for p in [0.005, 0.025, 0.05, 0.25, 0.5, 0.75, 0.95, 0.975, 0.995] {
            quantiles.insert(format!("{p:.4}"), quantile_sorted(&values, p));
        }

        // Quantile stability: the sorted sample is split into interleaved
        // sub-samples and the quantile is re-estimated in each. For an i.i.d.
        // sample this measures whether the quantile estimate survives
        // discarding most of the data — i.e. whether the tail is supported by
        // enough draws to be meaningful.
        let batches = CONVERGENCE_BATCHES.min(n.max(1));
        let mut lower_batch = Vec::new();
        let mut upper_batch = Vec::new();
        if n >= batches && batches > 1 {
            for b in 0..batches {
                let sub: Vec<f64> = values
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i % batches == b)
                    .map(|(_, v)| *v)
                    .collect();
                if sub.len() >= 2 {
                    lower_batch.push(quantile_sorted(&sub, tail));
                    upper_batch.push(quantile_sorted(&sub, 1.0 - tail));
                }
            }
        }
        let lower_quantile_spread = relative_spread(&lower_batch);
        let upper_quantile_spread = relative_spread(&upper_batch);

        let stable = variable_inputs > 0
            && n >= MIN_RELIABLE_SAMPLES
            && relative_standard_error.is_finite()
            && relative_standard_error <= 0.01
            && lower_quantile_spread.is_finite()
            && lower_quantile_spread <= 0.10
            && upper_quantile_spread.is_finite()
            && upper_quantile_spread <= 0.10;

        let criterion = if variable_inputs == 0 {
            "no input law varied; stability is not assessed".to_string()
        } else {
            format!(
                "stable iff samples >= {MIN_RELIABLE_SAMPLES}, relative standard error <= 0.01, \
                 and both {batches}-batch quantile spreads <= 0.10 (stability heuristic, not a \
                 convergence proof)"
            )
        };

        MonteCarloResult {
            samples: n,
            seed: config.seed,
            level: config.level,
            mean,
            median: quantile_sorted(&values, 0.5),
            lower: quantile_sorted(&values, tail),
            upper: quantile_sorted(&values, 1.0 - tail),
            min: values.first().copied().unwrap_or(f64::NAN),
            max: values.last().copied().unwrap_or(f64::NAN),
            std_dev,
            quantiles,
            method: PROPAGATION_METHOD.to_string(),
            method_version: PROPAGATION_METHOD_VERSION.to_string(),
            sampler: SAMPLER_ALGORITHM.to_string(),
            sampler_version: SAMPLER_VERSION.to_string(),
            semantics,
            interpretation: semantics.interpretation().to_string(),
            variable_inputs,
            truncated_samples: truncated,
            convergence: ConvergenceInfo {
                batches,
                standard_error,
                relative_standard_error,
                lower_quantile_spread,
                upper_quantile_spread,
                criterion,
                stable,
            },
        }
    }

    /// Backwards-compatible constructor: builds a result with mixed semantics
    /// and no truncation accounting.
    pub fn from_values(values: Vec<f64>, config: &MonteCarloConfig) -> Self {
        Self::from_samples(values, config, PropagationSemantics::PropagatedMixed, 0, 0)
    }

    /// The width of the reported central interval.
    pub fn interval_width(&self) -> f64 {
        self.upper - self.lower
    }
}

/// Decide the output semantics from the set of input laws actually used.
fn semantics_for(laws: &[&InputUncertainty]) -> PropagationSemantics {
    let variable: Vec<&&InputUncertainty> = laws.iter().filter(|l| l.is_variable()).collect();
    if variable.is_empty() {
        return PropagationSemantics::NoPropagatedUncertainty;
    }
    let mut credible = 0usize;
    let mut confidence = 0usize;
    let mut other = 0usize;
    for l in &variable {
        match l {
            InputUncertainty::NormalFromInterval { meaning, .. } => match meaning {
                IntervalMeaning::Credible => credible += 1,
                IntervalMeaning::Confidence => confidence += 1,
                IntervalMeaning::PlausibleRange => other += 1,
            },
            // A Beta over a failure probability is the conjugate posterior form
            // and is treated as a credible representation.
            InputUncertainty::Beta { .. } => credible += 1,
            _ => other += 1,
        }
    }
    if other == 0 && confidence == 0 && credible > 0 {
        PropagationSemantics::PropagatedCredible
    } else if other == 0 && credible == 0 && confidence > 0 {
        PropagationSemantics::PropagatedFrequentistInputs
    } else {
        PropagationSemantics::PropagatedMixed
    }
}

/// Propagate declared input uncertainty through the dependency-aware fault
/// tree.
///
/// `laws` maps basic-event id to its sampling law. Basic events absent from
/// `laws` are held at their point value and produce a `NO_DECLARED_UNCERTAINTY`
/// diagnostic.
pub fn propagate(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
    laws: &BTreeMap<String, InputUncertainty>,
    config: &MonteCarloConfig,
) -> Result<(MonteCarloResult, Diagnostics), ModelError> {
    propagate_with_frozen(tree, model, laws, config, &BTreeSet::new())
}

/// As [`propagate`], but with a set of inputs held fixed at their nominal
/// value.
///
/// Frozen inputs still consume their random draws, so a frozen run and a full
/// run with the same seed use **common random numbers**. That is what makes
/// the difference between the two runs attributable to the frozen input rather
/// than to sampling noise.
pub fn propagate_with_frozen(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
    laws: &BTreeMap<String, InputUncertainty>,
    config: &MonteCarloConfig,
    frozen: &BTreeSet<String>,
) -> Result<(MonteCarloResult, Diagnostics), ModelError> {
    config.validate()?;
    let evaluator = DependencyEvaluator::new(tree, model);
    evaluator.check_supported()?;

    let mut diagnostics = Diagnostics::new();

    // Resolve one law per leaf, in the tree's deterministic key order.
    let mut resolved: Vec<(String, InputUncertainty)> = Vec::with_capacity(tree.leaves.len());
    for (leaf, point) in &tree.leaves {
        let law = match laws.get(leaf) {
            Some(l) => {
                if frozen.contains(leaf) {
                    InputUncertainty::fixed(l.nominal().unwrap_or(*point))
                } else {
                    l.clone()
                }
            }
            None => {
                diagnostics.push(
                    AnalysisDiagnostic::warning(
                        code::NO_DECLARED_UNCERTAINTY,
                        "no uncertainty declared; held at its point value, so it \
                         contributes nothing to the reported interval",
                    )
                    .about(leaf),
                );
                InputUncertainty::fixed(*point)
            }
        };
        resolved.push((leaf.clone(), law));
    }

    let variable_inputs = resolved.iter().filter(|(_, l)| l.is_variable()).count();
    if variable_inputs == 0 {
        diagnostics.push(AnalysisDiagnostic::warning(
            code::NO_DECLARED_UNCERTAINTY,
            "no basic event declared propagatable uncertainty; the reported interval \
             has zero width because nothing was propagated, not because the estimate \
             is certain",
        ));
    }

    if !model.common_causes.is_empty() && variable_inputs > 0 {
        diagnostics.push(AnalysisDiagnostic::warning(
            code::CORRELATED_UNCERTAINTY_NOT_MODELLED,
            "basic events are dependent through a declared common cause, but their \
             parameter uncertainties are sampled independently; correlated parameter \
             uncertainty is not supported and the reported interval may be too narrow",
        ));
    }

    if config.samples < MIN_RELIABLE_SAMPLES && variable_inputs > 0 {
        diagnostics.push(AnalysisDiagnostic::warning(
            code::INSUFFICIENT_SAMPLES,
            format!(
                "{} samples requested; tail quantiles are unreliable below {}",
                config.samples, MIN_RELIABLE_SAMPLES
            ),
        ));
    }

    let mut rng = Rng::new(config.seed);
    let mut values = Vec::with_capacity(config.samples);
    let mut truncated = 0usize;
    let mut forced: BTreeMap<String, f64> = BTreeMap::new();

    for _ in 0..config.samples {
        forced.clear();
        for (leaf, law) in &resolved {
            let (v, t) = law.sample(&mut rng);
            if t {
                truncated += 1;
            }
            forced.insert(leaf.clone(), v);
        }
        values.push(evaluator.point_estimate_with(&forced)?);
    }

    let law_refs: Vec<&InputUncertainty> = resolved.iter().map(|(_, l)| l).collect();
    let semantics = semantics_for(&law_refs);
    let result =
        MonteCarloResult::from_samples(values, config, semantics, variable_inputs, truncated);

    let total_draws = config.samples.saturating_mul(resolved.len());
    if truncated > 0 && total_draws > 0 {
        let frac = truncated as f64 / total_draws as f64;
        diagnostics.push(AnalysisDiagnostic::warning(
            code::SAMPLE_TRUNCATION,
            format!(
                "{truncated} of {total_draws} input draws ({:.2}%) fell outside [0,1] and \
                 were clamped; the effective input law is truncated and its mean differs \
                 from the declared mean",
                frac * 100.0
            ),
        ));
    }

    if variable_inputs > 0 && !result.convergence.stable {
        diagnostics.push(AnalysisDiagnostic::warning(
            code::NOT_CONVERGED,
            format!(
                "stability indicators not met ({}); relative standard error {:.3e}, \
                 quantile spreads {:.3e}/{:.3e}",
                result.convergence.criterion,
                result.convergence.relative_standard_error,
                result.convergence.lower_quantile_spread,
                result.convergence.upper_quantile_spread
            ),
        ));
    }

    if result.mean > 0.0 && result.mean < 1e-9 {
        diagnostics.push(AnalysisDiagnostic::info(
            code::RARE_EVENT_PRECISION,
            format!(
                "propagated mean is {:.3e}; with {} samples the tail quantiles are \
                 supported by very few draws and should be read as order-of-magnitude only",
                result.mean, result.samples
            ),
        ));
    }

    Ok((result, diagnostics.sorted()))
}

/// Legacy entry point: propagate using `[lower, upper]` interval endpoints,
/// calibrating a truncated Normal to the configured level.
///
/// Retained for backwards compatibility. Prefer [`propagate`], which takes the
/// declared uncertainty law directly and therefore does not have to guess a
/// shape.
pub fn run_monte_carlo(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
    leaf_intervals: &BTreeMap<String, (f64, f64)>,
    config: &MonteCarloConfig,
) -> Result<MonteCarloResult, ModelError> {
    let mut laws = BTreeMap::new();
    for (leaf, (lo, hi)) in leaf_intervals {
        laws.insert(
            leaf.clone(),
            InputUncertainty::NormalFromInterval {
                lower: *lo,
                upper: *hi,
                level: config.level,
                meaning: IntervalMeaning::PlausibleRange,
            },
        );
    }
    propagate(tree, model, &laws, config).map(|(r, _)| r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::dependence::model::{GateKind, GateSpec};

    fn or_tree() -> FaultTreeSpec {
        let mut tree = FaultTreeSpec::new("top");
        tree.leaves.insert("A".into(), 0.01);
        tree.leaves.insert("B".into(), 0.02);
        tree.gates.insert(
            "top".into(),
            GateSpec {
                kind: GateKind::Or,
                inputs: vec!["A".into(), "B".into()],
                k: None,
            },
        );
        tree
    }

    #[test]
    fn zero_samples_is_rejected() {
        let config = MonteCarloConfig {
            samples: 0,
            seed: 1,
            level: 0.95,
        };
        assert!(config.validate().is_err());
        let err = propagate(
            &or_tree(),
            &DependencyModel::independent(),
            &BTreeMap::new(),
            &config,
        )
        .unwrap_err();
        assert!(matches!(err, ModelError::Unsupported(_)));
    }

    #[test]
    fn invalid_level_is_rejected() {
        for level in [0.0, 1.0, -0.5, f64::NAN] {
            let config = MonteCarloConfig {
                samples: 10,
                seed: 1,
                level,
            };
            assert!(config.validate().is_err(), "level {level} must be rejected");
        }
    }

    #[test]
    fn sample_count_is_respected_exactly() {
        for n in [1usize, 37, 500] {
            let config = MonteCarloConfig {
                samples: n,
                seed: 3,
                level: 0.95,
            };
            let (r, _) = propagate(
                &or_tree(),
                &DependencyModel::independent(),
                &BTreeMap::new(),
                &config,
            )
            .unwrap();
            assert_eq!(r.samples, n);
        }
    }

    #[test]
    fn no_declared_uncertainty_is_reported_not_hidden() {
        let config = MonteCarloConfig {
            samples: 100,
            seed: 3,
            level: 0.95,
        };
        let (r, diags) = propagate(
            &or_tree(),
            &DependencyModel::independent(),
            &BTreeMap::new(),
            &config,
        )
        .unwrap();
        assert_eq!(r.semantics, PropagationSemantics::NoPropagatedUncertainty);
        assert_eq!(r.variable_inputs, 0);
        assert_eq!(r.lower, r.upper);
        assert!(
            !r.convergence.stable,
            "must not claim stability with no inputs"
        );
        assert!(diags
            .iter()
            .any(|d| d.code == code::NO_DECLARED_UNCERTAINTY));
    }

    #[test]
    fn quantile_definition_is_linear_interpolation() {
        let v = vec![1.0, 2.0, 3.0, 4.0];
        assert!((quantile_sorted(&v, 0.5) - 2.5).abs() < 1e-12);
        assert!((quantile_sorted(&v, 0.0) - 1.0).abs() < 1e-12);
        assert!((quantile_sorted(&v, 1.0) - 4.0).abs() < 1e-12);
        // h = 3 * 0.25 = 0.75 -> 1 + 0.75*(2-1) = 1.75
        assert!((quantile_sorted(&v, 0.25) - 1.75).abs() < 1e-12);
    }

    #[test]
    fn semantics_do_not_silently_merge_interpretations() {
        let credible = InputUncertainty::Beta {
            alpha: 2.0,
            beta: 100.0,
        };
        let confidence = InputUncertainty::NormalFromInterval {
            lower: 0.01,
            upper: 0.03,
            level: 0.95,
            meaning: IntervalMeaning::Confidence,
        };
        assert_eq!(
            semantics_for(&[&credible]),
            PropagationSemantics::PropagatedCredible
        );
        assert_eq!(
            semantics_for(&[&confidence]),
            PropagationSemantics::PropagatedFrequentistInputs
        );
        assert_eq!(
            semantics_for(&[&credible, &confidence]),
            PropagationSemantics::PropagatedMixed
        );
    }

    #[test]
    fn frozen_input_uses_common_random_numbers() {
        let mut laws = BTreeMap::new();
        laws.insert(
            "A".to_string(),
            InputUncertainty::Beta {
                alpha: 2.0,
                beta: 198.0,
            },
        );
        laws.insert(
            "B".to_string(),
            InputUncertainty::Beta {
                alpha: 4.0,
                beta: 196.0,
            },
        );
        let config = MonteCarloConfig {
            samples: 4_000,
            seed: 17,
            level: 0.95,
        };
        let tree = or_tree();
        let model = DependencyModel::independent();
        let (full, _) = propagate(&tree, &model, &laws, &config).unwrap();
        let mut frozen = BTreeSet::new();
        frozen.insert("B".to_string());
        let (partial, _) = propagate_with_frozen(&tree, &model, &laws, &config, &frozen).unwrap();
        // Freezing removes variance, never adds it.
        assert!(
            partial.std_dev < full.std_dev,
            "freezing B should reduce output variance: {} vs {}",
            partial.std_dev,
            full.std_dev
        );
        assert_eq!(partial.samples, full.samples);
    }
}
