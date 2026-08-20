# Supplement Traceability Matrix

Companion to [`docs/SPEC_IMPLEMENTATION_MATRIX.md`](../SPEC_IMPLEMENTATION_MATRIX.md),
which covers only the core `etdl-specification` sections (2-11) and predates
every supplement built since. This matrix covers the supplements: each has
no upstream normative section in `etdl-specification` — its own reference
doc in this repository *is* its authority, per how each was designed (see
`docs/reference/standard-library.md`'s "extensibility" framing). Status
legend matches `SPEC_IMPLEMENTATION_MATRIX.md`'s.

## Standard Library / `std.probability`

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| `libraries:` import resolution, qualified-id splicing | IMPLEMENTED + TESTED | `etdl-compiler::stdlib` | `etdl-compiler/tests/` | — |
| `std.*` namespace protected from shadowing | IMPLEMENTED + TESTED | `etdl-compiler::stdlib` | existing | — |
| `Probability` bounded `[0,1]`, rejects (never clamps) invalid values | IMPLEMENTED + TESTED | `etdl-probability-core::probability` | existing + conformance | `LIB-PROB-001` |
| Composition ops (complement, AND/OR, conditional, Bayes) | IMPLEMENTED + TESTED | `etdl-probability-core::probability` | existing + conformance | `LIB-PROB-006`-`009` |
| Distributions (Bernoulli/Binomial/Beta/Exponential/Normal) | IMPLEMENTED + TESTED | `etdl-probability-core::distribution` | existing + conformance | `LIB-PROB-002`-`005` |
| `std.units`/`std.collections` | NOT IMPLEMENTED (documented) | — | — | out of scope, see `standard-library.md` |

## Generic Tree Event Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| One root, resolvable | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-001`, `TREE-004` |
| No cycles | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-002` |
| Valid child references | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-003` |
| Gate arity (AND/OR/NOT/XOR/K_OF_N) | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-005`, `TREE-006` |
| Strict tree (shared nodes rejected, not a DAG) | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-007` |
| Reachability (orphaned nodes rejected) | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-008` |
| Deterministic traversal | IMPLEMENTED + TESTED | `etdl-tree-core::traverse` | existing + conformance | `TREE-009` |
| Stack-safe traversal on deep trees | IMPLEMENTED + TESTED (fixed by this task) | `etdl-tree-core::tree`/`traverse` | conformance | `TREE-010` |
| Zero dependency on Reliability/Probability | IMPLEMENTED + TESTED | `etdl-tree-core/Cargo.toml` | conformance | `ARCH-002` |
| Compiler integration (`x-tree-event`, supplement gating) | IMPLEMENTED + TESTED | `etdl-compiler::tree_event` | existing | — |

## Reliability Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| `0 <= P(E) <= 1` for probability-like metrics | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | existing + conformance | `REL-001` |
| NaN/infinity always rejected | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | existing + conformance | `REL-002` |
| `Unknown` never resolves to a scalar | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | existing + conformance | `REL-003` |
| Rate metrics not bounded to `[0,1]` (non-negative only) | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | conformance | `REL-004` |
| No implicit metric conversion | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | existing + conformance | `REL-005` |
| Artifact round-trip (JSON/YAML), schema check | IMPLEMENTED + TESTED | `etdl-reliability-core::artifact` | existing + conformance | `ART-001`-`003` |
| Malformed artifact rejected, not panicking | IMPLEMENTED + TESTED | `etdl-reliability-core::artifact` | conformance | `ART-004` |
| Identity is event id, not array position | IMPLEMENTED + TESTED | `etdl-reliability-core::artifact` | conformance | `ART-005` |
| Duplicate estimate rejected | IMPLEMENTED + TESTED | `etdl-reliability-core::artifact` | existing + conformance | `ART-006` |

## Predictive Reliability Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| Exponential model: S(t)/h(t)/H(t) vs. independent reference | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | existing + conformance (independent oracle) | `PRED-001`, `PRED-002` |
| Weibull model: S(t)/h(t)/H(t), all shape regimes, vs. independent reference | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | existing + conformance (independent oracle) | `PRED-003` |
| `0 <= S(t) <= 1`, non-increasing | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | conformance | `PRED-004` |
| `0 <= F(t) <= 1` | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | conformance | `PRED-005` |
| `S(t) + F(t) = 1` | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | conformance | `PRED-006` |
| Parameter validity enforced at construction | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | existing + conformance | `PRED-007` |
| Numerical stability near `S(t) -> 0` | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | existing + conformance | `PRED-008` |
| Extrapolation flag (declared vs. undeclared range) | IMPLEMENTED + TESTED | `etdl-reliability::predictive` | existing | — |
| Censoring representation (construction only, no fitting) | IMPLEMENTED + TESTED (fitting explicitly deferred) | `etdl-reliability::predictive::censoring` | existing | — |
| Calibration adapter (read-only, from `ReliabilityArtifact`) | IMPLEMENTED + TESTED | `etdl-reliability::predictive::calibration_adapter` | existing | — |
| Tree integration reuses `tree_adapter` unchanged | IMPLEMENTED + TESTED | `etdl-reliability::predictive::tree` | existing | — |
| Requires Reliability + Probability (one-directional) | IMPLEMENTED + TESTED | `etdl-reliability/Cargo.toml` | conformance | `ARCH-003` |
| `std.reliability` ETDL-source facade | NOT IMPLEMENTED (documented gap) | — | — | recommended future work |

## Runtime Feedback & Calibration 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| Two-sided exact binomial test vs. independent reference | IMPLEMENTED + TESTED | `etdl-reliability::calibration` | existing + conformance (independent oracle) | `CAL-001` |
| Deterministic calibration vector (fixed inputs -> fixed outputs) | IMPLEMENTED + TESTED | `etdl-reliability::calibration` | conformance | `CAL-002` |
| `calibrate()` never mutates the input artifact | IMPLEMENTED + TESTED | `etdl-reliability::calibration` | existing + conformance | `CAL-003` |
| Insufficient-exposure flag | IMPLEMENTED + TESTED | `etdl-reliability::calibration` | existing + conformance | `CAL-004` |
| Full loop (predict -> observe -> calibrate -> new artifact) never mutates the original | IMPLEMENTED + TESTED | `etdl-reliability::predictive` | existing (`predictive_reliability.rs` integration test) | — |

## Cross-cutting: dependency graph / architecture

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| `etdl-probability-core` zero dependency on reliability | IMPLEMENTED + TESTED | `etdl-probability-core/Cargo.toml` | conformance | `ARCH-001` |
| Generic Tree must NOT require Reliability | IMPLEMENTED + TESTED | `etdl-tree-core/Cargo.toml` | conformance | `ARCH-002` |
| Predictive Reliability requires Reliability + Probability | IMPLEMENTED + TESTED | `etdl-reliability/Cargo.toml` | conformance | `ARCH-003` |
| Compiler's reliability dependency is optional | IMPLEMENTED + TESTED | `etdl-compiler/Cargo.toml` | conformance | `ARCH-004` |
| WASM excludes the heavy reliability engine | IMPLEMENTED + TESTED (finding documented, see conformance guide) | `etdl-wasm/Cargo.toml` | conformance | `ARCH-005` |
| Workspace dependency graph is acyclic | IMPLEMENTED + TESTED | whole workspace | conformance | `ARCH-006` |
| CLI's reliability-family dependencies are all optional | IMPLEMENTED + TESTED | `etdl-cli/Cargo.toml` | conformance | `ARCH-007` |
