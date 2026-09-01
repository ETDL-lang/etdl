# Worked example: ETDL Diagnostics Supplement 1.0

`correlation-demo.etdl` declares one Correlation (a telemetry span
attribute/value expected to trace back to a Fault-Tree cause) and one
Anomaly Rule (a node worth watching for anomalies), against an Operation
whose `onFailureProbabilitySource` points at a real Fault Tree.

```bash
etdl validate correlation-demo.etdl
etdl compile correlation-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.diagnostics")'
```

```text
$ etdl validate correlation-demo.etdl
document 'correlation-demo.etdl' is valid (0 errors, 0 warnings)
```

## What's declared

`x-diagnostics` (the same generic `x-*` extension mechanism `x-reliability`/
`x-tree-event`/`x-performance`/`x-safety` already use — **zero parser or
AST changes** were needed for this supplement either), gated by
`supplements: [{id: etdl.diagnostics, ...}]`. Per spec Section 6, this
supplement still performs **no automated correlation, root-cause
inference, or anomaly detection of any kind** — a Correlation is always
author-declared, never computed, and whether
`gateway-timeout-correlation`'s claim is actually true in production is an
empirical question this specification does not answer.

Unlike earlier revisions, generated code *does* now surface an
already-declared Correlation alongside an SLA anomaly it independently
detects at a matching node: inspect the generated `.rs` file for this
document and look for `record_failure_with_cause`/
`record_success_with_cause` on `ProcessPaymentOperation` (spec Section
6) — the plain `record_failure`/`record_success` calls a document
without this Correlation would generate are still there for every
*other*, uncorrelated node.

## Triggering each diagnostic

```yaml
# E-150: causeRef doesn't resolve to a Gate or Basic Event
causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/DoesNotExist"
```

```yaml
# E-151: two Correlation Objects (or two Anomaly Rule Objects) share an id
correlations:
  - id: dup
    ...
  - id: dup
    ...
```

```yaml
# W-412: the monitored Operation has no correlated cause on record
# (delete the correlations: block above, or point causeRef at a
# different Fault Tree than the monitored Operation's
# onFailureProbabilitySource)
```

`W-412` is a warning, not a rejection — `etdl validate` still exits 0. Try
deleting the `correlations:` block entirely (leaving `anomalyRules:` in
place) to see it fire.

```yaml
# E-152: spanAttribute is "etdl.node.id" but spanValue names no real node
spanAttribute: "etdl.node.id"
spanValue: "NoSuchNode"
```

`E-152` is only checked for `spanAttribute: "etdl.node.id"` specifically —
the one attribute the reference runtime ever emits
(`etdl_core::telemetry::attach_node_span_attribute`); any other
`spanAttribute` value is left unchecked, since both fields are otherwise
free-form (spec Section 4.1).

## Compatibility

Comment out the `supplements: [{id: etdl.diagnostics, ...}]` block (leaving
`x-diagnostics` in place) and re-run `etdl validate` — it stays valid with
zero diagnostics-supplement-related diagnostics, and compiling produces
byte-for-byte identical generated Rust (plain `record_failure`/
`record_success` calls, not the `_with_cause` variants), proving
`x-diagnostics` is additive metadata (spec Section 7).
