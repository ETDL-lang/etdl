//! Fault-tree evaluation with and without dependency awareness.
//!
//! The independence evaluator mirrors the ETDL core gate mathematics exactly
//! (AND/OR/XOR/NOT/VOTING/INHIBIT/PRIORITY_AND). It is used only when the
//! model assumes independence (or no dependency model is present), preserving
//! backward-compatible results.
//!
//! The dependency-aware evaluator conditions on the declared common causes:
//! for each joint state of the common-cause atoms, the affected leaves are set
//! conditionally and the tree is evaluated under the now-conditional
//! independence of the residual events. This is exact for the declared
//! "independent OR common cause" structure and correctly avoids double-counting.

use std::collections::{BTreeMap, HashMap};

use super::diagnostics::{code, AnalysisDiagnostic};
use super::model::{DependencyModel, FaultTreeSpec, GateKind, IndependenceAssumption, ModelError};

/// Evaluate a fault tree treating all leaves as independent (the classic
/// result). Mirrors the ETDL compiler's gate mathematics.
pub fn evaluate_independent(
    tree: &FaultTreeSpec,
    leaf_overrides: &BTreeMap<String, f64>,
) -> Result<f64, ModelError> {
    // Validate leaves first (never silently clamp).
    for (id, p) in &tree.leaves {
        if !p.is_finite() || !(0.0..=1.0).contains(p) {
            return Err(ModelError::LeafOutOfRange(id.clone(), *p));
        }
    }
    let mut probs: HashMap<String, f64> = HashMap::new();
    for (id, p) in &tree.leaves {
        let v = leaf_overrides.get(id).copied().unwrap_or(*p);
        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
            return Err(ModelError::ProbabilityOutOfRange(v));
        }
        probs.insert(id.clone(), v);
    }

    if tree.gates.is_empty() {
        return probs
            .get(&tree.top_event)
            .copied()
            .ok_or_else(|| ModelError::UnresolvableTopEvent(tree.top_event.clone()));
    }

    let order = topological_gate_order(tree)?;
    for gate_id in &order {
        let gate = tree.gates.get(gate_id).ok_or_else(|| {
            ModelError::UnknownNode(format!("gate '{gate_id}' missing during resolution"))
        })?;
        let mut inputs = Vec::with_capacity(gate.inputs.len());
        for input in &gate.inputs {
            let v = probs
                .get(input)
                .copied()
                .ok_or_else(|| ModelError::UnknownGateInput(gate_id.clone(), input.clone()))?;
            inputs.push(v);
        }
        let prob = gate_probability(gate.kind, &inputs, gate.k)?;
        probs.insert(gate_id.clone(), prob);
    }

    probs
        .get(&tree.top_event)
        .copied()
        .ok_or_else(|| ModelError::UnresolvableTopEvent(tree.top_event.clone()))
}

/// Compute the probability of a gate from its input probabilities.
pub fn gate_probability(kind: GateKind, inputs: &[f64], k: Option<u32>) -> Result<f64, ModelError> {
    let valid = |v: f64| -> Result<f64, ModelError> {
        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
            Err(ModelError::ProbabilityOutOfRange(v))
        } else {
            Ok(v)
        }
    };
    match kind {
        GateKind::And => inputs
            .iter()
            .copied()
            .try_fold(1.0, |acc, p| Ok(acc * valid(p)?)),
        GateKind::Or => {
            // Computed in log space: P = -expm1(sum ln(1 - p_i)).
            //
            // The naive form 1 - prod(1 - p_i) loses almost all significant
            // digits for rare events, because 1 - p rounds to 1.0 - eps and the
            // final subtraction cancels. ln1p/expm1 keep full relative
            // precision down to the smallest normal f64, which matters because
            // reliability models routinely carry probabilities at 1e-9 and
            // below. For ordinary probabilities the two forms agree to within
            // one ulp.
            let mut log_survival = 0.0f64;
            for p in inputs {
                let v = valid(*p)?;
                log_survival += (-v).ln_1p();
            }
            Ok((-log_survival.exp_m1()).clamp(0.0, 1.0))
        }
        GateKind::Not => {
            if inputs.len() != 1 {
                return Err(ModelError::Unsupported(
                    "NOT gate requires exactly 1 input".into(),
                ));
            }
            Ok(1.0 - valid(inputs[0])?)
        }
        GateKind::Xor => {
            if inputs.len() != 2 {
                return Err(ModelError::Unsupported(
                    "XOR gate requires exactly 2 inputs".into(),
                ));
            }
            let a = valid(inputs[0])?;
            let b = valid(inputs[1])?;
            Ok(a + b - 2.0 * a * b)
        }
        GateKind::Voting => {
            let k_val =
                k.ok_or_else(|| ModelError::Unsupported("VOTING gate requires k".into()))? as usize;
            let n = inputs.len();
            if k_val < 1 || k_val > n {
                return Err(ModelError::Unsupported(format!(
                    "VOTING gate: k={k_val} out of range [1, {n}]"
                )));
            }
            let ps: Vec<f64> = inputs.iter().map(|p| valid(*p)).collect::<Result<_, _>>()?;
            if ps.iter().all(|&p| (p - ps[0]).abs() < 1e-10) {
                let p = ps[0];
                let mut total = 0.0;
                for j in k_val..=n {
                    total += binomial(n, j) * p.powi(j as i32) * (1.0 - p).powi((n - j) as i32);
                }
                Ok(total.clamp(0.0, 1.0))
            } else {
                let mut poly = vec![1.0];
                for &p in &ps {
                    poly = multiply_poly(&poly, &[1.0 - p, p]);
                }
                let total: f64 = (k_val..=n).map(|j| poly[j]).sum();
                Ok(total.clamp(0.0, 1.0))
            }
        }
        GateKind::Inhibit => {
            if inputs.len() != 2 {
                return Err(ModelError::Unsupported(
                    "INHIBIT gate requires exactly 2 inputs".into(),
                ));
            }
            Ok(valid(inputs[0])? * valid(inputs[1])?)
        }
        GateKind::PriorityAnd => {
            let n = inputs.len();
            if n < 2 {
                return Err(ModelError::Unsupported(
                    "PRIORITY_AND gate requires at least 2 inputs".into(),
                ));
            }
            let mut log_p = 0.0;
            for p in inputs {
                let v = valid(*p)?;
                if v <= 0.0 {
                    return Ok(0.0);
                }
                log_p += v.ln();
            }
            log_p -= ln_factorial(n);
            Ok(log_p.exp().clamp(0.0, 1.0))
        }
    }
}

/// Topological order of gates (deterministic), detecting cycles.
fn topological_gate_order(tree: &FaultTreeSpec) -> Result<Vec<String>, ModelError> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for gate_id in tree.gates.keys() {
        in_degree.entry(gate_id.as_str()).or_insert(0);
        adj.entry(gate_id.as_str()).or_default();
    }
    for (gate_id, gate) in &tree.gates {
        for input in &gate.inputs {
            if tree.gates.contains_key(input.as_str()) {
                adj.entry(input.as_str())
                    .or_default()
                    .push(gate_id.as_str());
                *in_degree.entry(gate_id.as_str()).or_insert(0) += 1;
            }
        }
    }
    let mut queue: std::collections::VecDeque<&str> = std::collections::VecDeque::new();
    for gate_id in tree.gates.keys() {
        if in_degree.get(gate_id.as_str()).copied().unwrap_or(0) == 0 {
            queue.push_back(gate_id.as_str());
        }
    }
    let mut order = Vec::new();
    while let Some(id) = queue.pop_front() {
        order.push(id.to_string());
        if let Some(children) = adj.get(id) {
            for &child in children {
                if let Some(deg) = in_degree.get_mut(child) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }
    }
    if order.len() != tree.gates.len() {
        return Err(ModelError::GateCycle(tree.top_event.clone()));
    }
    if !order.contains(&tree.top_event) {
        order.push(tree.top_event.clone());
    }
    Ok(order)
}

fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let ln = ln_factorial(n) - ln_factorial(k) - ln_factorial(n - k);
    ln.exp().round()
}

fn ln_factorial(n: usize) -> f64 {
    if n <= 170 {
        (2..=n).fold(1.0f64, |acc, i| acc * i as f64).ln()
    } else {
        ln_gamma((n as f64) + 1.0)
    }
}

fn ln_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const P: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.5203681218851,
        -1259.1392167224028,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507343278686905,
        -0.13857109526572012,
        9.984_369_578_019_572e-6,
        1.5056327351493116e-7,
    ];
    if x < 0.5 {
        return std::f64::consts::PI.ln()
            - (std::f64::consts::PI * x).sin().ln()
            - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = P[0];
    let t = x + G + 0.5;
    for (i, coeff) in P.iter().enumerate().skip(1) {
        a += coeff / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

fn multiply_poly(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut result = vec![0.0; a.len() + b.len() - 1];
    for (i, &ca) in a.iter().enumerate() {
        for (j, &cb) in b.iter().enumerate() {
            result[i + j] += ca * cb;
        }
    }
    result
}

/// The dependency-aware evaluator.
///
/// If the model declares common causes (or a non-independent assumption), the
/// top-event probability is computed by conditioning on the joint state of all
/// common-cause atoms:
///
/// ```text
/// P(top) = Σ_state  P(state) · P(top | state)
/// ```
///
/// where `P(top | state)` evaluates the tree with each affected leaf set to its
/// conditional probability given that state of the common causes. This is exact
/// for the "independent residual OR common cause" model and never double-counts
/// the common-cause probability.
#[derive(Debug, Clone)]
pub struct DependencyEvaluator<'a> {
    tree: &'a FaultTreeSpec,
    model: &'a DependencyModel,
}

impl<'a> DependencyEvaluator<'a> {
    pub fn new(tree: &'a FaultTreeSpec, model: &'a DependencyModel) -> Self {
        DependencyEvaluator { tree, model }
    }

    /// Whether dependencies are present that affect the computation.
    pub fn has_dependencies(&self) -> bool {
        !self.model.common_causes.is_empty()
            || self.model.independence == IndependenceAssumption::NotAssumed
    }

    /// Diagnostics for every declared dependency the selected evaluation
    /// method cannot represent.
    ///
    /// The evaluator conditions exactly on declared common causes. It has no
    /// representation for a declared conditional probability, a `depends-on`
    /// edge, or a `conditional-on` edge. Rather than evaluate such a model as
    /// if those declarations were absent - which would silently return an
    /// independence answer for a model that explicitly says events are not
    /// independent - each is reported as an unsupported-analysis diagnostic.
    pub fn unsupported(&self) -> Vec<AnalysisDiagnostic> {
        let mut out = Vec::new();
        for c in &self.model.conditional {
            out.push(
                AnalysisDiagnostic::error(
                    code::UNSUPPORTED_DEPENDENCY,
                    format!(
                        "conditional probability P({} | {}) = {} is declared but the \
                         conditioning evaluator cannot represent it; the analysis is \
                         refused rather than evaluated under independence",
                        c.event, c.given, c.probability
                    ),
                )
                .about(&c.event),
            );
        }
        let cc_ids: std::collections::BTreeSet<&str> = self
            .model
            .common_causes
            .iter()
            .map(|cc| cc.id.as_str())
            .collect();
        for e in &self.model.edges {
            match e.kind {
                super::model::DependencyKind::DependsOn => out.push(
                    AnalysisDiagnostic::error(
                        code::UNSUPPORTED_DEPENDENCY,
                        format!(
                            "dependency edge '{}' -> '{}' (depends-on) is declared but the \
                             conditioning evaluator has no representation for a directed \
                             functional dependency; declare a common cause instead, or \
                             analyse this model externally",
                            e.from, e.to
                        ),
                    )
                    .about(format!("{}->{}", e.from, e.to)),
                ),
                super::model::DependencyKind::ConditionalOn => out.push(
                    AnalysisDiagnostic::error(
                        code::UNSUPPORTED_DEPENDENCY,
                        format!(
                            "dependency edge '{}' -> '{}' (conditional-on) is declared but \
                             conditional structures are not supported by this evaluator",
                            e.from, e.to
                        ),
                    )
                    .about(format!("{}->{}", e.from, e.to)),
                ),
                super::model::DependencyKind::CommonCause => {
                    if !cc_ids.contains(e.from.as_str()) {
                        out.push(
                            AnalysisDiagnostic::error(
                                code::UNSUPPORTED_DEPENDENCY,
                                format!(
                                    "common-cause edge '{}' -> '{}' names '{}' as the cause, \
                                     but no common cause with that id is declared, so it \
                                     carries no probability and cannot be conditioned on",
                                    e.from, e.to, e.from
                                ),
                            )
                            .about(format!("{}->{}", e.from, e.to)),
                        );
                    }
                }
            }
        }
        out
    }

    /// Fail if any declared dependency is unsupported.
    ///
    /// This is called before every evaluation. A model that declares a
    /// dependency this evaluator cannot represent produces an error, never a
    /// number.
    pub fn check_supported(&self) -> Result<(), ModelError> {
        let diags = self.unsupported();
        if diags.is_empty() {
            return Ok(());
        }
        let joined = diags
            .iter()
            .map(|d| format!("[{}] {}", d.code, d.message))
            .collect::<Vec<_>>()
            .join("; ");
        Err(ModelError::Unsupported(joined))
    }

    /// Compute the dependency-aware top-event probability.
    ///
    /// With no common causes, this is exactly the independence result (even if
    /// independence is declared `NotAssumed`, no common causes means the tree
    /// is evaluated under the independence of the remaining atoms, and the
    /// assumption is recorded — never silently swapped for an independence
    /// answer when dependencies exist).
    pub fn point_estimate(&self) -> Result<f64, ModelError> {
        self.check_supported()?;
        if self.model.common_causes.is_empty() {
            return evaluate_independent(self.tree, &BTreeMap::new());
        }
        self.conditioned_top(&BTreeMap::new())
    }

    /// Point estimate with a set of leaf values forced (used by importance and
    /// sensitivity: set a leaf to 1 or 0).
    pub fn point_estimate_with(&self, forced: &BTreeMap<String, f64>) -> Result<f64, ModelError> {
        self.check_supported()?;
        if self.model.common_causes.is_empty() {
            return evaluate_independent(self.tree, forced);
        }
        self.conditioned_top(forced)
    }

    /// `P(top | common cause `index` occurred / did not occur)`.
    ///
    /// This is the correct way to ask what a common cause is worth. Forcing
    /// the leaves it affects to 0 would not answer it: the conditioning loop
    /// would still switch them on in the states where the cause fires, and the
    /// residual independent failure paths would be discarded along with the
    /// shared one.
    ///
    /// The conditional is computed by restricting the state sum to the states
    /// consistent with the requested common-cause state and renormalising by
    /// their total probability:
    ///
    /// ```text
    /// P(top | C = c) = sum_{s : s_C = c} P(s) P(top | s)  /  sum_{s : s_C = c} P(s)
    /// ```
    ///
    /// so `P(top | C absent)` keeps each affected leaf at its **residual**
    /// independent probability `(q - p_C) / (1 - p_C)`, not at its full `q`.
    pub fn point_estimate_given_common_cause(
        &self,
        index: usize,
        occurred: bool,
    ) -> Result<f64, ModelError> {
        self.check_supported()?;
        if index >= self.model.common_causes.len() {
            return Err(ModelError::UnknownNode(format!(
                "common cause index {index} is out of range"
            )));
        }
        let mut fixed = BTreeMap::new();
        fixed.insert(index, occurred);
        self.conditioned_top_fixed(&BTreeMap::new(), &fixed)
    }

    /// Conditioned evaluation over all joint states of the common causes.
    fn conditioned_top(&self, forced: &BTreeMap<String, f64>) -> Result<f64, ModelError> {
        self.conditioned_top_fixed(forced, &BTreeMap::new())
    }

    /// Conditioned evaluation, optionally restricted to states in which the
    /// given common causes take a fixed value. With `fixed` empty this is the
    /// unconditional total probability.
    fn conditioned_top_fixed(
        &self,
        forced: &BTreeMap<String, f64>,
        fixed: &BTreeMap<usize, bool>,
    ) -> Result<f64, ModelError> {
        let ccs: Vec<&super::model::CommonCause> = self.model.common_causes.iter().collect();
        let k = ccs.len();
        if k > 20 {
            return Err(ModelError::Unsupported(format!(
                "conditioning over {k} common causes is unsupported (max 20)"
            )));
        }
        let n_states = 1usize << k;
        // Precompute, for each affected leaf, the list of common-cause indices.
        let mut affected: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, cc) in ccs.iter().enumerate() {
            for ev in &cc.affects {
                affected.entry(ev.clone()).or_default().push(i);
            }
        }

        let mut total = 0.0;
        let mut mass = 0.0;
        for state in 0..n_states {
            // Skip states inconsistent with the requested conditioning.
            if fixed
                .iter()
                .any(|(&i, &want)| (((state >> i) & 1) == 1) != want)
            {
                continue;
            }
            // P(state) = ∏ (cc on ? p : 1-p)
            let mut state_prob = 1.0;
            for (i, cc) in ccs.iter().enumerate() {
                let on = (state >> i) & 1 == 1;
                state_prob *= if on {
                    cc.probability
                } else {
                    1.0 - cc.probability
                };
            }
            if state_prob <= 0.0 {
                continue;
            }

            // Build leaf overrides for this state.
            let mut overrides: BTreeMap<String, f64> = forced.clone();
            for (leaf, idxs) in &affected {
                let any_on = idxs.iter().any(|&i| (state >> i) & 1 == 1);
                let base = if let Some(f) = forced.get(leaf) {
                    *f
                } else {
                    self.tree.leaves.get(leaf).copied().unwrap_or(0.0)
                };
                if any_on {
                    // The common cause is present: the leaf fails.
                    overrides.insert(leaf.clone(), 1.0);
                } else {
                    // No common cause: the leaf fails only via its independent
                    // component. P(leaf) = P(ind) + P(cc) - P(ind)P(cc).
                    // P(leaf | no cc) = (P(leaf) - P(cc)) / (1 - P(cc)).
                    let cc_union = union_probability(idxs, ccs.as_slice());
                    if cc_union >= 1.0 {
                        overrides.insert(leaf.clone(), 0.0);
                    } else {
                        let cond = (base - cc_union) / (1.0 - cc_union);
                        overrides.insert(leaf.clone(), cond.clamp(0.0, 1.0));
                    }
                }
            }
            let top_given_state = evaluate_independent(self.tree, &overrides)?;
            total += state_prob * top_given_state;
            mass += state_prob;
        }
        if fixed.is_empty() {
            return Ok(total.clamp(0.0, 1.0));
        }
        if mass <= 0.0 {
            // The conditioning event has probability zero, so the conditional
            // is undefined. Refuse rather than divide by zero.
            return Err(ModelError::Unsupported(
                "cannot condition on a common-cause state with zero probability".into(),
            ));
        }
        Ok((total / mass).clamp(0.0, 1.0))
    }
}

/// Probability that at least one of the given common causes occurred, treating
/// the common causes as independent: `1 - ∏(1 - p_i)`.
fn union_probability(idxs: &[usize], ccs: &[&super::model::CommonCause]) -> f64 {
    let mut prod = 1.0;
    for &i in idxs {
        prod *= 1.0 - ccs[i].probability;
    }
    1.0 - prod
}

/// Whether the tree is **coherent**: every gate is monotone in its inputs.
///
/// NOT and XOR gates make the tree non-coherent, because increasing a basic
/// event's probability can *decrease* the top-event probability. Several
/// importance measures (notably Fussell-Vesely) are defined only for coherent
/// trees; for a non-coherent tree they are not computed and a diagnostic is
/// emitted instead of a misleading number.
pub fn is_coherent(tree: &FaultTreeSpec) -> bool {
    !tree
        .gates
        .values()
        .any(|g| matches!(g.kind, GateKind::Not | GateKind::Xor))
}

/// The method identifier recorded on importance and sensitivity results.
pub const IMPORTANCE_METHOD: &str = "exact-conditioning";
/// Version of the importance computation. Bump on any definitional change.
pub const IMPORTANCE_METHOD_VERSION: &str = "1";
/// The sensitivity method identifier.
pub const SENSITIVITY_METHOD: &str = "finite-perturbation/absolute/two-sided";
/// Version of the sensitivity computation.
pub const SENSITIVITY_METHOD_VERSION: &str = "1";

/// Values below which a denominator is treated as zero for normalised measures.
pub const RELATIVE_EPSILON: f64 = 1e-15;

/// Importance analysis: how much each model entity contributes to the top
/// event, under the model as declared.
///
/// Every measure is computed by **exact conditioning** with the
/// dependency-aware evaluator - the leaf (or, for a common cause, every
/// affected leaf) is forced to 1 or to 0 and the tree is re-evaluated. No
/// finite-difference approximation is used, so the results are exact for the
/// declared model rather than accurate to first order.
///
/// Let `P` be the baseline top-event probability, `q_i` the entity's own
/// probability, and
///
/// ```text
/// P(1_i) = P(top | entity occurs)
/// P(0_i) = P(top | entity does not occur)
/// ```
///
/// then:
///
/// | Measure | Definition |
/// |---|---|
/// | Birnbaum | `I_B(i) = P(1_i) - P(0_i)` |
/// | Fussell-Vesely | `I_FV(i) = (P - P(0_i)) / P` (coherent trees only) |
/// | Criticality | `I_CR(i) = I_B(i) * q_i / P` |
/// | Risk Achievement Worth | `RAW(i) = P(1_i) / P` |
/// | Risk Reduction Worth | `RRW(i) = P / P(0_i)` |
///
/// For a coherent tree with independent basic events, `I_B` coincides with the
/// partial derivative `dP/dq_i`, because `P` is multilinear in the `q_i` -
/// this is why the conditioning form is the *correct* formal definition rather
/// than an approximation of one.
///
/// Importance is a **structural** measure: it answers "how much does the top
/// event depend on this entity?", not "how likely is this entity to fail" and
/// not "what fraction of the probability does it contribute". Those are
/// different questions with different answers.
pub fn importance(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
) -> Result<Vec<super::result::ImportanceEntry>, ModelError> {
    importance_result(tree, model).map(|r| r.entries)
}

/// As [`importance`], but returning the full result with method identity,
/// measure definitions, assumptions, and diagnostics.
pub fn importance_result(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
) -> Result<super::result::ImportanceResult, ModelError> {
    use super::result::{ImportanceEntry, ImportanceResult};

    let eval = DependencyEvaluator::new(tree, model);
    eval.check_supported()?;
    let top = eval.point_estimate()?;
    let coherent = is_coherent(tree);

    let mut diagnostics = super::diagnostics::Diagnostics::new();
    if !coherent {
        diagnostics.push(AnalysisDiagnostic::warning(
            code::NON_COHERENT_TREE,
            "the fault tree contains a NOT or XOR gate, so it is not coherent; \
             Fussell-Vesely and criticality importance are undefined for it and were \
             not computed. Birnbaum importance remains well defined.",
        ));
    }
    if top <= 0.0 {
        diagnostics.push(AnalysisDiagnostic::warning(
            code::ZERO_TOP_EVENT,
            "the top-event probability is zero, so every measure normalised by it \
             (Fussell-Vesely, criticality, RAW, RRW) is undefined and was not computed; \
             Birnbaum importance is still reported",
        ));
    }

    let normalise = top > RELATIVE_EPSILON;
    let mut entries = Vec::new();

    let mut push_entry = |id: String,
                          is_cc: bool,
                          own_probability: Option<f64>,
                          p_on: f64,
                          p_off: f64| {
        let birnbaum = p_on - p_off;
        let fussell_vesely = if coherent && normalise {
            Some(((top - p_off) / top).clamp(0.0, 1.0))
        } else {
            None
        };
        let criticality = match (coherent && normalise, own_probability) {
            (true, Some(q)) => Some(birnbaum * q / top),
            _ => None,
        };
        entries.push(ImportanceEntry {
            id,
            is_common_cause: is_cc,
            entity_kind: if is_cc { "common-cause" } else { "basic-event" }.to_string(),
            event_probability: own_probability,
            top_probability: top,
            top_given_occurred: p_on,
            top_given_absent: p_off,
            birnbaum,
            fussell_vesely,
            criticality,
            raw: if normalise { p_on / top } else { f64::NAN },
            rrw: if p_off > RELATIVE_EPSILON {
                top / p_off
            } else {
                f64::INFINITY
            },
        });
    };

    for (leaf, q) in &tree.leaves {
        let mut on = BTreeMap::new();
        on.insert(leaf.clone(), 1.0);
        let p_on = eval.point_estimate_with(&on)?;
        let mut off = BTreeMap::new();
        off.insert(leaf.clone(), 0.0);
        let p_off = eval.point_estimate_with(&off)?;
        push_entry(leaf.clone(), false, Some(*q), p_on, p_off);
    }

    // Common causes are first-class model entities and are analysed as such:
    // the result identifies the common cause itself, not the leaves it happens
    // to affect. A shared cause with a small individual probability can easily
    // outrank a larger independent failure.
    for (i, cc) in model.common_causes.iter().enumerate() {
        // Condition on the cause itself, not on the leaves it affects. In the
        // "absent" case the affected leaves keep their residual independent
        // probability, so the measure isolates the shared failure path.
        let p_on = if cc.probability > 0.0 {
            eval.point_estimate_given_common_cause(i, true)?
        } else {
            // A zero-probability cause cannot be conditioned on; the
            // structural answer is the tree with its affected leaves failed.
            let mut on = BTreeMap::new();
            for ev in &cc.affects {
                on.insert(ev.clone(), 1.0);
            }
            eval.point_estimate_with(&on)?
        };
        let p_off = if cc.probability < 1.0 {
            eval.point_estimate_given_common_cause(i, false)?
        } else {
            let mut off = BTreeMap::new();
            for ev in &cc.affects {
                off.insert(ev.clone(), 0.0);
            }
            eval.point_estimate_with(&off)?
        };
        push_entry(cc.id.clone(), true, Some(cc.probability), p_on, p_off);
    }

    // Deterministic ordering: Birnbaum descending, then id ascending so that
    // ties never depend on iteration order.
    entries.sort_by(|a, b| {
        b.birnbaum
            .partial_cmp(&a.birnbaum)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut assumptions = vec![
        "importance is computed by exact conditioning on the declared model, not by \
         finite differences"
            .to_string(),
        "importance measures structural dependence of the top event on an entity; it is \
         not a probability and not a probability contribution"
            .to_string(),
    ];
    if !model.common_causes.is_empty() {
        assumptions.push(
            "a common cause is conditioned as a single entity: all events it affects are \
             forced together"
                .to_string(),
        );
    }

    Ok(ImportanceResult {
        schema: super::result::IMPORTANCE_SCHEMA.to_string(),
        top_event: tree.top_event.clone(),
        baseline_top_probability: top,
        coherent,
        method: IMPORTANCE_METHOD.to_string(),
        method_version: IMPORTANCE_METHOD_VERSION.to_string(),
        measures: super::result::implemented_importance_measures(coherent),
        entries,
        assumptions,
        diagnostics: diagnostics.sorted().items,
    })
}

/// Sensitivity analysis: how much the top-event probability moves when one
/// input probability is perturbed.
///
/// The method is a **two-sided absolute finite perturbation**. For an input
/// with baseline `q`, the analysis evaluates
///
/// ```text
/// up:   q+ = min(1, q + delta)   ->  dP+ = P(top; q+) - P(top; q)
/// down: q- = max(0, q - delta)   ->  dP- = P(top; q-) - P(top; q)
/// ```
///
/// Both directions are evaluated because the relationship is not symmetric in
/// general: voting gates, nested structures and common-cause conditioning all
/// produce responses where a decrease does not simply mirror an increase.
/// Whether the two legs did in fact mirror each other is reported as
/// `two_sided_symmetric` rather than assumed.
///
/// The normalised (relative) sensitivity, or elasticity, is
///
/// ```text
/// S_rel(i) = (dP_top / P_top) / (dq_i / q_i)
/// ```
///
/// It is reported only when both denominators exceed
/// [`RELATIVE_EPSILON`]; otherwise it is `None` and a `NEAR_ZERO_BASELINE`
/// diagnostic is emitted. It is never approximated by substituting a small
/// constant for a zero denominator.
///
/// Sensitivity is a *parameter* question ("if this number were different, how
/// different would the answer be?"). Importance is a *structural* question.
/// The two are reported separately and are never derived from one another.
pub fn sensitivity(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
    delta: f64,
) -> Result<Vec<super::result::SensitivityEntry>, ModelError> {
    sensitivity_result(tree, model, delta).map(|r| r.entries)
}

/// As [`sensitivity`], but returning the full result with method identity,
/// perturbation size, assumptions, and diagnostics.
pub fn sensitivity_result(
    tree: &FaultTreeSpec,
    model: &DependencyModel,
    delta: f64,
) -> Result<super::result::SensitivityResult, ModelError> {
    use super::result::{PerturbationOutcome, SensitivityEntry, SensitivityResult};

    if !delta.is_finite() || delta <= 0.0 {
        return Err(ModelError::Unsupported(format!(
            "sensitivity perturbation must be finite and positive (got {delta})"
        )));
    }

    let eval = DependencyEvaluator::new(tree, model);
    eval.check_supported()?;
    let top_baseline = eval.point_estimate()?;
    let mut diagnostics = super::diagnostics::Diagnostics::new();

    let relative = |d_top: f64, base_q: f64, d_q: f64| -> Option<f64> {
        if top_baseline.abs() <= RELATIVE_EPSILON
            || base_q.abs() <= RELATIVE_EPSILON
            || d_q.abs() <= RELATIVE_EPSILON
        {
            None
        } else {
            Some((d_top / top_baseline) / (d_q / base_q))
        }
    };

    let mut entries: Vec<SensitivityEntry> = Vec::new();

    // ---- basic events -----------------------------------------------------
    for (leaf, base) in &tree.leaves {
        let mut legs: Vec<PerturbationOutcome> = Vec::new();
        for (direction, target) in [
            ("increase", (base + delta).min(1.0)),
            ("decrease", (base - delta).max(0.0)),
        ] {
            let applied = (target - base).abs() > RELATIVE_EPSILON;
            let top_perturbed = if applied {
                let mut forced = BTreeMap::new();
                forced.insert(leaf.clone(), target);
                eval.point_estimate_with(&forced)?
            } else {
                top_baseline
            };
            let d_top = top_perturbed - top_baseline;
            legs.push(PerturbationOutcome {
                direction: direction.to_string(),
                input_baseline: *base,
                input_perturbed: target,
                top_baseline,
                top_perturbed,
                delta: d_top,
                relative_sensitivity: relative(d_top, *base, target - base),
                applied,
            });
        }
        entries.push(build_entry(
            leaf.clone(),
            false,
            "basic-event",
            *base,
            top_baseline,
            delta,
            legs,
        ));
    }

    // ---- common causes ----------------------------------------------------
    // Perturbing a common cause means perturbing the cause's own probability
    // and re-conditioning, not nudging the leaves it affects. A common cause
    // may not exceed the probability of any event it affects, so the upward
    // leg is capped at that bound and reported as not applied when the cap
    // leaves no room to move.
    for (i, cc) in model.common_causes.iter().enumerate() {
        let ceiling = cc
            .affects
            .iter()
            .filter_map(|ev| tree.leaves.get(ev).copied())
            .fold(1.0f64, f64::min);
        let base = cc.probability;
        let mut legs: Vec<PerturbationOutcome> = Vec::new();
        for (direction, raw_target) in [
            ("increase", base + delta),
            ("decrease", base - delta),
        ] {
            let target = raw_target.clamp(0.0, ceiling);
            let applied = (target - base).abs() > RELATIVE_EPSILON;
            let top_perturbed = if applied {
                let mut perturbed = model.clone();
                perturbed.common_causes[i].probability = target;
                DependencyEvaluator::new(tree, &perturbed).point_estimate()?
            } else {
                top_baseline
            };
            if direction == "increase" && !applied && base >= ceiling {
                diagnostics.push(
                    AnalysisDiagnostic::info(
                        code::DEFAULT_APPLIED,
                        format!(
                            "common cause '{}' is already at its structural ceiling {:.6e} \
                             (the smallest probability among the events it affects), so the \
                             upward perturbation was not applied",
                            cc.id, ceiling
                        ),
                    )
                    .about(&cc.id),
                );
            }
            let d_top = top_perturbed - top_baseline;
            legs.push(PerturbationOutcome {
                direction: direction.to_string(),
                input_baseline: base,
                input_perturbed: target,
                top_baseline,
                top_perturbed,
                delta: d_top,
                relative_sensitivity: relative(d_top, base, target - base),
                applied,
            });
        }
        entries.push(build_entry(
            cc.id.clone(),
            true,
            "common-cause",
            base,
            top_baseline,
            delta,
            legs,
        ));
    }

    for e in &entries {
        if e.relative_sensitivity.is_none() {
            diagnostics.push(
                AnalysisDiagnostic::warning(
                    code::NEAR_ZERO_BASELINE,
                    format!(
                        "relative sensitivity is undefined for '{}': the baseline input \
                         probability ({:.6e}) or the baseline top-event probability \
                         ({:.6e}) is at or below {:.0e}. Absolute sensitivity is still \
                         reported.",
                        e.id, e.baseline, top_baseline, RELATIVE_EPSILON
                    ),
                )
                .about(&e.id),
            );
        }
    }

    entries.sort_by(|a, b| {
        b.delta
            .abs()
            .partial_cmp(&a.delta.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok(SensitivityResult {
        schema: super::result::SENSITIVITY_SCHEMA.to_string(),
        top_event: tree.top_event.clone(),
        top_baseline,
        method: SENSITIVITY_METHOD.to_string(),
        method_version: SENSITIVITY_METHOD_VERSION.to_string(),
        perturbation: delta,
        entries,
        assumptions: vec![
            format!(
                "absolute perturbation of +/-{delta} applied to one input at a time; all \
                 other inputs held at their baseline"
            ),
            "perturbed values are clamped to [0,1] (and, for a common cause, to the \
             smallest probability among the events it affects); a leg that could not move \
             is reported as not applied rather than silently omitted"
                .to_string(),
            "relative sensitivity is the elasticity (dP/P)/(dq/q) and is omitted when \
             either denominator is at or below 1e-15"
                .to_string(),
        ],
        diagnostics: diagnostics.sorted().items,
    })
}

fn build_entry(
    id: String,
    is_common_cause: bool,
    entity_kind: &str,
    baseline: f64,
    top_baseline: f64,
    perturbation: f64,
    legs: Vec<super::result::PerturbationOutcome>,
) -> super::result::SensitivityEntry {
    use super::result::SensitivityEntry;
    let increase = legs.iter().find(|l| l.direction == "increase").cloned();
    let decrease = legs.iter().find(|l| l.direction == "decrease").cloned();
    let (alternative, top_alternative, delta, relative_sensitivity) = match &increase {
        Some(l) => (
            l.input_perturbed,
            l.top_perturbed,
            l.delta,
            l.relative_sensitivity,
        ),
        None => (baseline, top_baseline, 0.0, None),
    };
    // Symmetric iff the two legs mirror one another: dP+ ~ -dP-.
    let two_sided_symmetric = match (&increase, &decrease) {
        (Some(u), Some(d)) if u.applied && d.applied => {
            let scale = u.delta.abs().max(d.delta.abs());
            Some(if scale <= RELATIVE_EPSILON {
                true
            } else {
                (u.delta + d.delta).abs() / scale <= 1e-6
            })
        }
        _ => None,
    };
    SensitivityEntry {
        id,
        is_common_cause,
        entity_kind: entity_kind.to_string(),
        baseline,
        alternative,
        top_baseline,
        top_alternative,
        delta,
        method: SENSITIVITY_METHOD.to_string(),
        perturbation,
        increase,
        decrease,
        relative_sensitivity,
        two_sided_symmetric,
        conditions: Vec::new(),
    }
}
