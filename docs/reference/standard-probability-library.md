# ETDL Standard Probability Library 1.0 (`std.probability`)

This document specifies `std.probability`: the domain-neutral mathematical
foundation beneath the reliability, and any future safety/security/risk,
domain. It extends [`standard-library.md`](standard-library.md) (the
five-layer architecture, import mechanism, versioning axes) with
`std.probability`-specific detail.

## Where this sits

```text
ETDL Core
   |
ETDL Standard Library
   |
std.probability      <- this document; native layer = etdl-probability-core
   |
Domain Libraries      (reliability today; safety/security/risk/predictive
   |                   analysis are future consumers, not implemented here)
   |
User Models
```

**The dependency direction is enforced, not just documented.**
`etdl-probability-core`'s own `Cargo.toml` has zero dependency on any
reliability crate — that is a structural fact checkable in the crate
manifest, not merely a convention. `etdl-reliability` depends on
`etdl-probability-core` (added by this task, via a small adapter module —
see "Reliability integration" below); the reverse dependency does not, and
must never, exist.

## Why most of this is a Rust crate, not ETDL source

ETDL is a declarative YAML document format with no general expression or
function-call syntax. The only embedded mini-language, ECEL, exists solely
to parse barrier branch conditions (comparisons, membership tests) — it has
no arithmetic, no function definitions, and cannot compute `complement(p)`
or a Binomial PMF. There is no honest way to express "compute the
complement of a referenced probability" as ETDL YAML.

What genuinely *is* expressible in pure ETDL — a reusable, named
probability **value** (the "probability literal" concept) — lives in
`etdl-compiler/stdlib/probability/lib.etdl` (three constants: `Certain`, `Impossible`,
`EvenOdds`, as basic events, resolved through the same qualified-id
mechanism as `std.events`/`std.logic`). Everything computational
(composition operations, distributions) is the `etdl-probability-core`
crate — the layer a Rust-implemented compiler extension, a future Tree
Event Supplement, or a domain library links against directly. See
`etdl-probability-core/examples/composition.rs` and
`etdl-probability-core/examples/distributions.rs` for the Rust-side usage
`examples/probability/basic.etdl`'s ETDL-source counterpart cannot show.

## Types

### `Probability` — the primitive

A validated scalar, `0 <= p <= 1`, constructed only through
`Probability::new`, which rejects (never clamps) NaN/infinite and
out-of-range values. `Probability::IMPOSSIBLE` (`0.0`) and
`Probability::CERTAIN` (`1.0`) are provided as named constants. Serializes
as a bare JSON number; deserialization re-validates (an out-of-range value
in a JSON/YAML file fails to deserialize, it does not silently load).

### Probability vs. `ProbabilityEstimate`

**`std.probability` does not introduce a second, competing estimate type.**
`Probability` carries no uncertainty, evidence, method, or provenance — it
is a plain mathematical value. `etdl-reliability-core::estimate::ProbabilityEstimate`
(state, metric, population, time basis, conditions, source, method,
uncertainty, provenance, version, status) remains the authoritative,
unchanged type for anything requiring that richer context, in the
reliability domain specifically. A future safety/security domain would
likely define its own analogous estimate wrapper around `Probability` as
its value type — this crate does not force every probability literal to
become an estimate, and does not force one universal estimate type on
every domain.

| Concept | Type | Owns |
|---|---|---|
| Probability | `etdl_probability_core::Probability` | a bare, validated `[0,1]` value |
| Probability estimate | `etdl_reliability_core::estimate::ProbabilityEstimate` | value + state + method + provenance (reliability domain) |
| Observed frequency | `etdl_reliability::observations::AggregateObservation` | failures/exposure counts (reliability domain; unchanged) |
| Failure rate | `etdl_probability_core::Rate` (generic) / reliability's `ProbabilityMetric::FailureRate` (domain-specific) | a non-negative per-unit quantity, never silently a `Probability` |
| Predictive probability | not implemented | future work; see "Future predictive reliability" |

### `Rate` — distinct from `Probability`

A non-negative quantity tagged with a free-text `per_unit` (e.g. `"hour"`).
There is no `From`/`Into` between `Rate` and `Probability` — converting a
rate to a probability requires an explicit model (e.g. the exponential
failure model already implemented in
`etdl-reliability::analysis::estimator::ExponentialRateEstimator`,
`P(failure by t) = 1 - exp(-lambda*t)`), and this crate never performs
that conversion implicitly. `per_unit` is a free-text label, not a
checked unit type — see "Units" below for why a real unit-checked type is
proposed future work, not implemented here.

## Composition operations

All in `etdl_probability_core::probability` (re-exported at the crate
root). Every operation that assumes something states the assumption in its
name — independence is never inferred.

| Function | Formula | Assumption stated in the name |
|---|---|---|
| `complement(a)` | `1 - P(A)` | none |
| `independent_and(a, b)` / `independent_and_n(&[...])` | `P(A)*P(B)` | independence |
| `independent_or(a, b)` / `independent_or_n(&[...])` | `P(A)+P(B)-P(A)P(B)`, computed as `1 - prod(1-Pi)` for n-ary (numerically stable, avoids inclusion-exclusion blow-up) | independence |
| `mutually_exclusive_or(a, b)` | `P(A)+P(B)` | mutual exclusivity — **rejects** (does not clamp) a sum exceeding 1, since that means the assumption itself is false for the given inputs |
| `conditional(joint, marginal_b)` | `P(A∩B)/P(B)` | none — requires the joint explicitly, never derives it by assuming independence |
| `bayes(likelihood, prior, marginal)` | `P(B\|A)*P(A)/P(B)` | none beyond the inputs supplied; rejects `P(B)=0` |

Mutual exclusivity and independence are kept structurally distinct:
`mutually_exclusive_or(0.2, 0.3) = 0.5` while
`independent_or(0.2, 0.3) = 0.44` for the *same* inputs — see
`probability.rs`'s `mutually_exclusive_and_independent_or_diverge_for_the_same_inputs`
test, which asserts the two never silently agree.

## Distributions

Five foundational distributions, each a validated, immutable value
(construction rejects invalid parameters):

| Distribution | Parameters | Support | Operations |
|---|---|---|---|
| `Bernoulli` | `p: Probability` | `{0, 1}` | `pmf`, `mean`, `variance` |
| `Binomial` | `n: u64 >= 1`, `p: Probability` | `{0, ..., n}` | `pmf`, `cdf`, `mean`, `variance` |
| `Beta` | `alpha > 0`, `beta > 0` | `[0, 1]` | `pdf`, `cdf`, `quantile`, `mean`, `variance` |
| `Exponential` | `lambda > 0` | `[0, infinity)` | `pdf`, `cdf`, `survival`, `quantile`, `mean`, `variance` |
| `Normal` | `mu`, `sigma > 0` | `(-infinity, infinity)` | `pdf`, `cdf`, `quantile`, `mean`, `variance` |

Terminology is used correctly and consistently: **PMF** for discrete
distributions (Bernoulli, Binomial), **PDF** for continuous ones (Beta,
Exponential, Normal), **CDF** for all of them, and **survival function**
(`1 - CDF`) named as such on `Exponential` rather than folded into a
generically-named function — the foundation a future hazard/survival
abstraction builds on without renaming anything.

None of these expose `sample()`. See "Determinism and sampling" below.

## Numerical semantics

- **Binomial PMF** is computed in log-space
  (`log_gamma(n+1) - log_gamma(k+1) - log_gamma(n-k+1) + k*ln(p) + (n-k)*ln(1-p)`,
  then exponentiated) to avoid overflow — a naive `n!/(k!(n-k)!)`
  computation overflows `f64` well before `n` reaches typical fault-tree
  exposure counts (millions of requests). Verified by
  `distribution::binomial::tests::large_n_does_not_overflow` at
  `n = 1,000,000`.
- **Binomial and Beta CDFs** both reduce to the regularized incomplete beta
  function `I_x(a,b)`, via the standard identity
  `P(Binomial(n,p) <= k) = I_{1-p}(n-k, k+1)` — the same identity
  `etdl-reliability`'s calibration module already uses for its own,
  independently implemented, exact binomial test.
- **Exponential CDF** uses `-expm1(-lambda*x)` rather than
  `1.0 - (-lambda*x).exp()`, avoiding catastrophic cancellation for small
  `lambda*x` — verified for `lambda = 1e-9` in
  `distribution::exponential::tests::stable_for_very_small_lambda_times_t`.
- **Normal CDF** uses the Abramowitz & Stegun 7.1.26 rational approximation
  to the error function: documented maximum absolute error ~1.5e-7.
- **Normal quantile** uses Peter Acklam's rational approximation:
  documented maximum relative error ~1.15e-9. (An earlier draft of this
  implementation mixed coefficients from two different published
  approximations and produced wrong results outside the central region;
  the fixed version is tested against known reference values — e.g.
  `normal_quantile(0.975) ~= 1.9599639845` — not just round-trip
  consistency, precisely because that class of bug would not necessarily
  show up in a naive round-trip check alone.)
- **Numerical tolerance policy**: no floating-point transcendental result
  is ever compared with `==`. Direct known-value tests use tight
  tolerances (`1e-9` to `1e-12`, matching each approximation's documented
  accuracy); round-trip tests (CDF then quantile) use looser tolerances
  (`1e-5` to `1e-6`) because they compound two independent approximations'
  errors — this is stated in the test itself, not silently loosened.
- **Floating-point model**: this crate uses plain `f64` throughout — the
  same floating-point semantics (IEEE 754 double precision, standard Rust
  rounding/overflow/underflow behavior) every other numeric type in this
  workspace already uses. No second numeric system was introduced.

## Determinism and sampling

Every function in `etdl-probability-core` is a pure, deterministic
mathematical evaluation — same inputs always produce the same outputs, and
compiler output built from these functions never depends on randomness.
**No function in this crate samples from a distribution.** Random sampling
/ Monte Carlo is explicitly out of scope for this crate:

- It belongs to an *optional* statistics layer, not the built-in
  foundation (see "Built-in vs. optional" below).
- `etdl-reliability::analysis::dependence::sampling` already exists,
  as the reliability domain's own seeded, documented (algorithm +
  version constants: `xorshift64star`) sampler for its dependency-aware
  Monte Carlo propagation — unchanged by this task, and not something
  `std.probability` re-implements or replaces.
- A future optional statistics library consuming `std.probability`'s
  distribution *math* (pdf/cdf/mean/variance — the parts sampling needs to
  validate against) to build a general-purpose sampler is architecturally
  supported (the types are `Copy`/`Serialize`/public) but not built here.

If `std.probability` ever grows a sampling capability, it must (per this
task's own requirement) make the algorithm, seed, and sample count
explicit — exactly the discipline `etdl-reliability`'s existing Monte
Carlo already follows — never implicit or silently nondeterministic
during ordinary compilation.

## Units

`std.probability` reuses the `Rate.per_unit: String` free-text label
rather than inventing a second, competing unit system, and does not
attempt real unit *checking* (nothing stops `Rate{value, per_unit:
"hour"}` and `Rate{value, per_unit: "minute"}` from being combined
incorrectly by calling code — that mistake is caught by neither this crate
nor, currently, anywhere else in ETDL). This mirrors exactly the
`std.units` deferral already documented in `standard-library.md`: ETDL's
core language has no unit-of-measure primitive at all (probabilities,
failure rates, and mission times are raw `f64` everywhere in the AST), and
building a real one is a language-design undertaking out of scope for this
task. `etdl-reliability-core::probability::TimeBasis` (an enum:
per-request, per-hour, ...) remains the reliability domain's own, more
specific, unit-adjacent concept — `std.probability` does not depend on it
(dependency direction) and does not duplicate it.

## Serialization

Every public type (`Probability`, `Rate`, all five distributions)
implements `serde::Serialize`/`Deserialize`. `Probability` serializes as a
bare JSON number and re-validates on deserialize (an out-of-range value in
a file fails to load, rather than silently becoming a `Probability`
outside `[0,1]`). No type in this crate serializes executable code —
every serialized value is plain data (parameters and results), consistent
with the rest of this codebase's "reliability artifacts are data, never
executed" discipline.

## Provenance

A plain mathematical `Probability` (e.g. `0.2`, used directly in a
composition expression) carries no provenance — it does not need any,
per this task's own guidance. An estimated probability derived from
evidence (e.g. `0.0024` from a Beta-Binomial posterior) retains its
*existing* provenance through the reliability system unchanged: nothing
about `ProbabilityEstimate.provenance` (dataset, model, model_version,
generated_at) changed. See "Reliability integration" below for how a
`Probability` value flows into that existing provenance-bearing type.

## Built-in vs. optional

| | Built-in (`etdl-probability-core`, this task) | Optional (future, not implemented) |
|---|---|---|
| Scope | `Probability`, `Rate`, composition operations, five foundational distributions | Heavy numerical analysis, large-scale Monte Carlo, specialized statistical algorithms |
| Rationale | Foundational, broadly useful outside any one domain, cheap to compute, WASM-safe with zero native dependency | Expensive, domain-specific, or requiring a sampler/RNG this crate deliberately excludes |
| Precedent | Mirrors `etdl-reliability-core` (built-in, minimal, WASM-safe) vs. `etdl-reliability` (optional, richer) — the split this repository already established for the reliability domain | `etdl-reliability`'s own `analysis::dependence` (Monte Carlo, importance/sensitivity) already plays this "optional, heavier" role for the reliability domain specifically |

This task does not build a new, separate "optional statistics" crate for
`std.probability` — `etdl-reliability` already demonstrates exactly this
shape (a richer, optional crate consuming a minimal, built-in one) via its
own relationship to `etdl-reliability-core`, and now, via the adapter, to
`etdl-probability-core` as well. Building a third, redundant example would
not demonstrate anything the existing relationship doesn't already prove.

## Reliability integration

`etdl-reliability::probability_adapter` (new, purely additive):

```rust
pub fn estimate_from_probability(
    event: impl Into<String>,
    state: ProbabilityState,
    p: Probability,
) -> ProbabilityEstimate;

pub fn probability_from_estimate(
    estimate: &ProbabilityEstimate,
) -> Result<Probability, ProbabilityAdapterError>;
```

Two directions, both additive — no existing reliability function's
signature or behavior changed. `etdl-reliability/tests/probability_integration.rs`
proves the full chain: a validated `Probability` -> `estimate_from_probability`
-> the *existing*, unmodified `ReliabilityArtifact`/`ArtifactResolver`
machinery, with no special-casing needed for a value that originated from
`etdl-probability-core`.

**Cross-validation, not a rewrite.** `etdl-probability-core` and
`etdl-reliability::analysis::estimator` each implement their own,
independent `log_gamma`/`regularized_beta`/`normal_quantile` — moving that
code out of the existing estimator into the new crate was deliberately
avoided (this task's own non-regression rule: "do not move existing native
implementations unnecessarily"). Two tests in `probability_adapter.rs`
assert the independent implementations agree on the same mathematical
questions (a Binomial CDF, a Beta-Binomial posterior mean) to a documented
tolerance, giving a correctness baseline for any future consolidation
without forcing one now.

## Future predictive reliability

Deliberately *not* implemented in this task, per its own scope:

- **Hazard rate / survival function**: `Exponential::survival` already
  exists under its correct name (not folded into `cdf`) specifically so a
  future time-dependent hazard/survival abstraction has a real foundation
  to extend rather than a renaming exercise.
- **Credible/prediction intervals**: `Beta::quantile` and
  `Normal::quantile` already provide the primitive a future interval
  abstraction would call (e.g. `[beta.quantile(0.025), beta.quantile(0.975)]`
  for a 95% credible interval) — no new interval *type* was introduced
  here beyond what `etdl-reliability-core::Uncertainty` already provides
  for the reliability domain specifically.
- **Time-dependent distributions**: none of the five distributions here
  are time-indexed; a survival-analysis extension is future work.
- **Quantiles**: implemented for `Beta` and `Normal` (cheap given the
  existing bisection/rational-approximation machinery); *not* implemented
  for `Binomial` (no cheap closed form without materially more code) or
  `Exponential` beyond its own analytic quantile, which *is* implemented.

## Future tree-event domains

A future Tree Event Supplement could compute `P(top_event)` using
`std.probability`'s composition operations (`independent_and`/
`independent_or`/`mutually_exclusive_or`, chosen per the tree's declared
gate semantics) without `std.probability` ever becoming aware that fault
trees, gates, or tree evaluation exist — this crate has no fault-tree
vocabulary anywhere in it (no `Gate`, no `BasicEvent`, no `FaultTree`
type). The dependency stays one-directional: tree-event domains would
depend on `std.probability`, never the reverse.

## CLI

`etdl capabilities` reports `std_probability: { available, schema, kind,
distributions, sampling }` — `sampling` is explicitly reported as
`"unavailable"`, never claimed, consistent with "Determinism and sampling"
above. No new command ecosystem was added (`etdl probability ...`) — per
this task's own instruction to prefer `etdl capabilities`/existing
patterns over a separate statistical command surface; `cargo run -p
etdl-probability-core --example composition` and `--example distributions`
serve the "inspect a value/distribution" role interactively.

## Ontology

No ontology changes. `etdl-reliability-ontology`'s `EntryKind` (Event,
Failure, FailureMode, Cause, Mechanism, Effect, Condition, Dependency,
Resource, Barrier, Mitigation) has no `Probability`/`Distribution`/`Rate`
concept to duplicate or extend — these are type-system concepts (what
shape a value has), not failure-taxonomy concepts (what a failure mode
*is*), and the two remain in their existing, separate layers. All existing
ontology entries: **UNCHANGED**.

## Specification

No core ETDL language specification changes. `libraries:`/qualified-id
resolution already exists (from the Standard Library Core task); this task
reuses it exactly as-is for `std.probability`'s three constants and adds
no new document-schema field, no new YAML syntax, and no new parser
behavior. All computational surface area is a Rust crate API, documented
here rather than in the language specification.

## Versioning

| Axis | Value | Distinct from |
|---|---|---|
| ETDL language version | `doc.etdl` (e.g. `"1.0.0"`) | crate/package/schema versions |
| `etdl-probability-core` crate version | Cargo semver (workspace `0.2.2`) | the schema below |
| `std.probability` package schema | `etdl.stdlib.probability/1.0` (`STD_PROBABILITY_SCHEMA`) | the crate version and the ETDL language version — deliberately not made identical to either, per this task's explicit instruction |
| `std.probability` library version (as declared in `etdl-compiler/stdlib/probability/lib.etdl`'s `library.version`) | `"1.0"` | resolved/checked via the same major-version-gate rule already used for every other library import |

## Compatibility

Adding `std.probability` changes nothing about how an existing `.etdl`
document that does not declare `libraries: [std.probability, ...]`
compiles, validates, or analyzes — confirmed by the full existing
workspace test suite passing unchanged (see the CHANGELOG entry for this
task). Existing `ReliabilityArtifact`s remain readable: `std.probability`
introduces no artifact-format change, and the reliability adapter only
*constructs* a `ProbabilityEstimate` from a `Probability` — it never
changes how an existing estimate is parsed or interpreted.
