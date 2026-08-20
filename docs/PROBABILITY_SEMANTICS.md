# ETDL Probability Semantics

This document defines, precisely and deterministically, how ETDL computes every
probability value. It exists so that the answer to "why did ETDL produce this
probability?" is a documented, deterministic chain of reasoning.

It mirrors ETDL Specification §5.16 and adds the numerical and stability rules
the reference implementation (`etdl-compiler/src/fault_tree.rs`) follows.

---

## 1. Sources of probability

### 1.1 Basic Event
A basic event supplies **exactly one** of:

- `probability` (`p`, `0 ≤ p ≤ 1`) — used directly.
- `failureRate` (`λ ≥ 0`) **with** `missionTime` (`t > 0`) — converted with the
  constant-failure-rate (exponential) model:

```
p = 1 − e^(−λ·t)
```

Supplying both, or neither, is a validation error `V-503`; `failureRate`
without `missionTime` is `V-504`.

### 1.2 Branch
A branch's effective probability is resolved by `Branch::effective_probability`
with this precedence:

1. `probabilitySource` (a fault-tree top event) — resolved at build time to the
   fault tree's computed top-event probability. When present, it is
   authoritative (ETDL §5.15).
2. `probability`
3. `probabilityOfSuccess`
4. `probabilityOfFailure`

A branch must declare at least one; the fault-tree-derived value participates in
the barrier's probability sum (V-203).

---

## 2. Gate formulas

All inputs are assumed **statistically independent** (ETDL §1.4). A document that
needs common-cause dependence must model it explicitly with a shared basic event
or intermediate gate; ETDL does not detect it automatically.

| Gate | Meaning | Formula |
|---|---|---|
| AND | all inputs occur | `∏ pᵢ` |
| OR | at least one input occurs | `1 − ∏ (1 − pᵢ)` |
| NOT | complement of one input | `1 − p` (exactly 1 input) |
| XOR | exactly one of two inputs | `p₁ + p₂ − 2·p₁·p₂` (exactly 2 inputs) |
| VOTING k/n | at least k of n inputs occur | see below |
| INHIBIT | primary AND conditioning | `p₁·p₂` (exactly 2 inputs; conditioning labelled by `inhibitCondition`) |
| PRIORITY_AND | all n inputs in listed order | `(∏ pᵢ) / n!` (uniform-ordering assumption) |

### 2.1 VOTING

For all inputs equal (`pᵢ = p` for all i):

```
P = Σ_{j=k}^{n} C(n,j) · pʲ · (1−p)ⁿ⁻ʲ
```

For heterogeneous probabilities, the probability-generating function is used:

```
P = [coefficients of xᵏ … xⁿ] in  ∏ᵢ (1 − pᵢ + pᵢ·x)
```

i.e. multiply the polynomials `(1−pᵢ + pᵢ·x)` in input order and sum the
coefficients of degree k..n. `k` must satisfy `1 ≤ k ≤ n` (V-502).

### 2.2 PRIORITY_AND ordering assumption

The specification permits either the default uniform-ordering approximation or a
sequence-dependent (Markov) analysis when failure rates are available. The
reference implementation uses the **uniform-ordering approximation**:
`P = (∏ pᵢ) / n!`, assuming every ordering of the n inputs is equally likely.
This assumption is explicit and documented; documents requiring exact
sequence-dependent analysis must not rely on it.

---

## 3. Top event

The top event's probability equals the resolved probability of its `rootCause`
(gate or basic event). Gates are evaluated bottom-up in topological order; a
gate's inputs must be fully resolved before the gate is computed (ETDL §5.16).

---

## 4. Numerical implementation

### 4.1 Floating point
All arithmetic is IEEE-754 `f64`. The computation order is deterministic given
the same input: inputs are collected in document (BTreeMap) order and gates are
topologically sorted in **sorted gate-id order** (deterministic).

### 4.2 Factorials and binomial coefficients (no overflow)
- `n!` is computed as a direct `f64` product for `n ≤ 170`; beyond that the
  log-gamma (Lanczos) approximation is used. This avoids u64/f64 overflow.
- `C(n,k)` is computed via `exp(ln n! − ln k! − ln (n−k)!)` rounded to the
  nearest integer. This is exact for all `n` where the result fits `f64` and
  never panics.

### 4.3 PRIORITY_AND in log space
`(∏ pᵢ)/n!` is computed as `exp(Σ ln pᵢ − ln n!)` to avoid overflow. An input of
exactly `0` yields `0` immediately (the whole product is 0). All `pᵢ` are
clamped to `[0,1]` before the log.

### 4.4 Range and clamping
- Branch probabilities must lie in `[0,1]`; a violation is `V-203`.
- Gate results are clamped to `[0,1]` as a final guard against accumulated
  floating-point error (which can otherwise produce `1.0000000000000002`).

### 4.5 Rounding in generated code
Generated Rust embeds resolved probabilities as literals with 6 decimal places
(`{:.6}`, e.g. `0.012987`). The **computed** value used for V-203 and W-402
comparisons is the full-precision value; only the emitted constant is rounded.
The rounding is deterministic.

---

## 5. Determinism guarantees

Given identical input, `resolve_fault_trees` returns an identical map, and the
ordering of multi-gate evaluation is identical, because:

- all document maps are `BTreeMap` (sorted by key),
- the topological-sort queue is seeded in sorted gate-id order,
- V-404 diagnostics are emitted in sorted id order.

The only intentionally non-deterministic runtime behavior anywhere is telemetry
timestamps and (optionally) retry jitter, neither of which affects any computed
probability.

---

## 6. Edge cases

| Case | Behavior |
|---|---|
| OR with a 1.0 input | `1 − ∏(1−pᵢ)` = 1.0 (one guaranteed input) |
| AND with a 0.0 input | 0.0 (deterministic cut) |
| NOT with p = 1.0 | 0.0 |
| VOTING k = n | equals AND |
| VOTING k = 1 | equals OR |
| VOTING with n = 1 | V-501 (needs ≥ 2 inputs) |
| PRIORITY_AND with a 0 input | 0.0 |
| Empty input list | rejected (V-501 gate arity) |

---

## 7. Validation rules that guard probability semantics

| Code | Rule |
|---|---|
| V-203 | branch probability range [0,1]; sibling sum = 1.0 ± 0.0001 |
| V-401 | a fault tree's probability cannot be computed (unresolved input, out-of-range) |
| V-502 | VOTING `1 ≤ k ≤ n` |
| V-503 | basic event supplies exactly one of probability/failureRate |
| V-504 | failureRate requires missionTime |
| W-402 | cached branch probability drifted from the freshly computed fault-tree value |

---

## 8. Traceability

For any emitted probability constant `X`, the chain of reasoning is:

1. Locate the fault tree named by the `onFailureProbabilitySource` pointer.
2. Read each reachable basic event's `probability` or compute `1−e^(−λt)`.
3. Combine via the gate formulas in §2 in topologically sorted order.
4. The top event's `rootCause` value is the tree's probability.
5. `X` = that value rounded to 6 decimals, emitted with a provenance comment
   naming the fault tree and section.
