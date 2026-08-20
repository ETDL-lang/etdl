# ETDL Predictive Reliability Supplement 1.0

This document specifies Predictive Reliability: the layer that answers "what
is the expected probability/reliability under specified conditions, over a
specified future time or exposure interval?" — as distinct from an
**estimate** (inference about a quantity, no time horizon) and an
**observation** (a record of what already happened).

## Where this sits

```text
ETDL Core
   |
std.probability (etdl-probability-core)
   |
[std.reliability facade — not yet built as a standalone crate; the existing
 etdl-reliability crate plays this role today. See "Known gap" below.]
   |
Predictive Reliability      <- this document; native layer =
   |                           etdl-reliability::predictive
ReliabilityArtifact + predictive metadata layered on top of it
```

```text
Generic Tree Event (etdl-tree-core)
   |
Reliability interpretation (etdl-reliability::tree_adapter, UNCHANGED)
   |
Predictive Reliability (etdl-reliability::predictive::tree — reuses
   tree_adapter as-is; adds no new tree-composition logic)
```

```text
Runtime Observation -> Calibration -> New Artifact -> Future Prediction
```

The last diagram is the discipline this module is built to preserve:
`etdl-reliability::predictive` **reads** calibrated artifacts
(`calibration_adapter`) and **never** touches
`etdl-reliability::calibration`, `dataset`, or the observation pipeline.
Calibration always produces a *new* artifact (unchanged, existing
machinery — see `docs/reliability/runtime-feedback-calibration.md`); this
module's only relationship to that artifact is to consume it for the next
prediction, on request, never automatically.

**Known gap, stated honestly:** a standalone `std.reliability` ETDL-source
facade crate was never built (deprioritized when the Generic Tree Event
Supplement task took priority). Predictive Reliability is built directly on
top of the existing `etdl-reliability` engine, which plays that role today.
Building the facade remains recommended future work; nothing in this module
would need to change when it lands — it would simply give the ETDL-source
layer a name to import.

## Why most of this is a Rust crate, not ETDL source

Same reasoning as `std.probability`: ETDL has no expression or
function-call syntax, so `S(t) = exp(-(t/lambda)^k)` cannot be written as
ETDL YAML. Everything in this module — models, provenance, tree
evaluation — is native Rust (`etdl-reliability::predictive`). There is no
ETDL-source counterpart to add here; unlike `std.probability`
(which has genuinely expressible probability *literals*), a time-to-failure
model has no meaningful "literal" form independent of the math that
evaluates it.

## Determinism

Every function in this module is closed-form and deterministic. There is
no sampling anywhere in `predictive` — Monte Carlo / Bayesian posterior
predictive simulation is explicitly **out of scope** for 1.0. If it is
ever added, it should reuse `etdl-reliability::analysis::dependence`'s
existing seeded `xorshift64star` sampler, not a second one.

## Core types (`etdl-reliability::predictive`)

- **`MissionTime { value, unit }`** — an explicit future time/exposure
  duration. `unit` is free text (the same `std.units` deferral documented
  in `standard-library.md` applies: there is no checked unit type yet, so
  matching a rate's unit to a mission time's unit is the caller's
  responsibility).
- **`PredictiveQuantity`** — a closed enum: `Survival`, `Reliability`
  (mathematically identical to `Survival` for a non-repairable system,
  named separately so a result states which reading was intended without a
  second formula), `FailureProbability`, `Hazard`, `CumulativeHazard`,
  `Density`. Never a bare unlabeled `f64` — this is what keeps hazard from
  ever being confused with a probability.
- **`ModelDescriptor`** — family name, parameters, explicit assumptions,
  and an optional `valid_range` the model is asserted valid over.
- **`PredictiveProvenance`** — where a model's parameters came from: an
  optional `source_artifact` (`ArtifactRef`, reusing the same type
  `etdl-reliability::analysis::dependence` already defines) and
  `source_estimate`, plus analyzer identity.
- **`PredictiveResult`** — one quantity, one event, one mission time, one
  model, with `conditions`, an `extrapolated` flag, and provenance.
  `PredictiveResult::new` computes `extrapolated` from
  `model.valid_range`; when no range was declared, `extrapolated` is
  `false` — "no declared range" is not itself evidence of validity, but
  this constructor never invents a range to compare against, so it does
  not claim extrapolation either.

`PredictiveResult` is a distinct type from `ProbabilityEstimate` — a
prediction always carries a time horizon; an estimate never does. They are
never collapsed into one type.

## `TimeToFailureModel` trait (`predictive::models`)

```rust
pub trait TimeToFailureModel {
    fn survival(&self, t: f64) -> f64;           // S(t)
    fn hazard(&self, t: f64) -> f64;              // h(t)
    fn cumulative_hazard(&self, t: f64) -> f64;   // H(t)
    fn density(&self, t: f64) -> f64;             // f(t) = h(t) * S(t), default impl
    fn failure_probability(&self, t: f64) -> f64; // F(t) = 1 - S(t), default impl
    fn mean(&self) -> Option<f64>;
    fn quantile(&self, q: f64) -> Option<f64>;
    fn descriptor(&self) -> ModelDescriptor;
}
```

Total functions: every method is defined for all finite `t >= 0`,
including `t = 0` and very large `t` — none panic.

### `ExponentialModel` (constant hazard)

A thin wrapper over `etdl_probability_core::distribution::Exponential` —
`survival(t)` is that type's own `survival(t)`, not a reimplementation.
`hazard(t) = lambda` for all `t`. This is the same model
`etdl-reliability`'s own `ExponentialRateEstimator` already fits from
data; this module does not duplicate that estimator, it consumes its
output (see "Calibration adapter" below).

**Reference test** (this module's own acceptance criterion, and a
regression test in `etdl-reliability/tests/predictive_reliability.rs`):
`lambda = 0.001/hour`, `t = 100 hours` => `R(t) = exp(-0.1) ≈ 0.904837`.

### `WeibullModel` (shape `k`, scale `lambda`)

Not present in `std.probability` (which stays domain-neutral); a genuinely
new implementation, scoped to time-to-failure use:

- `S(t) = exp(-(t/lambda)^k)`
- `h(t) = (k/lambda) * (t/lambda)^(k-1)`
- `H(t) = (t/lambda)^k` (computed directly, not via `-ln(S(t))`, so it
  stays accurate as `S(t) -> 0`, where a float's `ln` loses precision)
- `mean = lambda * Gamma(1 + 1/k)`

`k < 1`: decreasing hazard (infant mortality/burn-in). `k = 1`: constant
hazard — reduces to `ExponentialModel` with `lambda' = 1/lambda` (verified
in tests). `k > 1`: increasing hazard (wear-out/aging) — the case the
exponential model cannot represent at all, and the reason this supplement
exists.

`Gamma(x)` is computed via an independent Lanczos-approximation
reimplementation local to this module (`predictive::models::gamma_function`),
**not** shared with `etdl_probability_core::numerics::log_gamma`. That
function exists but its containing module (`numerics`) is private
(`mod numerics;`, not `pub mod`), so it is unreachable outside
`etdl-probability-core` — discovered while implementing this task. This is
the same "fresh reimplementation, not a shared dependency" pattern already
used between `etdl-reliability`'s own estimator and
`etdl-probability-core`'s numerics; cross-validated against known values
(`Gamma(1)=1`, `Gamma(2)=1`, `Gamma(3)=2`, `Gamma(0.5)=sqrt(pi)`) rather
than sharing code across the crate boundary.

## Censoring (`predictive::censoring`)

`CensoredObservation { time, censoring: CensoringKind }` with
`CensoringKind::{Right, Left, Interval { lower, upper }}` — a minimal, data-only
representation of "the true failure time was not observed exactly." This
is a **new, purely additive** type, deliberately kept separate from
`etdl-reliability::observations::AggregateObservation` (the type the
existing binomial calibration pipeline consumes) rather than folding
censoring into it, to avoid any risk to that already-tested pipeline.

**Explicitly out of scope for 1.0:** censored-data parameter estimation
(MLE, Kaplan-Meier, etc.). `CensoredObservation` can be constructed and
carried through provenance; nothing in this module fits a model to it.

## Calibration adapter (`predictive::calibration_adapter`)

`exponential_model_from_artifact(artifact, event)` reads a
`FailureRate`-metric `ProbabilityEstimate` out of an existing
`ReliabilityArtifact` and returns `(ExponentialModel, PredictiveProvenance)`.
This is the *only* supported way in 1.0 to go from "a calibrated estimate"
to "a predictive model" — it is read-only, and it is the sole point of
contact between this module and the rest of the reliability engine's
estimation/calibration machinery. It does not call `calibrate()` or touch
`ObservationDataset`; those remain exactly as they were.

Building a `WeibullModel` from an artifact is not supported in 1.0, because
the estimation pipeline does not currently produce a shape parameter —
`WeibullModel::new(shape, scale)` remains available from literal
parameters. This is a documented gap, not a silent omission.

## Tree integration (`predictive::tree`)

`evaluate_failure_probability_at(tree, leaf_models, t)` computes each
leaf's `F(t)` from its own `TimeToFailureModel`, builds the
`BTreeMap<String, Probability>` the existing
`tree_adapter::evaluate_assuming_independence` already expects, and calls
that function **directly, unmodified**. No new tree-composition or gate
logic exists in this module — the Generic Tree Event Supplement and its
reliability adapter are untouched.

## Numerical stability and edge cases

- `survival(0) = 1`, `cumulative_hazard(0) = 0`, `failure_probability(0) = 0`
  for both models, by direct branch (not by evaluating the general formula
  at a boundary where it could round incorrectly).
- `WeibullModel::survival` computes `exp(-H(t))` from the closed-form
  `H(t) = (t/lambda)^k`, not `1 - cdf`, avoiding cancellation as `S(t) -> 0`.
  Verified for `t` large enough that `S(t) < 1e-30` without producing `NaN`
  or a negative value.
- `WeibullModel::hazard`/`density` at `t = 0` branch explicitly on `shape`
  (`< 1` diverges to `+infinity`, `= 1` is `1/scale`, `> 1` is `0`) rather
  than evaluating `0^(k-1)`, which is undefined behavior territory in
  floating point for `k < 1`.
- `quantile(q)` returns `None` outside `(0, 1)` rather than extrapolating a
  meaningless time.

## Extrapolation

`ModelDescriptor.valid_range: Option<(f64, f64)>` and
`PredictiveResult::new`'s computed `extrapolated` flag are the whole of
1.0's extrapolation handling: a model may declare the time range it is
asserted valid over, and any prediction outside that range is flagged. No
range is invented or inferred by this module — an absent range means "not
declared," not "confirmed valid everywhere."

## CLI

No new command ecosystem — matching this task's own instruction not to
build one. `etdl capabilities` reports a `predictive_reliability` block
(`available`, `schema`, `models`, `quantities`, `sampling`,
`censored_data_fitting`), gated behind the same `reliability` cargo
feature as the rest of `etdl-reliability`; a lean
`--no-default-features` build reports `"available": false` and
`"schema": "unavailable"` without depending on the optional crate at
compile time (see `predictive_reliability_schema` in `etdl-cli/src/main.rs`).

## Deferred / explicitly out of scope for 1.0

- Monte Carlo / Bayesian posterior predictive sampling.
- Lognormal (or any distribution beyond exponential/Weibull) time-to-failure
  model.
- Censored-data parameter fitting (MLE, Kaplan-Meier, etc.).
- Goodness-of-fit / model-comparison infrastructure.
- Repairable-systems / availability / renewal-process modeling.
- Physics-of-failure / degradation modeling.
- A standalone `std.reliability` ETDL-source facade (see "Known gap"
  above).

Each of these was explicitly named as out of scope by the task
specification itself ("do not implement the full X system now"); none are
silently missing.

## Tests

`etdl-reliability/tests/predictive_reliability.rs` — the reference test
(`lambda=0.001/hr, t=100h`), Weibull aging/infant-mortality hazard
direction, the shape=1-equals-exponential equivalence, zero-survival
behavior at large `t`, quantile inversion, the extrapolation flag (both
"within range" and "no range declared" cases), censored-observation
construction and serde round-trip, tree + predictive composition, and the
full `predict -> observe -> calibrate -> new artifact -> new prediction`
loop, which asserts (via JSON-snapshot comparison, since
`ReliabilityArtifact` does not derive `PartialEq`) that the original
artifact and the original `PredictiveResult` are byte-for-byte unchanged
after calibration and after a new artifact is published — the central
correctness property this entire supplement exists to preserve.

Module-local unit tests (`predictive::models`, `predictive::censoring`,
`predictive::calibration_adapter`, `predictive::tree`) cover construction
validation and error paths for each type.
