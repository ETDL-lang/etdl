//! `cargo run -p etdl-reliability --example tree_to_artifact`
//!
//! The reliability domain interpreting a Generic Tree Event
//! (`../../examples/tree-event/reliability-consumer.etdl`'s structure,
//! reproduced here in code since tree *evaluation* is a Rust-side concern —
//! see `docs/reference/generic-tree-event-supplement.md` for why): leaf
//! probabilities, an explicit independence assumption, and the result fed
//! into the *existing*, unmodified `ReliabilityArtifact` machinery.

use std::collections::BTreeMap;

use etdl_probability_core::Probability;
use etdl_reliability::probability_adapter::estimate_from_probability;
use etdl_reliability::tree_adapter::evaluate_assuming_independence;
use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::estimate::ProbabilityState;
use etdl_tree_core::{GateKind, Tree, TreeNode};

fn main() {
    // Same structure as examples/tree-event/reliability-consumer.etdl's
    // `payment-gateway-availability` tree.
    let tree = Tree::new("payment-gateway-availability", "1", "GatewayUnavailable")
        .with_node(
            "NetworkTimeout",
            TreeNode::leaf_referencing("std.events.NetworkTimeout"),
        )
        .with_node(
            "DatabaseFailure",
            TreeNode::leaf().with_description("the database did not respond"),
        )
        .with_node(
            "GatewayUnavailable",
            TreeNode::gate(
                GateKind::Or,
                vec!["NetworkTimeout".to_string(), "DatabaseFailure".to_string()],
            ),
        );

    if let Err(errors) = tree.validate() {
        eprintln!("tree is invalid: {errors:?}");
        return;
    }
    println!("tree '{}' is structurally valid", tree.id);
    println!("  root: {}", tree.root);
    println!("  leaves: {}", tree.leaves().join(", "));

    // The reliability interpretation: explicit leaf probabilities and an
    // explicit independence assumption.
    let leaf_probabilities = BTreeMap::from([
        (
            "NetworkTimeout".to_string(),
            Probability::new(0.001).unwrap(),
        ),
        (
            "DatabaseFailure".to_string(),
            Probability::new(0.0003).unwrap(),
        ),
    ]);
    let top_probability = evaluate_assuming_independence(&tree, &leaf_probabilities)
        .expect("every leaf has a probability");
    println!(
        "P(GatewayUnavailable), assuming independence = {}",
        top_probability
    );

    // Feed the result into the existing reliability artifact machinery.
    let estimate = estimate_from_probability(
        "payment-gateway-availability.GatewayUnavailable",
        ProbabilityState::Estimated,
        top_probability,
    );
    let mut artifact = ReliabilityArtifact::new("payment-gateway-availability");
    artifact.version = Some("1.0.0".to_string());
    artifact.add(estimate).unwrap();

    println!(
        "ReliabilityArtifact '{}' now has {} estimate(s)",
        artifact.id,
        artifact.estimates.len()
    );
    println!(
        "  payment-gateway-availability.GatewayUnavailable = {}",
        artifact
            .get("payment-gateway-availability.GatewayUnavailable")
            .unwrap()
            .value
            .unwrap()
    );
}
