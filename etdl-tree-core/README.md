# etdl-tree-core

[![Crates.io](https://img.shields.io/crates/v/etdl-tree-core.svg)](https://crates.io/crates/etdl-tree-core)
[![Docs.rs](https://img.shields.io/docsrs/etdl-tree-core)](https://docs.rs/etdl-tree-core)

**The [ETDL](https://github.com/ETDL-lang/etdl) Generic Tree Event Supplement 1.0 native layer** — a domain-neutral tree-of-events structural model (nodes, gates, validation, traversal, serialization) with **zero dependency on any reliability or probability crate**, enforced by this crate's own `Cargo.toml`, not merely convention.

## Where this sits

```
ETDL Core
   |
ETDL Standard Library (std.events, std.logic, std.probability)
   |
Generic Tree Event Supplement    <- this crate
   |
   +-- Reliability  (interprets AND/OR/K_OF_N per fault-tree semantics,
   |                 evaluates probabilities via std.probability)
   +-- Safety        (not implemented; same tree, different meaning)
   +-- Security       (not implemented; same tree, different meaning)
```

Reliability, safety, security, and any future domain consume this crate for tree *structure* — they never flow the other way.

## A `TreeEvent` is not automatically a `Failure`

A `node::TreeNode` represents structure only: an event, condition, cause, consequence, or intermediate result, depending entirely on how a *consumer* interprets it. Nothing here has a notion of probability, failure, hazard, or risk — `NodeKind::Leaf` may reference an external event by a stable id (e.g. a `std.events` qualified id) but never resolves or interprets what that id means.

## What it provides

- **`Tree` / `node::TreeNode`** — the structural model itself.
- **`Tree::validate`** — cycle detection, gate arity, shared-node/reachability checks.
- **`traverse`** — children, leaves, ancestors, descendants, depth, preorder/postorder queries.

It does **not** evaluate a tree numerically — an `AND` node here is a structural label, not `P(A)*P(B)`. That evaluation belongs to a consuming domain library (e.g. [ETDL's fault-tree resolution](https://crates.io/crates/etdl-compiler), which interprets the same AND/OR/K-of-N structure per IEC 61025 semantics).

Full standard-library architecture: [`docs/reference`](https://github.com/ETDL-lang/etdl/tree/main/docs/reference) in the main repo.

## License

Apache-2.0
