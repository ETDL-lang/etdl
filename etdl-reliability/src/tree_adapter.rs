//! Adapter: Generic Tree Event (`etdl-tree-core`) -> a reliability
//! interpretation, via `std.probability`.
//!
//! `etdl-tree-core::Tree` has **no** dependency on this crate, on
//! `etdl-probability-core`, or on any notion of probability — it is pure
//! structure (nodes, gates, children). This module is where reliability
//! *consumes* that structure: it walks a tree and combines leaf
//! probabilities according to each gate's logical meaning, under an
//! **explicit** independence assumption (never inferred from the tree
//! structure itself — an `AND` node means logical conjunction; whether
//! `P(A and B) = P(A)*P(B)` is a modeling choice this function makes
//! visible by name, not something the tree asserts).
//!
//! This is *one* possible reliability interpretation of a generic tree,
//! not the only legitimate one, and not part of the tree supplement
//! itself — a future safety or security domain could interpret the exact
//! same tree differently (e.g. treating `AND` as "all conditions
//! co-occurred" without ever computing a probability at all). See
//! `docs/reference/generic-tree-event-supplement.md`.

use std::collections::BTreeMap;

use etdl_probability_core::{complement, independent_and_n, independent_or_n, Probability};
use etdl_tree_core::{GateKind, NodeKind, Tree};

/// A problem evaluating a tree under the independence interpretation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TreeEvaluationError {
    #[error("no probability was supplied for leaf '{0}'")]
    MissingLeafProbability(String),
    #[error("tree references node '{0}', which does not exist")]
    UnknownNode(String),
    #[error("probability composition at node '{node}' failed: {source}")]
    Probability {
        node: String,
        #[source]
        source: etdl_probability_core::ProbabilityError,
    },
}

/// Evaluate `tree`'s root probability, given each leaf's probability
/// **explicitly** (`leaf_probabilities`, keyed by node id — never inferred,
/// never defaulted) and an **explicit** independence assumption applied at
/// every `AND`/`OR`/`K_OF_N` gate. `NOT` is `complement`; `XOR` is "exactly
/// one of the two children" under independence.
///
/// This does not call the existing dependency-aware reliability analysis
/// (`etdl-reliability::analysis::dependence`) — a tree with genuine
/// dependence/common-cause structure should be analyzed with that existing,
/// unmodified machinery instead of this simple independence-only adapter,
/// which exists to demonstrate the generic-tree-to-reliability layering,
/// not to replace dependency-aware analysis.
pub fn evaluate_assuming_independence(
    tree: &Tree,
    leaf_probabilities: &BTreeMap<String, Probability>,
) -> Result<Probability, TreeEvaluationError> {
    evaluate_node(tree, &tree.root, leaf_probabilities)
}

fn evaluate_node(
    tree: &Tree,
    node_id: &str,
    leaf_probabilities: &BTreeMap<String, Probability>,
) -> Result<Probability, TreeEvaluationError> {
    let node = tree
        .nodes
        .get(node_id)
        .ok_or_else(|| TreeEvaluationError::UnknownNode(node_id.to_string()))?;

    match &node.kind {
        NodeKind::Leaf { .. } => leaf_probabilities
            .get(node_id)
            .copied()
            .ok_or_else(|| TreeEvaluationError::MissingLeafProbability(node_id.to_string())),
        NodeKind::Gate { gate, children } => {
            let child_probs = children
                .iter()
                .map(|c| evaluate_node(tree, c, leaf_probabilities))
                .collect::<Result<Vec<_>, _>>()?;
            evaluate_gate(node_id, *gate, &child_probs)
        }
    }
}

fn evaluate_gate(
    node_id: &str,
    gate: GateKind,
    child_probs: &[Probability],
) -> Result<Probability, TreeEvaluationError> {
    let wrap = |r: Result<Probability, etdl_probability_core::ProbabilityError>| {
        r.map_err(|source| TreeEvaluationError::Probability {
            node: node_id.to_string(),
            source,
        })
    };
    match gate {
        GateKind::And => wrap(independent_and_n(child_probs)),
        GateKind::Or => wrap(independent_or_n(child_probs)),
        GateKind::Not => Ok(complement(child_probs[0])),
        GateKind::Xor => {
            // Exactly one of two, under independence: p0(1-p1) + (1-p0)p1.
            let (p0, p1) = (child_probs[0].value(), child_probs[1].value());
            wrap(Probability::new(p0 * (1.0 - p1) + (1.0 - p0) * p1))
        }
        GateKind::KOfN(k) => {
            wrap(Probability::new(at_least_k_of_n_independent(child_probs, k as usize)))
        }
    }
}

/// `P(at least k of n independent events occur)`, via the standard
/// success-count polynomial recurrence (`O(n^2)`, appropriate for the
/// small trees this adapter targets — not the exact/optimized voting-gate
/// math already in `etdl-compiler::fault_tree`, which this crate does not
/// depend on and does not duplicate here beyond this simple recurrence).
fn at_least_k_of_n_independent(probs: &[Probability], k: usize) -> f64 {
    let mut poly = vec![1.0];
    for p in probs {
        let p = p.value();
        let mut next = vec![0.0; poly.len() + 1];
        for (i, &pi) in poly.iter().enumerate() {
            next[i] += pi * (1.0 - p);
            next[i + 1] += pi * p;
        }
        poly = next;
    }
    poly.iter().skip(k).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use etdl_tree_core::TreeNode;

    fn p(v: f64) -> Probability {
        Probability::new(v).unwrap()
    }

    #[test]
    fn and_gate_matches_independent_and() {
        let tree = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(GateKind::And, vec!["A".to_string(), "B".to_string()]),
            );
        let leaves = BTreeMap::from([("A".to_string(), p(0.01)), ("B".to_string(), p(0.02))]);
        let result = evaluate_assuming_independence(&tree, &leaves).unwrap();
        assert!((result.value() - 0.0002).abs() < 1e-12);
    }

    #[test]
    fn or_gate_matches_independent_or() {
        let tree = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(GateKind::Or, vec!["A".to_string(), "B".to_string()]),
            );
        let leaves = BTreeMap::from([("A".to_string(), p(0.01)), ("B".to_string(), p(0.02))]);
        let result = evaluate_assuming_independence(&tree, &leaves).unwrap();
        // 1 - (1-0.01)(1-0.02) = 0.0298
        assert!((result.value() - 0.0298).abs() < 1e-9);
    }

    #[test]
    fn not_gate_is_complement() {
        let tree = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("Top", TreeNode::gate(GateKind::Not, vec!["A".to_string()]));
        let leaves = BTreeMap::from([("A".to_string(), p(0.3))]);
        let result = evaluate_assuming_independence(&tree, &leaves).unwrap();
        assert!((result.value() - 0.7).abs() < 1e-12);
    }

    #[test]
    fn xor_gate_known_value() {
        let tree = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(GateKind::Xor, vec!["A".to_string(), "B".to_string()]),
            );
        let leaves = BTreeMap::from([("A".to_string(), p(0.5)), ("B".to_string(), p(0.5))]);
        let result = evaluate_assuming_independence(&tree, &leaves).unwrap();
        // 0.5*0.5 + 0.5*0.5 = 0.5
        assert!((result.value() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn k_of_n_matches_binomial_cdf_complement_for_equal_probabilities() {
        use etdl_probability_core::distribution::Binomial;

        let tree = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node("C", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(
                    GateKind::KOfN(2),
                    vec!["A".to_string(), "B".to_string(), "C".to_string()],
                ),
            );
        let leaves = BTreeMap::from([
            ("A".to_string(), p(0.1)),
            ("B".to_string(), p(0.1)),
            ("C".to_string(), p(0.1)),
        ]);
        let result = evaluate_assuming_independence(&tree, &leaves).unwrap();

        // Cross-check against etdl-probability-core's own Binomial:
        // P(at least 2 of 3) = 1 - P(X<=1) for X~Binomial(3, 0.1).
        let binom = Binomial::new(3, p(0.1)).unwrap();
        let expected = 1.0 - binom.cdf(1).value();
        assert!((result.value() - expected).abs() < 1e-9);
    }

    #[test]
    fn nested_tree_matches_hand_derivation() {
        // Top = A AND (B OR C), A=0.1, B=0.2, C=0.3
        // Inner = 1-(1-0.2)(1-0.3) = 0.44
        // Top = 0.1 * 0.44 = 0.044
        let tree = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node("C", TreeNode::leaf())
            .with_node(
                "Inner",
                TreeNode::gate(GateKind::Or, vec!["B".to_string(), "C".to_string()]),
            )
            .with_node(
                "Top",
                TreeNode::gate(GateKind::And, vec!["A".to_string(), "Inner".to_string()]),
            );
        let leaves = BTreeMap::from([
            ("A".to_string(), p(0.1)),
            ("B".to_string(), p(0.2)),
            ("C".to_string(), p(0.3)),
        ]);
        let result = evaluate_assuming_independence(&tree, &leaves).unwrap();
        assert!((result.value() - 0.044).abs() < 1e-9);
    }

    #[test]
    fn missing_leaf_probability_is_an_explicit_error_not_a_default() {
        let tree = Tree::new("t", "1", "A").with_node("A", TreeNode::leaf());
        let leaves = BTreeMap::new();
        let err = evaluate_assuming_independence(&tree, &leaves).unwrap_err();
        assert!(matches!(err, TreeEvaluationError::MissingLeafProbability(_)));
    }
}
