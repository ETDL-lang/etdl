# Worked example: ETDL Performance Supplement 1.0

`budget-demo.etdl` declares two Budget Objects (`ETDL-Performance-Supplement.md`
Section 4.1) against one Event Tree: a single-Operation budget on
`ProcessPaymentOperation`, and a whole-tree, end-to-end budget on
`OrderFulfillment` itself — the two `nodeRef` shapes the spec allows — plus
one Barrier Check Object (Section 4.2) linking a new `PaymentPerfBarrier`
node to the Operation-level budget, so it can branch on
`performance.in_budget` (Section 6.3).

```bash
etdl validate budget-demo.etdl
etdl compile budget-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.performance")'
```

```text
$ etdl validate budget-demo.etdl
document 'budget-demo.etdl' is valid (0 errors, 0 warnings)
```

## What's declared, and what it actually does now

`x-performance` (the same generic `x-*` extension mechanism `x-reliability`
and `x-tree-event` already use — **zero parser or AST changes** were needed
for this supplement, including for `performance.in_budget`), gated by
`supplements: [{id: etdl.performance, ...}]` exactly like the other core
supplements.

Unlike earlier revisions of this supplement, none of this is purely
declarative anymore (`ETDL-Performance-Supplement.md` Section 6):

- **`maxConcurrency: 200`/`expectedRatePerSecond: 50`** on
  `process-payment-budget` are structurally enforced — generated code
  acquires a concurrency permit and a rate token
  (`etdl_core::perf::enter`) before every call to
  `stripe_charge_handler`, a real block, not advisory.
- **`p99Ms: 2000`** becomes the effective timeout for that same call,
  since `ProcessPaymentOperation` declares no explicit `timeoutMs`/
  `retryPolicy` of its own — codegen synthesizes a single-attempt retry
  policy purely to gain the timeout wrapper.
- **`PaymentPerfBarrier`'s `OK`/`DEGRADED` branch selection** is driven
  live by `process-payment-budget`'s current status
  (`performance.in_budget`) — resolved via the `barrierChecks` entry
  linking that barrier to that budget, not by any field on the branch
  itself.
- **`order-fulfillment-e2e-budget`** (the whole-tree budget, no
  `maxConcurrency`/`expectedRatePerSecond` declared) contributes only
  latency observation for now — no Barrier links to it in this example,
  so nothing branches on it, but its percentiles are still tracked.

Inspect the generated code to see all of this directly:

```bash
etdl compile budget-demo.etdl --out-dir ./generated
cat generated/budget-demo.rs
```

## Compatibility

Comment out the `supplements: [{id: etdl.performance, ...}]` block (leaving
`x-performance` in place) and re-run `etdl validate` — it stays valid with
zero performance-related diagnostics. Then `etdl compile` the stripped
document: the generated code has none of `etdl_core::perf`'s registration/
enforcement/`in_budget` calls, and `PaymentPerfBarrier`'s condition can no
longer even parse as `performance.in_budget` without the supplement
declared (that would be `E-163`) — proving `x-performance` is fully
additive, never a precondition for parsing, validation, or a plain
build's generated code.

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

```yaml
# E-162: barrierChecks nodeRef doesn't resolve to a Barrier node
barrierChecks:
  - id: bad-guard
    nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
    budgetRef: process-payment-budget
```

```yaml
# E-162: barrierChecks budgetRef doesn't resolve to a declared budget
barrierChecks:
  - id: bad-guard
    nodeRef: "#/eventTrees/OrderFulfillment/nodes/PaymentPerfBarrier"
    budgetRef: no-such-budget
```

```yaml
# W-415: two barrierChecks entries declare the same nodeRef (still valid — warning only)
barrierChecks:
  - id: first
    nodeRef: "#/eventTrees/OrderFulfillment/nodes/PaymentPerfBarrier"
    budgetRef: process-payment-budget
  - id: second
    nodeRef: "#/eventTrees/OrderFulfillment/nodes/PaymentPerfBarrier"
    budgetRef: process-payment-budget
```

```yaml
# E-163: performance.in_budget used without the supplement declared, or
# combined with && instead of being the entire branch condition
condition: "performance.in_budget == true && message.payload.items[*].qty > 0"
```

`etdl validate --json budget-demo.etdl` reports the same diagnostics as the
plain-text form, machine-readably.
