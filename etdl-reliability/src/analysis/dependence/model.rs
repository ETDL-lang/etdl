//! Neutral fault-tree specification and dependency model.
//!
//! `FaultTreeSpec` is a compiler-independent description of a fault tree
//! (leaf probabilities + gates). `DependencyModel` declares dependencies,
//! common causes, and conditional probabilities. Both are designed so the
//! analysis never needs the ETDL parser or compiler.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Gate kinds (mirror the ETDL core set; no dependency on the parser crate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GateKind {
    And,
    Or,
    Not,
    Xor,
    Voting,
    Inhibit,
    PriorityAnd,
}

/// A gate with inputs (referencing leaves or other gate ids) and optional `k`
/// for VOTING.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateSpec {
    pub kind: GateKind,
    pub inputs: Vec<String>,
    pub k: Option<u32>,
}

/// A compiler-independent fault tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaultTreeSpec {
    pub top_event: String,
    /// leaf id -> probability.
    pub leaves: BTreeMap<String, f64>,
    /// gate id -> gate. Empty means the top event is a single leaf.
    pub gates: BTreeMap<String, GateSpec>,
}

impl FaultTreeSpec {
    pub fn new(top_event: impl Into<String>) -> Self {
        FaultTreeSpec {
            top_event: top_event.into(),
            leaves: BTreeMap::new(),
            gates: BTreeMap::new(),
        }
    }
}

/// Whether independence of basic events is assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IndependenceAssumption {
    /// Independence is explicitly assumed (e.g. declared by the engineer).
    Assumed,
    /// Independence is explicitly NOT assumed (dependencies exist).
    NotAssumed,
    /// The model does not state an assumption.
    Unspecified,
}

impl IndependenceAssumption {
    pub fn label(self) -> &'static str {
        match self {
            IndependenceAssumption::Assumed => "independent",
            IndependenceAssumption::NotAssumed => "not-assumed",
            IndependenceAssumption::Unspecified => "unspecified",
        }
    }
}

/// A declared common cause capable of producing multiple failure events.
///
/// `probability` is the probability of the common cause itself (e.g. the
/// shared-network-failure probability). It is traceable to ontology, evidence,
/// source, and engineering assumptions. A common cause is DECLARED here; it is
/// never inferred automatically from correlated observations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommonCause {
    pub id: String,
    /// Canonical ontology id (e.g. `failure.network.unreachable`), when known.
    pub ontology_id: Option<String>,
    /// Probability of the common cause occurring, in [0,1].
    pub probability: f64,
    /// The failure events this common cause produces (must be leaf ids in the
    /// fault tree).
    pub affects: Vec<String>,
    /// Evidence supporting the common cause (observations, discovery output).
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Where this common cause came from (discovery report, engineering, ...).
    #[serde(default)]
    pub source: Option<String>,
    /// Engineering assumptions behind the common cause.
    #[serde(default)]
    pub assumptions: Vec<String>,
}

impl CommonCause {
    pub fn new(id: impl Into<String>, probability: f64, affects: Vec<String>) -> Self {
        CommonCause {
            id: id.into(),
            ontology_id: None,
            probability,
            affects,
            evidence: Vec::new(),
            source: None,
            assumptions: Vec::new(),
        }
    }
}

/// A beta-factor model for a group of identical components.
///
/// λ_total = λ_independent + λ_ccf, and λ_ccf = β · λ_total. The common-cause
/// probability over a mission time is `1 - exp(-β·λ_total·t)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BetaFactor {
    /// β ∈ [0, 1].
    pub beta: f64,
    /// Total failure rate per component per time unit (identical across the
    /// group).
    pub lambda_total: f64,
    /// Mission time over which the common-cause probability is computed.
    pub mission_time: f64,
}

impl BetaFactor {
    pub fn common_cause_probability(&self) -> Result<f64, ModelError> {
        if !(0.0..=1.0).contains(&self.beta) {
            return Err(ModelError::InvalidBeta(self.beta));
        }
        if self.lambda_total < 0.0 || !self.lambda_total.is_finite() {
            return Err(ModelError::InvalidRate(self.lambda_total));
        }
        if self.mission_time < 0.0 || !self.mission_time.is_finite() {
            return Err(ModelError::InvalidMissionTime(self.mission_time));
        }
        // Numerically stable: -expm1(-βλt).
        Ok(-(-self.beta * self.lambda_total * self.mission_time).exp_m1())
    }

    pub fn independent_rate(&self) -> f64 {
        (1.0 - self.beta) * self.lambda_total
    }
}

/// A conditional probability `P(event | given)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalProbability {
    pub event: String,
    /// The conditioning event id (a declared condition or a leaf id).
    pub given: String,
    /// P(event | given), in [0,1].
    pub probability: f64,
}

/// The kind of a directed dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyKind {
    DependsOn,
    CommonCause,
    ConditionalOn,
}

/// A directed dependency edge in the model graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
}

/// The declared dependency model.
///
/// Independence is an explicit, inspectable assumption — never silent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencyModel {
    pub independence: IndependenceAssumption,
    #[serde(default)]
    pub common_causes: Vec<CommonCause>,
    #[serde(default)]
    pub conditional: Vec<ConditionalProbability>,
    #[serde(default)]
    pub edges: Vec<DependencyEdge>,
}

impl Default for DependencyModel {
    fn default() -> Self {
        DependencyModel {
            independence: IndependenceAssumption::Unspecified,
            common_causes: Vec::new(),
            conditional: Vec::new(),
            edges: Vec::new(),
        }
    }
}

impl DependencyModel {
    pub fn independent() -> Self {
        DependencyModel {
            independence: IndependenceAssumption::Assumed,
            ..Default::default()
        }
    }

    /// Validate the model against a fault tree. Checks:
    ///
    /// - all referenced nodes exist;
    /// - common causes have affected events, valid probabilities, and do not
    ///   exceed the affected leaf probabilities;
    /// - conditional probabilities are in [0,1] and reference known nodes;
    /// - no duplicate ids;
    /// - no dependency cycles (common-cause / edge graphs are acyclic);
    /// - no orphan common causes.
    pub fn validate(&self, tree: &FaultTreeSpec) -> Result<(), ModelError> {
        // Known node ids: leaves + gates + top event.
        let mut known: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        known.insert(tree.top_event.clone());
        known.extend(tree.leaves.keys().cloned());
        known.extend(tree.gates.keys().cloned());

        // Duplicate detection among model-declared ids.
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

        for cc in &self.common_causes {
            if !seen.insert(cc.id.clone()) {
                return Err(ModelError::DuplicateId(cc.id.clone()));
            }
            if cc.affects.is_empty() {
                return Err(ModelError::EmptyCommonCause(cc.id.clone()));
            }
            if !cc.probability.is_finite() || !(0.0..=1.0).contains(&cc.probability) {
                return Err(ModelError::CommonCauseProbabilityOutOfRange(
                    cc.id.clone(),
                    cc.probability,
                ));
            }
            for ev in &cc.affects {
                if !known.contains(ev) {
                    return Err(ModelError::UnknownAffectedEvent(cc.id.clone(), ev.clone()));
                }
                if let Some(&p) = tree.leaves.get(ev) {
                    if cc.probability > p + 1e-12 {
                        return Err(ModelError::CommonCauseExceedsEvent(
                            cc.id.clone(),
                            cc.probability,
                            ev.clone(),
                            p,
                        ));
                    }
                }
            }
        }

        for c in &self.conditional {
            if !known.contains(&c.event) {
                return Err(ModelError::ConditionalUnknownEvent(c.event.clone()));
            }
            // The `given` may be a leaf/gate or a declared condition id.
            if !known.contains(&c.given) && !seen.contains(&c.given) {
                return Err(ModelError::ConditionalUnknownGiven(c.given.clone()));
            }
            if !c.probability.is_finite() || !(0.0..=1.0).contains(&c.probability) {
                return Err(ModelError::ConditionalOutOfRange(
                    c.event.clone(),
                    c.given.clone(),
                    c.probability,
                ));
            }
        }

        for e in &self.edges {
            if !seen.insert(format!("{}->{}", e.from, e.to)) {
                return Err(ModelError::DuplicateId(format!("{}->{}", e.from, e.to)));
            }
            if !known.contains(&e.from) && !seen.contains(&e.from) {
                return Err(ModelError::UnknownNode(e.from.clone()));
            }
            if !known.contains(&e.to) && !seen.contains(&e.to) {
                return Err(ModelError::UnknownNode(e.to.clone()));
            }
        }

        // Cycle detection over model-declared edges (from -> to).
        let mut graph: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for e in &self.edges {
            graph.entry(e.from.clone()).or_default().push(e.to.clone());
        }
        // DFS-based cycle detection.
        let mut visiting: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut visited: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let nodes: Vec<String> = graph.keys().cloned().collect();
        for start in &nodes {
            if visited.contains(start) {
                continue;
            }
            if has_cycle(start, &graph, &mut visiting, &mut visited)? {
                return Err(ModelError::CycleDetected(start.clone()));
            }
        }

        // Orphan common cause: the affected events must be reachable in the
        // fault tree (they are validated as known leaves/gates already, so
        // "orphan" here means no affected event is actually a leaf).
        for cc in &self.common_causes {
            let reachable = cc.affects.iter().any(|ev| tree.leaves.contains_key(ev));
            if !reachable {
                return Err(ModelError::OrphanCommonCause(cc.id.clone()));
            }
        }

        Ok(())
    }

    /// The ids of every node referenced by the model (leaves, gates,
    /// common-cause ids, condition ids).
    pub fn referenced_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for cc in &self.common_causes {
            ids.push(cc.id.clone());
            ids.extend(cc.affects.iter().cloned());
        }
        for c in &self.conditional {
            ids.push(c.event.clone());
            ids.push(c.given.clone());
        }
        for e in &self.edges {
            ids.push(e.from.clone());
            ids.push(e.to.clone());
        }
        ids.sort();
        ids.dedup();
        ids
    }
}

/// Detect a cycle reachable from `start` in the given dependency graph.
fn has_cycle(
    start: &str,
    graph: &std::collections::BTreeMap<String, Vec<String>>,
    visiting: &mut std::collections::BTreeSet<String>,
    visited: &mut std::collections::BTreeSet<String>,
) -> Result<bool, ModelError> {
    if visiting.contains(start) {
        return Ok(true);
    }
    if visited.contains(start) {
        return Ok(false);
    }
    visiting.insert(start.to_string());
    if let Some(neighbors) = graph.get(start) {
        for n in neighbors {
            if has_cycle(n, graph, visiting, visited)? {
                return Ok(true);
            }
        }
    }
    visiting.remove(start);
    visited.insert(start.to_string());
    Ok(false)
}

/// Errors produced while validating or evaluating a dependency model.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelError {
    #[error("unknown fault-tree node '{0}' referenced by the model")]
    UnknownNode(String),
    #[error("common cause '{0}' declares no affected events")]
    EmptyCommonCause(String),
    #[error("common cause '{0}' references unknown affected event '{1}'")]
    UnknownAffectedEvent(String, String),
    #[error("common cause '{0}' probability {1} is outside [0,1]")]
    CommonCauseProbabilityOutOfRange(String, f64),
    #[error("common cause '{0}' probability {1} exceeds affected event '{2}' probability {3}")]
    CommonCauseExceedsEvent(String, f64, String, f64),
    #[error("conditional probability for '{0}' given '{1}' is outside [0,1]")]
    ConditionalOutOfRange(String, String, f64),
    #[error("conditional probability event '{0}' is not a known node")]
    ConditionalUnknownEvent(String),
    #[error("conditional probability given '{0}' is not a known node or declared condition")]
    ConditionalUnknownGiven(String),
    #[error("duplicate id '{0}' in the model")]
    DuplicateId(String),
    #[error("beta factor {0} is outside [0,1]")]
    InvalidBeta(f64),
    #[error("failure rate {0} is invalid (must be finite and non-negative)")]
    InvalidRate(f64),
    #[error("mission time {0} is invalid (must be finite and non-negative)")]
    InvalidMissionTime(f64),
    #[error("dependency cycle detected involving '{0}'")]
    CycleDetected(String),
    #[error("orphan common cause '{0}' affects no reachable node")]
    OrphanCommonCause(String),
    #[error("unsupported model: {0}")]
    Unsupported(String),
    #[error(
        "probability {0} is outside [0,1] (NaN/infinite/out-of-range rejected, never clamped)"
    )]
    ProbabilityOutOfRange(f64),
    #[error("leaf '{0}' probability {1} is outside [0,1]")]
    LeafOutOfRange(String, f64),
    #[error("gate '{0}' input '{1}' is neither a leaf nor a gate")]
    UnknownGateInput(String, String),
    #[error("fault tree cycle detected involving gate '{0}'")]
    GateCycle(String),
    #[error("top event '{0}' is not resolvable")]
    UnresolvableTopEvent(String),
}
