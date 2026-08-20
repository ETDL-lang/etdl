//! The reliability analysis-result artifact.
//!
//! An analysis result is **historical evidence**: it records what was
//! analysed, by which analyzer version, under which assumptions, with which
//! method, and what came out. It is not a cache of the current answer.
//!
//! Three rules govern this module:
//!
//! 1. **Results are immutable.** New samples, new observations, new estimates
//!    or a new analyzer version produce a *new* result with a new
//!    [`AnalysisResult::analysis_id`]. Nothing mutates a result in place.
//! 2. **Inputs are identified, never implied.** The result records the actual
//!    values and artifact versions analysed. "the current model" is not a
//!    recordable input.
//! 3. **Version kinds are kept apart.** Data version, model version, analyzer
//!    version, artifact schema version and analysis-result version are five
//!    different things and each has its own field.
//!
//! A [`ReliabilityArtifact`](etdl_reliability_core::artifact::ReliabilityArtifact)
//! holds *estimates*. This artifact holds the *result of an analysis run* over
//! them. The compiler consumes the former and never the latter: an engineer
//! reads an analysis result, decides, and writes the chosen deterministic
//! value into a reliability artifact.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::diagnostics::AnalysisDiagnostic;
use super::input_uncertainty::InputUncertainty;
use super::model::{DependencyModel, FaultTreeSpec, IndependenceAssumption};
use super::monte_carlo::MonteCarloResult;

/// The analysis-result artifact schema identifier.
pub const ANALYSIS_SCHEMA: &str = "etdl.reliability.analysis-result/1.0";

/// The schema version of the analysis-result artifact. Additive changes bump
/// the minor component; a breaking change bumps [`ANALYSIS_SCHEMA`] itself.
pub const ANALYSIS_SCHEMA_VERSION: &str = "1.1";

/// The importance-result schema identifier.
pub const IMPORTANCE_SCHEMA: &str = "etdl.reliability.importance-result/1.0";

/// The sensitivity-result schema identifier.
pub const SENSITIVITY_SCHEMA: &str = "etdl.reliability.sensitivity-result/1.0";

/// The uncertainty-contribution schema identifier.
pub const UNCERTAINTY_RANKING_SCHEMA: &str = "etdl.reliability.uncertainty-ranking/1.0";

/// The analyzer name recorded in results.
pub const ANALYZER: &str = "etdl-reliability";

// ---------------------------------------------------------------------------
// Importance
// ---------------------------------------------------------------------------

/// The importance measures this implementation computes, by name.
///
/// Measures documented as future extensions and deliberately **not**
/// implemented here: diagnostic importance, differential importance measure
/// (DIM), structural importance, and Barlow-Proschan importance. The result
/// type is a list of named measures precisely so they can be added without a
/// schema break.
pub fn implemented_importance_measures(coherent: bool) -> Vec<String> {
    let mut m = vec![
        "birnbaum".to_string(),
        "risk-achievement-worth".to_string(),
        "risk-reduction-worth".to_string(),
    ];
    if coherent {
        m.push("fussell-vesely".to_string());
        m.push("criticality".to_string());
    }
    m
}

/// One importance entry, for a basic event or a common cause.
///
/// Every number is accompanied by the conditional probabilities it was derived
/// from, so a reader can check the arithmetic without re-running the analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportanceEntry {
    /// The model entity analysed. For a common cause this is the cause's own
    /// id, not the ids of the events it affects.
    pub id: String,
    pub is_common_cause: bool,
    /// `basic-event` or `common-cause`.
    pub entity_kind: String,
    /// The entity's own probability `q_i`, when it has one.
    pub event_probability: Option<f64>,
    /// Baseline top-event probability `P`.
    pub top_probability: f64,
    /// `P(top | entity occurs)`.
    pub top_given_occurred: f64,
    /// `P(top | entity does not occur)`.
    pub top_given_absent: f64,
    /// Birnbaum importance `P(1_i) - P(0_i)`.
    pub birnbaum: f64,
    /// Fussell-Vesely importance `(P - P(0_i)) / P`. `None` for non-coherent
    /// trees or a zero top event.
    #[serde(default)]
    pub fussell_vesely: Option<f64>,
    /// Criticality importance `I_B(i) * q_i / P`.
    #[serde(default)]
    pub criticality: Option<f64>,
    /// Risk Achievement Worth `P(1_i) / P`.
    pub raw: f64,
    /// Risk Reduction Worth `P / P(0_i)`.
    pub rrw: f64,
}

/// A full importance result: which measures, over which model, under which
/// assumptions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportanceResult {
    pub schema: String,
    pub top_event: String,
    pub baseline_top_probability: f64,
    /// Whether the tree is coherent. Coherence determines which measures are
    /// defined.
    pub coherent: bool,
    pub method: String,
    pub method_version: String,
    /// The measures actually computed, by name.
    pub measures: Vec<String>,
    pub entries: Vec<ImportanceEntry>,
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<AnalysisDiagnostic>,
}

impl ImportanceResult {
    /// The entries ranked by a named measure, descending, skipping entries for
    /// which the measure is undefined.
    pub fn ranked_by(&self, measure: &str) -> Vec<(&ImportanceEntry, f64)> {
        let mut v: Vec<(&ImportanceEntry, f64)> = self
            .entries
            .iter()
            .filter_map(|e| {
                let value = match measure {
                    "birnbaum" => Some(e.birnbaum),
                    "fussell-vesely" => e.fussell_vesely,
                    "criticality" => e.criticality,
                    "risk-achievement-worth" => Some(e.raw),
                    "risk-reduction-worth" => Some(e.rrw),
                    _ => None,
                }?;
                if value.is_finite() {
                    Some((e, value))
                } else {
                    None
                }
            })
            .collect();
        v.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.id.cmp(&b.0.id))
        });
        v
    }
}

// ---------------------------------------------------------------------------
// Sensitivity
// ---------------------------------------------------------------------------

/// One leg of a two-sided perturbation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerturbationOutcome {
    /// `increase` or `decrease`.
    pub direction: String,
    pub input_baseline: f64,
    pub input_perturbed: f64,
    pub top_baseline: f64,
    pub top_perturbed: f64,
    /// `top_perturbed - top_baseline`.
    pub delta: f64,
    /// Elasticity `(dP/P) / (dq/q)`, or `None` when a denominator is at or
    /// below the relative epsilon.
    #[serde(default)]
    pub relative_sensitivity: Option<f64>,
    /// False when the perturbation could not move (already at a bound).
    pub applied: bool,
}

/// One sensitivity entry.
///
/// The flat `alternative` / `top_alternative` / `delta` fields describe the
/// **increase** leg and exist for continuity with earlier results; the
/// `increase` and `decrease` fields carry the full two-sided picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensitivityEntry {
    pub id: String,
    pub is_common_cause: bool,
    /// `basic-event` or `common-cause`.
    pub entity_kind: String,
    pub baseline: f64,
    pub alternative: f64,
    pub top_baseline: f64,
    pub top_alternative: f64,
    pub delta: f64,
    pub method: String,
    /// The absolute perturbation size requested.
    pub perturbation: f64,
    #[serde(default)]
    pub increase: Option<PerturbationOutcome>,
    #[serde(default)]
    pub decrease: Option<PerturbationOutcome>,
    /// Elasticity for the increase leg.
    #[serde(default)]
    pub relative_sensitivity: Option<f64>,
    /// Whether the two legs mirrored each other. `None` when only one leg
    /// could be applied. A `false` here is a finding, not a defect: it means
    /// the response is genuinely nonlinear around the baseline.
    #[serde(default)]
    pub two_sided_symmetric: Option<bool>,
    /// Conditions under which the baseline value holds.
    #[serde(default)]
    pub conditions: Vec<String>,
}

/// A full sensitivity result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensitivityResult {
    pub schema: String,
    pub top_event: String,
    pub top_baseline: f64,
    pub method: String,
    pub method_version: String,
    pub perturbation: f64,
    pub entries: Vec<SensitivityEntry>,
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<AnalysisDiagnostic>,
}

// ---------------------------------------------------------------------------
// Uncertainty contribution
// ---------------------------------------------------------------------------

/// How much of the output uncertainty is attributable to one input's
/// uncertainty.
///
/// This is **not** importance. Importance asks how much the top event depends
/// on an event's *occurrence*; this asks how much the width of the top event's
/// interval is driven by not knowing that event's *probability* well. A
/// well-understood dominant contributor can have high importance and near-zero
/// uncertainty contribution; a poorly measured minor contributor can be the
/// reverse. The two rankings answer different engineering questions and are
/// reported separately and labelled distinctly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UncertaintyContribution {
    pub id: String,
    /// The sampling law declared for this input.
    pub input_law: String,
    /// Output standard deviation with every input uncertain.
    pub baseline_std_dev: f64,
    /// Output standard deviation with this input held at its nominal value.
    pub std_dev_when_frozen: f64,
    /// `1 - Var(frozen)/Var(baseline)`, the fraction of output variance that
    /// disappears when this input stops being uncertain. Clamped to [0,1];
    /// negative raw values indicate Monte Carlo noise exceeding the effect and
    /// are reported as zero.
    pub variance_share: f64,
    /// Whether the effect exceeded the run's own noise floor.
    pub above_noise_floor: bool,
}

/// The uncertainty contribution ranking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UncertaintyRanking {
    pub schema: String,
    pub method: String,
    pub method_version: String,
    /// Statement of what the metric is and is not.
    pub metric: String,
    pub samples: usize,
    pub seed: u64,
    pub baseline_std_dev: f64,
    pub entries: Vec<UncertaintyContribution>,
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<AnalysisDiagnostic>,
}

// ---------------------------------------------------------------------------
// Inputs and provenance
// ---------------------------------------------------------------------------

/// A reference to an immutable input artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    /// What role the artifact played: `probability-estimates`,
    /// `dependency-model`, `uncertainty-inputs`, ...
    #[serde(default)]
    pub role: Option<String>,
}

impl ArtifactRef {
    pub fn new(id: impl Into<String>) -> Self {
        ArtifactRef {
            id: id.into(),
            version: None,
            schema: None,
            role: None,
        }
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }
}

/// One analysed input, snapshotted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSnapshot {
    pub id: String,
    /// `basic-event` or `common-cause`.
    pub kind: String,
    /// The point probability analysed.
    pub value: f64,
    /// The declared uncertainty law, when one was supplied.
    #[serde(default)]
    pub uncertainty: Option<String>,
}

/// The analysis input snapshot: everything the result was computed from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisInputs {
    pub top_event: String,
    pub basic_event_count: usize,
    pub gate_count: usize,
    pub inputs: Vec<InputSnapshot>,
    /// Referenced immutable artifacts, by id and version.
    #[serde(default)]
    pub artifacts: Vec<ArtifactRef>,
}

/// Everything needed to reproduce or audit the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisProvenance {
    pub analyzer: String,
    pub analyzer_version: String,
    /// The evaluation method (`independent` / `dependency-conditioning`).
    pub method: String,
    pub method_version: String,
    /// The uncertainty propagation method, when propagation ran.
    #[serde(default)]
    pub propagation_method: Option<String>,
    #[serde(default)]
    pub propagation_method_version: Option<String>,
    #[serde(default)]
    pub sampler: Option<String>,
    #[serde(default)]
    pub sampler_version: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub samples: Option<usize>,
    #[serde(default)]
    pub interval_level: Option<f64>,
    /// Version of the reliability ontology the input ids belong to.
    #[serde(default)]
    pub ontology_version: Option<String>,
    /// Optional RFC 3339 timestamp. Excluded from the analysis id so that two
    /// runs of the same analysis at different times share an identity.
    #[serde(default)]
    pub generated_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Summaries retained from the previous schema
// ---------------------------------------------------------------------------

/// A summary of a declared dependency for the report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencySummary {
    pub id: String,
    pub kind: String,
    pub probability: Option<f64>,
    pub affects: Vec<String>,
    pub ontology_id: Option<String>,
}

/// A summary of a declared conditional probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalSummary {
    pub event: String,
    pub given: String,
    pub probability: f64,
}

// ---------------------------------------------------------------------------
// The analysis result
// ---------------------------------------------------------------------------

/// The full analysis result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub schema: String,
    pub schema_version: String,
    /// Content-derived identity of this analysis. Two runs over identical
    /// inputs with identical methods and versions share an id; any change to
    /// inputs, method, seed, sample count or analyzer version produces a
    /// different one.
    pub analysis_id: String,
    /// Identity of the analysed model.
    pub model_id: String,
    #[serde(default)]
    pub model_version: Option<String>,
    pub top_event: String,
    /// Retained from schema 1.0.
    pub model_identity: String,
    pub analyzer: String,
    pub analyzer_version: String,
    pub method: String,
    pub independence: String,
    #[serde(default)]
    pub dependencies: Vec<DependencySummary>,
    #[serde(default)]
    pub conditional: Vec<ConditionalSummary>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    /// The analysed inputs, snapshotted.
    pub inputs: AnalysisInputs,
    /// The deterministic point estimate of the top-event probability.
    pub point_estimate: f64,
    /// Uncertainty result from the optional propagation run.
    pub uncertainty: Option<MonteCarloResult>,
    /// Importance entries, sorted by Birnbaum descending.
    #[serde(default)]
    pub importance: Vec<ImportanceEntry>,
    /// The full importance result, when importance was computed.
    #[serde(default)]
    pub importance_result: Option<ImportanceResult>,
    /// Sensitivity entries, sorted by |delta| descending.
    #[serde(default)]
    pub sensitivity: Vec<SensitivityEntry>,
    /// The full sensitivity result, when sensitivity was computed.
    #[serde(default)]
    pub sensitivity_result: Option<SensitivityResult>,
    /// Which inputs drive the output uncertainty, when requested.
    #[serde(default)]
    pub uncertainty_ranking: Option<UncertaintyRanking>,
    pub provenance: AnalysisProvenance,
    #[serde(default)]
    pub diagnostics: Vec<AnalysisDiagnostic>,
    /// The deterministic compiler result (what `etdl compile` would use).
    pub deterministic_result: f64,
}

/// A non-cryptographic 64-bit FNV-1a hash, used to derive a stable analysis
/// identity from content. It is an identity, not a security primitive.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

impl AnalysisResult {
    /// Assemble a result from its parts.
    ///
    /// This is the schema-1.0-compatible entry point. Callers wanting the full
    /// artifact should use the builder in
    /// [`super::analyze_with`](super::analyze_with).
    #[allow(clippy::too_many_arguments)]
    pub fn assemble(
        tree: &FaultTreeSpec,
        model: &DependencyModel,
        point: f64,
        uncertainty: Option<MonteCarloResult>,
        importance: Vec<ImportanceEntry>,
        sensitivity: Vec<SensitivityEntry>,
    ) -> Self {
        Self::build(
            tree,
            model,
            point,
            uncertainty,
            None,
            importance,
            None,
            sensitivity,
            None,
            Vec::new(),
            AnalysisMetadata::default(),
        )
    }

    /// Full assembly.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        tree: &FaultTreeSpec,
        model: &DependencyModel,
        point: f64,
        uncertainty: Option<MonteCarloResult>,
        laws: Option<&BTreeMap<String, InputUncertainty>>,
        importance: Vec<ImportanceEntry>,
        importance_result: Option<ImportanceResult>,
        sensitivity: Vec<SensitivityEntry>,
        sensitivity_result: Option<SensitivityResult>,
        diagnostics: Vec<AnalysisDiagnostic>,
        meta: AnalysisMetadata,
    ) -> Self {
        let dependencies: Vec<DependencySummary> = model
            .common_causes
            .iter()
            .map(|cc| DependencySummary {
                id: cc.id.clone(),
                kind: "common-cause".to_string(),
                probability: Some(cc.probability),
                affects: cc.affects.clone(),
                ontology_id: cc.ontology_id.clone(),
            })
            .chain(model.edges.iter().map(|e| DependencySummary {
                id: format!("{}->{}", e.from, e.to),
                kind: format!("{:?}", e.kind).to_lowercase(),
                probability: None,
                affects: vec![e.to.clone()],
                ontology_id: None,
            }))
            .collect();

        let conditional: Vec<ConditionalSummary> = model
            .conditional
            .iter()
            .map(|c| ConditionalSummary {
                event: c.event.clone(),
                given: c.given.clone(),
                probability: c.probability,
            })
            .collect();

        let mut assumptions = Vec::new();
        match model.independence {
            IndependenceAssumption::Assumed => {
                assumptions.push("independence of basic events assumed".to_string());
            }
            IndependenceAssumption::NotAssumed => {
                assumptions.push("independence of basic events NOT assumed".to_string());
            }
            IndependenceAssumption::Unspecified => {
                assumptions.push("independence assumption unspecified".to_string());
            }
        }
        for cc in &model.common_causes {
            assumptions.push(format!(
                "common cause '{}' declared (P = {:.6}) affects {:?}",
                cc.id, cc.probability, cc.affects
            ));
            assumptions.extend(cc.assumptions.iter().cloned());
        }
        assumptions.extend(meta.assumptions.iter().cloned());

        // Input snapshot: the actual analysed values, never "current".
        let mut inputs: Vec<InputSnapshot> = tree
            .leaves
            .iter()
            .map(|(id, v)| InputSnapshot {
                id: id.clone(),
                kind: "basic-event".to_string(),
                value: *v,
                uncertainty: laws
                    .and_then(|l| l.get(id))
                    .map(|law| law.describe()),
            })
            .collect();
        for cc in &model.common_causes {
            inputs.push(InputSnapshot {
                id: cc.id.clone(),
                kind: "common-cause".to_string(),
                value: cc.probability,
                uncertainty: laws.and_then(|l| l.get(&cc.id)).map(|law| law.describe()),
            });
        }

        let method = if model.common_causes.is_empty() {
            "independent".to_string()
        } else {
            "dependency-conditioning".to_string()
        };

        let provenance = AnalysisProvenance {
            analyzer: ANALYZER.to_string(),
            analyzer_version: env!("CARGO_PKG_VERSION").to_string(),
            method: method.clone(),
            method_version: "1".to_string(),
            propagation_method: uncertainty.as_ref().map(|u| u.method.clone()),
            propagation_method_version: uncertainty.as_ref().map(|u| u.method_version.clone()),
            sampler: uncertainty.as_ref().map(|u| u.sampler.clone()),
            sampler_version: uncertainty.as_ref().map(|u| u.sampler_version.clone()),
            seed: uncertainty.as_ref().map(|u| u.seed),
            samples: uncertainty.as_ref().map(|u| u.samples),
            interval_level: uncertainty.as_ref().map(|u| u.level),
            ontology_version: meta.ontology_version.clone(),
            generated_at: meta.generated_at.clone(),
        };

        let analysis_inputs = AnalysisInputs {
            top_event: tree.top_event.clone(),
            basic_event_count: tree.leaves.len(),
            gate_count: tree.gates.len(),
            inputs,
            artifacts: meta.artifacts.clone(),
        };

        let analysis_id = derive_analysis_id(&analysis_inputs, &provenance, model);

        AnalysisResult {
            schema: ANALYSIS_SCHEMA.to_string(),
            schema_version: ANALYSIS_SCHEMA_VERSION.to_string(),
            analysis_id,
            model_id: meta.model_id.clone(),
            model_version: meta.model_version.clone(),
            top_event: tree.top_event.clone(),
            model_identity: "fault-tree".to_string(),
            analyzer: ANALYZER.to_string(),
            analyzer_version: env!("CARGO_PKG_VERSION").to_string(),
            method,
            independence: model.independence.label().to_string(),
            dependencies,
            conditional,
            assumptions,
            inputs: analysis_inputs,
            point_estimate: point,
            uncertainty,
            importance,
            importance_result,
            sensitivity,
            sensitivity_result,
            uncertainty_ranking: meta.uncertainty_ranking.clone(),
            provenance,
            diagnostics,
            deterministic_result: point,
        }
    }

    /// Human-readable report.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Reliability Analysis\n");
        out.push_str("====================\n");
        out.push_str(&format!("Schema:          {}\n", self.schema));
        out.push_str(&format!("Analysis Id:     {}\n", self.analysis_id));
        out.push_str(&format!("Top Event:       {}\n", self.top_event));
        out.push_str(&format!(
            "Model:           {}{}\n",
            self.model_id,
            self.model_version
                .as_ref()
                .map(|v| format!(" @{v}"))
                .unwrap_or_default()
        ));
        out.push_str(&format!(
            "Analyzer:        {} v{}\n",
            self.analyzer, self.analyzer_version
        ));
        out.push_str(&format!("Method:          {}\n", self.method));
        out.push_str(&format!(
            "Independence:    {}\n",
            if self.independence == "independent" {
                "ASSUMED"
            } else {
                "NOT ASSUMED"
            }
        ));
        out.push('\n');
        out.push_str("Probability:\n");
        out.push_str(&format!("    {:.6e}\n", self.point_estimate));

        if let Some(mc) = &self.uncertainty {
            out.push('\n');
            out.push_str("Uncertainty:\n");
            out.push_str(&format!(
                "    {:.0}% {} [{:.6e}, {:.6e}]\n",
                mc.level * 100.0,
                mc.semantics.label(),
                mc.lower,
                mc.upper
            ));
            out.push_str(&format!(
                "    mean {:.6e}  median {:.6e}  sd {:.3e}\n",
                mc.mean, mc.median, mc.std_dev
            ));
            out.push_str(&format!(
                "    n={} seed={} sampler={}/{} varying inputs={}\n",
                mc.samples, mc.seed, mc.sampler, mc.sampler_version, mc.variable_inputs
            ));
            out.push_str(&format!(
                "    stability: {} (rse {:.2e})\n",
                if mc.convergence.stable {
                    "indicators met"
                } else {
                    "INDICATORS NOT MET"
                },
                mc.convergence.relative_standard_error
            ));
            out.push_str(&format!("    reads as: {}\n", mc.interpretation));
        }

        if !self.dependencies.is_empty() {
            out.push('\n');
            out.push_str("Dependencies:\n");
            for d in &self.dependencies {
                let p = d
                    .probability
                    .map(|v| format!(" P={v:.6e}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "    {} [{}]{} affects {:?}\n",
                    d.id, d.kind, p, d.affects
                ));
            }
        }

        if !self.importance.is_empty() {
            out.push('\n');
            out.push_str("Dominant Contributors (Birnbaum importance):\n");
            for (i, e) in self.importance.iter().take(5).enumerate() {
                out.push_str(&format!(
                    "  {}. {}{}\n",
                    i + 1,
                    e.id,
                    if e.is_common_cause { "  [common cause]" } else { "" }
                ));
                out.push_str(&format!(
                    "     Birnbaum: {:.6e}   metric: birnbaum   method: exact-conditioning\n",
                    e.birnbaum
                ));
                if let Some(fv) = e.fussell_vesely {
                    out.push_str(&format!("     Fussell-Vesely: {fv:.6e}\n"));
                }
                if let Some(cr) = e.criticality {
                    out.push_str(&format!("     Criticality: {cr:.6e}\n"));
                }
            }
        }

        if !self.sensitivity.is_empty() {
            out.push('\n');
            out.push_str("Sensitivity (two-sided finite perturbation):\n");
            for e in self.sensitivity.iter().take(5) {
                out.push_str(&format!("  {}:\n", e.id));
                out.push_str(&format!(
                    "     baseline q={:.6e}  perturbation +/-{:.3e}\n",
                    e.baseline, e.perturbation
                ));
                if let Some(up) = &e.increase {
                    out.push_str(&format!(
                        "     increase -> q={:.6e}  dP(top)={:+.6e}\n",
                        up.input_perturbed, up.delta
                    ));
                }
                if let Some(down) = &e.decrease {
                    out.push_str(&format!(
                        "     decrease -> q={:.6e}  dP(top)={:+.6e}\n",
                        down.input_perturbed, down.delta
                    ));
                }
                match e.relative_sensitivity {
                    Some(r) => out.push_str(&format!("     elasticity: {r:.4}\n")),
                    None => out.push_str("     elasticity: undefined (near-zero baseline)\n"),
                }
                if e.two_sided_symmetric == Some(false) {
                    out.push_str("     response is asymmetric around the baseline\n");
                }
            }
        }

        if let Some(r) = &self.uncertainty_ranking {
            out.push('\n');
            out.push_str("Uncertainty Contribution (NOT importance):\n");
            out.push_str(&format!("    metric: {}\n", r.metric));
            for e in r.entries.iter().take(5) {
                out.push_str(&format!(
                    "    {}: {:.1}% of output variance ({}){}\n",
                    e.id,
                    e.variance_share * 100.0,
                    e.input_law,
                    if e.above_noise_floor {
                        ""
                    } else {
                        "  [at or below Monte Carlo noise]"
                    }
                ));
            }
        }

        if !self.assumptions.is_empty() {
            out.push('\n');
            out.push_str("Assumptions:\n");
            for a in &self.assumptions {
                out.push_str(&format!("  - {a}\n"));
            }
        }

        let all_diags: Vec<&AnalysisDiagnostic> = self
            .diagnostics
            .iter()
            .chain(
                self.importance_result
                    .iter()
                    .flat_map(|r| r.diagnostics.iter()),
            )
            .chain(
                self.sensitivity_result
                    .iter()
                    .flat_map(|r| r.diagnostics.iter()),
            )
            .collect();
        if !all_diags.is_empty() {
            out.push('\n');
            out.push_str("Diagnostics:\n");
            for d in all_diags {
                out.push_str(&format!(
                    "  [{}] {}{}: {}\n",
                    d.code,
                    d.severity.label(),
                    d.subject
                        .as_ref()
                        .map(|s| format!(" ({s})"))
                        .unwrap_or_default(),
                    d.message
                ));
            }
        }
        out
    }
}

/// Optional metadata supplied by the caller when assembling a result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnalysisMetadata {
    pub model_id: String,
    pub model_version: Option<String>,
    pub ontology_version: Option<String>,
    pub generated_at: Option<String>,
    pub artifacts: Vec<ArtifactRef>,
    pub assumptions: Vec<String>,
    pub uncertainty_ranking: Option<UncertaintyRanking>,
}

/// Derive a stable, content-based analysis identity.
///
/// The identity covers the analysed inputs, the dependency model, and the
/// method/version/seed/sample-count. It deliberately excludes the timestamp,
/// so re-running the same analysis tomorrow yields the same id — which is what
/// makes "is this the same analysis?" answerable.
fn derive_analysis_id(
    inputs: &AnalysisInputs,
    provenance: &AnalysisProvenance,
    model: &DependencyModel,
) -> String {
    let mut buf = String::new();
    buf.push_str(&inputs.top_event);
    for i in &inputs.inputs {
        buf.push_str(&format!(
            "|{}:{}:{:.17e}:{}",
            i.kind,
            i.id,
            i.value,
            i.uncertainty.as_deref().unwrap_or("-")
        ));
    }
    for a in &inputs.artifacts {
        buf.push_str(&format!(
            "|artifact:{}:{}",
            a.id,
            a.version.as_deref().unwrap_or("-")
        ));
    }
    buf.push_str(&format!("|independence:{}", model.independence.label()));
    for cc in &model.common_causes {
        buf.push_str(&format!("|cc:{}:{:.17e}", cc.id, cc.probability));
    }
    for c in &model.conditional {
        buf.push_str(&format!(
            "|cond:{}|{}:{:.17e}",
            c.event, c.given, c.probability
        ));
    }
    for e in &model.edges {
        buf.push_str(&format!("|edge:{}->{}:{:?}", e.from, e.to, e.kind));
    }
    buf.push_str(&format!(
        "|analyzer:{}:{}|method:{}:{}|prop:{}:{}|sampler:{}:{}|seed:{}|n:{}|level:{}",
        provenance.analyzer,
        provenance.analyzer_version,
        provenance.method,
        provenance.method_version,
        provenance.propagation_method.as_deref().unwrap_or("-"),
        provenance
            .propagation_method_version
            .as_deref()
            .unwrap_or("-"),
        provenance.sampler.as_deref().unwrap_or("-"),
        provenance.sampler_version.as_deref().unwrap_or("-"),
        provenance
            .seed
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-".into()),
        provenance
            .samples
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-".into()),
        provenance
            .interval_level
            .map(|s| format!("{s:.6}"))
            .unwrap_or_else(|| "-".into()),
    ));
    format!("ana-{:016x}", fnv1a(buf.as_bytes()))
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// One input that differs between two analyses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputChange {
    pub id: String,
    pub kind: String,
    pub before: Option<f64>,
    pub after: Option<f64>,
    /// `(after - before) / before`, when both are present and before is
    /// non-zero.
    pub relative_change: Option<f64>,
    /// `added`, `removed`, `changed`, or `uncertainty-changed`.
    pub change: String,
}

/// A change in an entity's importance rank between two analyses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankChange {
    pub id: String,
    pub before_rank: Option<usize>,
    pub after_rank: Option<usize>,
    pub before_birnbaum: Option<f64>,
    pub after_birnbaum: Option<f64>,
}

/// The comparison of two analysis results, e.g. before and after a mitigation.
///
/// The comparison reports **what changed**. It does not attribute the
/// top-event change to any particular input: when more than one input,
/// assumption or method differs, `single_change` is false and
/// `causal_attribution` says so explicitly. Attributing an outcome to one of
/// several simultaneous modifications is an engineering judgement, and this
/// type refuses to make it on the engineer's behalf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisComparison {
    pub schema: String,
    pub before_analysis_id: String,
    pub after_analysis_id: String,
    pub top_event: String,
    pub before_probability: f64,
    pub after_probability: f64,
    pub absolute_change: f64,
    /// `(after - before) / before`, or `None` when before is zero.
    pub relative_change: Option<f64>,
    pub before_interval: Option<(f64, f64)>,
    pub after_interval: Option<(f64, f64)>,
    pub input_changes: Vec<InputChange>,
    pub assumption_changes: Vec<String>,
    pub method_changes: Vec<String>,
    pub importance_rank_changes: Vec<RankChange>,
    /// True when exactly one input changed and nothing else did.
    pub single_change: bool,
    pub causal_attribution: String,
    #[serde(default)]
    pub diagnostics: Vec<AnalysisDiagnostic>,
}

/// The comparison schema identifier.
pub const COMPARISON_SCHEMA: &str = "etdl.reliability.analysis-comparison/1.0";

/// Compare two analysis results.
pub fn compare(before: &AnalysisResult, after: &AnalysisResult) -> AnalysisComparison {
    let mut diagnostics = Vec::new();
    if before.top_event != after.top_event {
        diagnostics.push(AnalysisDiagnostic::warning(
            super::diagnostics::code::UNSUPPORTED_DEPENDENCY,
            format!(
                "the two analyses have different top events ('{}' vs '{}'); the \
                 comparison is between different questions",
                before.top_event, after.top_event
            ),
        ));
    }

    let index = |r: &AnalysisResult| -> BTreeMap<String, InputSnapshot> {
        r.inputs
            .inputs
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect()
    };
    let b = index(before);
    let a = index(after);

    let mut input_changes = Vec::new();
    let mut ids: Vec<&String> = b.keys().chain(a.keys()).collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        match (b.get(id), a.get(id)) {
            (Some(x), Some(y)) => {
                let value_changed = (x.value - y.value).abs() > 0.0;
                let uncertainty_changed = x.uncertainty != y.uncertainty;
                if value_changed || uncertainty_changed {
                    input_changes.push(InputChange {
                        id: id.clone(),
                        kind: y.kind.clone(),
                        before: Some(x.value),
                        after: Some(y.value),
                        relative_change: if x.value.abs() > 0.0 {
                            Some((y.value - x.value) / x.value)
                        } else {
                            None
                        },
                        change: match (value_changed, uncertainty_changed) {
                            (true, true) => "changed-and-uncertainty-changed".to_string(),
                            (true, false) => "changed".to_string(),
                            _ => "uncertainty-changed".to_string(),
                        },
                    });
                }
            }
            (Some(x), None) => input_changes.push(InputChange {
                id: id.clone(),
                kind: x.kind.clone(),
                before: Some(x.value),
                after: None,
                relative_change: None,
                change: "removed".to_string(),
            }),
            (None, Some(y)) => input_changes.push(InputChange {
                id: id.clone(),
                kind: y.kind.clone(),
                before: None,
                after: Some(y.value),
                relative_change: None,
                change: "added".to_string(),
            }),
            (None, None) => {}
        }
    }

    let before_assumptions: std::collections::BTreeSet<&String> =
        before.assumptions.iter().collect();
    let after_assumptions: std::collections::BTreeSet<&String> = after.assumptions.iter().collect();
    let mut assumption_changes: Vec<String> = before_assumptions
        .difference(&after_assumptions)
        .map(|s| format!("removed: {s}"))
        .collect();
    assumption_changes.extend(
        after_assumptions
            .difference(&before_assumptions)
            .map(|s| format!("added: {s}")),
    );
    assumption_changes.sort();

    let mut method_changes = Vec::new();
    if before.method != after.method {
        method_changes.push(format!(
            "evaluation method: {} -> {}",
            before.method, after.method
        ));
    }
    if before.analyzer_version != after.analyzer_version {
        method_changes.push(format!(
            "analyzer version: {} -> {}",
            before.analyzer_version, after.analyzer_version
        ));
    }
    if before.provenance.propagation_method != after.provenance.propagation_method
        || before.provenance.sampler_version != after.provenance.sampler_version
    {
        method_changes.push("uncertainty propagation method or sampler changed".to_string());
    }
    if before.provenance.seed != after.provenance.seed
        || before.provenance.samples != after.provenance.samples
    {
        method_changes.push(format!(
            "Monte Carlo configuration: seed {:?}/n {:?} -> seed {:?}/n {:?}",
            before.provenance.seed,
            before.provenance.samples,
            after.provenance.seed,
            after.provenance.samples
        ));
    }

    let rank_index = |r: &AnalysisResult| -> BTreeMap<String, (usize, f64)> {
        r.importance
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id.clone(), (i + 1, e.birnbaum)))
            .collect()
    };
    let rb = rank_index(before);
    let ra = rank_index(after);
    let mut rank_ids: Vec<&String> = rb.keys().chain(ra.keys()).collect();
    rank_ids.sort();
    rank_ids.dedup();
    let mut importance_rank_changes: Vec<RankChange> = rank_ids
        .into_iter()
        .filter_map(|id| {
            let x = rb.get(id);
            let y = ra.get(id);
            let changed = match (x, y) {
                (Some(p), Some(q)) => p.0 != q.0,
                _ => true,
            };
            if !changed {
                return None;
            }
            Some(RankChange {
                id: id.clone(),
                before_rank: x.map(|v| v.0),
                after_rank: y.map(|v| v.0),
                before_birnbaum: x.map(|v| v.1),
                after_birnbaum: y.map(|v| v.1),
            })
        })
        .collect();
    importance_rank_changes.sort_by(|p, q| p.id.cmp(&q.id));

    // A single change means exactly one input, modified in exactly one way,
    // with no assumption or method difference. An input whose value AND
    // uncertainty both moved is two simultaneous modifications, so it does not
    // qualify: attributing the outcome to "the value change" would be picking
    // one of two explanations without evidence.
    let single_change = input_changes.len() == 1
        && input_changes[0].change == "changed"
        && assumption_changes.is_empty()
        && method_changes.is_empty();

    let causal_attribution = if single_change {
        format!(
            "exactly one input changed ('{}') with no assumption or method change, so the \
             top-event difference is attributable to it under this model; whether the \
             real system changed the same way is an engineering question this tool does \
             not answer",
            input_changes[0].id
        )
    } else if input_changes.len() == 1 && input_changes[0].change.contains("and") {
        format!(
            "input '{}' changed in two ways at once (its value and its declared \
             uncertainty); the top-event difference is NOT attributed to either one \
             alone",
            input_changes[0].id
        )
    } else if input_changes.is_empty()
        && assumption_changes.is_empty()
        && method_changes.is_empty()
    {
        "no inputs, assumptions or methods differ between the two analyses".to_string()
    } else {
        format!(
            "{} input change(s), {} assumption change(s) and {} method change(s) occur \
             together; the top-event difference is NOT attributed to any one of them",
            input_changes.len(),
            assumption_changes.len(),
            method_changes.len()
        )
    };

    AnalysisComparison {
        schema: COMPARISON_SCHEMA.to_string(),
        before_analysis_id: before.analysis_id.clone(),
        after_analysis_id: after.analysis_id.clone(),
        top_event: after.top_event.clone(),
        before_probability: before.point_estimate,
        after_probability: after.point_estimate,
        absolute_change: after.point_estimate - before.point_estimate,
        relative_change: if before.point_estimate.abs() > 0.0 {
            Some((after.point_estimate - before.point_estimate) / before.point_estimate)
        } else {
            None
        },
        before_interval: before.uncertainty.as_ref().map(|u| (u.lower, u.upper)),
        after_interval: after.uncertainty.as_ref().map(|u| (u.lower, u.upper)),
        input_changes,
        assumption_changes,
        method_changes,
        importance_rank_changes,
        single_change,
        causal_attribution,
        diagnostics,
    }
}

impl AnalysisComparison {
    /// Human-readable comparison report.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Reliability Analysis Comparison\n");
        out.push_str("===============================\n");
        out.push_str(&format!("Top Event:   {}\n", self.top_event));
        out.push_str(&format!(
            "Before:      {} -> {:.6e}\n",
            self.before_analysis_id, self.before_probability
        ));
        out.push_str(&format!(
            "After:       {} -> {:.6e}\n",
            self.after_analysis_id, self.after_probability
        ));
        out.push_str(&format!("Change:      {:+.6e}", self.absolute_change));
        if let Some(r) = self.relative_change {
            out.push_str(&format!(" ({:+.2}%)", r * 100.0));
        }
        out.push('\n');
        if let (Some(b), Some(a)) = (self.before_interval, self.after_interval) {
            out.push_str(&format!(
                "Interval:    [{:.6e}, {:.6e}] -> [{:.6e}, {:.6e}]\n",
                b.0, b.1, a.0, a.1
            ));
        }
        if !self.input_changes.is_empty() {
            out.push_str("\nInput changes:\n");
            for c in &self.input_changes {
                out.push_str(&format!(
                    "  {} [{}] {:?} -> {:?} ({})\n",
                    c.id, c.kind, c.before, c.after, c.change
                ));
            }
        }
        if !self.assumption_changes.is_empty() {
            out.push_str("\nAssumption changes:\n");
            for c in &self.assumption_changes {
                out.push_str(&format!("  {c}\n"));
            }
        }
        if !self.method_changes.is_empty() {
            out.push_str("\nMethod changes:\n");
            for c in &self.method_changes {
                out.push_str(&format!("  {c}\n"));
            }
        }
        if !self.importance_rank_changes.is_empty() {
            out.push_str("\nImportance rank changes:\n");
            for c in &self.importance_rank_changes {
                out.push_str(&format!(
                    "  {}: rank {:?} -> {:?}\n",
                    c.id, c.before_rank, c.after_rank
                ));
            }
        }
        out.push_str("\nAttribution:\n");
        out.push_str(&format!("  {}\n", self.causal_attribution));
        out
    }
}
