# ETDL Performance Supplement 1.0 (`etdl.performance`)

Sections marked **NORMATIVE** define required behavior any conforming
implementation must have. Sections marked **INFORMATIVE** are examples,
guidance, and rationale — not requirements. This document summarizes the
normative spec at `ETDL-Performance-Supplement.md` (in the
`etdl-specification` repository) as implemented by `etdl-compiler`; the spec
itself is authoritative if the two ever disagree.

## 1. Purpose (INFORMATIVE)

Declares latency, concurrency, and throughput requirements against
structure ETDL Core already defines — an Operation node or a whole Event
Tree — and, unlike earlier revisions of this supplement, gives them real
teeth: generated code structurally enforces concurrency/rate limits,
derives a real timeout from `p99Ms` when none is declared, and maintains a
live rolling estimate a linked Barrier can validate against via the ECEL
path `performance.in_budget`.

## 2. Scope (NORMATIVE)

This supplement defines:

- The Budget Object data model (§4.1) and how a document declares budgets
  (`x-performance`, the same generic `x-*` extension mechanism every ETDL
  extension already uses — **no core language or parser change was made or
  is required**, including for `performance.in_budget` — see §6.3).
- The Barrier Check Object data model (§4.2): links a core Barrier node to
  a Budget it validates, declared entirely within `x-performance` — the
  same pattern the Safety Supplement already uses for its own
  `x-safety.barriers` (`nodeRef` naming a core Barrier node) rather than a
  new core AST field.
- Reference resolution (§5), validation (§9), and diagnostics (§20).
- Runtime enforcement and observation semantics (§6) — concurrency/rate
  are structurally guaranteed; latency gets a `p99Ms`-derived timeout
  fallback and live percentile tracking.

This supplement does **not** define:

- Any change to `etdl_core::sla::SlaTracker` or its env vars
  (`ETDL_SLA_WINDOW`/`ETDL_SLA_THRESHOLD`) — untouched, a separate
  mechanism for a different concern (declared-vs-observed branch/failure
  probability, not performance).
- Any change to `timeoutMs`/`retryPolicy`'s core meaning (§6.2) — an
  explicit `timeoutMs` always wins; a Budget only fills the gap when none
  is declared.
- Cross-service propagation of live performance data (unlike the Live
  Reliability Supplement's fault-tree values) — performance/concurrency is
  inherently a per-process concern.

## 3. Terminology (NORMATIVE)

| Term | Meaning |
|---|---|
| Budget | A Budget Object (§4.1): declared latency percentile targets, and optionally a concurrency/throughput requirement, for one Operation node or one whole Event Tree |
| `nodeRef` (Budget) | An Internal Reference resolving to either `#/eventTrees/<id>/nodes/<operation-id>` (a single Operation) or `#/eventTrees/<id>` (a whole Event Tree) |
| Percentile target | `p50Ms`/`p95Ms`/`p99Ms`: the latency, in milliseconds, a Budget declares acceptable at that percentile |
| Barrier Check | A Barrier Check Object (§4.2): links a core Barrier node to the Budget it validates via `performance.in_budget` |
| `budgetRef` | A Barrier Check's reference to a Budget Object's `id` |

## 4. Data model (NORMATIVE)

### 4.1 Budget Object

```rust
pub struct Budget {
    pub id: String,
    pub node_ref: String,               // nodeRef
    pub p50_ms: f64,                    // p50Ms
    pub p95_ms: f64,                    // p95Ms
    pub p99_ms: f64,                    // p99Ms
    pub max_concurrency: Option<i64>,   // maxConcurrency
    pub expected_rate_per_second: Option<f64>, // expectedRatePerSecond
}
```

`id` is REQUIRED and unique within `x-performance.budgets`. `p50Ms`/`p95Ms`/
`p99Ms` are REQUIRED, must each be positive and finite, and must satisfy
`p50Ms <= p95Ms <= p99Ms`. `maxConcurrency`/`expectedRatePerSecond` are
OPTIONAL and, if present, must be positive. `budgets` itself is OPTIONAL at
the top level of `x-performance` — an empty or absent list is not an error.

### 4.2 Barrier Check Object

```rust
pub struct BarrierCheck {
    pub id: String,
    pub node_ref: String,   // nodeRef — must resolve to a Barrier node
    pub budget_ref: String, // budgetRef — must resolve to a budgets[].id
}
```

`id` REQUIRED, unique within `x-performance.barrierChecks`. `nodeRef`
REQUIRED, node-level shape only (`^#/eventTrees/[^/]+/nodes/[^/]+$`), must
name a **Barrier** node. `budgetRef` REQUIRED, must equal some
`budgets[].id` in the same document. `barrierChecks` itself is OPTIONAL at
the top level — an empty or absent list is not an error, and a document
using only enforcement (§6.1/6.2) with no Barrier validation needs none at
all.

## 5. Reference resolution (NORMATIVE)

A Budget's `nodeRef` is checked against the document's own `eventTrees`,
not a generic JSON-Pointer walk: `#/eventTrees/<tree-id>` must name a
declared Event Tree; `#/eventTrees/<tree-id>/nodes/<node-id>` must name a
node that is specifically an **Operation** node (not a Barrier or
Consequence) within that tree. A `nodeRef` that resolves to neither shape
is `E-160` (§20).

A Barrier Check's `nodeRef` must name a **Barrier** node specifically (no
whole-tree form) — `E-162` otherwise. Its `budgetRef` must equal a Budget
`id` that itself parsed and validated successfully (a `budgetRef` naming a
Budget that failed its own validation is treated as unresolvable) — `E-162`
otherwise.

## 6. Runtime enforcement and observation (NORMATIVE)

Unlike earlier revisions of this supplement, a Budget's requirements are
**structurally enforced and continuously observed** by generated code —
implemented in `etdl_core::perf` (`etdl-core/src/perf.rs`), a small,
always-compiled-in module (no Cargo feature gate — it needs only `tokio`'s
`time`/`sync` features, which `etdl-core` already depends on
unconditionally). `etdl_core::sla::SlaTracker` is unrelated and untouched.

### 6.1 Concurrency and rate (`maxConcurrency`, `expectedRatePerSecond`)

Generated code acquires a concurrency permit (a `tokio::sync::Semaphore`
sized to `maxConcurrency`) and, if declared, a rate token (a small
hand-rolled async token bucket, since no existing dependency provides
one) **before** invoking the guarded code — `etdl_core::perf::enter`, a
real, unconditional `.await` wait, never advisory. The number of
concurrent guarded calls can never exceed `maxConcurrency`, by
construction.

- **Operation-level Budget**: wraps the single Operation's own handler
  call (`retry.execute(...)`, see §6.2).
- **Whole-Event-Tree Budget**: wraps the entire generated handler
  function, via a guard held for the function's lifetime (RAII — the
  handler has multiple exit points, e.g. an unretried Operation failure's
  early `return`, so recording/release happens on `Drop`, not an explicit
  call).

### 6.2 Latency (`p50Ms`/`p95Ms`/`p99Ms`)

Latency cannot be guaranteed before a call runs — only bounded and
observed:

- If the Operation declares an explicit `timeoutMs` (core Section 5.9),
  it is used unchanged.
- If not, and a Budget applies, the Budget's `p99Ms` becomes the
  effective per-attempt timeout. If the Operation also declares no
  `retryPolicy`, a single-attempt `RetryPolicy` (`max_attempts: 1`) is
  synthesized purely so the call still goes through `retry.execute`'s
  existing timeout-wrapped path — without this, an Operation with neither
  `retryPolicy` nor `timeoutMs` has **no timeout at all** today (core's
  own pre-existing behavior; a Budget's `p99Ms` would otherwise silently
  enforce nothing).
- Every observed call duration (including any time spent waiting for
  capacity in §6.1) is recorded into a bounded rolling window
  `performance.in_budget` (§6.3) reads from.

**Implication worth knowing**: when `p99Ms` also serves as the enforced
timeout (no explicit `timeoutMs`), a call can never be *observed*
exceeding `p99Ms` — it gets cut off at `p99Ms` first, so the recorded
sample never exceeds that ceiling either. `p99Ms`'s own contribution to
`performance.in_budget`'s check is then effectively vacuous; `p50Ms`/
`p95Ms` (smaller, not separately enforced) still meaningfully detect
drift. A document that wants `p99Ms` itself to be a meaningful, observable
ceiling — not just an enforced cutoff — should declare an explicit,
larger `timeoutMs` on the Operation (see
`etdl-cli/tests/fixtures/performance-check.etdl`, which does exactly this
for its own `in_budget` proof).

A whole-Event-Tree Budget's percentiles are **observational only** — §6.1's
capacity enforcement still applies at the tree level, but this version
derives no new end-to-end timeout from a tree-level `p99Ms` (would require
wrapping the entire handler body in `tokio::time::timeout`, a larger
structural codegen change — a documented follow-up, not built here).

### 6.3 `performance.in_budget` (ECEL)

A Barrier named by a Barrier Check's `nodeRef` may use
`performance.in_budget` in a branch condition — reusing ECEL's existing
Comparison grammar (`performance.in_budget == true`), the same choice the
Live Reliability Supplement made for `reliability.in_range`, rather than
introducing new grammar. `etdl-parser::ecel::parse_root_var` accepts
`performance` as a third path root (alongside `message`/`reliability`) —
grammar-level only; `etdl-compiler::typeck` is what gives the shape
meaning and rejects anything else as `E-163`.

It resolves via `etdl_core::perf::in_budget(budget_id)` to whether the
linked Budget currently appears to be met: every declared percentile is
at or under its ceiling, concurrency has not saturated `maxConcurrency`
(if declared, checked via `Semaphore::available_permits() == 0`), and the
observed rate over the last second has not exceeded
`expectedRatePerSecond` (if declared). With fewer than 5 latency
observations so far, it resolves to `true` (fail-open — the same
"insufficient data is not an anomaly" convention `SlaTracker`/Live
Reliability's `in_range` both already use).

Resolution is **implicit**, mirroring `reliability.in_range`: no node id
is written in the expression — codegen resolves it from the *enclosing
barrier's own* `x-performance.barrierChecks` entry (looked up by this
barrier's `nodeRef`), not any field on the branch itself, since (unlike
`probability_source`) there is no core AST field carrying this link. A
Barrier with no matching `barrierChecks` entry using
`performance.in_budget` is a codegen-time error, `E-109` (refuses to
compile, never silently emits a meaningless call) — should be
unreachable given `E-163` already requires the supplement declared, but
codegen stays defensive rather than trusting that alone.

## 7. Compiler integration (NORMATIVE)

Implemented entirely in `etdl-compiler::performance` — a plain module, no
dedicated structural crate (unlike the Tree Event Supplement's
`etdl-tree-core`), since a Budget/Barrier Check only cross-references the
document's own existing `eventTrees` and has no reusable structural model
a third domain would independently consume.

**Registered, but not pipeline-special-cased (NORMATIVE for 1.0).** Unlike
the Tree Event and Reliability supplements — each of which has its own
bespoke, hard-coded call inside `Compiler`'s pipeline (`lib.rs`) — the
Performance Supplement is wired in generically:

- `PerformanceExtension` is registered unconditionally in
  `extension::builtin_registry()`, exactly like the other core
  supplements, so `etdl capabilities`/`etdl supplement list`/the "is this
  supplement supported" check (E-108/W-407) all see it.
- `Compiler::new()` additionally seeds `Compiler::extensions` with a
  `PerformanceExtension` instance, so it runs through the same generic,
  registry-driven `EtdlExtension::validate`/`process` path
  (`Compiler::run_extensions`) a third-party `Compiler::with_extension`
  supplement uses — not a special-cased direct function call anywhere in
  `lib.rs`.
- `PerformanceExtension::descriptor()` (`EtdlExtension::descriptor`) returns
  a `SupplementDescriptor { summary, schema, diagnostic_codes, requires }`
  colocated with `parse_and_validate_performance` in this same module —
  `etdl capabilities`/`etdl supplement list` read it generically via
  `builtin_registry()`, so this page's own §20 diagnostics table and the
  CLI's output are two views of the same source, not two hand-kept copies.

Codegen (`etdl-compiler::codegen::rust`) reads `performance::PerformanceData`
(parsed once per `generate_all` call, `CodegenCtx.performance`, mirroring
the Live Reliability Supplement's own `CodegenCtx.live_reliability`) and
emits, only where a Budget/Barrier Check applies:

- One idempotent, `std::sync::Once`-guarded `etdl_core::perf::register_budget`
  call per declared Budget, invoked at the top of every handler
  (`generate_performance_registration`).
- Operation-level enforcement (§6.1/6.2) in `generate_operation_code`.
- Whole-Event-Tree enforcement (§6.1) in `generate_event_tree_handler`.
- `performance.in_budget` rendering (§6.3) in `render_condition`/
  `try_render_performance_condition`, alongside the pre-existing
  `reliability.in_range` rendering.

A document that does not declare `etdl.performance` produces
byte-identical generated code to a build without this supplement at all.

## 8. `x-performance` example (INFORMATIVE)

```yaml
supplements:
  - id: etdl.performance
    version: "1.0"

x-performance:
  budgets:
    - id: process-payment-budget
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
      p50Ms: 150
      p95Ms: 800
      p99Ms: 2000
      maxConcurrency: 200
      expectedRatePerSecond: 50
    - id: order-fulfillment-e2e-budget
      nodeRef: "#/eventTrees/OrderFulfillment"
      p50Ms: 400
      p95Ms: 2500
      p99Ms: 5000
  barrierChecks:
    - id: payment-perf-guard
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/PaymentPerfBarrier"
      budgetRef: process-payment-budget
```

See `examples/performance/budget-demo.etdl` for a complete, runnable
document.

## 9. Validation (NORMATIVE)

`performance::parse_and_validate_performance` checks, collecting every
problem in one pass:

1. `x-performance` is only processed when the document declares
   `supplements: [{id: etdl.performance, ...}]` — never merely because the
   field is present (§10).
2. A `budgets` key that is present but fails to deserialize as an array of
   Budget Objects is `E-160`.
3. A duplicate `id` within `budgets` is `E-160` (see §20's note — the
   normative spec's own diagnostic table has no dedicated code for this
   case, so it is folded into E-160's existing multi-condition bucket).
4. An unresolvable Budget `nodeRef` (§5) is `E-160`.
5. A non-finite or non-positive `p50Ms`/`p95Ms`/`p99Ms`, or a present but
   non-positive `maxConcurrency`/`expectedRatePerSecond`, is `E-160`.
6. `p50Ms > p95Ms` or `p95Ms > p99Ms` is `E-161`, checked independently of
   rule 5 — a budget with garbage percentile values gets exactly the
   diagnostics its actual values warrant, never a spurious extra one.
7. Two budgets declaring the same `nodeRef` is `W-413` — a warning, not a
   rejection; both budgets remain in the accepted result.
8. A `barrierChecks` key that fails to deserialize, a duplicate
   `barrierChecks` id, an unresolvable Barrier Check `nodeRef` (§5), or an
   unresolvable `budgetRef` (§5), is `E-162`.
9. Two `barrierChecks` entries declaring the same `nodeRef` is `W-415` —
   a warning, not a rejection; both entries remain in the accepted result.
10. A branch condition's `performance.*` path misuse (wrong shape, missing
    supplement declaration, or nested inside `&&`/`\|\|`/`!`) is `E-163`,
    reported by `typeck`, not this function.

## 10. Compatibility (NORMATIVE)

Silently ignoring `x-performance` (core Section 11.1's baseline behavior)
leaves a document fully valid under core alone — a document that does not
declare `etdl.performance` gets no Budget/Barrier Check processing, no
concurrency/rate enforcement, no timeout fallback, and no
`performance.in_budget` availability; generated code is unaffected.
`examples/performance/README.md` demonstrates this directly: removing the
`supplements:` declaration while leaving `x-performance` in place produces
zero performance-related diagnostics and zero effect on generated code.

## 11. Versioning (NORMATIVE)

| Axis | Value |
|---|---|
| Schema | `etdl.performance/1.0` (`etdl_compiler::performance::PERFORMANCE_SCHEMA`) |
| Supplement id/version (as declared via `supplements:`) | `etdl.performance` / `"1.0"`, checked by the same major-version-gate rule every supplement already uses |

This supplement was extended in place, still at version `1.0` — the
entire specification remains `Status: Under Development — NOT YET
RELEASED`, so there is no released `1.0` behavior to protect against this
change. A future `1.x` minor may add fields to the Budget or Barrier Check
Object (e.g. a memory or cost budget); it must not change the meaning of
`p50Ms`/`p95Ms`/`p99Ms` ordering, §6's enforcement semantics, or
`performance.in_budget`'s resolution without a major bump.

## 20. Diagnostics (NORMATIVE)

| Code | Meaning |
|---|---|
| `E-160` | A Budget Object's `nodeRef` does not resolve to an Event Tree or Operation node; a percentile/`maxConcurrency`/`expectedRatePerSecond` value is non-positive or non-finite; `budgets` failed to deserialize; or a duplicate `id` was declared |
| `E-161` | A Budget Object's percentile ordering is violated (`p50Ms > p95Ms` or `p95Ms > p99Ms`) |
| `E-162` | A Barrier Check Object's `nodeRef` does not resolve to a Barrier node, its `budgetRef` does not resolve to a declared Budget `id`, `barrierChecks` failed to deserialize, or a duplicate Barrier Check `id` was declared |
| `E-163` | A branch condition uses the `performance.*` ECEL path root without the document declaring `etdl.performance`, the path is not exactly `performance.in_budget`, or it is combined with `&&`/`\|\|`/`!` instead of being the entire condition — reported by `typeck`, not `performance::parse_and_validate_performance` |
| `W-413` | Two Budget Objects declare the same `nodeRef` — not an error, but only one is meaningfully authoritative for that node |
| `W-415` | Two Barrier Check Objects declare the same `nodeRef` — not an error, but only one is meaningfully authoritative for that Barrier |

`E-160`–`E-163`/`W-413`/`W-415` are scoped to this supplement's own
namespace of meaning; they do not collide with core Section 7's codes or
with any other supplement's codes.

## 21. CLI (INFORMATIVE)

No dedicated `etdl budget ...` subcommand exists, unlike the Tree Event
Supplement's `etdl tree validate`/`etdl tree inspect`. That command pair
exists because a tree has a genuine extract-and-render use case (preorder
listing, leaf enumeration for external tooling); a Budget/Barrier Check
Object is a handful of scalar fields plus references, already fully
visible via `etdl validate --json`/`etdl compile` diagnostics and `etdl
capabilities`. `etdl validate`/`etdl compile` surface `E-160`–`E-163`/
`W-413`/`W-415` automatically — declaring `supplements: [{id:
etdl.performance, version: "1.0"}]` is the only opt-in required:

```bash
etdl validate examples/performance/budget-demo.etdl
etdl compile examples/performance/budget-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.performance")'
```

The generated code's own runtime behavior (concurrency blocking, timeout
enforcement, `performance.in_budget`) is not something `etdl compile`
itself demonstrates — see `etdl-compiler/tests/performance_codegen_test.rs`
for a real, `cargo run`-executed proof.
