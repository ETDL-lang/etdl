//! [`Tree`]: a generic, domain-neutral tree of [`TreeNode`]s.
//!
//! **1.0 is strictly a tree, not a DAG**: every non-root node has exactly
//! one parent. A node referenced as a child by more than one gate is
//! rejected ([`TreeError::SharedNode`]) rather than silently treated as an
//! implicit DAG — see the crate-level docs for why, and how a future
//! version could introduce an explicit Tree/DAG distinction instead of
//! silently changing 1.0's meaning.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::node::{GateKind, NodeKind, TreeNode};

/// Schema identity for the Generic Tree Event Supplement.
pub const TREE_SCHEMA: &str = "etdl.tree-event/1.0";

fn default_schema() -> String {
    TREE_SCHEMA.to_string()
}

/// A generic tree of events. See the module docs for the tree/DAG
/// decision. Construction never validates automatically — call
/// [`Tree::validate`] explicitly (the same "validate, don't silently
/// repair" discipline the rest of this workspace already follows).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    #[serde(default = "default_schema")]
    pub schema: String,
    pub id: String,
    pub version: String,
    pub root: String,
    pub nodes: BTreeMap<String, TreeNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

/// A problem found while validating a tree. Validation never silently
/// repairs invalid data (no dropping a bad child, no picking an arbitrary
/// root) — it reports every problem it can find in one pass.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeError {
    #[error("tree '{tree}' has no nodes")]
    EmptyTree { tree: String },
    #[error("tree '{tree}': root id is empty")]
    MissingRoot { tree: String },
    #[error("tree '{tree}': root '{root}' is not a node in this tree")]
    UnknownRoot { tree: String, root: String },
    #[error("tree '{tree}': node '{node}' (gate {gate}) references unknown child '{child}'")]
    MissingChild {
        tree: String,
        node: String,
        gate: String,
        child: String,
    },
    #[error(
        "tree '{tree}': node '{node}' has {parent_count} parents ({parents:?}); a 1.0 tree \
         requires exactly one parent per non-root node (this is not a DAG — see the crate docs)"
    )]
    SharedNode {
        tree: String,
        node: String,
        parent_count: usize,
        parents: Vec<String>,
    },
    #[error("tree '{tree}': node '{node}' is not reachable from root '{root}'")]
    OrphanedNode {
        tree: String,
        node: String,
        root: String,
    },
    #[error("tree '{tree}': cycle detected: {}", chain.join(" -> "))]
    Cycle { tree: String, chain: Vec<String> },
    #[error(
        "tree '{tree}': node '{node}' ({gate} gate) has {got} child(ren), {expected} required"
    )]
    InvalidArity {
        tree: String,
        node: String,
        gate: String,
        got: usize,
        expected: String,
    },
    #[error("tree '{tree}': node '{node}' (K_OF_N gate) k={k} must satisfy 1 <= k <= n={n}")]
    InvalidKOfN {
        tree: String,
        node: String,
        k: u32,
        n: usize,
    },
}

impl Tree {
    pub fn new(id: impl Into<String>, version: impl Into<String>, root: impl Into<String>) -> Self {
        Tree {
            schema: TREE_SCHEMA.to_string(),
            id: id.into(),
            version: version.into(),
            root: root.into(),
            nodes: BTreeMap::new(),
            description: None,
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_node(mut self, id: impl Into<String>, node: TreeNode) -> Self {
        self.nodes.insert(id.into(), node);
        self
    }

    /// Full structural validation: non-empty, a resolvable root, every
    /// gate's children exist and have correct arity, exactly one parent
    /// per non-root node, every node reachable from root, and no cycles.
    /// Collects every problem found rather than stopping at the first.
    pub fn validate(&self) -> Result<(), Vec<TreeError>> {
        let mut errors = Vec::new();

        if self.nodes.is_empty() {
            errors.push(TreeError::EmptyTree {
                tree: self.id.clone(),
            });
            return Err(errors);
        }
        if self.root.trim().is_empty() {
            errors.push(TreeError::MissingRoot {
                tree: self.id.clone(),
            });
            return Err(errors);
        }
        if !self.nodes.contains_key(&self.root) {
            errors.push(TreeError::UnknownRoot {
                tree: self.id.clone(),
                root: self.root.clone(),
            });
            return Err(errors);
        }

        // Arity + missing-child + parent-count, in one pass over every
        // gate's declared children.
        let mut parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (node_id, node) in &self.nodes {
            if let NodeKind::Gate { gate, children } = &node.kind {
                self.check_arity(node_id, *gate, children, &mut errors);
                for child in children {
                    if !self.nodes.contains_key(child) {
                        errors.push(TreeError::MissingChild {
                            tree: self.id.clone(),
                            node: node_id.clone(),
                            gate: gate.label().to_string(),
                            child: child.clone(),
                        });
                        continue;
                    }
                    parents
                        .entry(child.clone())
                        .or_default()
                        .push(node_id.clone());
                }
            }
        }
        for (node_id, ps) in &parents {
            if ps.len() > 1 {
                errors.push(TreeError::SharedNode {
                    tree: self.id.clone(),
                    node: node_id.clone(),
                    parent_count: ps.len(),
                    parents: ps.clone(),
                });
            }
        }

        // Only attempt reachability/cycle detection if references are
        // structurally sound enough to walk safely.
        if errors.is_empty() {
            self.check_reachability_and_cycles(&mut errors);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn check_arity(
        &self,
        node_id: &str,
        gate: GateKind,
        children: &[String],
        errors: &mut Vec<TreeError>,
    ) {
        let n = children.len();
        match gate {
            GateKind::And | GateKind::Or => {
                if n < 2 {
                    errors.push(TreeError::InvalidArity {
                        tree: self.id.clone(),
                        node: node_id.to_string(),
                        gate: gate.label().to_string(),
                        got: n,
                        expected: "at least 2".to_string(),
                    });
                }
            }
            GateKind::Not => {
                if n != 1 {
                    errors.push(TreeError::InvalidArity {
                        tree: self.id.clone(),
                        node: node_id.to_string(),
                        gate: gate.label().to_string(),
                        got: n,
                        expected: "exactly 1".to_string(),
                    });
                }
            }
            GateKind::Xor => {
                if n != 2 {
                    errors.push(TreeError::InvalidArity {
                        tree: self.id.clone(),
                        node: node_id.to_string(),
                        gate: gate.label().to_string(),
                        got: n,
                        expected: "exactly 2".to_string(),
                    });
                }
            }
            GateKind::KOfN(k) => {
                if n < 1 {
                    errors.push(TreeError::InvalidArity {
                        tree: self.id.clone(),
                        node: node_id.to_string(),
                        gate: gate.label().to_string(),
                        got: n,
                        expected: "at least 1".to_string(),
                    });
                } else if k < 1 || k as usize > n {
                    errors.push(TreeError::InvalidKOfN {
                        tree: self.id.clone(),
                        node: node_id.to_string(),
                        k,
                        n,
                    });
                }
            }
        }
    }

    /// Iterative (not recursive) depth-first walk from the root, checking
    /// reachability and cycles. An earlier recursive version (one Rust
    /// function call per tree node on the path from the root) could
    /// overflow the process stack on a deep-but-valid tree — caught by
    /// `etdl-conformance`'s `TREE-010` vector (stack/recursion safety, see
    /// task "Predictive Reliability... Conformance" §43/44) — so this
    /// walks with an explicit, heap-allocated stack of `(node, child
    /// index)` frames instead, simulating the call stack exactly: this
    /// produces byte-identical `TreeError::Cycle` chains and the same
    /// orphaned-node detection as the recursive version did, just without
    /// the process-stack depth risk.
    fn check_reachability_and_cycles(&self, errors: &mut Vec<TreeError>) {
        let mut visited: BTreeSet<String> = BTreeSet::new();
        let mut on_path: Vec<String> = Vec::new(); // currently-on-path, for cycle detection

        struct Frame<'a> {
            children: std::slice::Iter<'a, String>,
        }

        if self.try_enter(&self.root, &mut visited, &mut on_path, errors) {
            let mut call_stack: Vec<Frame> = vec![Frame {
                children: self
                    .nodes
                    .get(self.root.as_str())
                    .map(|n| n.children())
                    .unwrap_or(&[])
                    .iter(),
            }];

            while let Some(frame) = call_stack.last_mut() {
                let mut descended = false;
                for child in frame.children.by_ref() {
                    if !self.nodes.contains_key(child) {
                        continue;
                    }
                    if self.try_enter(child, &mut visited, &mut on_path, errors) {
                        call_stack.push(Frame {
                            children: self
                                .nodes
                                .get(child.as_str())
                                .expect("checked above")
                                .children()
                                .iter(),
                        });
                        descended = true;
                        break;
                    }
                    // try_enter returned false (cycle or already fully
                    // explored): mirrors the recursive version's early
                    // `return`, i.e. just continue this frame's sibling loop.
                }
                if !descended {
                    on_path.pop();
                    call_stack.pop();
                }
            }
        }

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                errors.push(TreeError::OrphanedNode {
                    tree: self.id.clone(),
                    node: node_id.clone(),
                    root: self.root.clone(),
                });
            }
        }
    }

    /// Attempts to "enter" `node_id` the way the original recursive `walk`
    /// did at the top of its body: records a [`TreeError::Cycle`] and
    /// returns `false` if `node_id` is already on the current path, is a
    /// no-op-and-returns-`false` if it was already fully explored via
    /// another path, or marks it visited/on-path and returns `true`
    /// otherwise (the caller should then descend into its children).
    fn try_enter(
        &self,
        node_id: &str,
        visited: &mut BTreeSet<String>,
        on_path: &mut Vec<String>,
        errors: &mut Vec<TreeError>,
    ) -> bool {
        if on_path.iter().any(|n| n == node_id) {
            let mut chain = on_path.clone();
            chain.push(node_id.to_string());
            errors.push(TreeError::Cycle {
                tree: self.id.clone(),
                chain,
            });
            return false;
        }
        if !visited.insert(node_id.to_string()) {
            return false;
        }
        on_path.push(node_id.to_string());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::NodeStatus;

    #[test]
    fn single_leaf_tree_is_valid() {
        let t = Tree::new("t", "1", "A").with_node("A", TreeNode::leaf());
        assert!(t.validate().is_ok());
    }

    #[test]
    fn empty_tree_is_rejected() {
        let t = Tree::new("t", "1", "A");
        let errs = t.validate().unwrap_err();
        assert!(matches!(errs[0], TreeError::EmptyTree { .. }));
    }

    #[test]
    fn unknown_root_is_rejected() {
        let t = Tree::new("t", "1", "Missing").with_node("A", TreeNode::leaf());
        let errs = t.validate().unwrap_err();
        assert!(matches!(errs[0], TreeError::UnknownRoot { .. }));
    }

    #[test]
    fn and_gate_with_two_children_is_valid() {
        let t = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(GateKind::And, vec!["A".to_string(), "B".to_string()]),
            );
        assert!(t.validate().is_ok());
    }

    #[test]
    fn and_gate_with_one_child_is_invalid_arity() {
        let t = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("Top", TreeNode::gate(GateKind::And, vec!["A".to_string()]));
        let errs = t.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, TreeError::InvalidArity { .. })));
    }

    #[test]
    fn not_gate_requires_exactly_one_child() {
        let t = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(GateKind::Not, vec!["A".to_string(), "B".to_string()]),
            );
        let errs = t.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, TreeError::InvalidArity { .. })));
    }

    #[test]
    fn xor_gate_requires_exactly_two_children() {
        let t = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node("Top", TreeNode::gate(GateKind::Xor, vec!["A".to_string()]));
        let errs = t.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, TreeError::InvalidArity { .. })));
    }

    #[test]
    fn k_of_n_valid() {
        let t = Tree::new("t", "1", "Top")
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
        assert!(t.validate().is_ok());
    }

    #[test]
    fn k_of_n_k_exceeds_n_is_rejected() {
        let t = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(GateKind::KOfN(5), vec!["A".to_string()]),
            );
        let errs = t.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, TreeError::InvalidKOfN { .. })));
    }

    #[test]
    fn k_of_n_k_zero_is_rejected() {
        let t = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf())
            .with_node(
                "Top",
                TreeNode::gate(GateKind::KOfN(0), vec!["A".to_string()]),
            );
        let errs = t.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, TreeError::InvalidKOfN { .. })));
    }

    #[test]
    fn missing_child_reference_is_rejected() {
        let t = Tree::new("t", "1", "Top").with_node(
            "Top",
            TreeNode::gate(GateKind::And, vec!["A".to_string(), "B".to_string()]),
        );
        let errs = t.validate().unwrap_err();
        assert_eq!(
            errs.iter()
                .filter(|e| matches!(e, TreeError::MissingChild { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn cycle_is_rejected_not_infinitely_recursed() {
        // A -> B -> A (each a NOT gate, satisfying arity so the cycle
        // check itself is what's under test).
        let t = Tree::new("t", "1", "A")
            .with_node("A", TreeNode::gate(GateKind::Not, vec!["B".to_string()]))
            .with_node("B", TreeNode::gate(GateKind::Not, vec!["A".to_string()]));
        let errs = t.validate().unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, TreeError::Cycle { .. })));
    }

    #[test]
    fn shared_node_two_parents_is_rejected_not_silently_a_dag() {
        let t = Tree::new("t", "1", "Top")
            .with_node("Shared", TreeNode::leaf())
            .with_node("B", TreeNode::leaf())
            .with_node(
                "G1",
                TreeNode::gate(GateKind::Not, vec!["Shared".to_string()]),
            )
            .with_node(
                "Top",
                TreeNode::gate(
                    GateKind::And,
                    vec!["G1".to_string(), "B".to_string(), "Shared".to_string()],
                ),
            );
        let errs = t.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, TreeError::SharedNode { .. })));
    }

    #[test]
    fn orphaned_node_not_reachable_from_root_is_rejected() {
        let t = Tree::new("t", "1", "Top")
            .with_node("Top", TreeNode::leaf())
            .with_node("Disconnected", TreeNode::leaf());
        let errs = t.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, TreeError::OrphanedNode { .. })));
    }

    #[test]
    fn nested_gates_are_valid() {
        // Top = A AND (B OR C)
        let t = Tree::new("t", "1", "Top")
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
        assert!(t.validate().is_ok());
    }

    #[test]
    fn node_status_is_optional_and_round_trips() {
        let node = TreeNode::leaf().with_status(NodeStatus::Candidate);
        let json = serde_json::to_string(&node).unwrap();
        let back: TreeNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, Some(NodeStatus::Candidate));
    }

    #[test]
    fn validation_collects_every_problem_not_just_the_first() {
        let t = Tree::new("t", "1", "Top").with_node(
            "Top",
            TreeNode::gate(GateKind::Xor, vec!["A".to_string()]), // wrong arity AND missing child at once
        );
        let errs = t.validate().unwrap_err();
        assert!(errs.len() >= 2, "expected multiple errors, got {errs:?}");
    }

    #[test]
    fn tree_serializes_and_deserializes_preserving_everything() {
        let mut t = Tree::new("t", "1", "Top")
            .with_node("A", TreeNode::leaf().with_description("leaf A"))
            .with_node("B", TreeNode::leaf_referencing("std.events.Occurred"))
            .with_node(
                "Top",
                TreeNode::gate(GateKind::Or, vec!["A".to_string(), "B".to_string()])
                    .with_description("top gate"),
            );
        t.metadata.insert("owner".to_string(), "team-x".to_string());
        t.description = Some("a demo tree".to_string());

        let json = serde_json::to_string_pretty(&t).unwrap();
        let back: Tree = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
        assert!(back.validate().is_ok());
        assert_eq!(back.metadata.get("owner"), Some(&"team-x".to_string()));
    }

    #[test]
    fn node_identity_is_the_map_key_not_a_redundant_field() {
        // TreeNode carries no id of its own; the same node value under two
        // different keys is legitimately two different (structurally
        // identical) nodes -- this test exists to keep that decision
        // visible rather than silently reintroducing a redundant field.
        let t = Tree::new("t", "1", "A")
            .with_node("A", TreeNode::leaf())
            .with_node("B", TreeNode::leaf());
        assert_eq!(t.nodes.len(), 2);
        assert_eq!(t.nodes["A"], t.nodes["B"]);
    }
}
