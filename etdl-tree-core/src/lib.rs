//! ETDL Generic Tree Event Supplement 1.0 — native layer.
//!
//! A domain-neutral tree-of-events structural model: nodes, gates,
//! validation, traversal, serialization. **This crate has no dependency on
//! any reliability or probability crate** — that is a structural fact
//! checkable in `Cargo.toml`, not merely a convention. Reliability, safety,
//! security, and future domains consume this crate; it must never depend
//! on them.
//!
//! ```text
//! ETDL Core
//!    |
//! ETDL Standard Library (std.events, std.logic, std.probability)
//!    |
//! Generic Tree Event Supplement    <- this crate
//!    |
//!    +-- Reliability  (interprets AND/OR/K_OF_N per fault-tree semantics,
//!    |                 evaluates probabilities via std.probability)
//!    +-- Safety        (not implemented; same tree, different meaning)
//!    +-- Security       (not implemented; same tree, different meaning)
//! ```
//!
//! # A `TreeEvent` is not automatically a `Failure`
//!
//! A [`node::TreeNode`] represents structure only — an event, condition,
//! cause, consequence, or intermediate result, depending entirely on how a
//! *consumer* interprets it. Nothing in this crate has a notion of
//! probability, failure, hazard, or risk. `NodeKind::Leaf` may reference an
//! external event by a stable id (e.g. a `std.events` qualified id) but
//! never resolves or interprets what that id means.
//!
//! # Structure, evaluation, and domain interpretation are three separate
//! things
//!
//! This crate provides only the first: [`Tree`]/[`node::TreeNode`]
//! structure, [`Tree::validate`] (cycles, arity, shared nodes,
//! reachability), and [`traverse`] queries (children, leaves, ancestors,
//! descendants, depth, preorder, postorder). It does **not** evaluate a
//! tree numerically — an `AND` node here is a structural label, not
//! `P(A)*P(B)`. A consuming domain library evaluates a tree using
//! `std.probability`'s composition operations (which themselves require
//! an *explicit* independence/mutual-exclusivity choice — never inferred
//! from tree structure) or its own domain-specific analysis. See
//! `etdl-reliability/tests/tree_integration.rs` for a worked example of
//! exactly that separation.
//!
//! # 1.0 is a tree, not a DAG
//!
//! Every non-root node has exactly one parent; a node referenced by more
//! than one gate is rejected ([`tree::TreeError::SharedNode`]), not
//! silently treated as a shared/DAG node. See [`tree`]'s module docs.

pub mod node;
pub mod traverse;
pub mod tree;

pub use node::{GateKind, NodeKind, NodeStatus, TreeNode};
pub use tree::{Tree, TreeError, TREE_SCHEMA};
