# etdl-core

[![Crates.io](https://img.shields.io/crates/v/etdl-core.svg)](https://crates.io/crates/etdl-core)
[![Docs.rs](https://img.shields.io/docsrs/etdl-core)](https://docs.rs/etdl-core)

**The runtime library [ETDL](https://github.com/ETDL-lang/etdl)-generated code depends on** — and the one authoritative implementation of ETDL runtime semantics every other language target binds to (via [`etdl-runtime-ffi`](https://crates.io/crates/etdl-runtime-ffi)) rather than re-implementing.

## Components

| Component | Purpose |
|---|---|
| `BranchMonitor` | Records taken branches and operation failures per node, against build-time-resolved probabilities; feeds SLA anomaly detection |
| `RetryPolicy` / `BackoffStrategy` | Async retry with exponential or fixed backoff, a total attempt budget, and a per-attempt timeout |
| `SlaTracker` | Rolling-window anomaly detection (`ETDL_SLA_WINDOW`, `ETDL_SLA_THRESHOLD` env vars) — compares observed vs. declared probabilities |
| `ChaosController` | Declared, seeded, node-scoped failure injection, guarded off in production via `ETDL_ENV` |
| `condition::{matches, contains}` | ECEL `matches` (RE2-compatible regex via the `regex` crate) / `in` (set membership) — the runtime half of ECEL condition evaluation |
| `Publisher` | The `consequence: send` boundary generated code calls into — implement it against your real broker/client |
| `telemetry::inject_traceparent` | W3C trace context propagation |
| `observation` | Reliability observation types + sinks (JSON Lines, OTel-shaped, in-memory capturing) |

## Why this crate, specifically, is the one every target binds to

ETDL compiles event trees (IEC 62502) and fault trees (IEC 61025) into code with build-time-resolved probabilities baked in as constants — not runtime guesses. `etdl-core` is where the *behavior* around those constants lives: retry sequencing, branch/failure accounting, SLA comparison, chaos injection. Generated Rust code links this crate directly; every other target (Java, Python, Go, .NET) reaches the exact same implementation through `etdl-runtime-ffi`'s C ABI, so a `matches` regex or a retry backoff delay behaves byte-for-byte identically no matter which language evaluates it.

## Example

```rust
use etdl_core::{BranchMonitor, BackoffStrategy, RetryPolicy};

let mut monitor = BranchMonitor::new("InventoryCheckBarrier");
monitor.record_branch("SUCCESS", 0.95);

let retry = RetryPolicy {
    max_attempts: 3,
    backoff_ms: 250,
    strategy: BackoffStrategy::Exponential,
};
```

Full architecture: [`docs/architecture.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture.md) and [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md) (the FFI boundary story).

## License

Apache-2.0
