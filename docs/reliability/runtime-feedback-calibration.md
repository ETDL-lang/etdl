# Runtime Feedback & Calibration

This document describes how a compiled, deployed ETDL service's runtime
behavior is compared against the reliability artifact that predicted it —
and the discipline that keeps that comparison from ever silently becoming a
change to production behavior.

## The pipeline

```
ENGINEERING MODEL
   |
DISCOVERY -> EVIDENCE -> ESTIMATION -> RELIABILITY ARTIFACT
   |
ETDL COMPILATION -> GENERATED SERVICE
   |
RUNTIME OBSERVATIONS            (etdl-core::observation, lightweight)
   |
OBSERVATION DATASET              (etdl-reliability::dataset, versioned, immutable)
   |
OBSERVED RELIABILITY             (AggregateObservation: failures / exposure)
   |
PREDICTED vs. OBSERVED ANALYSIS  (etdl-reliability::calibration)
   |
CALIBRATION STATUS               (an engineering report, not a decision)
   |
ENGINEERING REVIEW
   |
NEW RELIABILITY ARTIFACT (if the engineer chooses) -> recompile -> redeploy
```

**Runtime observations MUST NOT automatically change compiled
probabilities.** The loop is *observe → analyze → review → publish a new
artifact → rebuild*. Nothing in this codebase closes that loop
automatically; [`calibrate`](../../etdl-reliability/src/calibration.rs) takes
`&ReliabilityArtifact`, never `&mut ReliabilityArtifact`, and there is no
function anywhere in `etdl-reliability::calibration` or `etdl-reliability::dataset`
that could feed a calibration result back into an artifact, a fault tree, or
generated code.

## Runtime observations (`etdl-core::observation`)

`BranchMonitor::record_branch` and `record_failure` — already called by
generated code for every branch/consequence evaluation — now also emit a
`ReliabilityObservation` through an `ObservationSink`, in addition to their
existing SLA-anomaly tracking. This is the runtime observation path
completed: previously these methods updated the SLA tracker only and never
reached any sink.

An observation records:

- `id` — a stable identifier (never array position; see below)
- `event` — the node/operation id
- `timestamp` — RFC 3339, computed without a chrono dependency
- `service`, `operation`, `environment`, `deployment` — where it ran
- `service_version`, `build_ref` — what ran: the software version and a
  stable reference to the compiled reliability artifact/build, so an analyst
  can trace "which model predicted this" without the runtime carrying the
  whole artifact (see `etdl-build-manifest.json`)
- `outcome`, `conditions`, `duration_ms`, `trace_id`

The runtime does **not** compute statistics, run Monte Carlo, or call a
reliability library. It records data; the analysis layer interprets it. The
declared/predicted probability is deliberately **not** duplicated onto every
observation — it lives in the artifact, referenced by `build_ref`, so it
never goes stale when the artifact is recalibrated.

Sinks ship two reference implementations: `CapturingSink` (in-memory, for
tests) and `JsonlSink` (appends one JSON Lines record per observation,
`write` + `flush`, no buffering that could lose data on process exit).
`NoopSink` remains the default — attaching a sink is opt-in.

## Observation identity

Every observation and every `AggregateObservation` carries an explicit `id`.
An `ObservationDataset` rejects members without one, and rejects duplicate
ids within the dataset — including after reordering, since identity is never
array position.

## Observation datasets (`etdl-reliability::dataset`)

An `ObservationDataset` is the logical unit of "observations collected under
known conditions, over a known period, from a known source" — it does not
require every observation to live in one physical file, only that the
logical dataset is identifiable by `(id, version)`.

```yaml
schema: etdl.reliability.observation-dataset/1.0
id: prod-region-a
version: "1"
collection_period:
  start: "2026-01-01T00:00:00Z"
  end: "2026-01-31T23:59:59Z"
observations:
  - id: obs-1
    failure_mode: failure.gateway.timeout
    exposure: 100000
    failures: 37
    exposure_unit: per-request
    conditions: [production]
```

**Datasets are immutable.** New observations never modify an existing
dataset value — they become a new `ObservationDataset` with a new `version`.
Nothing in `dataset.rs` mutates observations in place; publishing more data
is always constructing a new value.

## Controlled aggregation

`aggregate_across(&[&dataset, ...], failure_mode)` sums observations for one
failure mode **only when their exposure unit and conditions match exactly**.
A unit mismatch (e.g. `per-request` vs `per-hour`) or a condition mismatch
(e.g. `production` vs `high-load`) is a hard error, not a silent sum — this
mirrors the same non-conflation discipline the artifact and estimator layers
already enforce for probability metrics and time bases.

The result carries `AggregationProvenance`: every contributing dataset
(`id@version`) and every contributing observation id, sorted for
deterministic output regardless of the order datasets were passed in.

## Predicted vs. observed (`etdl-reliability::calibration`)

`calibrate(&artifact, event, &observation, dataset_refs, &config)` compares
one artifact prediction to one observed aggregate:

- **expected failures** = `exposure * predicted_value` (often more intuitive
  than a bare probability, e.g. "expected 10, observed 50")
- **difference** = `observed_proportion - predicted_value`
- **ratio** = `observed_proportion / predicted_value` (withheld with a
  diagnostic when the predicted value is zero, never divided-by-zero)
- **p-value** — an *exact* two-sided binomial test, not a normal
  approximation, reusing the same regularized-incomplete-beta machinery
  `analysis::estimator` already uses for Beta-Binomial credible intervals:

  ```
  H0: the true failure rate equals the predicted value p0
  P(X <= k) = I_{1-p0}(n-k, k+1)
  P(X >= k) = I_{p0}(k, n-k+1)
  p-value    = min(2 * min(P(X<=k), P(X>=k)), 1)
  ```

  where `k` = observed failures, `n` = observed exposure. This accounts for
  sample size automatically: the same 10-percentage-point difference is
  "consistent" at `n=10` and "significant" at `n=10,000,000`.

### Calibration status

| Status | Meaning |
|---|---|
| `consistent` | H0 not rejected at `alpha` (default 0.05) |
| `potential_deviation` | Rejected at `alpha` but not at `strict_alpha` (default 0.01) |
| `significant_deviation` | Rejected at `strict_alpha` — see "model drift" below |
| `insufficient_data` | Exposure below `min_exposure` (default 20); the p-value is still reported, but not asserted confidently |
| `unsupported_comparison` | Metric, conditions, or time basis don't match between prediction and observation; no test was run |

A difference is never labeled "the prediction is wrong" merely because
`observed != predicted`. `consistent` and `potential_deviation` are both
legitimate day-to-day outcomes; only `significant_deviation`, computed under
matching conditions with a defined test at a defined significance level, is
exposed as `CalibrationResult::is_drift()`.

### What "significant" does and does not mean

`p_value < strict_alpha` is a *statistical* statement: given the model, this
much data would be surprising if the true rate equalled the prediction. It
is **not** an *engineering* statement about whether the difference matters
operationally, and it is not a command to change anything. A five-sigma
difference between `0.00099` and `0.00101` at very large `n` is
"statistically significant" and usually engineering-irrelevant; the engineer
decides which it is.

### Conditional comparison

`calibrate` refuses to compare a prediction made under one set of conditions
against an observation made under a different set (e.g.
`P(Failure | high_load)` vs. observed `P(Failure | normal_load)`), and
refuses to compare a probability-like prediction under a mismatched time
basis. Both produce `unsupported_comparison`, not a number.

### Rate-based metrics

Calibration of `FailureRate`/`EventFrequency` predictions (preserving their
unit, e.g. `2e-5/hour` vs `3e-5/hour`, rather than converting to a
probability) is **not implemented** in this version — `calibrate` reports
`unsupported_comparison` with diagnostic `RC001` rather than silently
treating a rate as a probability. This is documented as a known limitation,
not a silent gap: only `ProbabilityMetric::Probability` and `Availability`
(the metrics `is_probability_like()` already recognizes) are calibrated.

## Diagnostics

| Code | Meaning |
|---|---|
| `RC001` | Metric is not probability-like; rate-based calibration is not implemented |
| `RC002` | Predicted and observed conditions do not match |
| `RC003` | Predicted time basis does not match observed exposure unit |
| `RC004` | Exposure is below the configured minimum |
| `RC005` | Predicted probability is zero; ratio is undefined |

## What is intentionally not implemented yet

- Rate-based (non-probability) calibration — documented above.
- Correlated uncertainty between the runtime-observed rate and the
  artifact's own declared uncertainty (e.g. combining a Beta-posterior
  prediction with a binomial observation into one interval) — the current
  comparison treats the prediction as a fixed point value, which is what the
  compiler resolves to a scalar anyway.
- CSV export of observation datasets — YAML/JSON follow the same convention
  as every other reliability artifact in this repository; a CSV adapter can
  be added without changing the underlying types.

## CLI

```bash
etdl reliability calibrate <artifact.rprob> <event> \
  --dataset prod-week-1.yaml --dataset prod-week-2.yaml \
  [--alpha 0.05] [--strict-alpha 0.01] [--min-exposure 20] \
  [--output calibration-result.json]
```

Exit code is `0` for every computed status, including `significant_deviation`
— a drift finding is data for engineering review, not a CLI failure. Exit
code `1` is reserved for actual errors: an unreadable/invalid artifact or
dataset, a missing prediction, or incompatible datasets that cannot be
aggregated.
