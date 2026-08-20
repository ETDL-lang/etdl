//! End-to-end: Generic Tree Event (`etdl-tree-core`) -> reliability
//! interpretation (`etdl-reliability::tree_adapter`) -> `std.probability`
//! -> the *existing*, unmodified `ReliabilityArtifact`/`ArtifactResolver`.
//! `etdl-tree-core` itself never depends on reliability or probability —
//! this test proves the whole chain works with the dependency arrow
//! pointing only one way.

use std::collections::BTreeMap;

use etdl_probability_core::Probability;
use etdl_reliability::probability_adapter::estimate_from_probability;
use etdl_reliability::tree_adapter::evaluate_assuming_independence;
use etdl_reliability_core::artifact::{ArtifactResolver, ReliabilityArtifact, ResolveOutcome, UnknownProbabilityPolicy};
use etdl_reliability_core::estimate::ProbabilityState;
use etdl_tree_core::{GateKind, Tree, TreeNode};

#[test]
fn generic_tree_flows_through_reliability_into_an_artifact() {
    // Step 1: a genuinely generic tree -- no reliability vocabulary
    // anywhere in etdl-tree-core. `AnyIssue` = ConditionA OR EventB.
    let tree = Tree::new("device-monitoring", "1", "AnyIssue")
        .with_node(
            "ConditionA",
            TreeNode::leaf_referencing("std.events.ConditionMet"),
        )
        .with_node("EventB", TreeNode::leaf().with_description("event B occurred"))
        .with_node(
            "AnyIssue",
            TreeNode::gate(
                GateKind::Or,
                vec!["ConditionA".to_string(), "EventB".to_string()],
            ),
        );
    tree.validate().expect("tree is structurally valid");

    // Step 2: reliability interprets the tree, under an EXPLICIT
    // independence assumption, using std.probability for the leaf values.
    let leaf_probabilities = BTreeMap::from([
        ("ConditionA".to_string(), Probability::new(0.01).unwrap()),
        ("EventB".to_string(), Probability::new(0.02).unwrap()),
    ]);
    let top_probability = evaluate_assuming_independence(&tree, &leaf_probabilities).unwrap();
    // 1 - (1-0.01)(1-0.02) = 0.0298
    assert!((top_probability.value() - 0.0298).abs() < 1e-9);

    // Step 3: the result flows into the *existing*, unmodified reliability
    // artifact machinery via the probability adapter -- nothing below this
    // line knows the value originated from a generic tree.
    let estimate = estimate_from_probability(
        "device-monitoring.AnyIssue",
        ProbabilityState::Estimated,
        top_probability,
    );
    let mut artifact = ReliabilityArtifact::new("device-monitoring");
    artifact.version = Some("1.0.0".to_string());
    artifact.add(estimate).unwrap();

    let resolver = ArtifactResolver::new(UnknownProbabilityPolicy::Error);
    let outcome = resolver
        .resolve(&artifact, "device-monitoring.AnyIssue")
        .unwrap();
    let ResolveOutcome::Resolved(resolved) = outcome else {
        panic!("expected Resolved, got {outcome:?}");
    };
    assert!((resolved.value - 0.0298).abs() < 1e-9);
}

#[test]
fn the_same_tree_would_evaluate_differently_under_a_different_domain_interpretation() {
    // The point of keeping structure and interpretation separate: this
    // tree's OR gate could equally be evaluated by a future domain that
    // does NOT compute a probability at all (e.g. a safety domain
    // asserting "any of these conditions triggers a hazard flag," a
    // boolean, not a probability). This test only proves the reliability
    // interpretation is not baked into the tree structure -- by evaluating
    // the same structure with different leaf inputs and confirming the
    // tree itself is unchanged.
    let tree = Tree::new("t", "1", "Top")
        .with_node("A", TreeNode::leaf())
        .with_node("B", TreeNode::leaf())
        .with_node(
            "Top",
            TreeNode::gate(GateKind::Or, vec!["A".to_string(), "B".to_string()]),
        );
    let tree_snapshot = tree.clone();

    let leaves_1 = BTreeMap::from([
        ("A".to_string(), Probability::new(0.1).unwrap()),
        ("B".to_string(), Probability::new(0.1).unwrap()),
    ]);
    let leaves_2 = BTreeMap::from([
        ("A".to_string(), Probability::new(0.9).unwrap()),
        ("B".to_string(), Probability::new(0.9).unwrap()),
    ]);
    let r1 = evaluate_assuming_independence(&tree, &leaves_1).unwrap();
    let r2 = evaluate_assuming_independence(&tree, &leaves_2).unwrap();

    assert_ne!(r1.value(), r2.value());
    assert_eq!(tree, tree_snapshot, "evaluating the tree must never mutate it");
}
