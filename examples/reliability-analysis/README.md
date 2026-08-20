# Worked example: uncertainty, importance and sensitivity

This directory holds the analysis-side inputs for a complete run over the
payment service in [`../reliability/`](../reliability/). It answers two
questions that a top-event probability alone cannot:

> **Which failure should the team investigate first?**
>
> **Which probability estimate contributes most to the uncertainty?**

They have different answers. That is the point.

Every number below was produced by the code in this repository; the assertions
live in `etdl-reliability/tests/end_to_end.rs`.

---

## The model

```text
GatewayOrDb = OR( GatewayTimeout, GatewayUnreachable, DatabaseUnavailable )

  GatewayTimeout       q = 1.0e-2
  GatewayUnreachable   q = 1.2e-3
  DatabaseUnavailable  q = 5.0e-3

  SharedNetworkFailure (q = 2.0e-4) affects GatewayUnreachable
                                        and DatabaseUnavailable
```

The fault tree is the one in `../reliability/payment-service.etdl`; the
probabilities come from `../reliability/payment-gateway.rprob`.

| File | Role |
|---|---|
| `dependencies.yaml` | the dependency / common-cause model |
| `uncertainty-before.yaml` | declared uncertainty per basic event |
| `uncertainty-after.yaml` | the same, after the gateway timeout mitigation |

---

## Running it

```bash
# Deterministic summary — no sampling, unchanged from before this feature.
etdl analyze ../reliability/payment-service.etdl

# Dependency-aware point estimate, importance and sensitivity.
etdl analyze ../reliability/payment-service.etdl \
    --dependencies dependencies.yaml

# Add uncertainty propagation and the data-collection ranking.
etdl analyze ../reliability/payment-service.etdl \
    --dependencies dependencies.yaml \
    --uncertainty uncertainty-before.yaml \
    --monte-carlo 20000 --seed 20260818 \
    --uncertainty-ranking \
    --output before.json

# The same after the mitigation, then compare.
etdl analyze ../reliability/payment-service.etdl \
    --dependencies dependencies.yaml \
    --uncertainty uncertainty-after.yaml \
    --monte-carlo 20000 --seed 20260818 \
    --uncertainty-ranking \
    --output after.json

etdl reliability compare before.json after.json
```

`--json` gives machine-readable output with a stable schema.

---

## The result

```text
Probability:
    1.593525e-2

Uncertainty:
    95% propagated-quantile-interval [1.281659e-2, 1.919859e-2]
    mean 1.593422e-2  median 1.590156e-2  sd 1.645e-3
    n=20000 seed=20260818 sampler=xorshift64star/1 varying inputs=3
    stability: indicators met (rse 7.30e-4)
```

The interval is labelled `propagated-quantile-interval` because the inputs mix
Beta posteriors with a vendor confidence interval. With mixed statistical
interpretations there is no single clean reading, so none is claimed. Had every
input been a Beta, it would read `propagated-credible-interval`.

### Question 1 — what to fix

Ranked by Fussell-Vesely, the share of top-event probability that disappears if
the event is eliminated:

| Entity | Birnbaum | Fussell-Vesely | Criticality |
|---|---|---|---|
| `GatewayTimeout` | 9.940e-1 | **0.6238** | 0.6238 |
| `DatabaseUnavailable` | 9.888e-1 | 0.2979 | 0.3103 |
| `GatewayUnreachable` | 9.851e-1 | 0.0618 | 0.0742 |
| `SharedNetworkFailure` *(common cause)* | 9.843e-1 | 0.0124 | 0.0124 |

**`GatewayTimeout` has the highest Fussell-Vesely importance (0.624).** That is
the quantitative evidence. The tool does not say "fix it" — a team may have
good reasons to work on something else, and that decision is theirs.

Note the common cause appears as **its own entity**, identified by its own id
rather than folded into the leaves it affects.

### Question 2 — what to measure

| Input | Variance share | Declared law |
|---|---|---|
| `DatabaseUnavailable` | **78.2%** | `normal-from-confidence-interval(95%: [3.5e-3, 6.5e-3])` |
| `GatewayUnreachable` | 21.3% | `beta(2.4, 1997.6)` |
| `GatewayTimeout` | 0.0% *(below noise floor)* | `beta(10000, 990000)` |

`GatewayTimeout` is the dominant *contributor* and contributes essentially
**nothing** to the width of the answer: it has a million observations behind it.
`DatabaseUnavailable` contributes half as much probability but comes from a
vendor interval, and it drives 78% of the output variance.

> **Fix `GatewayTimeout`. Measure `DatabaseUnavailable`.**

Collapsing importance and uncertainty into one "score" would have hidden this
entirely. The two metrics are computed by different methods, carry different
names, and are never converted into one another.

### Sensitivity

```text
GatewayTimeout:       increase +9.940e-5   decrease -9.940e-5   elasticity  0.6238
DatabaseUnavailable:  increase +9.890e-5   decrease -9.890e-5   elasticity  0.3103
GatewayUnreachable:   increase +9.852e-5   decrease -9.852e-5   elasticity  0.0742
SharedNetworkFailure: increase -9.844e-5   decrease +9.842e-5   elasticity -0.0124
                      response is asymmetric around the baseline
```

The common cause is worth a second look. Raising its probability *lowers* the
top event slightly, because at fixed leaf totals more of each leaf's failure
probability becomes shared rather than independent, and an OR gate over
overlapping causes is less likely to fire than one over disjoint ones. The
response is also asymmetric. A one-sided analysis would have reported neither.

### Diagnostics

```text
[RA007] warning: basic events are dependent through a declared common cause, but
        their parameter uncertainties are sampled independently; correlated
        parameter uncertainty is not supported and the reported interval may be
        too narrow
[RA010] warning: 8 of 60000 input draws (0.01%) fell outside [0,1] and were
        clamped
[RA004] info (GatewayTimeout): variance share at or below the Monte Carlo noise
        floor at 20000 samples
```

RA007 is a real limitation, stated rather than hidden. RA010 is the cost of
calibrating a Normal to interval endpoints — a reason to prefer a Beta where
counts exist.

---

## Before and after mitigation

The mitigation cuts `GatewayTimeout` from `1.0e-2` to `2.0e-3`, and the
accompanying instrumentation work tightens its posterior from `Beta(10000,
990000)` to `Beta(2000, 998000)`.

```text
Top Event:   GatewayOrDb
Before:      ana-5e0160b0981c94c9 -> 1.593525e-2
After:       ana-645fbce5b8b4ba0e -> 7.983209e-3
Change:      -7.952038e-3 (-49.90%)
Interval:    [1.281659e-2, 1.919859e-2] -> [4.840698e-3, 1.127972e-2]

Input changes:
  GatewayTimeout [basic-event] Some(0.01) -> Some(0.002)
                 (changed-and-uncertainty-changed)

Importance rank changes:
  DatabaseUnavailable: rank Some(2) -> Some(1)
  GatewayTimeout:      rank Some(1) -> Some(2)

Attribution:
  input 'GatewayTimeout' changed in two ways at once (its value and its
  declared uncertainty); the top-event difference is NOT attributed to either
  one alone
```

Two things worth noting.

**The priority picture changed.** `DatabaseUnavailable` is now the top
contributor — its Fussell-Vesely rises from 0.298 to 0.599 while
`GatewayTimeout` falls from 0.624 to 0.249. The next piece of work is a
different one.

**Attribution is refused.** Two properties of the same input moved at once, so
the tool declines to say which produced the halving. Had only the probability
changed, `single_change` would be true and the change attributed — still scoped
to the model, with the caveat that whether the real system changed the same way
is not a question this tool answers.

The two analyses have different `analysis_id`s and both survive. Results are
immutable historical evidence; a new run never edits an old one.

---

## What happens next

```text
analysis -> analysis result -> engineer selects a value
         -> ReliabilityArtifact -> compiler -> deterministic generated value
```

Nothing above runs during ordinary compilation. An engineer reads the analysis,
decides which number to stand behind, and writes it into a `.rprob` artifact.
The compiler consumes that artifact and never samples a distribution.

---

## Reproducing

Every run is seeded. The same model, inputs, seed, sample count, sampler version
and analyzer version give bit-identical output — asserted on raw float bits in
`etdl-reliability/tests/uncertainty_analysis.rs`. The `analysis_id` is a content
hash over inputs, model and method, excluding the timestamp, so re-running the
same analysis tomorrow yields the same id.

See [`../../docs/reliability/uncertainty-importance-sensitivity.md`](../../docs/reliability/uncertainty-importance-sensitivity.md)
for the formulas, assumptions and limitations behind every metric here.
