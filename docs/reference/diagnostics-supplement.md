# ETDL Diagnostics Supplement 1.0 (`etdl.diagnostics`)

Sections marked **NORMATIVE** define required behavior any conforming
implementation must have. Sections marked **INFORMATIVE** are examples,
guidance, and rationale — not requirements. This document summarizes the
normative spec at `ETDL-Diagnostics-Supplement.md` (in the
`etdl-specification` repository) as implemented by `etdl-compiler`; the
spec itself is authoritative if the two ever disagree.

## 1. Purpose (INFORMATIVE)

Declares which runtime telemetry attribute a document's author expects to
correlate with which Fault-Tree cause, for a human or an external tool
doing post-incident triage to consult. A Correlation is always
author-declared, never computed — this supplement still performs no
automated root-cause inference or anomaly detection of its own
(`etdl_core::sla::SlaTracker` already decides *whether* something is
anomalous, unchanged). Unlike earlier revisions, generated code now MAY
*surface* an already-declared Correlation alongside an SLA anomaly it
independently detects at a matching node (§6.1) — presentation of
declared metadata at a moment decided elsewhere, not new inference.

## 2. Scope (NORMATIVE)

This supplement defines:

- The Correlation Object and Anomaly Rule Object data models (§4) and how a
  document declares them (`x-diagnostics`, the same generic `x-*` extension
  mechanism every ETDL extension already uses — **no core language or
  parser change was made or is required**).
- Reference resolution: a Correlation's `causeRef` must resolve to a Gate
  or Basic Event in a declared Fault Tree; an Anomaly Rule's `monitors`
  must resolve to a node of **any** kind; a Correlation's `spanValue`
  must resolve to a real node id when its `spanAttribute` is exactly
  `"etdl.node.id"` (§5).
- How generated code surfaces an already-declared Correlation alongside a
  detected anomaly (§6.1, INFORMATIVE — no new ECEL path).

This supplement does **not** define:

- Any automated correlation, root-cause inference, anomaly detection, or
  telemetry ingestion. It is a static, author-declared lookup table; whether
  a Correlation's claim is empirically accurate is not answered here.
- Any new detection mechanism, or ECEL path — `SlaTracker`'s per-call
  lifetime and the reference runtime's `eprintln!`-based telemetry stubs
  are unchanged; §6.1 only threads one additional optional argument
  through an already-existing call.

## 3. Terminology (NORMATIVE)

| Term | Meaning |
|---|---|
| Correlation | A Correlation Object (§4.1): a declared association between a runtime telemetry span attribute/value and a Fault-Tree cause |
| Cause | A Fault-Tree Gate or Basic Event, referenced by a Correlation's `causeRef` |
| Anomaly Rule | An Anomaly Rule Object (§4.2): a declaration that a node's runtime behavior is worth monitoring, without specifying a new detection mechanism (`etdl_core::sla::SlaTracker` already exists for that) |

## 4. Data model (NORMATIVE)

```rust
pub struct Correlation {
    pub id: String,
    pub span_attribute: String,  // spanAttribute
    pub span_value: String,      // spanValue
    pub cause_ref: String,       // causeRef
    pub description: Option<String>,
}

pub struct AnomalyRule {
    pub id: String,
    pub monitors: String,        // any node kind
    pub description: Option<String>,
}
```

`id`/`spanAttribute`/`spanValue`/`causeRef` are REQUIRED on a Correlation
and unique within `x-diagnostics.correlations`. `id`/`monitors` are
REQUIRED on an Anomaly Rule and unique within `x-diagnostics.anomalyRules`
— the two collections are independent namespaces; a Correlation and an
Anomaly Rule may share an `id`.

## 5. Reference resolution (NORMATIVE)

`causeRef` (`^#/faultTrees/[^/]+/(gates|basicEvents)/[^/]+$`) is checked
against the document's own `faultTrees`. `monitors`
(`^#/eventTrees/[^/]+/nodes/[^/]+$`) is checked against the document's own
`eventTrees` and accepts **any** node kind (Barrier, Operation, or
Consequence) — unlike Performance's Operation-only and Safety's
Barrier-only `nodeRef` restrictions. Both use the same manual-parse style
(`performance::resolve_node_ref`/`safety`'s equivalents); no generic
JSON-Pointer resolver exists in this codebase for same-document references.

Unlike earlier revisions, a Correlation's `spanValue` is *also* checked
(`node_id_exists_anywhere`, document-wide — not scoped to one tree) when
its `spanAttribute` is exactly `"etdl.node.id"`, the only attribute the
reference runtime's `etdl_core::telemetry::attach_node_span_attribute`
ever emits — `E-152` otherwise. Any other `spanAttribute` value is left
unchecked (both fields are free-form per §4.1; this specification does
not own or interpret third-party telemetry attribute names).

## 6. Compiler integration (NORMATIVE)

Implemented entirely in `etdl-compiler::diagnostics` — a plain module, no
dedicated structural crate, for the same reasoning as
[Performance](performance-supplement.md)/[Safety](safety-supplement.md).

**Registered, but not pipeline-special-cased (NORMATIVE for 1.0).**
`DiagnosticsExtension` is registered unconditionally in
`extension::builtin_registry()` and separately seeded into
`Compiler::new()`'s `extensions` list, running through the same generic
`EtdlExtension::validate`/`process` path a third-party
`Compiler::with_extension` supplement uses — the same shape as Performance
and Safety, not a special-cased direct call in `lib.rs`.
`DiagnosticsExtension::descriptor()` returns a `SupplementDescriptor`
colocated with `parse_and_validate_diagnostics` in this same module, which
`etdl capabilities`/`etdl supplement list` read generically — see
[Performance](performance-supplement.md#7-compiler-integration-normative)
for the full mechanism.

### 6.1 Surfacing a correlation alongside a detected anomaly (INFORMATIVE)

Codegen (`etdl-compiler::codegen::rust`) reads `diagnostics::DiagnosticsData`
(parsed once per `generate_all` call, `CodegenCtx.diagnostics`, mirroring
`CodegenCtx.safety`/`CodegenCtx.security`). For a node whose id matches
some Correlation's `spanValue` (`spanAttribute == "etdl.node.id"`,
`diagnostics_correlation_for`), generated code calls
`etdl_core::monitor::BranchMonitor::record_branch_with_cause`,
`record_failure_with_cause`, or `record_success_with_cause` instead of
the plain `record_branch`/`record_failure`/`record_success` — passing the
matched Correlation's `causeRef`/`description` as extra arguments. Those
methods are otherwise identical to their plain counterparts; the only
difference is that `etdl_core::telemetry::emit_anomaly_event` receives an
extra `Some(CorrelatedCause { .. })` argument when an anomaly is
detected, appending `cause_ref=... description=...` to its (still
`eprintln!`-based) output. A node with no matching Correlation, or a
document that doesn't declare `etdl.diagnostics` at all, keeps generating
the plain calls — byte-identical to before this feature existed. "First
match wins" if more than one Correlation targets the same node id; this
module doesn't otherwise forbid that overlap.

## 7. `x-diagnostics` example (INFORMATIVE)

```yaml
supplements:
  - id: etdl.diagnostics
    version: "1.0"

x-diagnostics:
  correlations:
    - id: gateway-timeout-correlation
      spanAttribute: "etdl.node.id"
      spanValue: "ProcessPaymentOperation"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
  anomalyRules:
    - id: payment-operation-anomaly
      monitors: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
```

See `examples/diagnostics/correlation-demo.etdl` for a complete, runnable
document.

## 8. Validation (NORMATIVE)

`diagnostics::parse_and_validate_diagnostics` checks, collecting every
problem in one pass:

1. `x-diagnostics` is only processed when the document declares
   `supplements: [{id: etdl.diagnostics, ...}]` (§9).
2. A `correlations`/`anomalyRules` key that is present but fails to
   deserialize as an array is `E-150` (no dedicated "manifest invalid" code
   exists here either — folded into E-150, matching precedent).
3. A duplicate `id` within `correlations` or within `anomalyRules` is
   `E-151` (this one *is* explicit in the spec's diagnostic table, unlike
   Performance's/Safety's own duplicate-id gap).
4. An unresolvable `causeRef`/`monitors` (§5) is `E-150`.
5. A Correlation's `spanAttribute` exactly `"etdl.node.id"` with a
   `spanValue` not naming a real node anywhere in the document (§5) is
   `E-152`.
6. **W-412**: an Anomaly Rule's `monitors` node is an Operation, and either
   it has no `onFailureProbabilitySource` at all, or it does but no
   declared Correlation's `causeRef` targets the *same* Fault Tree that
   source points into. `monitors` resolving to a Barrier or Consequence is
   never checked by this rule. This is one interpretation of the spec's own
   somewhat open-ended prose ("neither `onFailureProbabilitySource` nor any
   Fault Tree reachable from this document that a Correlation Object's
   `causeRef` could plausibly connect it to") — see `diagnostics.rs`'s
   module doc comment for the exact reading implemented.

## 9. Compatibility (NORMATIVE)

Silently ignoring `x-diagnostics` (core Section 11.1's baseline behavior)
leaves a document fully valid under core alone — correlation and
anomaly-rule metadata are additive, never a precondition for parsing,
validation, code generation, or runtime telemetry behavior.
`examples/diagnostics/README.md` demonstrates this directly.

## 10. Versioning (NORMATIVE)

| Axis | Value |
|---|---|
| Schema | `etdl.diagnostics/1.0` (`etdl_compiler::diagnostics::DIAGNOSTICS_SCHEMA`) |
| Supplement id/version (as declared via `supplements:`) | `etdl.diagnostics` / `"1.0"`, checked by the same major-version-gate rule every supplement already uses |

## 20. Diagnostics (NORMATIVE)

| Code | Meaning |
|---|---|
| `E-150` | A Correlation's `causeRef`, or an Anomaly Rule's `monitors`, does not resolve; or `correlations`/`anomalyRules` failed to deserialize |
| `E-151` | Two Correlation Objects, or two Anomaly Rule Objects, declare the same `id` (within their own collection) |
| `E-152` | A Correlation's `spanAttribute` is exactly `"etdl.node.id"` but `spanValue` does not name a real node anywhere in the document |
| `W-412` | A monitored Operation has no correlated cause on record (§8, rule 6) |

## 21. CLI (INFORMATIVE)

No dedicated `etdl diagnostics ...` subcommand exists, for the same
reasoning [Performance](performance-supplement.md#21-cli-informative)
gives.

```bash
etdl validate examples/diagnostics/correlation-demo.etdl
etdl compile examples/diagnostics/correlation-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.diagnostics")'
```
