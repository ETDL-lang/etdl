# ETDL Event Tree Analysis

This document describes the event-tree engine in the ETDL reference compiler and
its IEC 62502 alignment. See `ECEL.md` for condition semantics and
`PROBABILITY_SEMANTICS.md` for probabilities.

---

## 1. Model

An Event Tree (§5.5) starts at an `initiatingEvent` and flows through `nodes`:

| Node | Meaning |
|---|---|
| Barrier | decision gate; branches carry an ECEL condition and a probability |
| Operation | side-effecting action; `next` (success), `onFailure` (failure), retry/timeout |
| Consequence | terminal outcome; `send` (publish) or `terminate` |

A path is a sequence from the initiating event to a Consequence.

---

## 2. Validation rules (event trees)

| Code | Rule |
|---|---|
| V-101 | `next`/`onFailure`/`initiatingEvent.next` does not resolve to a node in the tree |
| V-102 | cycle among nodes |
| V-103 | a node is unreachable from the initiating event |
| V-104 | a path from the initiating event does not terminate in a Consequence |
| V-201 | a barrier has fewer than 2 branches |
| V-202 | more than one `default` branch, or `default` not evaluated last |
| V-203 | branch probabilities must be in [0,1] and sibling branches sum to 1.0 ± 0.0001 |
| V-204 | an ECEL condition references a field absent from the AsyncAPI schema or has a type mismatch |
| V-301 | an operation's `handler` is not a valid identifier |
| V-302 | a `send` consequence omits `channel` or `message` |
| W-401 | an operation has no `onFailure` path |

### 2.1 Cycle detection
A cycle among nodes is detected with a 3-color DFS. Consequences are terminal
(marked black) so that re-visiting a consequence from two branches is not
mis-flagged as a cycle.

### 2.2 Termination
`check_terminal_paths` verifies that every path from the initiating event ends in
a Consequence; otherwise `V-104` is emitted.

---

## 3. Branch semantics

- A barrier's branches are evaluated **in document order**.
- The `default` branch matches only when no preceding sibling matched, and must
  be last (V-202).
- Every branch declares a probability (or `probabilitySource`); sibling
  probabilities must sum to 1.0 ± 0.0001 (V-203).

### Worked example

```
OrderFulfillment:
  initiatingEvent.next: InventoryCheckBarrier
  InventoryCheckBarrier (barrier):
    SUCCESS 0.95  condition: message.payload.items[*].qty > 0  → ProcessPaymentOperation
    FAILURE 0.05  condition: default                          → OutOfStockConsequence
  ProcessPaymentOperation (operation):
    next → FulfillmentConsequence
    onFailure → PaymentFailedConsequence
    retryPolicy: {maxAttempts: 3, backoffMs: 250, exponential}
    timeoutMs: 5000
```

Branch probabilities 0.95 + 0.05 = 1.0 ✓.

---

## 4. Retry and timeout semantics

From `etdl-core/src/retry.rs` (`RetryPolicy::execute`):

- Runs the handler up to `max_attempts` times, each under a **per-attempt**
  `timeout`.
- First `Ok` is returned immediately; `Err` is retained and retried.
- `fixed`: constant `backoff_ms` between attempts.
- `exponential`: `backoff_ms · 2^attempt`.
- Sleep occurs between attempts only (not after the last).
- If all attempts fail with an `Err`, the last error is returned.
- **If all attempts time out** (no `Err` captured), the runtime returns a
  `WorkflowError` instead of panicking (hardened; see `RUNTIME.md`).

`maxAttempts` default 1, `backoffMs` default 0, strategy default `fixed`,
`timeoutMs` per attempt default 5000 ms in generated code.

---

## 5. Generated behavior

Per event tree, generated code (Rust backend):

- emits `pub async fn handle_<snake_case(id)>(message: <Type>, publisher: &dyn Publisher) -> Result<(), WorkflowError>`,
- creates one `BranchMonitor` per handler call, reused across every barrier
  the traversal reaches,
- records `record_branch(node_id, outcome, effective_probability)` on the
  taken branch — `node_id` is that specific barrier's own id, so reuse of
  one monitor across several barriers still attributes each one correctly,
- runs operations via `RetryPolicy` with the declared timeout,
- records `record_failure(node_id, &err, Some(linked_probability)|None)` on
  failure,
- publishes consequence messages via `publisher.publish(channel, payload)`,
- returns `Err(WorkflowError)` if an operation fails with no `onFailure`.

See `docs/reference/crates.md` and the generated-code compile check
(`etdl-compiler/tests/codegen_test.rs`).

---

## 6. Canonical examples

- `etdl-cli/tests/fixtures/order-fulfillment.etdl` — the spec §13 worked example.
- `etdl-cli/tests/fixtures/advanced-fault-tree.etdl` — advanced gates, event
  types, transfers.
- `docs/examples/order-fulfillment.md`, `docs/examples/payment-saga.md` —
  walkthroughs.
