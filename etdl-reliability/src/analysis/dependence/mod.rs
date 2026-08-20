//! Dependency-aware reliability analysis.
//!
//! This module answers six questions about a fault tree, and keeps them
//! separate:
//!
//! | Question | Answered by |
//! |---|---|
//! | What is the estimated probability? | [`DependencyEvaluator::point_estimate`] |
//! | How uncertain is that estimate? | [`monte_carlo::propagate`] |
//! | How does input uncertainty affect the top event? | [`MonteCarloResult`] |
//! | Which events contribute most? | [`importance_result`] |
//! | Which input should be investigated first? | [`sensitivity_result`] + importance |
//! | Which assumption drives the result? | the result's assumptions and diagnostics |
//!
//! ## Four concepts that are never interconverted
//!
//! - **Probability** — how likely an event is.
//! - **Uncertainty** — how well the probability itself is known.
//! - **Sensitivity** — how much the answer moves when an input moves.
//! - **Importance** — how much the top event depends on an entity structurally.
//!
//! A high-importance event may be precisely known and therefore contribute no
//! uncertainty. A high-uncertainty input may barely move the answer. Collapsing
//! these into one "score" would destroy exactly the information an engineer
//! needs, so each is computed by its own method and reported under its own
//! name.
//!
//! ## The most important rule
//!
//! If the model declares a dependency, analysis either computes it correctly
//! or rejects the model as unsupported. It never silently substitutes an
//! independence-based answer. See
//! [`DependencyEvaluator::check_supported`].
//!
//! ## Deterministic compilation is unaffected
//!
//! ```text
//! analysis -> analysis result -> engineer selects -> reliability artifact
//!          -> compiler -> deterministic generated value
//! ```
//!
//! Nothing in this module runs during ordinary compilation. The compiler
//! depends on `etdl-reliability-core` only, and never samples a distribution.

pub mod diagnostics;
mod evaluate;
pub mod input_uncertainty;
mod model;
mod monte_carlo;
mod result;
pub mod sampling;

use std::collections::{BTreeMap, BTreeSet};

pub use diagnostics::{code as diagnostic_code, AnalysisDiagnostic, Diagnostics, Severity};
pub use evaluate::{
    importance, importance_result, is_coherent, sensitivity, sensitivity_result,
    DependencyEvaluator, IMPORTANCE_METHOD, IMPORTANCE_METHOD_VERSION, RELATIVE_EPSILON,
    SENSITIVITY_METHOD, SENSITIVITY_METHOD_VERSION,
};
pub use input_uncertainty::{InputUncertainty, IntervalMeaning, UnsupportedUncertainty};
pub use model::{
    BetaFactor, CommonCause, ConditionalProbability, DependencyEdge, DependencyKind,
    DependencyModel, FaultTreeSpec, GateKind, GateSpec, IndependenceAssumption, ModelError,
};
pub use monte_carlo::{
    propagate, propagate_with_frozen, run_monte_carlo, ConvergenceInfo, MonteCarloConfig,
    MonteCarloResult, PropagationSemantics, DEFAULT_SAMPLES, MIN_RELIABLE_SAMPLES,
    PROPAGATION_METHOD, PROPAGATION_METHOD_VERSION,
};
pub use result::{
    compare, implemented_importance_measures, AnalysisComparison, AnalysisInputs,
    AnalysisMetadata, AnalysisProvenance, AnalysisResult, ArtifactRef, ConditionalSummary,
    DependencySummary, ImportanceEntry, ImportanceResult, InputChange, InputSnapshot,
    PerturbationOutcome, RankChange, SensitivityEntry, SensitivityResult, UncertaintyContribution,
    UncertaintyRanking, ANALYSIS_SCHEMA, ANALYSIS_SCHEMA_VERSION, COMPARISON_SCHEMA,
    IMPORTANCE_SCHEMA, SENSITIVITY_SCHEMA, UNCERTAINTY_RANKING_SCHEMA,
};

/// The default perturbation used for sensitivity when the caller supplies
/// none. Applied only through [`AnalysisOptions::default`], and reported in
/// the result so it is never a hidden constant.
pub const DEFAULT_PERTURBATION: f64 = 1e-3;

/// The method identifier for the uncertainty contribution ranking.
pub const UNCERTAINTY_RANKING_METHOD: &str = "variance-freeze-one-at-a-time/common-random-numbers";
/// Version of the uncertainty contribution ranking.
pub const UNCERTAINTY_RANKING_METHOD_VERSION: &str = "1";

/// What to compute, and with what parameters.
#[derive(Debug, Clone)]
pub struct AnalysisOptions {
    /// Monte Carlo configuration. `None` skips uncertainty propagation
    /// entirely.
    pub monte_carlo: Option<MonteCarloConfig>,
    /// Declared uncertainty law per input id. Inputs absent from this map are
    /// held at their point value and produce a diagnostic.
    pub inputs: BTreeMap<String, InputUncertainty>,
    /// Absolute perturbation size for sensitivity.
    pub perturbation: f64,
    pub compute_importance: bool,
    pub compute_sensitivity: bool,
    /// Compute the uncertainty contribution ranking. Costs one extra
    /// propagation run per uncertain input, so it is opt-in.
    pub compute_uncertainty_ranking: bool,
    /// Identity metadata recorded in the result.
    pub metadata: AnalysisMetadata,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        AnalysisOptions {
            monte_carlo: None,
            inputs: BTreeMap::new(),
            perturbation: DEFAULT_PERTURBATION,
            compute_importance: true,
            compute_sensitivity: true,
            compute_uncertainty_ranking: false,
            metadata: AnalysisMetadata {
                model_id: "fault-tree".to_string(),
                ..Default::default()
            },
        }
    }
}

/// Run a full dependency-aware analysis and return a traceable result artifact.
///
/// Retained with its original signature. Prefer [`analyze_with`], which does
/// not force a Monte Carlo run and lets uncertainty laws be declared per input.
pub fn analyze(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
    config: &MonteCarloConfig,
) -> Result<AnalysisResult, ModelError> {
    let options = AnalysisOptions {
        monte_carlo: Some(config.clone()),
        ..Default::default()
    };
    analyze_with(tree, model, &options)
}

/// Run an analysis with explicit options.
///
/// Fails when the model declares a dependency structure the evaluator cannot
/// represent. It never returns an independence-based number for a model that
/// says its events are dependent.
pub fn analyze_with(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
    options: &AnalysisOptions,
) -> Result<AnalysisResult, ModelError> {
    model.validate(tree)?;
    let evaluator = DependencyEvaluator::new(tree, model);
    evaluator.check_supported()?;

    let point = evaluator.point_estimate()?;
    let mut diagnostics = Diagnostics::new();

    // ---- uncertainty propagation ------------------------------------------
    let mut uncertainty = None;
    let mut ranking = None;
    if let Some(config) = &options.monte_carlo {
        let (mc, diags) = propagate(tree, model, &options.inputs, config)?;
        diagnostics.extend(diags);

        if options.compute_uncertainty_ranking {
            let (rank, rank_diags) = uncertainty_ranking(tree, model, &options.inputs, config, &mc)?;
            diagnostics.extend(rank_diags);
            ranking = Some(rank);
        }
        uncertainty = Some(mc);
    }

    // ---- importance --------------------------------------------------------
    let importance_res = if options.compute_importance {
        Some(importance_result(tree, model)?)
    } else {
        None
    };

    // ---- sensitivity -------------------------------------------------------
    let sensitivity_res = if options.compute_sensitivity {
        Some(sensitivity_result(tree, model, options.perturbation)?)
    } else {
        None
    };

    if point > 0.0 && point < 1e-9 {
        diagnostics.push(AnalysisDiagnostic::info(
            diagnostic_code::RARE_EVENT_PRECISION,
            format!(
                "the top-event probability is {point:.6e}; gate arithmetic is evaluated in \
                 log space and is accurate to about one ulp, but any reported precision \
                 beyond that of the input estimates themselves is not meaningful"
            ),
        ));
    }

    let mut meta = options.metadata.clone();
    meta.uncertainty_ranking = ranking;

    Ok(AnalysisResult::build(
        tree,
        model,
        point,
        uncertainty,
        Some(&options.inputs),
        importance_res
            .as_ref()
            .map(|r| r.entries.clone())
            .unwrap_or_default(),
        importance_res,
        sensitivity_res
            .as_ref()
            .map(|r| r.entries.clone())
            .unwrap_or_default(),
        sensitivity_res,
        diagnostics.sorted().items,
        meta,
    ))
}

/// Rank inputs by how much of the output uncertainty each one accounts for.
///
/// # Method
///
/// For each uncertain input `i`, the propagation is re-run with `i` held at
/// its nominal value while every other input keeps its declared law. Because
/// frozen inputs still consume their random draws, the two runs use **common
/// random numbers**, so the difference in output variance is attributable to
/// `i` rather than to sampling noise. The reported share is
///
/// ```text
/// share_i = 1 - Var(top | q_i fixed) / Var(top)
/// ```
///
/// # What this is not
///
/// This is **not** importance, and the shares are **not** a variance
/// decomposition that sums to one — with a nonlinear model the individual
/// effects do not partition the total. Treat it as a ranking of where better
/// evidence would most reduce output uncertainty, which is precisely the
/// question "which probability estimate deserves more measurement?".
///
/// An input whose share falls at or below the run's own Monte Carlo noise
/// floor is flagged `above_noise_floor: false` rather than being reported as a
/// small but real effect.
pub fn uncertainty_ranking(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
    laws: &BTreeMap<String, InputUncertainty>,
    config: &MonteCarloConfig,
    baseline: &MonteCarloResult,
) -> Result<(UncertaintyRanking, Diagnostics), ModelError> {
    let mut diagnostics = Diagnostics::new();
    let baseline_var = baseline.std_dev * baseline.std_dev;
    let mut entries = Vec::new();

    // The noise floor: the sampling error of a variance estimate from n draws
    // is approximately Var * sqrt(2/(n-1)).
    let noise_floor = if baseline.samples > 1 {
        (2.0 / (baseline.samples as f64 - 1.0)).sqrt()
    } else {
        f64::INFINITY
    };

    for (id, law) in laws {
        if !law.is_variable() {
            continue;
        }
        if !tree.leaves.contains_key(id) {
            continue;
        }
        let mut frozen = BTreeSet::new();
        frozen.insert(id.clone());
        let (frozen_result, _) = propagate_with_frozen(tree, model, laws, config, &frozen)?;
        let frozen_var = frozen_result.std_dev * frozen_result.std_dev;
        let raw_share = if baseline_var > 0.0 {
            1.0 - frozen_var / baseline_var
        } else {
            0.0
        };
        entries.push(UncertaintyContribution {
            id: id.clone(),
            input_law: law.describe(),
            baseline_std_dev: baseline.std_dev,
            std_dev_when_frozen: frozen_result.std_dev,
            variance_share: raw_share.clamp(0.0, 1.0),
            above_noise_floor: raw_share > noise_floor,
        });
    }

    entries.sort_by(|a, b| {
        b.variance_share
            .partial_cmp(&a.variance_share)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    if baseline_var <= 0.0 {
        diagnostics.push(AnalysisDiagnostic::warning(
            diagnostic_code::NO_DECLARED_UNCERTAINTY,
            "the baseline propagation produced zero output variance, so no input can be \
             ranked by uncertainty contribution",
        ));
    }
    for e in &entries {
        if !e.above_noise_floor {
            diagnostics.push(
                AnalysisDiagnostic::info(
                    diagnostic_code::NOT_CONVERGED,
                    format!(
                        "the variance share for '{}' ({:.4}) is at or below the Monte Carlo \
                         noise floor ({:.4}) at {} samples; increase the sample count before \
                         reading it as a real effect",
                        e.id, e.variance_share, noise_floor, baseline.samples
                    ),
                )
                .about(&e.id),
            );
        }
    }

    Ok((
        UncertaintyRanking {
            schema: UNCERTAINTY_RANKING_SCHEMA.to_string(),
            method: UNCERTAINTY_RANKING_METHOD.to_string(),
            method_version: UNCERTAINTY_RANKING_METHOD_VERSION.to_string(),
            metric: "fraction of top-event variance removed when this input's uncertainty is \
                     eliminated; a data-collection priority signal, NOT an importance measure \
                     and NOT a variance decomposition that sums to one"
                .to_string(),
            samples: baseline.samples,
            seed: baseline.seed,
            baseline_std_dev: baseline.std_dev,
            entries,
            assumptions: vec![
                "one input is frozen at a time; interaction effects are not separated"
                    .to_string(),
                "frozen and baseline runs share random draws (common random numbers), so \
                 the difference reflects the frozen input and not sampling noise"
                    .to_string(),
                "parameter uncertainties are sampled independently across inputs".to_string(),
            ],
            diagnostics: diagnostics.clone().sorted().items,
        },
        diagnostics.sorted(),
    ))
}
