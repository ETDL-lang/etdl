//! Predictive evaluation of a Generic Tree Event
//! ([`etdl_tree_core::Tree`]) at a given mission time.
//!
//! This does **not** add any new tree-composition logic. It computes each
//! leaf's failure probability at time `t` from that leaf's own
//! [`TimeToFailureModel`], then hands the resulting
//! `BTreeMap<String, Probability>` straight to
//! [`crate::tree_adapter::evaluate_assuming_independence`], unchanged —
//! the same function the (non-predictive) Reliability interpretation of a
//! tree already uses. The only thing this module adds is "where do the
//! leaf probabilities come from when the question has a time horizon,"
//! nothing about how gates combine them.

use std::collections::BTreeMap;

use etdl_probability_core::Probability;
use etdl_tree_core::Tree;

use crate::predictive::models::TimeToFailureModel;
use crate::tree_adapter::{self, TreeEvaluationError};

/// Evaluate `tree`'s root failure probability `F(t)` at mission time `t`,
/// given each leaf's time-to-failure model. `leaf_models` is keyed by node
/// id, exactly like `tree_adapter::evaluate_assuming_independence`'s
/// `leaf_probabilities` — explicit, never inferred, never defaulted.
///
/// The independence assumption applied at every gate is the same one
/// `tree_adapter` already documents; this function does not add or change
/// that assumption.
pub fn evaluate_failure_probability_at(
    tree: &Tree,
    leaf_models: &BTreeMap<String, &dyn TimeToFailureModel>,
    t: f64,
) -> Result<Probability, TreeEvaluationError> {
    let leaf_probabilities: BTreeMap<String, Probability> = leaf_models
        .iter()
        .filter_map(|(id, model)| {
            Probability::new(model.failure_probability(t))
                .ok()
                .map(|p| (id.clone(), p))
        })
        .collect();

    tree_adapter::evaluate_assuming_independence(tree, &leaf_probabilities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predictive::models::ExponentialModel;
    use etdl_tree_core::{GateKind, TreeNode};

    #[test]
    fn and_gate_of_two_exponential_leaves_at_mission_time() {
        // A: lambda=0.001/hr, B: lambda=0.002/hr, both at t=100h.
        let a = ExponentialModel::new(0.001).unwrap();
        let b = ExponentialModel::new(0.002).unwrap();

        let tree = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(GateKind::And, vec!["A".to_string(), "B".to_string()]),
            );

        let leaf_models: BTreeMap<String, &dyn TimeToFailureModel> = BTreeMap::from([
            ("A".to_string(), &a as &dyn TimeToFailureModel),
            ("B".to_string(), &b as &dyn TimeToFailureModel),
        ]);

        let result = evaluate_failure_probability_at(&tree, &leaf_models, 100.0).unwrap();

        let fa = 1.0 - (-0.001f64 * 100.0).exp();
        let fb = 1.0 - (-0.002f64 * 100.0).exp();
        let expected = fa * fb;
        assert!((result.value() - expected).abs() < 1e-9);
    }

    #[test]
    fn missing_leaf_model_is_an_explicit_error() {
        let tree = Tree::new("t", "1", "A").with_node("A", TreeNode::leaf());
        let leaf_models: BTreeMap<String, &dyn TimeToFailureModel> = BTreeMap::new();
        let err = evaluate_failure_probability_at(&tree, &leaf_models, 10.0).unwrap_err();
        assert!(matches!(
            err,
            TreeEvaluationError::MissingLeafProbability(_)
        ));
    }
}
