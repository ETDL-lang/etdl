//! Live, decentralized, per-node fault-tree probability recomputation
//! ("Live Reliability" — `etdl.live-reliability` supplement, off by
//! default; see `docs/reference/live-reliability.md`).
//!
//! Unlike everything else in this crate, values here are compile-time
//! **authoritative**: once a fault tree opts in, its nodes' current live
//! values — not the declared compile-time constant — are what
//! `codegen/rust.rs` reads for `record_branch`/`record_failure` and for
//! branch selection (`reliability.in_range`). This is a deliberate,
//! explicitly opt-in departure from the "runtime never changes compiled
//! probabilities" discipline documented in
//! `docs/reliability/runtime-feedback-calibration.md` — that discipline
//! still governs the offline `.rprob` artifact / `etdl-reliability::calibrate`
//! workflow untouched; this is a separate, bounded, per-process runtime
//! layer for fault trees that specifically ask for it.
//!
//! # Why not `etdl-tree-core`
//!
//! `etdl-tree-core::Tree` (the Generic Tree Event Supplement's structure)
//! is deliberately **not a DAG** — "every non-root node has exactly one
//! parent... a node referenced as a child by more than one gate is
//! rejected" (its own module doc). Real fault trees allow a basic event to
//! be a common-cause input to more than one gate, so that structure can't
//! represent one in general. This module has its own small DAG
//! (`LiveFaultTree`), not a forced reuse of a structure that doesn't fit.
//! The *math* (Part 1's gate combinators) is still fully shared with
//! `etdl-compiler::fault_tree` — only the structural container differs.
//!
//! # Cross-service propagation
//!
//! No shared memory between services: [`outbound_snapshot`] serializes
//! every node's current value for a fault tree so generated code can
//! attach it to an outgoing message's headers; [`apply_inbound`] reads that
//! same shape back out of a received message and merges it into this
//! service's local view of nodes declared `inbound` (owned upstream, never
//! locally observed). See `docs/reference/live-reliability.md`'s wire
//! shape.

use etdl_probability_core::{distribution::Beta, gate, independent_and_n, independent_or_n, Probability};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

/// The gate types a live fault tree can recombine — mirrors
/// `etdl_parser::ast::GateType` without depending on the parser crate
/// (this is the runtime crate; generated code emits plain values, never
/// AST types).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LiveGateKind {
    And,
    Or,
    Not,
    Xor,
    Voting(usize),
    Inhibit,
    PriorityAnd,
}

/// Where a leaf's live value comes from. `Local` leaves accumulate their
/// own observations (`record_observation`); `Inbound` leaves are owned by
/// an upstream service and only ever change via [`apply_inbound`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LeafSource {
    Local { prior_strength: f64 },
    Inbound,
}

/// An error building or registering a [`LiveFaultTreeBuilder`]. Setup-time
/// only.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LiveError {
    #[error("live fault tree '{0}': cycle among gates")]
    Cycle(String),
    #[error("live fault tree '{0}': gate '{1}' references unknown child '{2}'")]
    UnknownChild(String, String, String),
    #[error("live fault tree '{0}': top event '{1}' is not a declared node")]
    UnknownTopEvent(String, String),
}

struct BetaEstimator {
    alpha: f64,
    beta: f64,
}

impl BetaEstimator {
    fn seed(declared_probability: f64, prior_strength: f64) -> Self {
        let strength = prior_strength.max(1e-6);
        let p = declared_probability.clamp(0.0, 1.0);
        BetaEstimator {
            alpha: (p * strength).max(1e-6),
            beta: ((1.0 - p) * strength).max(1e-6),
        }
    }

    fn observe(&mut self, occurred: bool) {
        if occurred {
            self.alpha += 1.0;
        } else {
            self.beta += 1.0;
        }
    }

    fn mean(&self) -> f64 {
        Beta::new(self.alpha, self.beta)
            .map(|b| b.mean())
            .unwrap_or(self.alpha / (self.alpha + self.beta))
    }
}

enum LeafState {
    Local(BetaEstimator),
    Inbound(Option<f64>),
}

enum NodeState {
    Leaf {
        state: LeafState,
        /// The value this leaf started at — this document's own declared
        /// probability for the basic event, for both `Local` and `Inbound`
        /// (an `Inbound` leaf's *current* value, unlike its baseline, is
        /// still `None` until the first message arrives — see
        /// [`LeafState::Inbound`]). The baseline `reliability.in_range`
        /// compares the current value against.
        baseline: Option<f64>,
    },
    Gate {
        kind: LiveGateKind,
        children: Vec<String>,
        current: Option<f64>,
        baseline: Option<f64>,
    },
}

/// A live, per-process fault tree: structure plus every node's current
/// value. Built once via [`LiveFaultTreeBuilder::register`], then updated
/// incrementally by [`record_observation`]/[`apply_inbound`].
pub struct LiveFaultTree {
    nodes: HashMap<String, NodeState>,
    /// child id -> gate ids that take it as an input, for ancestor walks.
    parents: HashMap<String, Vec<String>>,
    /// Gates only, children-before-parents. Computed once at registration
    /// (the structure never changes at runtime, only values do).
    topo_order: Vec<String>,
    #[allow(dead_code)]
    top_event_id: String,
}

impl LiveFaultTree {
    fn current_value(&self, node_id: &str) -> Option<f64> {
        match self.nodes.get(node_id)? {
            NodeState::Leaf { state, .. } => match state {
                LeafState::Local(est) => Some(est.mean()),
                LeafState::Inbound(v) => *v,
            },
            // A gate always has *a* value immediately after registration
            // (its baseline, computed from every leaf's declared
            // probability, local or inbound) even before any observation
            // or inbound push refreshes `current` — the baseline pass in
            // `register` already ran the same gate math, so falling back
            // to it is exact, not a placeholder.
            NodeState::Gate { current, baseline, .. } => current.or(*baseline),
        }
    }

    fn baseline(&self, node_id: &str) -> Option<f64> {
        match self.nodes.get(node_id)? {
            NodeState::Leaf { baseline, .. } => *baseline,
            NodeState::Gate { baseline, .. } => *baseline,
        }
    }

    fn ancestors_of(&self, node_id: &str) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();
        queue.push_back(node_id);
        while let Some(id) = queue.pop_front() {
            if let Some(parents) = self.parents.get(id) {
                for parent in parents {
                    if seen.insert(parent.clone()) {
                        queue.push_back(parent);
                    }
                }
            }
        }
        seen
    }

    fn recompute_gate(&mut self, gate_id: &str, use_baseline: bool) {
        let Some(NodeState::Gate { kind, children, .. }) = self.nodes.get(gate_id) else {
            return;
        };
        let kind = *kind;
        let children = children.clone();

        let mut values = Vec::with_capacity(children.len());
        for child in &children {
            let v = if use_baseline {
                self.baseline(child)
            } else {
                self.current_value(child)
            };
            match v {
                Some(v) => values.push(v),
                None => return, // a child has no value yet (cold-start inbound leaf) — can't compute
            }
        }

        let Ok(probs): Result<Vec<Probability>, _> =
            values.iter().map(|&p| Probability::new(p)).collect()
        else {
            return;
        };
        let result = match kind {
            LiveGateKind::And => independent_and_n(&probs).map_err(|e| e.to_string()),
            LiveGateKind::Or => independent_or_n(&probs).map_err(|e| e.to_string()),
            LiveGateKind::Not => gate::not(&probs).map_err(|e| e.to_string()),
            LiveGateKind::Xor => gate::xor(&probs).map_err(|e| e.to_string()),
            LiveGateKind::Voting(k) => gate::k_of_n(&probs, k).map_err(|e| e.to_string()),
            LiveGateKind::Inhibit => gate::inhibit(&probs).map_err(|e| e.to_string()),
            LiveGateKind::PriorityAnd => gate::priority_and(&probs).map_err(|e| e.to_string()),
        };
        let Ok(p) = result else { return };

        if let Some(NodeState::Gate { current, baseline, .. }) = self.nodes.get_mut(gate_id) {
            if use_baseline {
                *baseline = Some(p.value());
            } else {
                *current = Some(p.value());
            }
        }
    }

    /// Recomputes every gate ancestor of `leaf_id`, in dependency order, so
    /// each one reads its children's *current* (already-refreshed-this-
    /// pass) values — never stale ones.
    fn propagate_from(&mut self, leaf_id: &str, use_baseline: bool) {
        let ancestors = self.ancestors_of(leaf_id);
        if ancestors.is_empty() {
            return;
        }
        let order = self.topo_order.clone();
        for gate_id in &order {
            if ancestors.contains(gate_id) {
                self.recompute_gate(gate_id, use_baseline);
            }
        }
    }

    fn record_observation(&mut self, leaf_id: &str, occurred: bool) {
        let Some(NodeState::Leaf {
            state: LeafState::Local(est),
            ..
        }) = self.nodes.get_mut(leaf_id)
        else {
            return;
        };
        est.observe(occurred);
        self.propagate_from(leaf_id, false);
    }

    fn apply_inbound_value(&mut self, node_id: &str, value: f64) {
        let Some(NodeState::Leaf {
            state: LeafState::Inbound(slot),
            ..
        }) = self.nodes.get_mut(node_id)
        else {
            return;
        };
        // `baseline` is never touched here — it was already fixed at
        // registration from this leaf's own declared probability (see
        // `LiveFaultTreeBuilder::inbound_leaf`), same as a `local` leaf's.
        // An earlier version of this function let the *first* inbound
        // value double as the baseline, which meant a service's very first
        // contact with an already-drifted upstream silently redefined
        // "normal" to match it — `reliability.in_range` could never see a
        // deviation it hadn't already baked into its own reference point.
        *slot = Some(value);
        self.propagate_from(node_id, false);
    }
}

/// Builds and registers a [`LiveFaultTree`]. Generated code (when a
/// document declares `etdl.live-reliability` for a fault tree) calls this
/// once at startup — see `docs/reference/live-reliability.md`.
pub struct LiveFaultTreeBuilder {
    nodes: HashMap<String, NodeState>,
    child_specs: HashMap<String, Vec<String>>,
    top_event_id: String,
}

impl LiveFaultTreeBuilder {
    pub fn new(top_event_id: impl Into<String>) -> Self {
        LiveFaultTreeBuilder {
            nodes: HashMap::new(),
            child_specs: HashMap::new(),
            top_event_id: top_event_id.into(),
        }
    }

    /// A basic event this service observes locally. `declared_probability`
    /// seeds the live Beta-Binomial estimator's prior;
    /// `prior_strength` is how many pseudo-observations that declared
    /// value is worth before real observations start moving it (see the
    /// supplement schema's documented default).
    pub fn local_leaf(
        mut self,
        id: impl Into<String>,
        declared_probability: f64,
        prior_strength: f64,
    ) -> Self {
        self.nodes.insert(
            id.into(),
            NodeState::Leaf {
                state: LeafState::Local(BetaEstimator::seed(declared_probability, prior_strength)),
                baseline: Some(declared_probability.clamp(0.0, 1.0)),
            },
        );
        self
    }

    /// A basic event owned by an upstream service — never locally
    /// observed, only ever updated via [`apply_inbound`]. `declared_probability`
    /// (this document's own declared value for the basic event, same field
    /// a `local` leaf's declared value comes from) seeds the baseline
    /// immediately, exactly like [`Self::local_leaf`] — **not** the first
    /// inbound value received, which would let an upstream service's
    /// current (possibly already-drifted) value silently redefine what
    /// "normal" means here.
    pub fn inbound_leaf(mut self, id: impl Into<String>, declared_probability: f64) -> Self {
        self.nodes.insert(
            id.into(),
            NodeState::Leaf {
                state: LeafState::Inbound(None),
                baseline: Some(declared_probability.clamp(0.0, 1.0)),
            },
        );
        self
    }

    pub fn gate(mut self, id: impl Into<String>, kind: LiveGateKind, children: Vec<String>) -> Self {
        let id = id.into();
        self.child_specs.insert(id.clone(), children.clone());
        self.nodes.insert(
            id,
            NodeState::Gate {
                kind,
                children,
                current: None,
                baseline: None,
            },
        );
        self
    }

    /// Validates the structure (every gate's children exist, no cycle),
    /// computes each node's baseline (bottom-up, from every leaf's declared
    /// probability — `local` or `inbound`, both seed a baseline at
    /// construction — using the same shared gate math
    /// `etdl-compiler::fault_tree` uses at compile time, run once more
    /// here so this module never needs the compiler's own resolved
    /// value), and inserts the tree into the process-wide registry under
    /// `fault_tree_id`.
    pub fn register(self, fault_tree_id: impl Into<String>) -> Result<(), LiveError> {
        let fault_tree_id = fault_tree_id.into();

        for (gate_id, children) in &self.child_specs {
            for child in children {
                if !self.nodes.contains_key(child) {
                    return Err(LiveError::UnknownChild(
                        fault_tree_id,
                        gate_id.clone(),
                        child.clone(),
                    ));
                }
            }
        }
        if !self.nodes.contains_key(&self.top_event_id) {
            return Err(LiveError::UnknownTopEvent(fault_tree_id, self.top_event_id));
        }

        let topo_order = topological_order(&self.child_specs)
            .map_err(|_| LiveError::Cycle(fault_tree_id.clone()))?;

        let mut parents: HashMap<String, Vec<String>> = HashMap::new();
        for (gate_id, children) in &self.child_specs {
            for child in children {
                parents.entry(child.clone()).or_default().push(gate_id.clone());
            }
        }

        let mut tree = LiveFaultTree {
            nodes: self.nodes,
            parents,
            topo_order,
            top_event_id: self.top_event_id,
        };

        // Compute every baseline bottom-up: for each leaf (local or
        // inbound — both have a declared-probability baseline set at
        // construction, see `local_leaf`/`inbound_leaf`), propagate its
        // baseline up through its ancestors, in topological order overall
        // so grandparents see already-baselined parents.
        let leaf_ids: Vec<String> = tree
            .nodes
            .iter()
            .filter_map(|(id, n)| match n {
                NodeState::Leaf { .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        for leaf_id in &leaf_ids {
            tree.propagate_from(leaf_id, true);
        }

        REGISTRY
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(fault_tree_id, tree);
        Ok(())
    }
}

fn topological_order(child_specs: &HashMap<String, Vec<String>>) -> Result<Vec<String>, ()> {
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    for (gate_id, children) in child_specs {
        let deps = children.iter().filter(|c| child_specs.contains_key(*c)).count();
        in_degree.insert(gate_id.clone(), deps);
        for child in children {
            if child_specs.contains_key(child) {
                dependents.entry(child.clone()).or_default().push(gate_id.clone());
            }
        }
    }
    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut order = Vec::new();
    while let Some(id) = queue.pop_front() {
        order.push(id.clone());
        if let Some(deps) = dependents.get(&id) {
            for parent in deps {
                let d = in_degree.get_mut(parent).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(parent.clone());
                }
            }
        }
    }
    if order.len() != child_specs.len() {
        return Err(());
    }
    Ok(order)
}

static REGISTRY: OnceLock<Mutex<HashMap<String, LiveFaultTree>>> = OnceLock::new();

fn with_tree<R>(fault_tree_id: &str, f: impl FnOnce(&mut LiveFaultTree) -> R) -> Option<R> {
    let registry = REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
    let mut reg = registry.lock().unwrap();
    reg.get_mut(fault_tree_id).map(f)
}

/// Records that `basic_event_id` (within `fault_tree_id`) occurred or not,
/// updating its live estimate and propagating the change up through every
/// gate that depends on it. A silent no-op for an unregistered fault tree
/// or an unknown/non-`local` basic event — never a panic, since a document
/// without `etdl.live-reliability` never registers anything and this
/// function must still be safe to have codegen not call at all.
pub fn record_observation(fault_tree_id: &str, basic_event_id: &str, occurred: bool) {
    with_tree(fault_tree_id, |t| t.record_observation(basic_event_id, occurred));
}

/// The current live value for a basic event, gate, or top event —
/// `None` if the fault tree isn't registered, the node is unknown, or
/// (for an `inbound` leaf, or a gate depending on one) no value has
/// arrived yet.
pub fn current_probability(fault_tree_id: &str, node_id: &str) -> Option<f64> {
    with_tree(fault_tree_id, |t| t.current_value(node_id)).flatten()
}

/// Whether `node_id`'s current live value is within `threshold` of its
/// baseline (the value computed from every leaf's *declared* probability —
/// `local` or `inbound` — fixed once at registration, never redefined by
/// observations or inbound pushes). Mirrors `SlaTracker`'s own "insufficient
/// data => not anomalous" default: `true` when there isn't yet a current
/// value to compare (cold start, or an `inbound` leaf that hasn't received
/// anything yet — its baseline is already known, but it has no *current*
/// value until the first push) — the same fail-open choice
/// `SlaTracker::MIN_OBSERVATIONS` already makes.
pub fn in_range(fault_tree_id: &str, node_id: &str, threshold: f64) -> bool {
    with_tree(fault_tree_id, |t| {
        match (t.current_value(node_id), t.baseline(node_id)) {
            (Some(current), Some(baseline)) => (current - baseline).abs() <= threshold,
            _ => true,
        }
    })
    .unwrap_or(true)
}

/// Every node's current value for `fault_tree_id`, in the wire shape
/// generated code attaches to an outgoing message's headers (see
/// `docs/reference/live-reliability.md`). An unregistered fault tree
/// yields an empty `nodes` object, not an error — a service that sends a
/// message referencing a tree it doesn't (yet) track should not fail to
/// send because of it.
pub fn outbound_snapshot(fault_tree_id: &str) -> serde_json::Value {
    let nodes = with_tree(fault_tree_id, |t| {
        let mut m = serde_json::Map::new();
        for id in t.nodes.keys() {
            if let Some(v) = t.current_value(id) {
                m.insert(id.clone(), serde_json::json!(v));
            }
        }
        m
    })
    .unwrap_or_default();

    serde_json::json!({
        "etdl.live-reliability/1.0": {
            "fault_tree_id": fault_tree_id,
            "nodes": nodes,
            "observed_at": crate::observation::now_rfc3339(),
        }
    })
}

/// Reads the shape [`outbound_snapshot`] produces out of a received
/// message's headers and merges each node's value into this service's
/// local view of it. A safe no-op for a malformed payload, an unknown
/// fault tree, or a node this service hasn't declared `inbound` — a
/// service may receive messages carrying trees or nodes it doesn't track,
/// and tampering with the header can only skew this service's own
/// estimate, never crash it (see `docs/reference/live-reliability.md`'s
/// trust-boundary note).
pub fn apply_inbound(value: &serde_json::Value) {
    let Some(payload) = value.get("etdl.live-reliability/1.0") else {
        return;
    };
    let Some(fault_tree_id) = payload.get("fault_tree_id").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(nodes) = payload.get("nodes").and_then(|v| v.as_object()) else {
        return;
    };
    with_tree(fault_tree_id, |t| {
        for (node_id, v) in nodes {
            if let Some(p) = v.as_f64() {
                t.apply_inbound_value(node_id, p);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test registers under its own fault-tree id — the registry is
    /// process-wide (`static REGISTRY`), shared across every test in this
    /// binary, so distinct ids are what keeps tests independent under the
    /// default parallel test runner (not a locking/serialization scheme).
    fn unique_id(name: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        format!("{name}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    fn build_two_leaf_or(ft_id: &str, a_p: f64, b_p: f64) {
        LiveFaultTreeBuilder::new("Top")
            .local_leaf("A", a_p, 20.0)
            .local_leaf("B", b_p, 20.0)
            .gate("Top", LiveGateKind::Or, vec!["A".to_string(), "B".to_string()])
            .register(ft_id)
            .unwrap();
    }

    #[test]
    fn baseline_matches_hand_computed_or_combination_immediately_after_register() {
        let ft = unique_id("or-baseline");
        build_two_leaf_or(&ft, 0.1, 0.2);

        // independent OR: 0.1 + 0.2 - 0.1*0.2 = 0.28
        let top = current_probability(&ft, "Top").unwrap();
        assert!((top - 0.28).abs() < 1e-9, "got {top}");
        // Leaves start at exactly their declared probability (Beta mean of
        // a freshly-seeded prior).
        assert!((current_probability(&ft, "A").unwrap() - 0.1).abs() < 1e-9);
        assert!((current_probability(&ft, "B").unwrap() - 0.2).abs() < 1e-9);
    }

    #[test]
    fn leaf_observation_propagates_up_to_the_gate() {
        let ft = unique_id("or-propagate");
        build_two_leaf_or(&ft, 0.1, 0.2);

        // Drive A's estimate up with a run of "occurred" observations —
        // its live mean should move meaningfully above its 0.1 prior, and
        // the OR gate above it should move too, using B's still-unchanged
        // current value.
        for _ in 0..200 {
            record_observation(&ft, "A", true);
        }
        let a = current_probability(&ft, "A").unwrap();
        let b = current_probability(&ft, "B").unwrap();
        let top = current_probability(&ft, "Top").unwrap();

        assert!(a > 0.5, "200 occurrences should pull A's estimate well above its 0.1 prior, got {a}");
        assert!((b - 0.2).abs() < 1e-9, "B was never observed, must stay at its prior");
        let expected_top = a + b - a * b;
        assert!(
            (top - expected_top).abs() < 1e-9,
            "Top must recombine from A and B's *current* values: expected {expected_top}, got {top}"
        );
    }

    #[test]
    fn current_probability_is_none_for_an_unregistered_tree_or_unknown_node() {
        assert_eq!(current_probability("no-such-tree", "X"), None);
        let ft = unique_id("unknown-node");
        build_two_leaf_or(&ft, 0.1, 0.2);
        assert_eq!(current_probability(&ft, "NoSuchNode"), None);
    }

    #[test]
    fn record_observation_on_unregistered_tree_is_a_safe_no_op() {
        // Must not panic — a document without etdl.live-reliability never
        // registers anything, and codegen must still be safe to call this
        // unconditionally in that case.
        record_observation("never-registered", "A", true);
    }

    #[test]
    fn in_range_true_within_threshold_false_beyond_it() {
        let ft = unique_id("in-range");
        build_two_leaf_or(&ft, 0.1, 0.2);
        for _ in 0..200 {
            record_observation(&ft, "A", true);
        }
        // A moved from 0.1 to well above 0.5 — far outside a tight
        // threshold, comfortably inside a loose one.
        assert!(!in_range(&ft, "A", 0.05), "should have drifted past a tight threshold");
        assert!(in_range(&ft, "A", 0.99), "should still be within a very loose threshold");
    }

    #[test]
    fn in_range_fails_open_when_there_is_no_data_yet() {
        // Mirrors SlaTracker's own MIN_OBSERVATIONS default: insufficient
        // data reads as "in range", never as an anomaly.
        assert!(in_range("no-such-tree", "X", 0.01));
    }

    #[test]
    fn inbound_leaf_has_no_current_value_until_apply_inbound_then_gate_updates() {
        let ft = unique_id("inbound");
        LiveFaultTreeBuilder::new("Top")
            .local_leaf("A", 0.1, 20.0)
            .inbound_leaf("Upstream", 0.05)
            .gate("Top", LiveGateKind::Or, vec!["A".to_string(), "Upstream".to_string()])
            .register(&ft)
            .unwrap();

        // Cold start: Upstream itself has no *current* value yet (nothing
        // received), but its baseline (0.05, its own declared probability)
        // is already known, so the gate it feeds already has a baseline-
        // derived value too — never `None` at cold start.
        assert_eq!(current_probability(&ft, "Upstream"), None);
        let cold_start_top = current_probability(&ft, "Top").unwrap();
        let baseline_expected = 0.1 + 0.05 - 0.1 * 0.05;
        assert!(
            (cold_start_top - baseline_expected).abs() < 1e-9,
            "got {cold_start_top}"
        );

        apply_inbound(&serde_json::json!({
            "etdl.live-reliability/1.0": {
                "fault_tree_id": ft,
                "nodes": { "Upstream": 0.3 },
            }
        }));

        assert!((current_probability(&ft, "Upstream").unwrap() - 0.3).abs() < 1e-9);
        let top = current_probability(&ft, "Top").unwrap();
        let expected = 0.1 + 0.3 - 0.1 * 0.3;
        assert!((top - expected).abs() < 1e-9, "got {top}");
    }

    #[test]
    fn inbound_leaf_baseline_is_fixed_at_registration_not_redefined_by_first_push() {
        // The bug this guards against: an earlier implementation let the
        // *first* inbound value double as the baseline, so a service's
        // very first contact with an already-drifted upstream silently
        // redefined "normal" to match it — `in_range` could then never
        // observe a deviation, no matter how far the value had drifted.
        let ft = unique_id("inbound-baseline");
        LiveFaultTreeBuilder::new("Upstream")
            .inbound_leaf("Upstream", 0.1)
            .register(&ft)
            .unwrap();

        // First-ever contact already carries a heavily drifted value — as
        // it would for a consumer service booting up after its upstream
        // has been running (and drifting) for a while.
        apply_inbound(&serde_json::json!({
            "etdl.live-reliability/1.0": {
                "fault_tree_id": ft,
                "nodes": { "Upstream": 0.9 },
            }
        }));

        assert!(
            !in_range(&ft, "Upstream", 0.1),
            "0.9 is far outside a 0.1 threshold around the declared baseline of 0.1 — \
             in_range must not have silently adopted 0.9 as the new baseline"
        );
    }

    #[test]
    fn apply_inbound_on_unknown_fault_tree_is_a_safe_no_op() {
        apply_inbound(&serde_json::json!({
            "etdl.live-reliability/1.0": {
                "fault_tree_id": "no-such-tree",
                "nodes": { "X": 0.5 },
            }
        }));
        assert_eq!(current_probability("no-such-tree", "X"), None);
    }

    #[test]
    fn apply_inbound_ignores_malformed_payloads() {
        apply_inbound(&serde_json::json!({"not": "the right shape"}));
        apply_inbound(&serde_json::json!(null));
        apply_inbound(&serde_json::json!("a string, not an object"));
        // No panic is the whole assertion.
    }

    #[test]
    fn outbound_snapshot_round_trips_through_apply_inbound() {
        let sender_ft = unique_id("sender");
        build_two_leaf_or(&sender_ft, 0.15, 0.25);

        let snapshot = outbound_snapshot(&sender_ft);
        assert_eq!(
            snapshot["etdl.live-reliability/1.0"]["fault_tree_id"],
            serde_json::json!(sender_ft)
        );
        let top_in_snapshot = snapshot["etdl.live-reliability/1.0"]["nodes"]["Top"]
            .as_f64()
            .unwrap();
        assert!((top_in_snapshot - (0.15 + 0.25 - 0.15 * 0.25)).abs() < 1e-9);

        // A second, receiving service's fault tree declares the sender's
        // "Top" as its own inbound leaf under a matching id.
        let receiver_ft = unique_id("receiver");
        LiveFaultTreeBuilder::new("ReceiverTop")
            .inbound_leaf("Top", 0.15)
            .local_leaf("Local", 0.05, 20.0)
            .gate(
                "ReceiverTop",
                LiveGateKind::And,
                vec!["Top".to_string(), "Local".to_string()],
            )
            .register(&receiver_ft)
            .unwrap();

        // Re-key the snapshot's fault_tree_id to the receiver's own, as
        // codegen's `apply_inbound` call site would after reading it off
        // the message the sender actually published.
        let mut relabeled = snapshot;
        relabeled["etdl.live-reliability/1.0"]["fault_tree_id"] =
            serde_json::json!(receiver_ft);
        apply_inbound(&relabeled);

        let received = current_probability(&receiver_ft, "Top").unwrap();
        assert!((received - top_in_snapshot).abs() < 1e-9);
        let combined = current_probability(&receiver_ft, "ReceiverTop").unwrap();
        assert!((combined - (top_in_snapshot * 0.05)).abs() < 1e-9);
    }

    #[test]
    fn register_rejects_unknown_child_reference() {
        let ft = unique_id("bad-child");
        let err = LiveFaultTreeBuilder::new("Top")
            .local_leaf("A", 0.1, 20.0)
            .gate("Top", LiveGateKind::Or, vec!["A".to_string(), "Missing".to_string()])
            .register(&ft)
            .unwrap_err();
        assert!(matches!(err, LiveError::UnknownChild(_, _, _)), "got {err:?}");
    }

    #[test]
    fn register_rejects_unknown_top_event() {
        let ft = unique_id("bad-top");
        let err = LiveFaultTreeBuilder::new("NoSuchTopEvent")
            .local_leaf("A", 0.1, 20.0)
            .register(&ft)
            .unwrap_err();
        assert!(matches!(err, LiveError::UnknownTopEvent(_, _)), "got {err:?}");
    }
}
