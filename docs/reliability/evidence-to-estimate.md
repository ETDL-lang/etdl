# Evidence → Estimate → Artifact

This document explains the reliability engineering workflow that connects
discovery, review, observation, estimation, and compilation into a single
traceable pipeline. The central goal: **every numerical reliability value can
be traced back to its origin and method.**

```
SOURCE CODE
    ↓
FAILURE DISCOVERY            "what failures are possible?"
    ↓
DISCOVERY CANDIDATE
    ↓
ENGINEERING REVIEW           accept / reject / remap / ignore
    ↓
FAILURE MODE                 stable identity (survives source movement)
    ↓
OBSERVATIONS / EVIDENCE      100,000 requests, 37 failures
    ↓
STATISTICAL ESTIMATION       method + interval + provenance
    ↓
PROBABILITY ESTIMATE         typed, versioned, immutable
    ↓
RELIABILITY ARTIFACT         .rprob (schema etdl.reliability.artifact/1.0)
    ↓
ETDL COMPILER                resolves a deterministic scalar
    ↓
FAULT TREE / EVENT TREE
    ↓
DETERMINISTIC GENERATED CODE
```

## Three quantities that must never be conflated

| Quantity | Meaning | Produced by |
|---|---|---|
| **Discovery confidence** | "How confident is the analyzer that this candidate/classification is correct?" | `etdl-failure-discovery` |
| **Statistical uncertainty** | "How uncertain is the estimated reliability quantity?" | the estimator (confidence/credible interval) |
| **Failure probability** | "Under the declared model, conditions and exposure, how likely is the failure?" | the estimator + the compiler's resolution |

`discovery confidence = 0.95` must **never** become `failure probability =
0.95`. The pipeline keeps them in separate structures.

## 1. Discovery → review

`etdl-failure-discovery` produces `DiscoveryCandidate`s (possible, not proven).
An engineer reviews each one. The review is recorded as an immutable,
versioned `ReviewRecord` (candidate id, report id, status, selected ontology,
rationale, assumptions, notes). `ReviewedFailureMode` links the accepted
candidate to a stable failure-mode identity.

```rust
let mut review = ReviewRecord::new(candidate_id, ReviewStatus::Accepted);
review.selected_ontology_id = Some("failure.dependency.timeout".into());
review.rationale = Some("matches production HTTP timeout path".into());
let failure_mode = ReviewedFailureMode::from_review(review);
```

Review never mutates the discovery report. A new decision is a new record.

## 2. Observations

An `AggregateObservation` expresses evidence with an explicit exposure:

```yaml
failure_mode: failure.gateway.timeout
exposure: 100000
failures: 37
exposure_unit: per-request
conditions: [production]
source: prod-obs-2026-08
version: "1"
```

Validation rejects: zero exposure, failures > exposure, negative counts,
missing failure mode, invalid intervals. Exposure is explicit — two estimates
with different exposure bases are never silently merged.

## 3. Estimation

Estimators implement `ReliabilityEstimator` and return a typed
`ProbabilityEstimate` (never a bare `f64`):

- `EmpiricalBinomialEstimator` — `p_hat = failures / exposure`, Wilson
  confidence interval, frequentist.
- `BetaBinomialEstimator` — Bayesian posterior from a declared prior; the point
  is the **posterior mean**; the interval is an equal-tailed **credible**
  interval.
- `ExponentialRateEstimator` — constant hazard rate; converts
  `P(failure by t) = 1 - exp(-λt)` using the numerically stable `-expm1`.

Every estimate carries:

- failure mode id, metric, value, state (`Estimated`), population/exposure,
  conditions, method, uncertainty, provenance (dataset, model, model version),
  and version.

```rust
let estimator = EmpiricalBinomialEstimator::new();
let estimate = estimator.estimate(&observation, &config)?;
// estimate.value == Some(0.00037), estimate.uncertainty == ConfidenceInterval
```

## 4. Artifact

The `ReliabilityArtifact` (`.rprob`) stores estimates keyed by canonical key.
Multiple estimates for the same failure mode under different conditions,
versions, or populations coexist without overwriting. `add` rejects exact
duplicates (no silent overwrite).

## 5. Deterministic selection and conflicts

`select_estimate` picks an estimate deterministically for a
(failure mode, metric, conditions, population) key. When two estimates claim
the same context with different values, the policy decides:

- `Error` (default) — report the conflict; never silently choose.
- `FirstWins` — caller-provided priority order.
- `ExplicitArtifact(id)` / `ExplicitVersion(v)` — deterministic selection.

## 6. Compilation

The compiler resolves the artifact estimate to a deterministic scalar, feeds it
into the fault tree, and records provenance in the build manifest:

```json
{
  "value": 0.00037,
  "method": "binomial/empirical/binomial",
  "state": "Estimated",
  "conditions": ["production"],
  "provenance": { "dataset": "prod-obs-2026-08", "model": "binomial", ... }
}
```

## Concrete example

```
failure:        failure.gateway.timeout
discovered at:  src/payment.rs:143
discovery ev:   HTTP client timeout path
ontology:       failure.dependency.timeout @ 1.0
observations:   2,400 / 1,000,000 requests (production)
estimation:     empirical binomial
estimate:       0.0024
interval:       Wilson 95% [0.00231, 0.00249]
artifact:       payment-reliability.rprob v1.0
compile:        fault tree uses 0.0024
```

## CLI workflow

```bash
etdl discover ./service                      # find candidates
# ... engineer reviews ...
etdl reliability estimate observations.yaml \
    --method empirical --output gateway.rprob
etdl reliability trace gateway.rprob failure.gateway.timeout
etdl reliability inspect gateway.rprob
etdl compile service.etdl
```

## Probability is conditional

An engineering probability always has a context:

```
P(Failure | population, conditions, exposure, model)
```

`0.0024` is **not** a universal property of the software; it is the estimated
failure probability per request under production conditions under the binomial
model.

## The feedback loop

```
DESIGN → DISCOVERY → ESTIMATION → COMPILE → RUN → OBSERVE → RE-ESTIMATE
                                                          ↓
                                                NEW ARTIFACT → NEW BUILD
```

Runtime telemetry collects observations (`etdl-core`). Those observations feed
a future analysis that produces a new artifact for the next build. The system
**never** automatically changes production behavior from observations —
engineering review remains the control boundary.

## Limitations

- Simple empirical/binomial estimation does **not** handle censored data.
- Binomial estimation assumes independent trials under the declared exposure.
- Correlated/clustered/common-cause failures are not modeled (identity can
  represent them; the estimators do not).
- No uncertainty propagation through the fault tree yet (the deterministic
  top-event value is not a distribution).
