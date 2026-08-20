# ETDL Runtime (`etdl-core`)

This document states exactly what the ETDL runtime (`etdl-core`) guarantees,
how to configure it safely, and what it deliberately does **not** guarantee.

---

## 1. Components

| Component | Purpose | Async? |
|---|---|---|
| `BranchMonitor` | records taken branches and failures with their declared probabilities; drives SLA + chaos + telemetry | no |
| `RetryPolicy` / `BackoffStrategy` | async retry with fixed/exponential backoff and per-attempt timeout | yes (tokio `time`) |
| `SlaTracker` | rolling-window observed-vs-declared frequency comparison (§9.3) | no |
| `ChaosController` | deterministic, scoped, seeded failure injection; production-guarded | no |
| `Publisher` (trait) + `NoopPublisher` + `ChannelCapturingPublisher` | transport-agnostic consequence `send` | no |
| `condition::{contains, matches}` | ECEL `in` / `matches` runtime helpers (RE2-compatible `regex`) | no |
| telemetry | `inject_traceparent` (W3C), SLA anomaly events, node span attributes | no |

Only `RetryPolicy::execute` needs a Tokio runtime (with the time driver). All
other components are runtime-free and usable from sync code.

---

## 2. BranchMonitor

- `new(node_id)` creates a monitor with its own `SlaTracker` and `ChaosController`.
- `record_branch(outcome, declared_probability)`:
  1. consults `ChaosController` — if chaos fires, the branch record is **dropped**
     (this is the injection mechanism; the declared probability is not recorded),
  2. records the outcome in `SlaTracker`,
  3. emits an anomaly event if the observed frequency diverges.
- `record_failure(operation_id, error, declared_probability)`:
  - records `"{operation_id}.failure"` with the linked fault-tree probability
    when `Some`; prints the error to stderr.
- `flush()` (via `Drop`) prints a per-node evaluation summary to stderr.

All interior state is behind `Arc<Mutex<_>>`; the monitor is `Send + Sync`. A
poisoned mutex (a panic while a lock is held) makes the monitor fail closed.

---

## 3. RetryPolicy

`execute(f, timeout)` semantics:

- Runs `f` up to `max_attempts` times, each under a **per-attempt** `timeout`.
- First `Ok` returns immediately.
- `Err` is retained as the last error and retried.
- A timeout is logged and retried (a timeout produces no error value).
- Between attempts, sleeps `backoff_ms` (fixed) or `backoff_ms · 2^attempt`
  (exponential, **saturating** — cannot overflow).
- Exhaustion returns:
  - `RetryError::Exhausted(last_error)` if the final attempt produced an `Err`,
  - `RetryError::TimedOut` if only timeouts occurred (or `max_attempts == 0`).
- **Never panics.**

Defaults: `max_attempts = 1`, `backoff_ms = 0`, strategy `fixed`.

The error type `RetryError<E>` implements `std::error::Error` (source = the
handler error) and `Display`, so it composes with `WorkflowError`.

---

## 4. SlaTracker

- Rolling window per node (default 1000 evaluations), bounded (oldest evicted).
- `record(node, outcome, declared, occurred)` records one evaluation; `occurred`
  is whether `outcome` was actually observed.
- Observed frequency = `count(outcome in window) / window length`.
- Anomaly when `|observed − declared| > threshold` **and** ≥ 10 observations.
- `ETDL_SLA_WINDOW` (usize) and `ETDL_SLA_THRESHOLD` (f64) configure defaults;
  invalid values silently fall back to 1000 / 0.10.

---

## 5. ChaosController — safe by default

- **Disabled unless `ETDL_CHAOS` is explicitly truthy** (`true|1|yes|on`).
- **Ignored when production is detected.** Production detection probes
  `ETDL_ENV` → `DEPLOYMENT_ENVIRONMENT` → `ENVIRONMENT` → `ENV` and matches
  `production`, `prod`, `prd`, `live` as an exact value or as a token with
  separators/suffixes (e.g. `production-us-east-1`, `prod_eu_1`, `prd2`).
- `ETDL_CHAOS_SEED` (u64) makes injection deterministic.
- `ETDL_CHAOS_SCOPE` (comma-separated node ids, or `*`) restricts injection.
- Injection is a deterministic parity decision (`hash % 2 == 0` seeded, or
  `counter % 2 == 0` unseeded).

**Safety contract:** with no environment configured, chaos is **off**. In any
environment whose name is unset or clearly not production, a stray
`ETDL_CHAOS=true` can activate injection — deployers must ensure production
containers set an environment variable. This is documented, not silently hidden.

---

## 6. Publisher

Generated handlers take `publisher: &dyn Publisher` and call
`publisher.publish(channel, &serde_json::to_value(payload)?)`. The runtime ships:

- `NoopPublisher` — logs and discards (default for tests/bring-up).
- `ChannelCapturingPublisher` — records `(channel, payload)` for assertions.
- Applications implement `Publisher` for a real transport and SHOULD inject the
  W3C `traceparent` (see §7) into outbound messages.

---

## 7. Telemetry

- `inject_traceparent(message_type)` returns a W3C `traceparent`:
  `00-<32 hex trace-id>-<16 hex span-id>-01`. IDs come from OS randomness
  (`getrandom`) with a deterministic time+counter fallback; both are non-zero
  and correctly sized.
- `attach_node_span_attribute(node_id)` logs `etdl.node.id=<id>`.
- `emit_anomaly_event(...)` logs SLA anomalies.
- All telemetry currently writes to **stderr**. This is vendor-neutral but noisy
  in library contexts; the interface is intentionally the smallest seam that an
  OpenTelemetry integration can be layered onto without vendor lock-in.

---

## 8. Error model

`WorkflowError` (re-exported `telemetry::Error`) is the generated-code error
type. `From` impls convert `String`, `serde_json::Error`, and `PublishError`
into it, so `?` works naturally in generated handlers.

---

## 9. Environment variable reference

| Variable | Purpose | Default |
|---|---|---|
| `ETDL_CHAOS` | enable chaos (`true\|1\|yes\|on`) | off |
| `ETDL_CHAOS_SEED` | deterministic chaos seed | none (parity) |
| `ETDL_CHAOS_SCOPE` | node-id allow-list for chaos | all |
| `ETDL_ENV` | authoritative environment | — |
| `DEPLOYMENT_ENVIRONMENT` / `ENVIRONMENT` / `ENV` | fallback environment probes | — |
| `ETDL_SLA_WINDOW` | SLA rolling window size | 1000 |
| `ETDL_SLA_THRESHOLD` | SLA deviation threshold | 0.10 |

---

## 10. Guarantees and non-guarantees

**Guaranteed:**
- No panic on retry exhaustion, backoff overflow, or malformed config.
- Deterministic chaos when seeded; chaos off by default and in production.
- Well-formed W3C traceparents.
- Bounded SLA memory (rolling windows).
- All probability-based observability compares observed vs declared with a
  documented threshold.

**Not guaranteed (deliberate):**
- The runtime does **not** compute probabilities (that is compile-time).
- `ChaosController` uses a parity coin, not a probability-weighted path, unless
  an application maps it onto declared probabilities.
- Telemetry is stderr text, not OTLP; no tracing SDK is embedded.
- The runtime does not retry publishing or compensate; that is application
  responsibility.
- Chaos activation depends on environment detection; misconfiguration is the
  deployer's risk (documented above).
