//! TREE-* vectors: Generic Tree Event Supplement structural conformance.
//! Covers task §16 (tree invariants) with both positive and negative
//! vectors against `etdl-tree-core`.

use etdl_conformance::vector::{ConformanceVector, Level};
use etdl_tree_core::{GateKind, Tree, TreeError, TreeNode};

#[test]
fn tree_001_a_valid_tree_has_exactly_one_root_and_validates() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-001",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "a well-formed tree has one root, reachable from it, and validates with no errors",
    );
    let tree = Tree::new("t", "1", "Top")
        .with_node("A", TreeNode::leaf())
        .with_node("B", TreeNode::leaf())
        .with_node(
            "Top",
            TreeNode::gate(GateKind::And, vec!["A".to_string(), "B".to_string()]),
        );
    assert!(tree.validate().is_ok(), "{}", VECTOR.id);
}

#[test]
fn tree_002_cycles_are_rejected() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-002",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "no cycles: a tree whose gate children eventually reference an ancestor must be rejected",
    );
    let tree = Tree::new("t", "1", "A")
        .with_node("A", TreeNode::gate(GateKind::Not, vec!["B".to_string()]))
        .with_node("B", TreeNode::gate(GateKind::Not, vec!["A".to_string()]));
    let errors = tree
        .validate()
        .expect_err(&format!("{}: must reject", VECTOR.id));
    assert!(
        errors.iter().any(|e| matches!(e, TreeError::Cycle { .. })),
        "{}: expected a Cycle error, got {errors:?}",
        VECTOR.id
    );
}

#[test]
fn tree_003_unknown_child_reference_is_rejected() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-003",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "valid child references: a gate referencing a node id that does not exist must be rejected",
    );
    let tree = Tree::new("t", "1", "Top").with_node(
        "Top",
        TreeNode::gate(GateKind::And, vec!["A".to_string(), "Ghost".to_string()]),
    );
    let errors = tree
        .validate()
        .expect_err(&format!("{}: must reject", VECTOR.id));
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, TreeError::MissingChild { .. })),
        "{}: expected MissingChild, got {errors:?}",
        VECTOR.id
    );
}

#[test]
fn tree_004_unknown_root_is_rejected() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-004",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "one root where required: `root` must reference an existing node id",
    );
    let tree = Tree::new("t", "1", "NoSuchNode").with_node("A", TreeNode::leaf());
    let errors = tree
        .validate()
        .expect_err(&format!("{}: must reject", VECTOR.id));
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, TreeError::UnknownRoot { .. })),
        "{}: expected UnknownRoot, got {errors:?}",
        VECTOR.id
    );
}

#[test]
fn tree_005_gate_arity_is_validated_per_gate_kind() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-005",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "valid gate parameters: NOT requires exactly one child, XOR requires exactly two, \
         AND/OR require at least two",
    );
    let not_with_two = Tree::new("t", "1", "Top")
        .with_node("A", TreeNode::leaf())
        .with_node("B", TreeNode::leaf())
        .with_node(
            "Top",
            TreeNode::gate(GateKind::Not, vec!["A".to_string(), "B".to_string()]),
        );
    assert!(
        not_with_two
            .validate()
            .expect_err(&format!(
                "{}: NOT with two children must be rejected",
                VECTOR.id
            ))
            .iter()
            .any(|e| matches!(e, TreeError::InvalidArity { .. })),
        "{}",
        VECTOR.id
    );

    let and_with_one = Tree::new("t", "1", "Top")
        .with_node("A", TreeNode::leaf())
        .with_node("Top", TreeNode::gate(GateKind::And, vec!["A".to_string()]));
    assert!(
        and_with_one
            .validate()
            .expect_err(&format!(
                "{}: AND with one child must be rejected",
                VECTOR.id
            ))
            .iter()
            .any(|e| matches!(e, TreeError::InvalidArity { .. })),
        "{}",
        VECTOR.id
    );
}

#[test]
fn tree_006_k_of_n_bounds_are_validated() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-006",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "K_OF_N requires 1 <= k <= n (number of children)",
    );
    let k_exceeds_n = Tree::new("t", "1", "Top")
        .with_node("A", TreeNode::leaf())
        .with_node("B", TreeNode::leaf())
        .with_node(
            "Top",
            TreeNode::gate(GateKind::KOfN(5), vec!["A".to_string(), "B".to_string()]),
        );
    assert!(
        k_exceeds_n
            .validate()
            .expect_err(&format!("{}: k=5 > n=2 must be rejected", VECTOR.id))
            .iter()
            .any(|e| matches!(e, TreeError::InvalidKOfN { .. })),
        "{}",
        VECTOR.id
    );

    let k_zero = Tree::new("t", "1", "Top")
        .with_node("A", TreeNode::leaf())
        .with_node("B", TreeNode::leaf())
        .with_node(
            "Top",
            TreeNode::gate(GateKind::KOfN(0), vec!["A".to_string(), "B".to_string()]),
        );
    assert!(
        k_zero
            .validate()
            .expect_err(&format!("{}: k=0 must be rejected", VECTOR.id))
            .iter()
            .any(|e| matches!(e, TreeError::InvalidKOfN { .. })),
        "{}",
        VECTOR.id
    );
}

#[test]
fn tree_007_shared_nodes_are_rejected_not_silently_a_dag() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-007",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "this is a tree (strict single-parent), not a DAG: a node with two parents must be rejected",
    );
    let tree = Tree::new("t", "1", "Top")
        .with_node("Shared", TreeNode::leaf())
        .with_node(
            "Left",
            TreeNode::gate(GateKind::Not, vec!["Shared".to_string()]),
        )
        .with_node(
            "Right",
            TreeNode::gate(GateKind::Not, vec!["Shared".to_string()]),
        )
        .with_node(
            "Top",
            TreeNode::gate(GateKind::And, vec!["Left".to_string(), "Right".to_string()]),
        );
    let errors = tree
        .validate()
        .expect_err(&format!("{}: must reject", VECTOR.id));
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, TreeError::SharedNode { .. })),
        "{}: expected SharedNode, got {errors:?}",
        VECTOR.id
    );
}

#[test]
fn tree_008_orphaned_nodes_are_rejected() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-008",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "every node must be reachable from the root",
    );
    let tree = Tree::new("t", "1", "Top")
        .with_node("Top", TreeNode::leaf())
        .with_node("Unreachable", TreeNode::leaf());
    let errors = tree
        .validate()
        .expect_err(&format!("{}: must reject", VECTOR.id));
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, TreeError::OrphanedNode { .. })),
        "{}: expected OrphanedNode, got {errors:?}",
        VECTOR.id
    );
}

#[test]
fn tree_009_traversal_is_deterministic() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-009",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md",
        "deterministic traversal: preorder/postorder/leaves produce the same sequence on \
         repeated calls, independent of BTreeMap iteration happening to vary",
    );
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
    tree.validate().expect("valid");

    let first_preorder = tree.preorder();
    let second_preorder = tree.preorder();
    assert_eq!(first_preorder, second_preorder, "{}: preorder", VECTOR.id);

    let first_leaves = tree.leaves();
    let second_leaves = tree.leaves();
    assert_eq!(first_leaves, second_leaves, "{}: leaves", VECTOR.id);
}

#[test]
fn tree_010_deep_but_valid_tree_does_not_overflow_the_stack() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "TREE-010",
        Level::Full,
        "docs/reference/generic-tree-event-supplement.md",
        "resource limits / stack safety (task §43/§44): a deeply nested but valid tree \
         must validate and traverse without crashing the process",
    );
    // 5,000 nested NOT gates: deep, but a legitimate tree, not adversarial
    // malformed input — the malformed-input case (cycles) is TREE-002.
    const DEPTH: usize = 5_000;
    let mut tree = Tree::new("t", "1", "n0").with_node("leaf", TreeNode::leaf());
    for i in 0..DEPTH {
        let node_id = format!("n{i}");
        let child = if i == DEPTH - 1 {
            "leaf".to_string()
        } else {
            format!("n{}", i + 1)
        };
        tree = tree.with_node(node_id, TreeNode::gate(GateKind::Not, vec![child]));
    }
    assert!(
        tree.validate().is_ok(),
        "{}: deep tree must validate",
        VECTOR.id
    );
    let preorder = tree.preorder();
    assert_eq!(
        preorder.len(),
        DEPTH + 1,
        "{}: every node visited once",
        VECTOR.id
    );
}
