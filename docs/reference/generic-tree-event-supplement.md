# ETDL Generic Tree Event Supplement 1.0 (`etdl.tree-event`)

Sections marked **NORMATIVE** define required behavior any conforming
implementation must have. Sections marked **INFORMATIVE** are examples,
guidance, and rationale — not requirements.

## 1. Purpose (INFORMATIVE)

A domain-neutral tree-of-events structural model — nodes, logical gates,
validation, traversal, serialization — that reliability, safety, security,
risk, and future engineering domains can each interpret according to their
own semantics, without any of them owning the structure itself. This
supplement defines the structure. It does not define what an `AND` gate
*means* to a consuming domain, does not compute probabilities, and does not
know what a "failure" is.

## 2. Scope (NORMATIVE)

This supplement defines:

- The `Tree`/`TreeNode`/`GateKind` structural model (native layer:
  `etdl-tree-core`, zero dependency on any reliability or probability
  crate).
- How a document declares trees (`x-tree-event`, the same generic `x-*`
  extension mechanism every ETDL extension already uses — **no core
  language or parser change was made or is required**).
- Structural validation (cycles, arity, shared nodes, reachability,
  references) and traversal.
- Serialization.

This supplement does **not** define:

- Probability, failure, hazard, or any other domain semantics (owned by
  consuming domains — reliability's interpretation lives in
  `etdl-reliability::tree_adapter`, purely as a consumer).
- A macro/template/parameterization system.
- Visualization, large-scale graph analysis, or optimization (see
  §17, Built-in vs. optional).

## 3. Terminology (NORMATIVE)

| Term | Meaning |
|---|---|
| Tree | A [`Tree`], identified by `(id, version)`, with exactly one root and zero or more nodes |
| Node | A [`TreeNode`], identified by its key in `Tree::nodes` (never a separate field, never array position) |
| Leaf | A node with no children (`NodeKind::Leaf`) |
| Gate | A node combining children through a [`GateKind`] |
| Root | The single node every other node in the tree is reachable from |
| Domain interpretation | What a *consumer* (reliability, safety, ...) decides a tree/node/gate means — never defined by this supplement |

`[`Type`]` markers refer to `etdl-tree-core` Rust types (`node.rs`,
`tree.rs`).

## 4. Tree model (NORMATIVE)

```rust
pub struct Tree {
    pub schema: String,       // "etdl.tree-event/1.0"
    pub id: String,
    pub version: String,
    pub root: String,         // a node id
    pub nodes: BTreeMap<String, TreeNode>,
    pub description: Option<String>,
    pub metadata: BTreeMap<String, String>,
}
```

**A tree, not a DAG (NORMATIVE for 1.0).** Every non-root node has exactly
one parent. A node referenced as a child by more than one gate is a
validation error (`TreeError::SharedNode`), never silently accepted as a
shared/DAG node. A future version could introduce an explicit Tree/DAG
distinction (a `shared: true` marker, say); 1.0 does not, and does not
pretend to.

## 5. Node model (NORMATIVE)

```rust
pub struct TreeNode {
    pub kind: NodeKind,        // Leaf { event_ref } | Gate { gate, children }
    pub description: Option<String>,
    pub status: Option<NodeStatus>,   // Discovered | Candidate | Accepted | Rejected
    pub metadata: BTreeMap<String, String>,
}
```

A `Leaf`'s `event_ref` is a stable, opaque reference string (e.g. a
`std.events` qualified id) — **this supplement never resolves or
interprets it.** `NodeStatus` exists for discovered-event integration (§13)
and is independently defined here, not shared with
`etdl-reliability-ontology::FailureStatus` — see §16 (Ontology).

## 6. Root (NORMATIVE)

`Tree::root` names exactly one node id, which must exist in `Tree::nodes`.
This supplement calls it "root," never "top event" — that framing, where
meaningful, belongs to a consuming domain (reliability may choose to call
its root a "top event"; this supplement does not use that term anywhere in
`etdl-tree-core`).

## 7. Child relationships (NORMATIVE)

A gate's `children: Vec<String>` names other node ids by reference (never
inline nested structure) — every child must exist in `Tree::nodes`
(`TreeError::MissingChild` otherwise).

## 8. Gates (NORMATIVE)

| `GateKind` | Arity | Meaning (structural, not probabilistic) |
|---|---|---|
| `And` | >= 2 | All children |
| `Or` | >= 2 | At least one child |
| `Not` | exactly 1 | The negation of one child |
| `Xor` | exactly 2 | Exactly one of two children |
| `KOfN(k)` | >= 1, `1 <= k <= n` | At least `k` of `n` children |

These reuse the same boolean vocabulary ETDL's native fault-tree
`GateType` and `std.logic` already use (§16) — this supplement does not
redefine AND/OR/NOT/XOR, it reuses the same names for the same logical
meaning, applied to a different (tree, not fault-tree-basic-event)
structural context. NAND/NOR and other gates are deliberately not added in
1.0 (§9's "do not add every possible Boolean gate").

## 9. Validation (NORMATIVE)

[`Tree::validate`] checks, collecting every problem in one pass (never
stopping at the first, never silently repairing):

1. Non-empty (`TreeError::EmptyTree`).
2. A non-empty, resolvable root (`MissingRoot`, `UnknownRoot`).
3. Every gate has correct arity for its kind (`InvalidArity`,
   `InvalidKOfN`).
4. Every referenced child exists (`MissingChild`).
5. Every non-root node has exactly one parent (`SharedNode`).
6. Every node is reachable from root (`OrphanedNode`).
7. No cycles (`Cycle`) — via depth-first search with an explicit
   currently-on-path stack; a node reappearing on that stack is the cycle,
   reported with the full chain, never an infinite recursion or stack
   overflow.

## 10. Identity (NORMATIVE)

Node identity is the `BTreeMap` key `Tree::nodes` stores it under —
**never** array position, never a separate redundant `id` field on the
node value (matching this workspace's existing convention for exactly this
kind of map, e.g. `FaultTree::basic_events`). Tree identity is `(id,
version)`. Both remain stable under reordering, serialization, and
deserialization.

## 11. Traversal (NORMATIVE)

`children`, `leaves`, `ancestors`, `descendants`, `depth`, `preorder`,
`postorder` — see `traverse.rs`. All deterministic (sorted where order
isn't structurally implied, e.g. `leaves()`) and assume a validated tree
(cycles could make several of these loop; validation is a separate,
explicit precondition, per §12).

## 12. Structure, evaluation, and domain interpretation are separate (NORMATIVE)

This is the central discipline this supplement exists to enforce:

```text
Tree structure          <- etdl-tree-core (this supplement)
       |
Tree evaluation          <- a consumer's pure function over Tree + its own inputs
       |
Domain interpretation    <- what the evaluated result MEANS to that domain
```

`etdl-tree-core` provides only the first. It never computes a probability,
never assumes independence or dependence, and never decides whether an
`AND` node represents a failure, a hazard, or a mere logical fact. See
§14 (Reliability integration) for how one consumer applies the second and
third layers on top, without this supplement knowing.

## 13. Discovered events (INFORMATIVE)

A node's `status: Discovered | Candidate | Accepted | Rejected` lets a
consumer represent the existing discovery lifecycle
(`etdl-failure-discovery` -> candidate -> engineering review -> accepted)
directly as tree-node metadata, without this supplement needing to know
what "discovery" means or automatically promoting a discovered node to
accepted — that promotion remains an explicit engineering decision made
elsewhere, exactly as it already is for the existing reliability discovery
pipeline (unchanged by this supplement).

## 14. Reliability integration (NORMATIVE for the adapter's existence; the adapter's specific formulas are INFORMATIVE)

`etdl-reliability::tree_adapter::evaluate_assuming_independence` is *one*
reliability interpretation: given a `&Tree` and caller-supplied leaf
probabilities (`std.probability::Probability`, never inferred or
defaulted), it combines them per gate under an **explicit** independence
assumption (`AND`/`OR`/`K_OF_N` via `std.probability`'s composition
functions; `NOT` via `complement`; `XOR` via the standard two-event
exclusive-or formula). `etdl-tree-core` has no dependency on
`etdl-reliability` or `etdl-probability-core` — confirmed structurally by
`etdl-tree-core/Cargo.toml`, not merely documented. See
`etdl-reliability/tests/tree_integration.rs` for the full chain: `Tree` ->
`tree_adapter` -> `std.probability` -> the *existing*, unmodified
`ReliabilityArtifact`/`ArtifactResolver`.

A tree with genuine dependence/common-cause structure should be analyzed
with the existing, unmodified `etdl-reliability::analysis::dependence`
machinery instead — `tree_adapter`'s independence-only evaluation exists to
demonstrate the generic-tree-to-reliability layering, not to replace
dependency-aware analysis.

## 15. Serialization (NORMATIVE)

Every public type (`Tree`, `TreeNode`, `NodeKind`, `GateKind`,
`NodeStatus`) implements `serde::Serialize`/`Deserialize`. No type
serializes executable code — every serialized value is plain structural
data.

## 16. Ontology (NORMATIVE)

| Concept | Classification | Reason |
|---|---|---|
| `Event`, `Condition`, `Cause`, `Consequence` (reliability ontology, `etdl-reliability-ontology`) | **UNCHANGED** | Not touched by this supplement; `etdl-tree-core` does not depend on `etdl-reliability-ontology` |
| AND/OR/NOT/XOR (as logical concepts) | **UNCHANGED** | Already owned by ETDL's native fault-tree `GateType` and `std.logic`; this supplement reuses the same names/meanings for a different structural context, never redefining them |
| `Tree`, `TreeNode`, `GateKind`, `NodeStatus` | **NEW** | Genuinely new generic concepts; could not be expressed with existing types without conflating tree structure with fault-tree-specific `BasicEvent` (which carries `probability`/`failure_rate`/`mission_time` — reliability-flavored fields a domain-neutral tree node must not have) |
| `FailureMode`, `ReliabilityArtifact`, `Calibration` (reliability domain) | **UNCHANGED** | Not moved, not duplicated, not referenced by `etdl-tree-core` |

No ontology concept was duplicated; the only additions are the four
generic tree concepts, none of which existed under another name.

## 17. Built-in vs. optional (NORMATIVE)

| | Built-in (`etdl-tree-core` + `etdl-compiler::tree_event`, this task) | Optional (future, not implemented) |
|---|---|---|
| Scope | Node/tree representation, structural validation, basic traversal, the five gates, serialization | Visualization, large-scale graph analysis, specialized optimization, advanced tree transformations |
| Registration | `etdl.tree-event` registered **unconditionally** in `builtin_registry()` (not behind the `reliability` Cargo feature) — domain-neutral infrastructure, not an optional reliability feature | N/A |

## 18. Versioning (NORMATIVE)

| Axis | Value |
|---|---|
| Tree schema | `etdl.tree-event/1.0` (`TREE_SCHEMA`) |
| Supplement id/version (as declared via `supplements:`) | `etdl.tree-event` / `"1.0"`, checked by the same major-version-gate rule (E-106/E-107-equivalent) every supplement already uses |
| `etdl-tree-core` crate version | Cargo semver, independent of the schema string above |

Future minor versions must remain able to read a 1.0 tree; a future major
version bump is required only for a change that would misinterpret 1.0
structure.

## 19. Compatibility (NORMATIVE)

Adding this supplement changes nothing about how an existing `.etdl`
document without `x-tree-event`/`supplements: [{id: etdl.tree-event}]`
compiles, validates, or analyzes — `parse_and_validate_trees` returns
immediately with no diagnostics when the supplement isn't declared (the
same "only processed when explicitly declared" discipline the reliability
supplement already follows). Existing `ReliabilityArtifact`s, calibration,
and runtime observation are untouched; no existing reliability source file
was modified to add this supplement.

## 20. Diagnostics (NORMATIVE)

| Code | Meaning |
|---|---|
| `E-120` | `x-tree-event` manifest is invalid (missing `trees`, or malformed YAML) |
| `E-121` | A declared tree failed structural validation (wraps any `TreeError`: missing root, cycle, missing node, invalid gate/arity, shared node, orphaned node) |
| `E-122` | Two trees in the same document declare the same `id` |

Extends the existing `E-1xx` structural-diagnostic family (the same family
`asyncapi_imports`, `supplements`, and `libraries:` already use) rather
than inventing a new prefix.

## 21. CLI (INFORMATIVE)

```bash
etdl tree validate <file.etdl>
etdl tree inspect <file.etdl>
```

Both require `supplements: [{id: etdl.tree-event, version: "1.0"}]` to be
declared in the document (§19). `etdl validate`/`etdl compile` already
surface the same `E-120`/`E-121`/`E-122` diagnostics automatically —
`etdl tree validate`/`inspect` exist for a tree-focused view, not a second
validation path.
