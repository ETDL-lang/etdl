# ETDL Performance Supplement 1.0 (`etdl.performance`)

Sections marked **NORMATIVE** define required behavior any conforming
implementation must have. Sections marked **INFORMATIVE** are examples,
guidance, and rationale — not requirements. This document summarizes the
normative spec at `ETDL-Performance-Supplement.md` (in the
`etdl-specification` repository) as implemented by `etdl-compiler`; the spec
itself is authoritative if the two ever disagree.

## 1. Purpose (INFORMATIVE)

Declares latency percentile targets and, optionally, a throughput
expectation against structure ETDL Core already defines — an Operation node
or a whole Event Tree. It gives no new enforcement mechanism: a budget is a
declared expectation for downstream tooling and deployment configuration to
consult, not something this compiler measures or enforces against real
runtime latency.

## 2. Scope (NORMATIVE)

This supplement defines:

- The Budget Object data model (§4) and how a document declares budgets
  (`x-performance`, the same generic `x-*` extension mechanism every ETDL
  extension already uses — **no core language or parser change was made or
  is required**).
- Reference resolution: a Budget's `nodeRef` must resolve to an Operation
  node or an Event Tree already defined elsewhere in the same document.
- Validation (§9) and diagnostics (§20).

This supplement does **not** define:

- Any runtime enforcement mechanism, SLA translation, or automatic mapping
  onto `etdl_core::sla::SlaTracker` (§6).
- Any probability, retry, or timeout semantics — those remain exactly what
  core Section 5.9 (`timeoutMs`, `retryPolicy`) already defines, unchanged.

## 3. Terminology (NORMATIVE)

| Term | Meaning |
|---|---|
| Budget | A Budget Object (§4): declared latency percentile targets, and optionally a throughput expectation, for one Operation node or one whole Event Tree |
| `nodeRef` | An Internal Reference resolving to either `#/eventTrees/<id>/nodes/<operation-id>` (a single Operation) or `#/eventTrees/<id>` (a whole Event Tree) |
| Percentile target | `p50Ms`/`p95Ms`/`p99Ms`: the latency, in milliseconds, a Budget declares acceptable at that percentile |

## 4. Data model (NORMATIVE)

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

## 5. Reference resolution (NORMATIVE)

`nodeRef` is checked against the document's own `eventTrees`, not a generic
JSON-Pointer walk: `#/eventTrees/<tree-id>` must name a declared Event Tree;
`#/eventTrees/<tree-id>/nodes/<node-id>` must name a node that is
specifically an **Operation** node (not a Barrier or Consequence) within
that tree. A `nodeRef` that resolves to neither shape is `E-160` (§20).

## 6. Relationship to runtime enforcement (NORMATIVE)

A Budget's percentile targets are declarative, not self-enforcing. The
reference `etdl_core` runtime already has the mechanism a deployment would
configure to watch for a budget violation: `SlaTracker`'s rolling window
(`ETDL_SLA_WINDOW`, `ETDL_SLA_THRESHOLD`). This supplement performs no
automatic translation from a declared `p95Ms` into an `ETDL_SLA_THRESHOLD`
value — that translation, if a deployment wants it, is an operational
decision outside this specification's scope.

## 7. Compiler integration (NORMATIVE)

Implemented entirely in `etdl-compiler::performance` — a plain module, no
dedicated structural crate (unlike the Tree Event Supplement's
`etdl-tree-core`), since a Budget only cross-references the document's own
existing `eventTrees` and has no reusable structural model a third domain
would independently consume.

**Registered, but not pipeline-special-cased (NORMATIVE for 1.0).** Unlike
the Tree Event and Reliability supplements — each of which has its own
bespoke, hard-coded call inside `Compiler`'s pipeline (`lib.rs`) — the
Performance Supplement is wired in generically:

- `PerformanceExtension` is registered unconditionally in
  `extension::builtin_registry()`, exactly like the other two, so `etdl
  capabilities`/`etdl supplement list`/the "is this supplement supported"
  check (E-108/W-407) all see it.
- `Compiler::new()` additionally seeds `Compiler::extensions` with a
  `PerformanceExtension` instance, so it runs through the same generic,
  registry-driven `EtdlExtension::validate`/`process` path
  (`Compiler::run_extensions`) a third-party `Compiler::with_extension`
  supplement uses — not a special-cased direct function call anywhere in
  `lib.rs`.
- `PerformanceExtension::descriptor()` (`EtdlExtension::descriptor`) returns
  a `SupplementDescriptor { summary, schema, diagnostic_codes, requires }`
  colocated with `parse_and_validate_budgets` in this same module — `etdl
  capabilities`/`etdl supplement list` read it generically via
  `builtin_registry()`, so this page's own §20 diagnostics table and the
  CLI's output are two views of the same source, not two hand-kept copies.

"Built-in" therefore means only that it ships compiled into the binary and
is auto-registered — not that it has bespoke pipeline code of its own. A
future core supplement should prefer this shape over adding another
special-cased call.

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
```

See `examples/performance/budget-demo.etdl` for a complete, runnable
document.

## 9. Validation (NORMATIVE)

`performance::parse_and_validate_budgets` checks, collecting every problem
in one pass:

1. `x-performance` is only processed when the document declares
   `supplements: [{id: etdl.performance, ...}]` — never merely because the
   field is present (§10).
2. A `budgets` key that is present but fails to deserialize as an array of
   Budget Objects is `E-160`.
3. A duplicate `id` within `budgets` is `E-160` (see §20's note — the
   normative spec's own diagnostic table has no dedicated code for this
   case, so it is folded into E-160's existing multi-condition bucket).
4. An unresolvable `nodeRef` (§5) is `E-160`.
5. A non-finite or non-positive `p50Ms`/`p95Ms`/`p99Ms`, or a present but
   non-positive `maxConcurrency`/`expectedRatePerSecond`, is `E-160`.
6. `p50Ms > p95Ms` or `p95Ms > p99Ms` is `E-161`, checked independently of
   rule 5 — a budget with garbage percentile values gets exactly the
   diagnostics its actual values warrant, never a spurious extra one.
7. Two budgets declaring the same `nodeRef` is `W-413` — a warning, not a
   rejection; both budgets remain in the accepted result.

## 10. Compatibility (NORMATIVE)

Silently ignoring `x-performance` (core Section 11.1's baseline behavior)
leaves a document fully valid under core alone — declared budgets are
additive metadata, never a precondition for parsing, validation, or code
generation. `examples/performance/README.md` demonstrates this directly:
removing the `supplements:` declaration while leaving `x-performance` in
place produces zero performance-related diagnostics.

## 11. Versioning (NORMATIVE)

| Axis | Value |
|---|---|
| Schema | `etdl.performance/1.0` (`etdl_compiler::performance::PERFORMANCE_SCHEMA`) |
| Supplement id/version (as declared via `supplements:`) | `etdl.performance` / `"1.0"`, checked by the same major-version-gate rule every supplement already uses |

A future `1.x` minor may add fields to the Budget Object (e.g. a memory or
cost budget); it must not change the meaning of `p50Ms`/`p95Ms`/`p99Ms`
ordering without a major bump.

## 20. Diagnostics (NORMATIVE)

| Code | Meaning |
|---|---|
| `E-160` | A Budget Object's `nodeRef` does not resolve to an Event Tree or Operation node; a percentile/`maxConcurrency`/`expectedRatePerSecond` value is non-positive or non-finite; `budgets` failed to deserialize; or a duplicate `id` was declared |
| `E-161` | A Budget Object's percentile ordering is violated (`p50Ms > p95Ms` or `p95Ms > p99Ms`) |
| `W-413` | Two Budget Objects declare the same `nodeRef` — not an error, but only one is meaningfully authoritative for that node |

`E-160`/`E-161`/`W-413` are scoped to this supplement's own namespace of
meaning; they do not collide with core Section 7's codes or with any other
supplement's codes.

## 21. CLI (INFORMATIVE)

No dedicated `etdl budget ...` subcommand exists, unlike the Tree Event
Supplement's `etdl tree validate`/`etdl tree inspect`. That command pair
exists because a tree has a genuine extract-and-render use case (preorder
listing, leaf enumeration for external tooling); a Budget Object is five
scalar fields plus a reference, already fully visible via `etdl validate
--json`/`etdl compile` diagnostics and `etdl capabilities`. `etdl
validate`/`etdl compile` surface `E-160`/`E-161`/`W-413` automatically —
declaring `supplements: [{id: etdl.performance, version: "1.0"}]` is the
only opt-in required:

```bash
etdl validate examples/performance/budget-demo.etdl
etdl compile examples/performance/budget-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.performance")'
```
