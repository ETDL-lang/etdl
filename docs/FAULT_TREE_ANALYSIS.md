# ETDL Fault Tree Analysis

This document describes the fault-tree engine in the ETDL reference compiler
(`etdl-compiler/src/fault_tree.rs`) and its IEC 61025 alignment. It includes
worked examples and the exact validation rules.

See also `PROBABILITY_SEMANTICS.md` for the formulas; this document focuses on
the fault-tree engine and analysis behavior.

---

## 1. Model

A Fault Tree Object (§5.11) has:

- `topEvent` — the undesired event; its `rootCause` names the gate or basic
  event whose probability equals the top event's.
- `gates` — named AND/OR/NOT/XOR/VOTING/INHIBIT/PRIORITY_AND gates.
- `basicEvents` — named leaves.
- `transfers` — cross-tree references (visualization/navigation).

Gate and basic-event IDs share one namespace and must not collide (V-402).

---

## 2. Resolution algorithm

`resolve_fault_trees(doc, diagnostics) -> BTreeMap<ft_id, f64>`

For each fault tree:

1. Compute each basic event's probability (§PROBABILITY_SEMANTICS 1.1).
2. Topologically sort gates by dependency, in **sorted gate-id order**
   (deterministic). A cycle is reported (V-403).
3. Evaluate gates bottom-up; each gate's inputs must already be resolved.
4. The top event's `rootCause` resolves (gates checked first, then basic
   events) to the tree's probability.

### Worked example (spec §13)

```
PaymentGatewayFailure:
  topEvent.rootCause: GatewayUnavailableOrRejected  (OR)
  GatewayUnreachable: p = 0.008
  ChargeRejected:     λ = 0.00021, t = 24 → 1 − e^(−0.00021·24) ≈ 0.005027
```

```
P = 1 − (1 − 0.008)(1 − 0.005027) ≈ 0.012987
```

The compiler emits:

```rust
// Computed from faultTrees.PaymentGatewayFailure.topEvent at build time (Section 5.16)
const PROCESS_PAYMENT_OPERATION_FAILURE_PROBABILITY: f64 = 0.012987;
```

### Worked example (VOTING 2-of-3)

Three identical units, each `p = 0.1`:

```
P = C(3,2)·0.1²·0.9 + C(3,3)·0.1³ = 3·0.01·0.9 + 0.001 = 0.028
```

### Worked example (PRIORITY_AND)

"A fails, then B fails", `p_A = 0.2, p_B = 0.3`:

```
P = (0.2 · 0.3) / 2! = 0.03
```

(Uniform-ordering assumption; see PROBABILITY_SEMANTICS §2.2.)

---

## 3. Validation rules (fault trees)

| Code | Rule |
|---|---|
| V-401 | `rootCause` or a gate input does not resolve |
| V-402 | a gate and a basic event share an ID |
| V-403 | cycle among gates |
| V-404 | a gate/basic event unreachable from `rootCause` |
| V-501 | gate arity (AND/OR ≥2, NOT =1, XOR =2, VOTING ≥2, INHIBIT =2, PRIORITY_AND ≥2) |
| V-502 | VOTING `1 ≤ k ≤ n` |
| V-503 | basic event supplies both or neither probability/failureRate |
| V-504 | failureRate without missionTime |
| V-505 | INHIBIT without `inhibitCondition` |
| V-506 | transfer target not `#/faultTrees/<id>/…` or references a missing tree |
| W-406 | house event declares a probability/failureRate (boundary condition) |

---

## 4. Minimal cut sets (MOCUS)

`enumerate_minimal_cut_sets(ft) -> Result<Vec<Vec<String>>, String>`

Implements the MOCUS algorithm (ETDL §8.6):

- OR gates: replace the gate row with one row per input (branching).
- AND / INHIBIT / PRIORITY_AND gates: replace the gate with its inputs
  conjoined in one row.
- VOTING k/n: replace with one row per size-k combination of inputs.
- Rows are then minimized (set-subsume).

Constraints:

- Only **coherent** trees (AND/OR/VOTING/INHIBIT/PRIORITY_AND) are supported.
  NOT/XOR gates produce an error (a non-coherent tree has no classical minimal
  cut sets).
- Expansion is capped at `MAX_CUT_SET_ROWS = 100_000` rows; exceeding the cap is
  an error rather than unbounded memory growth.

Cut-set enumeration is a SHOULD-level tool per the spec; it is exposed as a
library function and is not yet wired into the CLI.

---

## 5. Numerical correctness

- `n!` via direct f64 product (n ≤ 170) or log-gamma beyond; no overflow.
- `C(n,k)` via log-factorials; exact, never panics.
- PRIORITY_AND in log space.
- Gate results clamped to `[0,1]`.
- Full determinism (sorted gate order, sorted diagnostic order).

Regression tests in `fault_tree.rs` cover: INHIBIT, PRIORITY_AND (2/3-input,
large-n), VOTING homogeneous vs heterogeneous, `binomial_coeff(70,35)` (overflow
case), `ln_gamma` consistency.

---

## 6. Limitations (documented)

- Independence is assumed; common-cause dependence must be modeled explicitly.
- PRIORITY_AND uses uniform ordering, not a Markov sequence model.
- Cut-set enumeration refuses non-coherent trees.
- `transfers` are navigational; they do not inline the target tree into the
  source tree's probability computation (an explicit `rootCause` link is
  required).
