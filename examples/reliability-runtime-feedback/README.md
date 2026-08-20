# Worked example: predicted vs. observed, and why "stale" is not "wrong"

This continues the `payment-gateway` scenario from
[`../reliability-analysis/`](../reliability-analysis/): the team mitigated
`GatewayTimeout`, cutting its declared probability from `0.01` to `0.002`.
Two weeks after deploying the fix, they check whether reality agrees with
the model — without ever letting that check change the model automatically.

Every number below was produced by `etdl reliability calibrate`; the exact
values are also asserted in `etdl-reliability/tests/calibration.rs`.

| File | Role |
|---|---|
| `artifact-stale.rprob` | the *old* artifact, `failure.gateway.timeout = 0.01` — as if the engineer forgot to republish it after the mitigation shipped |
| `artifact-current.rprob` | the artifact actually deployed, `failure.gateway.timeout = 0.002` |
| `dataset-week-1.yaml` | production observations, week 1 (`110 / 50000`) |
| `dataset-week-2.yaml` | production observations, week 2 (`120 / 50000`) |

Two separate weekly datasets are used deliberately — a dataset is immutable
once published, so week 2's data becomes a *new* dataset rather than an edit
to week 1's, and `calibrate` aggregates across both explicitly rather than
requiring one physical file.

## Running it

```bash
etdl reliability calibrate artifact-stale.rprob failure.gateway.timeout \
  --dataset dataset-week-1.yaml --dataset dataset-week-2.yaml

etdl reliability calibrate artifact-current.rprob failure.gateway.timeout \
  --dataset dataset-week-1.yaml --dataset dataset-week-2.yaml
```

## Result 1 — comparing against the stale artifact

```text
Calibration: failure.gateway.timeout
  predicted: 0.010000 (Probability)
  observed:  230/100000 = 0.002300
  expected failures: 1000.00 (observed: 230)
  p-value: 0.000000 (H0: the true failure rate for 'failure.gateway.timeout' under the observed
  conditions equals the predicted value 0.010000 (two-sided binomial test))
  status: SignificantDeviation
note: this indicates model drift under the configured significance level; review before
publishing a new estimate. Nothing has been changed automatically.
```

The artifact still says `1.0e-2`; production is running at `2.3e-3` — less
than a quarter of the predicted rate, with 1000 failures expected against
230 observed. The p-value is far below `1e-190`: this is not sampling noise,
it is a stale model. `calibrate` reports `SignificantDeviation` (drift) and
exits `0` — a drift finding is a report for engineering review, not a tool
failure, and the artifact file on disk is byte-for-byte unchanged.

## Result 2 — comparing against the current artifact

```text
Calibration: failure.gateway.timeout
  predicted: 0.002000 (Probability)
  observed:  230/100000 = 0.002300
  expected failures: 200.00 (observed: 230)
  p-value: 0.040245 (H0: the true failure rate for 'failure.gateway.timeout' under the observed
  conditions equals the predicted value 0.002000 (two-sided binomial test))
  status: PotentialDeviation
```

Once the correct artifact is used, the picture changes: 200 failures
expected, 230 observed — a 15% excess. At `p = 0.040`, this is below the
default `alpha = 0.05` but *not* below the stricter `strict_alpha = 0.01`
used for `SignificantDeviation`. `calibrate` reports `PotentialDeviation`,
not `Consistent` and not `SignificantDeviation` — worth a second look next
week, not an incident.

## The point

The *same observed data* (`230 / 100000`) produces two different
conclusions depending on which prediction it is compared against. Neither
run is "the tool being wrong" — `calibrate` is answering a well-defined
statistical question (does this data look like it came from a process with
failure rate `p0`?) against whichever `p0` you gave it. Feeding it a stale
prediction and being told "drift" is the tool working correctly, not a
false alarm.

## What happens next

```text
observe -> analyze -> review -> publish a new artifact -> rebuild
```

Neither `calibrate` invocation above wrote to `artifact-stale.rprob` or
`artifact-current.rprob` (`diff` them before/after — they are unchanged). If
week 3's data keeps the gap at `PotentialDeviation` or pushes it to
`SignificantDeviation`, an engineer decides whether to re-run
`etdl reliability estimate` over the accumulated observations and publish
`payment-gateway` version `1.4.0` — a distinct, versioned artifact the
compiler picks up on the next build. `calibrate` never does this itself.
