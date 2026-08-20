//! [`TreeNode`]: a generic, domain-neutral node in a [`crate::Tree`].
//!
//! A node is either a **leaf** (no children, optionally referencing an
//! external event by a stable id — this crate never interprets what that
//! reference means) or a **gate** (combines its children through a
//! [`GateKind`]). Nothing here assumes failure, success, or any particular
//! domain meaning — see the crate-level docs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What a node represents structurally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NodeKind {
    /// A leaf: no children. `event_ref`, when present, is a stable
    /// reference to an externally-defined event (e.g. a
    /// `std.events`-qualified id, or any other identifier meaningful to a
    /// consumer) — this crate does not resolve or interpret it.
    Leaf {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_ref: Option<String>,
    },
    /// An intermediate node: combines `children` through `gate`.
    Gate { gate: GateKind, children: Vec<String> },
}

/// A generic logical combinator. Reuses the same boolean vocabulary
/// `std.logic`/ETDL's native fault-tree `GateType` already use — this is a
/// structural label, not a probability computation (see the crate-level
/// docs' "structure vs. evaluation" separation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum GateKind {
    And,
    Or,
    Not,
    Xor,
    /// At least `k` of the node's children — "k-of-n", `n` being the
    /// number of children. Not called `Voting` (that framing belongs to a
    /// consuming domain, e.g. reliability); this is the generic logical
    /// threshold gate.
    #[serde(rename = "K_OF_N")]
    KOfN(u32),
}

impl GateKind {
    pub fn label(self) -> &'static str {
        match self {
            GateKind::And => "AND",
            GateKind::Or => "OR",
            GateKind::Not => "NOT",
            GateKind::Xor => "XOR",
            GateKind::KOfN(_) => "K_OF_N",
        }
    }
}

/// A generic engineering-review lifecycle status, independently defined
/// here (not shared with `etdl-reliability-ontology::FailureStatus`, which
/// this crate must not depend on — see the crate-level docs' dependency
/// direction). Optional: most hand-written tree nodes carry no status at
/// all; this exists primarily for discovered-event integration (a
/// discovered candidate event becoming a tree node before engineering
/// review accepts it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Discovered,
    Candidate,
    Accepted,
    Rejected,
}

/// A node in a [`crate::Tree`]. Identity is the `BTreeMap` key
/// `Tree::nodes` stores it under — never array position or display label —
/// required for provenance, artifacts, analysis, and external references
/// to remain stable across reordering. `TreeNode` itself carries no
/// separate `id` field, matching this workspace's existing convention for
/// exactly this kind of map (e.g. `FaultTree::basic_events`/`Gate` — the
/// map key is the sole identity, never duplicated onto the value).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeNode {
    #[serde(flatten)]
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<NodeStatus>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl TreeNode {
    /// A leaf node with no external reference.
    pub fn leaf() -> Self {
        TreeNode {
            kind: NodeKind::Leaf { event_ref: None },
            description: None,
            status: None,
            metadata: BTreeMap::new(),
        }
    }

    /// A leaf node referencing an external event by id.
    pub fn leaf_referencing(event_ref: impl Into<String>) -> Self {
        TreeNode {
            kind: NodeKind::Leaf {
                event_ref: Some(event_ref.into()),
            },
            description: None,
            status: None,
            metadata: BTreeMap::new(),
        }
    }

    /// A gate node.
    pub fn gate(gate: GateKind, children: Vec<String>) -> Self {
        TreeNode {
            kind: NodeKind::Gate { gate, children },
            description: None,
            status: None,
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_status(mut self, status: NodeStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// This node's children, if it is a gate; empty for a leaf.
    pub fn children(&self) -> &[String] {
        match &self.kind {
            NodeKind::Gate { children, .. } => children,
            NodeKind::Leaf { .. } => &[],
        }
    }

    pub fn is_leaf(&self) -> bool {
        matches!(self.kind, NodeKind::Leaf { .. })
    }
}
