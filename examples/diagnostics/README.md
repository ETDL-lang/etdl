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

## What's declared, and why no automated-inference language appears

`x-diagnostics` (the same generic `x-*` extension mechanism `x-reliability`/
`x-tree-event`/`x-performance`/`x-safety` already use — **zero parser or
AST changes** were needed for this supplement either), gated by
`supplements: [{id: etdl.diagnostics, ...}]`. Per spec Section 6, this
supplement performs **no automated correlation, root-cause inference,
anomaly detection, or telemetry ingestion of any kind** — it is a static,
author-declared lookup table a human or external triage tool consults.
Whether `gateway-timeout-correlation`'s claim is actually true in production
is an empirical question this specification does not answer.

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

## Compatibility

Comment out the `supplements: [{id: etdl.diagnostics, ...}]` block (leaving
`x-diagnostics` in place) and re-run `etdl validate` — it stays valid with
zero diagnostics-supplement-related diagnostics, and compiling produces
byte-for-byte identical generated Rust, proving `x-diagnostics` is additive
metadata (spec Section 7).
