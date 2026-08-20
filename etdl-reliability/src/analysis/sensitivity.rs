//! Named importance metrics, and the legacy flat contribution result.
//!
//! [`ImportanceMetric`] is the canonical vocabulary of importance measures for
//! the whole crate: the dependency-aware analysis in
//! [`crate::analysis::dependence`] names its measures from this enum rather
//! than inventing a parallel one.
//!
//! [`SensitivityResult`] here is the **legacy** flat contribution shape kept
//! for compatibility. It computes `p_i / P(top)` for an OR gate, which is a
//! probability contribution ratio and nothing more — it is not Birnbaum
//! importance, not a sensitivity, and not valid when events are dependent. New
//! work should use
//! [`dependence::importance_result`](crate::analysis::dependence::importance_result)
//! and
//! [`dependence::sensitivity_result`](crate::analysis::dependence::sensitivity_result),
//! which carry method identity, provenance and diagnostics.

use serde::{Deserialize, Serialize};

/// The importance metric used (named explicitly, never ambiguous).
///
/// Implemented by the dependency-aware analysis: [`Birnbaum`](Self::Birnbaum),
/// [`FussellVesely`](Self::FussellVesely), [`Criticality`](Self::Criticality),
/// [`RiskAchievementWorth`](Self::RiskAchievementWorth) and
/// [`RiskReductionWorth`](Self::RiskReductionWorth).
///
/// Reserved for future implementation, listed here so the vocabulary is stable
/// before the algorithms land: [`Diagnostic`](Self::Diagnostic),
/// [`Differential`](Self::Differential), [`Structural`](Self::Structural).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportanceMetric {
    /// Birnbaum importance: `P(top | i occurs) - P(top | i absent)`. For a
    /// coherent tree with independent basic events this equals the partial
    /// derivative `dP(top)/dq_i`.
    Birnbaum,
    /// Fussell-Vesely importance: `(P(top) - P(top | i absent)) / P(top)`.
    /// Coherent trees only.
    FussellVesely,
    /// Criticality importance: `Birnbaum_i * q_i / P(top)`.
    Criticality,
    /// Risk Achievement Worth: `P(top | i occurs) / P(top)`.
    RiskAchievementWorth,
    /// Risk Reduction Worth: `P(top) / P(top | i absent)`.
    RiskReductionWorth,
    /// Raw probability contribution `q_i / P(top)`. A ratio of probabilities,
    /// not an importance measure in the structural sense.
    Contribution,
    /// Diagnostic importance `P(i | top)`. Not implemented.
    Diagnostic,
    /// Differential importance measure (DIM). Not implemented.
    Differential,
    /// Structural importance (probability-free). Not implemented.
    Structural,
}

impl ImportanceMetric {
    /// The stable metric name used in JSON output.
    pub fn name(self) -> &'static str {
        match self {
            ImportanceMetric::Birnbaum => "birnbaum",
            ImportanceMetric::FussellVesely => "fussell-vesely",
            ImportanceMetric::Criticality => "criticality",
            ImportanceMetric::RiskAchievementWorth => "risk-achievement-worth",
            ImportanceMetric::RiskReductionWorth => "risk-reduction-worth",
            ImportanceMetric::Contribution => "contribution",
            ImportanceMetric::Diagnostic => "diagnostic",
            ImportanceMetric::Differential => "differential",
            ImportanceMetric::Structural => "structural",
        }
    }

    /// Whether this crate computes the metric today.
    pub fn is_implemented(self) -> bool {
        matches!(
            self,
            ImportanceMetric::Birnbaum
                | ImportanceMetric::FussellVesely
                | ImportanceMetric::Criticality
                | ImportanceMetric::RiskAchievementWorth
                | ImportanceMetric::RiskReductionWorth
                | ImportanceMetric::Contribution
        )
    }

    /// Whether the metric's definition requires a coherent (monotone) tree.
    pub fn requires_coherence(self) -> bool {
        matches!(
            self,
            ImportanceMetric::FussellVesely | ImportanceMetric::Criticality
        )
    }
}

/// Sensitivity/importance of the top event to each contributor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensitivityResult {
    pub top_event: String,
    pub metric: ImportanceMetric,
    pub contributors: Vec<Contributor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contributor {
    pub event: String,
    pub contribution: f64,
}

impl SensitivityResult {
    /// Compute contribution of each input to an OR gate's top probability:
    /// `contribution_i = p_i / P(top)` for `P(top) > 0`.
    ///
    /// Valid only for a single OR gate over **independent** inputs. It is a
    /// probability ratio, not an importance measure: it must not be described
    /// as a probability of failure or as Birnbaum importance.
    pub fn or_contributions(top_event: &str, inputs: &[f64], top_probability: f64) -> Self {
        let contributors = if top_probability <= 0.0 {
            inputs
                .iter()
                .enumerate()
                .map(|(i, _)| Contributor {
                    event: format!("input{}", i),
                    contribution: 0.0,
                })
                .collect()
        } else {
            inputs
                .iter()
                .enumerate()
                .map(|(i, p)| Contributor {
                    event: format!("input{}", i),
                    contribution: p / top_probability,
                })
                .collect()
        };
        SensitivityResult {
            top_event: top_event.to_string(),
            metric: ImportanceMetric::Contribution,
            contributors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_names_are_stable() {
        assert_eq!(ImportanceMetric::Birnbaum.name(), "birnbaum");
        assert_eq!(ImportanceMetric::FussellVesely.name(), "fussell-vesely");
        assert!(ImportanceMetric::Birnbaum.is_implemented());
        assert!(!ImportanceMetric::Diagnostic.is_implemented());
        assert!(ImportanceMetric::FussellVesely.requires_coherence());
        assert!(!ImportanceMetric::Birnbaum.requires_coherence());
    }

    #[test]
    fn contribution_is_not_importance() {
        // OR(0.7, 0.2): the contribution ratio for A is 0.7/0.76 = 0.921,
        // while Birnbaum importance for A is 1 - 0.2 = 0.8. Different
        // questions, different numbers; the metric names must not be
        // interchanged.
        let top = 1.0 - (0.3 * 0.8);
        let r = SensitivityResult::or_contributions("top", &[0.7, 0.2], top);
        let contribution = r.contributors[0].contribution;
        let birnbaum = 1.0 - 0.2;
        assert!((contribution - 0.7 / top).abs() < 1e-12);
        assert!((contribution - birnbaum).abs() > 0.1);
        assert_eq!(r.metric, ImportanceMetric::Contribution);
    }

    #[test]
    fn or_contributions() {
        // OR(0.7, 0.2): top = 1-(0.3*0.8) = 0.76
        let top = 1.0 - (0.3 * 0.8);
        let r = SensitivityResult::or_contributions("top", &[0.7, 0.2], top);
        assert!((r.contributors[0].contribution - 0.7 / top).abs() < 1e-9);
    }
}
