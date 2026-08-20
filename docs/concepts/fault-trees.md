# Fault Trees

> Reference: **IEC 61025:2006 — Fault tree analysis (FTA)**

A fault tree is a Boolean model of how **basic events** combine through **gates** into a **top event** — the failure you care about. ETDL implements fault tree analysis so failure probability is an **exact, build-time-resolved constant**, not a runtime estimate.

## Anatomy of a fault tree

```yaml
faultTrees:
  PaymentGatewayFailure:
    topEvent:
      id: PaymentCaptureFailed
      rootCause: GatewayUnavailableOrRejected
    gates:
      GatewayUnavailableOrRejected:
        type: OR
        inputs:
          - GatewayUnreachable
          - ChargeRejected
    basicEvents:
      GatewayUnreachable:
        probability: 0.008
      ChargeRejected:
        failureRate: 0.00021
        missionTime: 24
```

| Field | Meaning |
|---|---|
| `topEvent` | The failure being modeled; `rootCause` names the gate or basic event at the root |
| `gates` | Combinatorial logic nodes (AND, OR, NOT, XOR, VOTING) |
| `basicEvents` | Leaf events with a probability or an exponential failure model |

## Basic events: two probability models

### Direct probability

```yaml
GatewayUnreachable:
  probability: 0.008
```

### Exponential failure model (`failureRate` + `missionTime`)

Per IEC 61025, a constant failure rate λ over a mission time *t* gives:

```
P_failure = 1 − e^(−λ·t)
```

```yaml
ChargeRejected:
  failureRate: 0.00021   # failures per hour
  missionTime: 24        # hours
```

→ `P = 1 − e^(−0.00021·24) ≈ 0.00503`

The compiler computes this at build time and emits:

```rust
const PROCESS_PAYMENT_OPERATION_FAILURE_PROBABILITY: f64 = 0.012987;
```

## Gates

| Gate | Semantics | Formula (independent events) |
|---|---|---|
| `AND` | All inputs fail | `∏ pᵢ` |
| `OR` | At least one input fails | `1 − ∏ (1 − pᵢ)` |
| `NOT` | Complement of single input | `1 − p` |
| `XOR` | Exactly one of two inputs fails | `p₁ + p₂ − 2·p₁·p₂` |
| `VOTING` | At least *k* of *n* inputs fail (`k` field) | binomial / Poisson-binomial tail |

VOTING example — 2-of-3 redundancy for a quorum:

```yaml
QuorumLoss:
  type: VOTING
  k: 2
  inputs:
    - NodeA
    - NodeB
    - NodeC
```

## Validation

The compiler enforces:

- **V-401** — top-event probability computation errors (missing inputs, out-of-range probabilities, missing `failureRate`/`missionTime` pair)
- **V-403** — cycles in the gate graph
- NOT gates require exactly 1 input; XOR requires exactly 2; VOTING requires `1 ≤ k ≤ n`

Gates are resolved in topological order so every input's probability is known.

## Minimal cut sets (MOCUS)

`enumerate_minimal_cut_sets` implements the **MOCUS** algorithm (Method for Obtaining Cut Sets) from IEC 61025: it expands gates bottom-up, replacing OR gates by branching and AND gates by concatenation, then minimizes to the smallest sets of basic events whose joint failure causes the top event.

- Coherent trees only: NOT and XOR gates are rejected (V-40x behavior).
- This is a SHOULD-level tool per the spec; a CLI flag exposing it is on the [roadmap](../README.md#roadmap).

## Linking to event trees

Fault trees plug into event trees via `onFailureProbabilitySource`:

```yaml
ProcessPaymentOperation:
  onFailureProbabilitySource: "#/faultTrees/PaymentGatewayFailure/topEvent"
```

See [Probability Linking](probability-linking.md).
