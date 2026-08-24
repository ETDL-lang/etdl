# Worked example: ETDL Performance Supplement 1.0

`budget-demo.etdl` declares two Budget Objects (`ETDL-Performance-Supplement.md`
Section 4.1) against one Event Tree: a single-Operation budget on
`ProcessPaymentOperation`, and a whole-tree, end-to-end budget on
`OrderFulfillment` itself — the two `nodeRef` shapes the spec allows.

```bash
etdl validate budget-demo.etdl
etdl compile budget-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.performance")'
```

```text
$ etdl validate budget-demo.etdl
document 'budget-demo.etdl' is valid (0 errors, 0 warnings)
```

## What's declared, and why no probability/enforcement language appears

`x-performance` (the same generic `x-*` extension mechanism `x-reliability`
and `x-tree-event` already use — **zero parser or AST changes** were needed
for this supplement either), gated by `supplements: [{id: etdl.performance,
...}]` exactly like the other two. `p50Ms`/`p95Ms`/`p99Ms`/`maxConcurrency`/
`expectedRatePerSecond` are the only vocabulary here — no retry policy, no
probability, no SLA threshold configuration. Per spec Section 6, a Budget's
percentile targets are a declared expectation for downstream tooling and
deployment configuration to consult; the compiler validates their shape and
does not enforce them against measured runtime latency.

## Proving it's purely declarative

Compile the document, then compile a version of it with the `supplements:`
declaration and `x-performance` block removed entirely — the generated Rust
is byte-for-byte identical:

```bash
etdl compile budget-demo.etdl --out-dir /tmp/with-perf
# ...remove supplements:/x-performance from a copy...
etdl compile budget-demo-stripped.etdl --out-dir /tmp/without-perf
diff /tmp/with-perf/budget-demo.rs /tmp/without-perf/budget-demo-stripped.rs
# (identical apart from the filename)
```

## Triggering each diagnostic

```yaml
# E-161: percentile ordering violated
p50Ms: 900
p95Ms: 800
p99Ms: 2000
```

```yaml
# E-160: nodeRef doesn't resolve to an Event Tree or Operation node
nodeRef: "#/eventTrees/DoesNotExist"
```

```yaml
# W-413: two budgets declare the same nodeRef (still valid — warning only)
- id: first
  nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
  ...
- id: second
  nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
  ...
```

`etdl validate --json budget-demo.etdl` reports the same diagnostics as the
plain-text form, machine-readably.

## Compatibility

Comment out the `supplements: [{id: etdl.performance, ...}]` block (leaving
`x-performance` in place) and re-run `etdl validate` — it stays valid with
zero performance-related diagnostics, proving `x-performance` is additive
metadata, never a precondition for parsing or validation (spec Section 7).
