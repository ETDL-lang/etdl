//! Reliability analysis: statistical estimation and importance/sensitivity.
//!
//! This is the **analysis path** — separate from the deterministic compilation
//! path. Analysis consumes observations/evidence and produces typed
//! [`ProbabilityEstimate`]s with provenance; it does not parse `.etdl`
//! directly and never mutates the source model.

pub mod dependence;
pub mod estimation;
pub mod estimator;
pub mod sensitivity;

pub use dependence::{analyze, analyze_with, AnalysisOptions};
pub use estimation::{BetaBinomial, EmpiricalEstimate, ExponentialFailure};
pub use estimator::{
    beta_credible_interval, beta_quantile, builtin_estimators, normal_quantile, regularized_beta,
    wilson, BetaBinomialEstimator, EmpiricalBinomialEstimator, EstimationConfig, EstimationError,
    ExponentialRateEstimator, ReliabilityEstimator, StatisticalInterpretation,
};
pub use sensitivity::{ImportanceMetric, SensitivityResult};
