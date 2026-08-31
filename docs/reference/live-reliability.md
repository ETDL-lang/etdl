# ETDL Live Reliability Supplement (`etdl.live-reliability`)

**Not yet part of the normative `etdl-specification` repository** — unlike
this workspace's other supplement reference docs, this page is the primary
description of what's implemented, not a summary of upstream spec text. A
reserved-namespace entry and normative companion document in
`etdl-specification` are a tracked cross-repo follow-up.

## 1. Purpose

Every other supplement, and ETDL Core itself, treats a fault tree's
resolved probability as a **compile-time constant**: `etdl-compiler`
resolves it once, bakes it into generated code as a literal, and nothing
at runtime ever changes it — see
[`docs/reliability/runtime-feedback-calibration.md`](../reliability/runtime-feedback-calibration.md)'s
"runtime observations MUST NOT automatically change compiled probabilities"
invariant, which governs the offline `.rprob` artifact /
`etdl-reliability::calibrate` human-review workflow.

`etdl.live-reliability` is a **deliberate, explicit, opt-in exception** to
that invariant for the specific fault trees that declare it: each
declaring basic event maintains a live, incrementally-updated probability
estimate from its own observations, gates recombine from their children's
*current* live values (using the exact same AND/OR/NOT/XOR/VOTING/
INHIBIT/PRIORITY_AND math `etdl-compiler::fault_tree` uses at compile
time), and — the "authoritative" part — a barrier reading a live-tracked
node uses the *current* value, not the stale compile-time constant, both
for what it records and for branch selection.

A fault tree that doesn't declare this supplement is completely
unaffected: the invariant above still holds for it without exception.

## 2. Decentralized by design — no central server

Each node's live state lives in the process that observes it —
`etdl-core::live`'s process-wide registry, not a shared database or
analysis service. A fault tree whose nodes span multiple deployed services
propagates values through the messages already flowing between them: a
sending service attaches its current values to an outgoing message's
headers (`etdl_core::live::outbound_snapshot`); a receiving service reads
them back out and merges them into its own local view
(`etdl_core::live::apply_inbound`) for basic events it declares `inbound`
(owned upstream, never locally observed). See §5.

Proven, not just asserted: `etdl-compiler/tests/live_reliability_two_service_test.rs`
compiles two independent fixtures (a `local`-only producer and an
`inbound`-only consumer sharing the same fault-tree id by convention) and
runs each as its **own `cargo run` subprocess** — genuinely two OS
processes, each with its own `etdl_core::live` registry, the same
isolation two real deployed services have — handing the producer's
`outbound_snapshot` headers to the consumer through a file standing in for
a message broker. The consumer's basic event is `inbound`, so its branch
selection flipping to ABNORMAL can only be explained by the value having
actually crossed the process boundary.

## 3. `x-live-reliability` schema

```yaml
x-live-reliability:
  faultTrees:
    - id: PaymentGatewayFailure       # must resolve under this document's faultTrees
      threshold: 0.10                 # optional, default 0.10 (matches ETDL_SLA_THRESHOLD's default)
      basicEvents:
        - id: GatewayUnreachable      # must be a basic event of that fault tree
          source: local               # local | inbound
          priorStrength: 20           # optional, default 20; local only
        - id: ChargeRejected
          source: inbound
```

- **`threshold`** — how far a node's current live value may drift from its
  *baseline* (the value computed from every leaf's declared probability —
  `local` **or** `inbound`, both seed a baseline at registration, once,
  never redefined afterward — using the same shared gate math run one more
  time) before `reliability.in_range` (§4) reports `false`. One threshold
  per fault tree, mirroring `ETDL_SLA_THRESHOLD` being a single
  process-wide value.
- **`local`** basic event — this service observes it directly
  (`etdl_core::live::record_observation`, called wherever generated code
  already calls `record_branch`/`record_failure`/`record_success` for that
  node). `priorStrength` is how many pseudo-observations the declared
  probability is worth before real observations start moving the estimate
  — the same Beta-Binomial method `etdl reliability estimate --method
  beta-binomial` already uses, run incrementally instead of as a batch
  step.
- **`inbound`** basic event — owned by an upstream service; this
  document's own generated code never calls `record_observation` for it,
  only ever receives a *current value* via §5. Its **baseline**, unlike its
  current value, is known immediately at registration — seeded from this
  document's own declared probability for the basic event, exactly like a
  `local` leaf's (`priorStrength` doesn't apply, since it's never locally
  estimated). This matters: an earlier implementation let the *first*
  inbound value double as the baseline too, so a service's very first
  contact with an already-drifted upstream silently redefined "normal" to
  match it — `reliability.in_range` could then never detect a deviation,
  no matter how far the value had drifted. Baseline is now fixed at
  registration for every leaf, local or inbound, and never moves again.

## 4. `reliability.in_range` (ECEL)

A Barrier branch whose condition compares `reliability.in_range` to a
boolean literal is selected by whether *this barrier's own* linked
fault-tree node (its existing `probability_source`) currently reads within
`threshold` of its baseline — no node id is written in the expression;
it's resolved from the barrier's own reference, the same implicit scoping
`record_branch` already has. `reliability.in_range` reuses ECEL's existing
`Comparison` grammar rather than introducing a new "bare boolean path"
shape, so it must be written as a comparison (`== true`/`== false`), never
bare, and never nested inside `&&`/`||`/`!` — any other shape, or use
without the document declaring this supplement, is **E-173**. A node with
no current value yet (cold start, or an `inbound` leaf that hasn't
received anything — its *baseline* is already known, but not its current
value) reads as `true` (in range) — the same fail-open default
`SlaTracker`'s own `MIN_OBSERVATIONS` gate already uses.

A branch's own `probability_source` not resolving to a fault tree declared
under `x-live-reliability` (typeck's E-173 only checks the supplement is
declared and the path shape, not this) is a codegen-time error, **E-109**
— refuses to compile rather than silently emitting a meaningless call;
should be unreachable in a document typeck already accepts, but codegen
stays defensive rather than trusting that alone.

```yaml
RiskBarrier:
  type: barrier
  branches:
    - outcome: NORMAL
      condition: "reliability.in_range == true"
      next: ProcessPaymentOperation
    - outcome: ABNORMAL
      condition: default
      next: InvestigateFaultOperation   # an ordinary event-tree path —
                                         # fault trees stay probability
                                         # *sources*, never a branch target
```

## 5. Cross-service wire shape

```json
{
  "etdl.live-reliability/1.0": {
    "fault_tree_id": "PaymentGatewayFailure",
    "nodes": { "GatewayUnreachable": 0.0091, "GatewayUnavailableOrRejected": 0.0134 },
    "observed_at": "2026-08-29T12:00:00Z"
  }
}
```

Carried under a message's `headers` (already a first-class,
ECEL-readable/generated-struct field — no new envelope shape). Matched by
convention: the sending and receiving documents must use the same
`fault_tree_id`/node id strings, the same trust model `asyncapi_imports`/
channel names already use elsewhere in the language, not a
compiler-verified cross-document reference. No authentication or integrity
check — tampering with the header can only skew the receiving service's
own estimate, never crash it (`apply_inbound` validates shape and silently
ignores anything malformed or referencing an untracked tree/node).

## 6. Diagnostics

See [`docs/DIAGNOSTICS.md`](../DIAGNOSTICS.md#live-reliability-supplement-e-17x--w-414)
(`E-170`/`E-171`/`E-172`/`E-173`/`W-414`). `E-173` (malformed or misplaced
`reliability.*` path) is reported by `typeck`, not by this supplement's own
schema parser — listed there anyway since it's only ever relevant when this
supplement is in play.

## 7. Non-goals

- Does not change anything about the offline `.rprob` artifact /
  `etdl-reliability::calibrate` workflow — that remains the
  human-reviewed engineering record for every fault tree that doesn't
  declare this supplement.
- Does not validate that a cross-service `fault_tree_id`/node id pairing
  is semantically correct — only that the JSON shape parses.
- Does not populate `ReliabilityObservation.duration_ms` or any
  latency/performance metric — purely a probability-recomputation layer.
