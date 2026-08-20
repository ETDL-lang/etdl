# Dependency-Aware Reliability Analysis

Reliability Analysis 2.0 makes ETDL reliability analysis engineering-grade by
handling dependent events, common-cause failures, conditional probabilities,
uncertainty, importance, and sensitivity — without breaking the classic
independent fault-tree mathematics.

> **Independence is an assumption, not a mathematical fact.**

> **Common-cause failure can dominate system reliability even when individual
> component failure probabilities are small.**

```
                    ┌─────────────────┐
                    │ Source Discovery│
                    └────────┬────────┘
                             ↓
                    ┌─────────────────┐
                    │ Failure/Ontology│
                    └────────┬────────┘
                             ↓
                    ┌─────────────────┐
                    │ Evidence/Data   │
                    └────────┬────────┘
                             ↓
                    ┌─────────────────┐
                    │ Estimation      │
                    └────────┬────────┘
                             ↓
                    ┌─────────────────┐
                    │ Reliability     │
                    │ Artifact        │
                    └────────┬────────┘
                             ↓
                    ┌─────────────────┐   <-- dependency / CCF analysis lives here
                    │ Dependency /    │
                    │ CCF Analysis    │
                    └────────┬────────┘
                             ↓
                    ┌─────────────────┐
                    │ Fault/Event Tree│
                    └────────┬────────┘
                             ↓
                    ┌─────────────────┐
                    │ Deterministic   │
                    │ Compiler Result │
                    └─────────────────┘
```

## The core engineering problem

The classic fault-tree mathematics assumes independent basic events:

```
P(A AND B) = P(A) · P(B)     (only when A, B independent)
```

That is not always valid. If `ServerAUnavailable` and `ServerBUnavailable` are
both caused by a shared `SharedNetworkFailure`, then in general:

```
P(A AND B) ≠ P(A) · P(B)
```

The system must represent this dependency explicitly and compute it correctly.

## Strict semantic separation

These concepts stay distinct and are never collapsed:

| Concept | Meaning |
|---|---|
| **Event** | an occurrence |
| **Failure mode** | a stable, ontology-aligned failure identity |
| **Basic event** | a leaf in a fault tree |
| **Root / common cause** | a declared cause of multiple failure events |
| **Condition** | the context `P(A \| B)` is conditioned on |
| **Probability** | a number in [0,1] |
| **Dependency** | a declared relationship between events |
| **Uncertainty** | the interval/distribution around an estimate |
| **Evidence** | observations/discovery supporting a claim |
| **Model assumption** | an explicit, inspectable modeling choice |

## Independence must be explicit

`DependencyModel` carries an `independence` field:

```yaml
independence: not-assumed   # or: assumed | unspecified
```

The analysis records the assumption in the result. It never silently assumes
independence when dependencies are declared.

## Common-cause failures (CCF)

A common cause is **declared**, never inferred:

```yaml
common_causes:
  - id: SharedNetworkFailure
    ontology_id: failure.network.unreachable
    probability: 0.0005
    affects: [GatewayTimeout, GatewayUnreachable]
    evidence: ["both correlate with network degradation"]
    source: engineering
    assumptions: ["independent residual per event"]
```

A CCF is traceable to ontology, evidence, source, engineering assumption, and
probability. **Correlation is not causation**: discovery may propose a possible
shared cause, but the reliability model requires engineering confirmation.

### Evaluation: conditioning on common causes

When common causes are declared, the top-event probability is computed by
**conditioning on the joint state of all common-cause atoms**:

```
P(top) = Σ_state  P(state) · P(top | state)
```

For each joint state of the common causes, the affected leaves are set
conditionally (a leaf fails when any of its common causes occurs; otherwise it
fails via its independent residual `(P(leaf) - P(cc)) / (1 - P(cc))`), and the
tree is evaluated under the conditional independence of the residual events.
This is **exact** for the "independent residual OR common cause" model and
never double-counts the common-cause probability.

Limits: at most 20 independent common causes (2²⁰ joint states).

### Beta-factor model

The β-factor model is supported for identical components:

```
λ_total = λ_independent + λ_ccf
λ_ccf = β · λ_total
P(CCF over t) = 1 - exp(-β · λ_total · t)
```

Only mathematically valid formulations are accepted: β ∈ [0,1], finite
non-negative rates and mission time. This is **not** a universal CCF model —
it is one well-defined model. Future models (MGL, shock, alpha-factor,
explicit conditional) can implement the same boundary.

## Conditional probabilities

`P(A | B)` is represented and validated:

```yaml
conditional:
  - event: database.failure
    given: network.degraded
    probability: 0.5
```

Conditions remain part of the estimate identity (an estimate for
`P(failure | high_load)` is never merged with `P(failure)`). Out-of-range
probabilities and unknown condition references are rejected.

## Uncertainty

Each estimate may carry an interval/distribution (confidence vs credible vs
prediction intervals are distinguished). A seeded **Monte Carlo** propagation
samples each leaf from its interval (or point value), evaluates the
dependency-aware top event per sample, and reports the empirical distribution:

```
Uncertainty: 95% interval [1.02e-2, 1.67e-2] (n=10000, seed=42)
```

Monte Carlo is **optional**, never part of deterministic compilation. An
explicit seed makes runs reproducible; seed, sample count, and analyzer version
are recorded in the result.

## Importance and sensitivity

Importance is a *structural contribution* measure (how much the top event
depends on an input). Sensitivity is a *parameter perturbation* (how much the
top event changes when an input's value changes). They are distinct and both
reported.

- **Birnbaum**: `P(top | input=1) − P(top | input=0)`
- **Risk Achievement Worth (RAW)**: `P(top | input=1) / P(top)`
- **Risk Reduction Worth (RRW)**: `P(top) / P(top | input=0)`
- **Sensitivity**: `Δtop` when a leaf moves from a baseline to an alternative.

Common causes appear in the importance list, so the analysis can identify the
top contributing basic event and the top contributing common cause.

## Traceable results

Every analysis produces a versioned artifact
(`etdl.reliability.analysis-result/1.0`):

```
Reliability Analysis
====================
Schema:          etdl.reliability.analysis-result/1.0
Top Event:       GatewayFailure
Model:           fault-tree
Method:          dependency-conditioning
Independence:    NOT ASSUMED
Point Estimate:  3.398459e-3
Uncertainty:     95% interval [...] (n=10000, seed=42)
Common Causes:
  SharedNetworkFailure (P=0.000500) affects ["GatewayTimeout", "GatewayUnreachable"]
Dominant Contributors (Birnbaum):
  [CCF] SharedNetworkFailure = 9.995000e-1
        GatewayTimeout = 9.988000e-1
Assumptions:
  - independence of basic events NOT assumed
  - common cause 'SharedNetworkFailure' declared (P = 0.000500) ...
```

## CLI

Dependency-aware analysis is optional and explicit:

```bash
# Classic independence analysis (unchanged)
etdl analyze service.etdl

# Dependency-aware analysis with a declared model
etdl analyze service.etdl --dependencies deps.yaml

# Add Monte Carlo uncertainty propagation
etdl analyze service.etdl --dependencies deps.yaml --monte-carlo 20000 --seed 7

# Machine-readable
etdl analyze service.etdl --dependencies deps.yaml --json
```

The `--dependencies` file is a `DependencyModel` (YAML or JSON). It is
**data**, never executed.

## Validation

The model is validated before analysis:

- unknown references rejected;
- common cause without affected events rejected;
- CCF probability outside [0,1] rejected (never clamped);
- CCF probability exceeding an affected event's probability rejected;
- conditional probabilities outside [0,1] or referencing unknown nodes rejected;
- duplicate ids rejected;
- dependency cycles rejected;
- orphan common causes rejected.

## The most important rule

> If the model says `A depends on B`, then either calculate it correctly **or**
> reject the model as unsupported. Never silently produce an
> independence-based answer.

## Limitations

- At most 20 independent common causes per evaluation (conditioning is 2^k).
- Monte Carlo intervals are approximate (Normal calibrated to the input
  interval, truncated to [0,1]); they are not exact bounds.
- Only the β-factor CCF model is implemented; other CCF models are future work.
- Conditional probabilities are represented and validated but do not yet
  change gate evaluation semantics (the fault tree uses the declared leaf
  probabilities).
- No full Bayesian network engine.
- Sensitivity sweeps and optimization are not implemented.
