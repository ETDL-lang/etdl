# Observability Exporters

`etdl-core` can optionally export what `BranchMonitor` already observes —
branch outcomes, operation successes/failures, and SLA anomalies — to three
external observability systems, each behind its own off-by-default Cargo
feature:

| Feature | Dialect | Direction | Third-party crates |
|---|---|---|---|
| `exporter-prometheus` | PromQL | pull — a compiled-in scrape endpoint | `metrics`, `metrics-exporter-prometheus` |
| `exporter-loki` | LogQL | push — to a Loki-compatible push API | `tracing`, `tracing-subscriber`, `tracing-loki` |
| `exporter-otlp` | OTLP | push — to an OTel Collector, via the OpenTelemetry Rust SDK, over HTTP+protobuf | `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` |

None of these are required for `etdl compile`, and a build with none enabled
behaves exactly as `etdl-core` always has. Enabling one never pulls in
another — pick any subset.

## Design: ambient global backends

`metrics`, `tracing`, and `opentelemetry` are each a **process-global
recorder/subscriber/provider**: install one once at startup, and any code
anywhere in the process can emit through it via a plain macro or global
call — no handle threaded through call sites. So instead of adding a sink
parameter to `BranchMonitor` or changing `etdl-compiler`'s codegen,
`BranchMonitor::record_branch`/`record_success`/`record_failure`
(`etdl-core/src/monitor.rs`) each carry a small, additive
`#[cfg(feature = "exporter-...")]` block that reports through whichever
backend is compiled in, alongside the existing `ObservationSink` call.
Generated code is completely unaware of this — the same `BranchMonitor::new(id)`
call `etdl-compiler` has always emitted works unchanged whether zero, one, or
all three exporters are compiled into the `etdl-core` the generated code
links against.

This is also what makes the design extensible: if a future ETDL supplement
(safety, security, diagnostics) gains its own runtime evaluation, that code
can report through the exact same `metrics::counter!`/`tracing::info!`/
`opentelemetry::global::meter(...)` calls directly — no new trait, no
coordination with the `exporters` module required.

## What's exposed today

Counts, by `(node_id, outcome)` or `operation_id`:

- `etdl_branch_total` / `etdl.branch.total` — every `record_branch` call
- `etdl_operation_success_total` / `etdl.operation.success.total`
- `etdl_operation_failure_total` / `etdl.operation.failure.total`
- `etdl_sla_anomaly_total` / `etdl.sla.anomaly.total` — whenever
  `SlaTracker` flags a deviation (same condition `telemetry::emit_anomaly_event`
  already logs)

(Prometheus/Loki use the `etdl_snake_case` naming; OTLP uses
`etdl.dotted.case`, matching each ecosystem's own convention.)

**Known gap**: `ReliabilityObservation.duration_ms` — the field a
latency-style "performance" metric would read — is defined but never
populated anywhere in `etdl-core` or `etdl-compiler`'s generated code today.
A duration/latency histogram would be real but permanently empty until
something starts populating it, which needs a separate, small
`etdl-compiler` codegen change (time the handler call, pass the elapsed
value into `record_success`/`record_failure`). This pass ships count-based
metrics only rather than a metric that looks wired up but is silently
always zero.

Safety/security/diagnostics supplement data (hazards, threat models,
budgets) is compile-time-declared, not something the runtime evaluates —
out of scope here; see "Design" above for why adding it later doesn't
require reworking this module.

## Enabling from Rust

```toml
etdl-core = { version = "...", features = ["exporter-prometheus"] }
```

```rust
etdl_core::exporters::prometheus::install("0.0.0.0:9464".parse()?)?;
// ... run generated handlers as normal — /metrics is now live ...
```

`exporter-loki` and `exporter-otlp` are two-tier: a low-level function
(`loki::layer`, `otlp::build`) for an app that manages its own `tracing`
subscriber or OTel providers, and a convenience `install` that sets one up
globally for an app with none yet:

```rust
etdl_core::exporters::loki::install(
    "http://localhost:3100".parse()?,
    HashMap::from([("service".to_string(), "payment-gateway".to_string())]),
)?;

etdl_core::exporters::otlp::install("http://localhost:4318")?;
```

Combine any subset — each `install` only touches its own backend.

## Non-Rust targets (`etdl-runtime-ffi`)

Every language target other than Rust (`etdl-target-java/python/go/dotnet`)
already reaches `BranchMonitor`'s emit side automatically: their generated
code calls `etdl_branch_monitor_record_branch`/`_record_success`/
`_record_failure`, which call straight into the same `etdl_core::BranchMonitor`
methods this feature instruments. No FFI change was needed for that part.

What a non-Rust host *does* need is a way to call the one-time setup step,
since it can't call an `etdl-core` Rust function directly:

```c
int etdl_exporter_prometheus_install(const char *bind_addr);
int etdl_exporter_loki_install(const char *loki_url, const char *labels_json);
int etdl_exporter_otlp_install(const char *endpoint);
```

Build the shared library with the matching feature (`cargo build -p
etdl-runtime-ffi --features exporter-prometheus`), call the install function
once at startup from the host language via its own FFI mechanism (JNI,
ctypes, cgo, P/Invoke — the same one it already uses for
`etdl_branch_monitor_new` etc.), then run generated handlers as normal.
Calling an install function in a library built *without* the matching
feature returns `ETDL_ERR_NOT_COMPILED_IN` (not a missing symbol — the
function always exists, so no conditional `dlsym`/`GetProcAddress` probing
is required).

`labels_json` for `etdl_exporter_loki_install` is a JSON object of string
labels (e.g. `{"service":"payments"}`); `NULL` means none.

`etdl_exporter_loki_install` installs a fresh global `tracing` subscriber —
don't call it if the host process already manages its own; there is no FFI
equivalent of the low-level `layer()`/`build()` functions, since composing
into an existing subscriber/provider is a Rust-API-shaped operation.

Idiomatic per-language wrappers (e.g. a Java `EtdlExporters` class) live in
each target's own repository (`etdl-target-java` etc.), not here — this page
covers only what `etdl-runtime-ffi` (in this repo) exposes.

## Caveats

- No authentication and no TLS configuration surfaced by any exporter — bind
  the Prometheus listener to a private/loopback address, or put it behind
  your own reverse proxy, if it needs to be reachable beyond localhost.
  Loki/OTLP endpoint URLs can be `https://` if the target itself terminates
  TLS; nothing here disables certificate validation.
- `exporter-loki`'s `install()` and `exporter-otlp`'s background export both
  work whether or not the calling thread is already inside a Tokio runtime
  — each falls back to a dedicated background thread/runtime when there
  isn't one (the same pattern `metrics-exporter-prometheus`'s own
  `install()` uses), so they work equally from a Rust caller's own async
  runtime and from a non-Rust FFI caller with no Rust runtime at all.
