# Uncertainty, Sensitivity and Importance Analysis

This document defines every metric ETDL's reliability analysis computes: its
formula, its assumptions, how to read it, and what it does **not** mean.

It covers `etdl-reliability`, the optional analysis crate. Nothing here runs
during ordinary ETDL compilation. See
[`dependency-analysis.md`](dependency-analysis.md) for the dependency and
common-cause model this builds on.

---

## 0. Four concepts, never interchanged

| Concept | Question | Example answer |
|---|---|---|
| **Probability** | How likely is this event? | `P(GatewayTimeout) = 1.0e-2` |
| **Uncertainty** | How well is that probability known? | `Beta(10000, 990000)`, sd `9.9e-5` |
| **Sensitivity** | If the input moved, how much would the answer move? | `dP(top) = +9.99e-5` for `dq = +1e-4` |
| **Importance** | How much does the top event depend on this entity? | `Birnbaum = 0.9988` |

A fifth concept lives in `etdl-failure-discovery` and is also distinct:

| **Discovery confidence** | How sure are we this candidate is a real failure mechanism? | `0.72` |

These are five different numbers answering five different questions. The
implementation keeps them in separate types with separate method identifiers,
and no code path converts one into another.

The most common misreading is treating importance as a probability. It is not.
Birnbaum importance of `0.9988` does not mean "99.88% likely to fail"; it means
"switching this event on rather than off moves the top-event probability by
0.9988". An event with probability `1e-9` can have Birnbaum importance near 1.

**Uncertainty is not probability.** `P = 1.12e-2` is a statement about the
system. `[1.02e-2, 1.31e-2]` is a statement about our knowledge of that
statement. Widening the second does not make the system less reliable; it makes
our claim less precise.

---

## 1. Representing uncertainty

Uncertainty representation lives in the **built-in** crate
`etdl-reliability-core`, so a reliability artifact can carry it without pulling
in any analysis machinery.

```rust
pub enum Uncertainty {
    ConfidenceInterval(ConfidenceInterval), // frequentist, has a level
    CredibleInterval(ConfidenceInterval),   // Bayesian, has a level
    Distribution(Distribution),             // parametric
    Interval(Interval),                     // plain range, NO coverage claim
    LowerBound(LowerBound),
    UpperBound(UpperBound),
}
```

A deterministic value is the absence of an `uncertainty` field, not a special
variant: an estimate is `value: Some(p), uncertainty: None`.

### 1.1 Confidence is not credible

```yaml
uncertainty: { kind: confidence-interval, level: 0.95, lower: 0.001, upper: 0.004 }
uncertainty: { kind: credible-interval,   level: 0.95, lower: 0.001, upper: 0.004 }
```

Identical numbers, different claims:

- **Confidence interval** — over repeated sampling, intervals constructed this
  way cover the true value 95% of the time. It says nothing about *this*
  interval.
- **Credible interval** — given the prior and the data, the posterior
  probability that the value lies in this interval is 95%.

`Uncertainty::kind()` returns the distinction as a value, and
`UncertaintyKind::interpretation()` returns the sentence. `PartialEq` treats the
two as different. Nothing in the pipeline silently relabels one as the other.

### 1.2 A plain interval makes no coverage claim

`Uncertainty::Interval` exists for vendor min/max figures and engineering
bounds. `level()` returns `None` for it — the type refuses to invent a
coverage number that was never claimed.

---

## 2. Uncertainty propagation

### 2.1 The method

**Monte Carlo propagation**, the single propagation method implemented.

```text
for s in 1..=N:
    for each basic event i:  q_i^(s) ~ L_i          (declared sampling law)
    P_top^(s) = evaluate(tree, dependency model, q^(s))
report mean, median and empirical quantiles of { P_top^(s) }
```

The fault tree is evaluated **in full for every sample**, using the exact
dependency-aware evaluator. The point estimate is not computed once with the
input interval bolted onto the result — that would propagate nothing.

Identifiers recorded in every result:

| Field | Value |
|---|---|
| `method` | `monte-carlo-propagation` |
| `method_version` | `1` |
| `sampler` | `xorshift64star` |
| `sampler_version` | `1` |

### 2.2 Sampling laws

`InputUncertainty::from_declared` converts a declared `Uncertainty` into a
sampling law, or fails:

| Declared | Law | Notes |
|---|---|---|
| `Distribution(Beta{a,b})` | `Beta(a,b)` | exact; support already `[0,1]` |
| `Distribution(Normal{m,s})` | `Normal(m,s)` | clamped to `[0,1]`, clamps counted |
| `Distribution(LogNormal{m,s})` | `LogNormal(m,s)` | clamped to `[0,1]` |
| `Interval{lo,hi}` | `Uniform[lo,hi]` | maximum entropy on a bounded range |
| `ConfidenceInterval`/`CredibleInterval` | Normal calibrated to the endpoints | **approximate**, see below |
| `LowerBound` / `UpperBound` | *refused* | a one-sided bound is not a distribution |
| any other `Distribution` | *refused* | diagnostic `RA002` |

Beta variates are drawn as `X/(X+Y)` with `X ~ Gamma(a)`, `Y ~ Gamma(b)`
(Marsaglia–Tsang), which is exact and behaves correctly for the extremely
asymmetric shapes reliability work produces — `Beta(1, 1e6)` is tested directly.

**The interval calibration is approximate.** Given `[lo, hi]` at level `L`, a
Normal is fitted with `mu = (lo+hi)/2` and `sigma = (hi-lo)/(2 z)` where `z` is
the standard normal quantile at `(1+L)/2`. This assumes symmetry and normality,
which is a poor fit near 0 or 1. Prefer declaring a Beta. When draws are clamped
into `[0,1]`, the count is reported and diagnostic `RA010` fires: the effective
law is truncated and its mean is no longer the declared mean.

### 2.3 What the output interval means

The reported `[lower, upper]` are **empirical quantiles of the propagated
distribution of the top-event probability**, computed with linear interpolation
between order statistics (R type-7). They are:

- not a confidence interval for an estimator,
- not a bound,
- not a guaranteed range.

The result states which of four cases applies:

| `semantics` | When | Reading |
|---|---|---|
| `propagated-credible-interval` | every input law is a Bayesian representation | a credible interval for the top event, given the input posteriors and the structure |
| `propagated-quantile-interval-from-confidence-inputs` | every input is confidence-calibrated | **not** a confidence interval; propagating endpoint-calibrated intervals through a nonlinear function does not preserve coverage |
| `propagated-quantile-interval` | mixed or shape-only inputs | an empirical quantile interval and nothing more |
| `no-propagated-uncertainty` | nothing varied | the point estimate repeated; a modelling gap, not certainty |

### 2.4 Convergence

Reported as **stability indicators**, never as proof:

```text
standard_error          = sd / sqrt(N)
relative_standard_error = standard_error / |mean|
quantile spread         = (max - min) / mean of the quantile re-estimated
                          across 5 interleaved sub-samples
```

`stable` is true only when `N >= 1000`, relative standard error `<= 0.01`, and
both quantile spreads `<= 0.10`. The exact rule is written into
`convergence.criterion` in every result, and includes the words *stability
heuristic, not a convergence proof*. A run is never called converged because a
fixed sample count completed.

### 2.5 Reproducibility

Same model, inputs, seed, sample count, sampler version and analyzer version
give **bit-identical** results — asserted on raw float bits in
`tests/uncertainty_analysis.rs`, not on rounded comparisons. Changing the
sampler or any draw order requires bumping `sampler_version`; results from
different sampler versions are not expected to match and must not be compared
as if they were.

Sample count must be greater than zero. The default (`10_000`) is applied only
where an API requires one, and is reported via diagnostic `RA011`.

### 2.6 Rare events

Gate arithmetic is evaluated in log space where it matters. For an OR gate:

```text
P = 1 - prod(1 - q_i)   evaluated as   -expm1( sum log1p(-q_i) )
```

The naive form loses almost all significant digits for `q ~ 1e-12`, because
`1 - q` rounds to within an epsilon of 1 and the final subtraction cancels.
`log1p`/`expm1` retain full relative precision down to the smallest normal
`f64`. Importance and sensitivity are verified at `1e-3`, `1e-6`, `1e-9` and
`1e-12`.

Precision beyond that of the input estimates is not meaningful, and diagnostic
`RA012` says so when the top event falls below `1e-9`.

---

## 3. Importance

All measures are computed by **exact conditioning** with the dependency-aware
evaluator — the entity is forced on or off and the tree is re-evaluated. No
finite-difference approximation is involved.

Let `P` be the baseline top-event probability, `q_i` the entity's own
probability, and

```text
P(1_i) = P(top | entity occurs)
P(0_i) = P(top | entity does not occur)
```

| Measure | Formula | Requires coherence |
|---|---|---|
| Birnbaum `I_B` | `P(1_i) - P(0_i)` | no |
| Fussell-Vesely `I_FV` | `(P - P(0_i)) / P` | yes |
| Criticality `I_CR` | `I_B(i) * q_i / P` | yes |
| Risk Achievement Worth | `P(1_i) / P` | no |
| Risk Reduction Worth | `P / P(0_i)` | no |

### 3.1 Why conditioning is the correct definition

For a coherent tree with independent basic events, `P(top)` is **multilinear**
in the `q_i`, so

```text
P = q_i * P(1_i) + (1 - q_i) * P(0_i)
dP/dq_i = P(1_i) - P(0_i) = I_B(i)
```

The conditioning form *is* the partial derivative, not an approximation of it.
`tests/importance.rs` checks agreement with a numerical derivative of the closed
form. Under a dependency model the derivative interpretation no longer holds,
but the conditioning definition remains exactly computable and is what is
reported.

### 3.2 Worked derivation

```text
TOP = OR(A, B),  qA = 0.02, qB = 0.05, independent
P    = 1 - 0.98 * 0.95 = 0.069

P(1_A) = 1            P(0_A) = qB = 0.05
I_B(A)  = 1 - 0.05                = 0.95
I_FV(A) = (0.069 - 0.05) / 0.069  = 0.2754
I_CR(A) = 0.95 * 0.02 / 0.069     = 0.2754
RAW(A)  = 1 / 0.069               = 14.49
RRW(A)  = 0.069 / 0.05            = 1.38
```

### 3.3 Coherence

A tree containing NOT or XOR is **non-coherent**: raising a basic event's
probability can lower the top event. Fussell-Vesely and criticality are defined
only for coherent trees. For a non-coherent tree they are reported as `null`
with diagnostic `RA008`; Birnbaum, which needs no monotonicity, is still
reported.

### 3.4 Common causes are first-class entities

A common cause is analysed as **itself**, not as the leaves it affects:

```text
P(top | C = c) = sum_{s : s_C = c} P(s) P(top | s) / sum_{s : s_C = c} P(s)
```

so `P(top | C absent)` keeps each affected leaf at its **residual** independent
probability `(q - p_C)/(1 - p_C)`. Forcing the affected leaves to zero would be
wrong: it would discard the independent failure paths along with the shared one.

This is why a small shared cause can outrank a much larger independent failure.
In the worked example, `SharedNetworkFailure` at `2.0e-4` has Birnbaum `0.989`
while `DbPrimaryDown` at `4.0e-3` — twenty times more probable — has `4.7e-3`.
The shared cause defeats the database redundancy outright; the hardware failure
consumes one of its two legs. Ranking by probability would have inverted this.

### 3.5 Fussell-Vesely without duplicating MOCUS

The textbook definition of Fussell-Vesely is the probability of the union of
minimal cut sets containing `i`, divided by `P`. For a coherent tree that is
identically `(P - P(0_i)) / P`, which the conditioning evaluator computes
exactly and **dependency-aware**. The compiler's existing
`enumerate_minimal_cut_sets` (MOCUS) is not duplicated, and cut-set
probabilities are not formed by multiplying dependent events as if independent.

Fussell-Vesely shares do **not** partition the top-event probability. They
overlap wherever cut sets overlap, and the total across entities is not one.
Rendering them as a pie chart is a misreading.

### 3.6 Future measures

Reserved in `ImportanceMetric`, deliberately not implemented: diagnostic
importance `P(i | top)`, the differential importance measure (DIM), structural
importance, Barlow-Proschan importance. `ImportanceResult.measures` is a list of
names precisely so these can be added without a schema break.

---

## 4. Sensitivity

Method: **two-sided absolute finite perturbation**
(`finite-perturbation/absolute/two-sided`, version `1`).

```text
up:   q+ = min(1, q + delta)   ->  dP+ = P(top; q+) - P(top; q)
down: q- = max(0, q - delta)   ->  dP- = P(top; q-) - P(top; q)
```

One input is perturbed at a time; all others stay at baseline.

### 4.1 Both directions, always

The relationship is not symmetric in general. Common-cause conditioning, voting
gates and nested structures all produce responses where a decrease does not
mirror an increase. Whether the legs *did* mirror is **reported** as
`two_sided_symmetric`, not assumed. In the worked example the common cause comes
back asymmetric — and its response to an increase is slightly *negative*, which
a one-sided analysis would have missed entirely.

A leg that cannot move (an input already at 0 or 1, or a common cause at its
structural ceiling) is reported with `applied: false` rather than silently
omitted or recorded as a genuine zero.

### 4.2 Relative sensitivity

The elasticity:

```text
S_rel(i) = (dP_top / P_top) / (dq_i / q_i)
```

Read as: a 1% relative change in `q_i` moves `P(top)` by `S_rel(i)` percent. It
is comparable across inputs of wildly different magnitudes, which absolute
sensitivity is not.

It is reported **only** when both denominators exceed `1e-15`. Otherwise it is
`null` with diagnostic `RA006`. No epsilon is substituted for a zero
denominator; absolute sensitivity is still reported in that case.

### 4.3 Worked derivation

```text
TOP = OR(A, B),  qA = 0.01, qB = 0.02, delta = 0.001
P     = 1 - 0.99  * 0.98 = 0.0298
P(q+) = 1 - 0.989 * 0.98 = 0.03078   dP+ = +0.00098
P(q-) = 1 - 0.991 * 0.98 = 0.02882   dP- = -0.00098

elasticity = (0.00098 / 0.0298) / (0.001 / 0.01) = 0.3289
```

---

## 5. Uncertainty contribution ranking

Answers: *which probability estimate should receive more evidence?*

Method: `variance-freeze-one-at-a-time/common-random-numbers`, version `1`.

```text
share_i = 1 - Var(top | q_i held at its nominal value) / Var(top)
```

The frozen run reuses the same seed **and still consumes the frozen input's
random draws**, so the two runs share random numbers. The difference therefore
reflects the frozen input rather than sampling noise.

### 5.1 This is not importance

Importance asks how much the top event depends on an event *occurring*. This
asks how much the width of the answer is driven by not knowing that event's
probability. In the worked example they give different answers:

```text
investigate first (Fussell-Vesely):  GatewayTimeout        0.891
measure better (variance share):     GatewayUnreachable    98.3%
```

`GatewayTimeout` is the dominant contributor and is heavily instrumented, so it
contributes almost nothing to the interval. `GatewayUnreachable` contributes
eight times less probability but is known an order of magnitude less precisely,
and sits on the same OR path. **Fix the first; measure the second.**

`DbPrimaryDown` has the widest posterior of all four inputs and still lands near
zero: it enters through an AND, so its influence is damped by its redundant
partner. A wide input the structure damps out does not drive the answer.

### 5.2 Limits

- Shares do **not** sum to one. With a nonlinear model, one-at-a-time effects do
  not partition the variance.
- Interaction effects are not separated.
- A share at or below the Monte Carlo noise floor `sqrt(2/(N-1))` is flagged
  `above_noise_floor: false` rather than reported as a small real effect.
- This is not value-of-information analysis. It ranks where evidence would help;
  it does not compute how much a specific number of new observations would
  narrow the interval. The API is shaped to allow that later.

---

## 6. Dependency interaction

Analysis either computes a declared dependency correctly or refuses.

| Declared | Handling |
|---|---|
| `IndependenceAssumption::Assumed` | classic independent evaluation |
| Common causes | exact conditioning over the joint state of all causes |
| Conditional probabilities `P(A\|B)` | **refused**, diagnostic `RA001` |
| `depends-on` / `conditional-on` edges | **refused**, diagnostic `RA001` |
| `common-cause` edge with no matching declared cause | **refused**, diagnostic `RA001` |

`DependencyEvaluator::check_supported()` runs before every evaluation, so
importance, sensitivity and propagation all inherit the refusal. There is no
code path that evaluates a dependent model under independence.

### 6.1 Correlated parameter uncertainty is not modelled

Event dependence and parameter-uncertainty correlation are different things.
The former is handled exactly. The latter is **not supported**: when a model
declares a common cause and propagation runs, diagnostic `RA007` fires and
states that the reported interval may be too narrow. This is a documented
limitation, not a silent approximation.

---

## 7. Diagnostics

| Code | Meaning |
|---|---|
| `RA001` | unsupported dependency structure; independence was not substituted |
| `RA002` | declared uncertainty cannot be turned into a sampling law |
| `RA003` | sample count below the reliable threshold for tail quantiles |
| `RA004` | convergence/stability indicators not met |
| `RA005` | invalid uncertainty bounds |
| `RA006` | near-zero baseline; relative measure undefined and withheld |
| `RA007` | correlated parameter uncertainty not modelled |
| `RA008` | non-coherent tree; coherence-dependent measures withheld |
| `RA009` | zero top-event probability; normalised measures withheld |
| `RA010` | samples clamped into `[0,1]`; effective law is truncated |
| `RA011` | a default was applied; the value used is reported |
| `RA012` | rare-event precision limit |
| `RA013` | no declared uncertainty; contributes nothing to the interval |

Codes are stable. Adding one is a minor change; changing the meaning of an
existing one is not permitted.

---

## 8. Results are evidence

An analysis result is **immutable historical evidence**. New samples,
observations, estimates or an analyzer version produce a *new* result with a new
`analysis_id`, never an edit to an existing one.

`analysis_id` is a content hash (FNV-1a, non-cryptographic — an identity, not a
security primitive) over the analysed inputs, the dependency model, and the
method/version/seed/sample-count. It deliberately **excludes** the timestamp, so
re-running the same analysis tomorrow yields the same id. That is what makes
"is this the same analysis?" answerable.

Five version kinds are kept distinct:

| Field | What it versions |
|---|---|
| `schema` / `schema_version` | the result artifact format |
| `model_version` | the analysed model |
| `analyzer_version` | the analysis code |
| `inputs.artifacts[].version` | the reliability artifacts consumed |
| `provenance.method_version`, `sampler_version` | the algorithms |

Inputs are recorded as an explicit snapshot — actual ids, values and uncertainty
laws. "The current model" is not a recordable input.

---

## 9. Comparison

`compare(before, after)` reports the top-event change, the input changes, the
assumption changes, the method changes and the importance rank changes.

It does **not** attribute the outcome. `single_change` is true only when
exactly one input changed, in exactly one way, with no assumption or method
difference. An input whose value *and* uncertainty both moved counts as two
simultaneous modifications and does not qualify. Even when it does,
`causal_attribution` scopes the claim to the model and says that whether the
real system changed the same way is a question the tool does not answer.

---

## 10. What this analysis does not do

- It does not decide anything. It never says "fix X". It says
  "X has the highest Fussell-Vesely importance (0.891)" and leaves the decision
  where it belongs.
- It does not model correlated parameter uncertainty.
- It does not implement conditional probability tables or directed dependency
  edges. It refuses models that declare them.
- It does not compute cut-set contributions under a dependency model.
- It does not perform value-of-information analysis.
- It does not calibrate against runtime observations. The result artifact
  carries enough identity and version information for that future workflow, but
  the workflow itself is not implemented.
- It does not replace engineering judgement, and no quantity it produces should
  be read as if it did.
