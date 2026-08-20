//! Traversal queries over a validated [`Tree`]: `children`, `leaves`,
//! `ancestors`, `descendants`, `depth`, `preorder`, `postorder`.
//!
//! These assume `tree` already passed [`Tree::validate`] — cycles could
//! make several of these queries loop forever. That is a precondition, not
//! re-checked here (validation is a separate, explicit step, per the
//! crate's structure/evaluation separation), except where noted.

use std::collections::BTreeMap;

use crate::node::NodeKind;
use crate::tree::Tree;

impl Tree {
    /// The ids of `node_id`'s children, empty for a leaf or an unknown id.
    pub fn children(&self, node_id: &str) -> &[String] {
        self.nodes.get(node_id).map(|n| n.children()).unwrap_or(&[])
    }

    /// Every leaf node's id, in deterministic (sorted) order.
    pub fn leaves(&self) -> Vec<&str> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.is_leaf())
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// A map from child id to parent id, built once from every gate's
    /// declared children. In a validated (non-shared-node) tree this is a
    /// true one-to-one parent map; on an unvalidated tree with shared
    /// nodes, the last-encountered parent wins (deterministic by
    /// `BTreeMap` iteration order, but not meaningful — validate first).
    fn parent_map(&self) -> BTreeMap<&str, &str> {
        let mut parents = BTreeMap::new();
        for (id, node) in &self.nodes {
            if let NodeKind::Gate { children, .. } = &node.kind {
                for child in children {
                    parents.insert(child.as_str(), id.as_str());
                }
            }
        }
        parents
    }

    /// `node_id`'s ancestors, nearest first, root last. Empty for the root
    /// itself or an unknown id.
    pub fn ancestors(&self, node_id: &str) -> Vec<&str> {
        let parents = self.parent_map();
        let mut out = Vec::new();
        let mut current = node_id;
        while let Some(&parent) = parents.get(current) {
            out.push(parent);
            current = parent;
        }
        out
    }

    /// `node_id`'s depth: 0 for the root, 1 for the root's direct
    /// children, and so on. `None` for an unknown id or a node not
    /// reachable from the root via ancestry.
    pub fn depth(&self, node_id: &str) -> Option<usize> {
        if !self.nodes.contains_key(node_id) {
            return None;
        }
        if node_id == self.root {
            return Some(0);
        }
        Some(self.ancestors(node_id).len())
    }

    /// Every descendant of `node_id` (children, grandchildren, ...), in
    /// preorder. Iterative (explicit stack), not recursive — a recursive
    /// version overflowed the process stack on a deep-but-valid tree; see
    /// `check_reachability_and_cycles`'s doc comment in `tree.rs` for the
    /// same fix applied there, and `etdl-conformance`'s `TREE-010` vector.
    pub fn descendants(&self, node_id: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut stack: Vec<&str> = self
            .children(node_id)
            .iter()
            .rev()
            .map(String::as_str)
            .collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            for child in self.children(id).iter().rev() {
                stack.push(child.as_str());
            }
        }
        out
    }

    /// A preorder walk from the root: each node before its children.
    pub fn preorder(&self) -> Vec<&str> {
        let mut out = vec![self.root.as_str()];
        out.extend(self.descendants(&self.root));
        out
    }

    /// A postorder walk from the root: each node after its children.
    /// Iterative two-stack technique (not recursive, for the same
    /// stack-safety reason as `descendants` above): push in preorder-like
    /// fashion onto `pending`, recording visit order onto `finished`, then
    /// drain `finished` in reverse — this produces exactly the order the
    /// original recursive "children before node" version did, node for
    /// node, verified by this crate's existing postorder tests.
    pub fn postorder(&self) -> Vec<&str> {
        let mut finished: Vec<&str> = Vec::new();
        let mut pending: Vec<&str> = vec![self.root.as_str()];
        while let Some(id) = pending.pop() {
            finished.push(id);
            for child in self.children(id) {
                pending.push(child.as_str());
            }
        }
        finished.reverse();
        finished
    }
}

#[cfg(test)]
mod tests {
    use crate::node::{GateKind, TreeNode};
    use crate::tree::Tree;

    // Top = A AND (B OR C)
    fn sample() -> Tree {
        Tree::new("t", "1", "Top")
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
            )
    }

    #[test]
    fn children_of_root() {
        let t = sample();
        assert_eq!(t.children("Top"), &["A".to_string(), "Inner".to_string()]);
        assert!(t.children("A").is_empty());
    }

    #[test]
    fn leaves_are_sorted_and_exclude_gates() {
        let t = sample();
        assert_eq!(t.leaves(), vec!["A", "B", "C"]);
    }

    #[test]
    fn ancestors_nearest_first_root_last() {
        let t = sample();
        assert_eq!(t.ancestors("B"), vec!["Inner", "Top"]);
        assert_eq!(t.ancestors("Top"), Vec::<&str>::new());
    }

    #[test]
    fn depth_matches_ancestor_chain_length() {
        let t = sample();
        assert_eq!(t.depth("Top"), Some(0));
        assert_eq!(t.depth("A"), Some(1));
        assert_eq!(t.depth("Inner"), Some(1));
        assert_eq!(t.depth("B"), Some(2));
        assert_eq!(t.depth("nonexistent"), None);
    }

    #[test]
    fn descendants_of_root_is_everything_but_root() {
        let t = sample();
        let mut d = t.descendants("Top");
        d.sort();
        let mut expected = vec!["A", "B", "C", "Inner"];
        expected.sort();
        assert_eq!(d, expected);
    }

    #[test]
    fn preorder_visits_node_before_children() {
        let t = sample();
        let order = t.preorder();
        assert_eq!(order[0], "Top");
        let top_idx = order.iter().position(|&n| n == "Top").unwrap();
        let inner_idx = order.iter().position(|&n| n == "Inner").unwrap();
        let b_idx = order.iter().position(|&n| n == "B").unwrap();
        assert!(top_idx < inner_idx);
        assert!(inner_idx < b_idx);
    }

    #[test]
    fn postorder_visits_children_before_node() {
        let t = sample();
        let order = t.postorder();
        assert_eq!(order.last(), Some(&"Top"));
        let inner_idx = order.iter().position(|&n| n == "Inner").unwrap();
        let b_idx = order.iter().position(|&n| n == "B").unwrap();
        let c_idx = order.iter().position(|&n| n == "C").unwrap();
        let top_idx = order.iter().position(|&n| n == "Top").unwrap();
        assert!(b_idx < inner_idx);
        assert!(c_idx < inner_idx);
        assert!(inner_idx < top_idx);
    }

    #[test]
    fn preorder_and_postorder_visit_every_node_exactly_once() {
        let t = sample();
        let mut pre = t.preorder();
        let mut post = t.postorder();
        pre.sort();
        post.sort();
        assert_eq!(pre.len(), 5);
        assert_eq!(pre, post);
    }
}
