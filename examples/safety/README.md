# Worked examples: ETDL Safety Supplement 1.0

| File | Proves |
|---|---|
| `hazard-demo.etdl` | one Hazard classified against the Section 4.1 risk matrix, and two Safety Barriers on genuinely independent network paths — validates cleanly; also exercises SIL &harr; PFD enforcement (Section 6.1) and `safety.sil_maintained` (Section 6.2), since `RetryBarrier`'s `FAILURE` branch is backed by a real, live-tracked Fault Tree rather than a static literal |
| `contradictory-independence.etdl` | the same two barriers, but sharing a `commonCauseGroup` while mutually claiming `independentOf` each other — `E-132` (a self-contradiction among the *declared* claims themselves) |
| `shared-cause.etdl` | two fault-tree-backed barriers whose `independentOf` claim is empirically false — their Fault Trees share a basic event, caught by Section 6.3's minimal-cut-set analysis, not by anything the declared claims alone would reveal — `E-134` |

## `hazard-demo.etdl`

```bash
etdl validate hazard-demo.etdl
etdl compile hazard-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.safety")'
```

```text
$ etdl validate hazard-demo.etdl
document 'hazard-demo.etdl' is valid (0 errors, 0 warnings)
```

## What's declared

`x-safety` (the same generic `x-*` extension mechanism `x-reliability`,
`x-tree-event`, and `x-performance` already use — **zero parser or AST
changes** were needed for this supplement either), gated by `supplements:
[{id: etdl.safety, ...}]`. `severity`/`likelihood`/`riskIndex` classify the
Hazard; `sil`/`failureOutcome`/`independentOf`/`commonCauseGroup` give the
two core Barrier nodes (`RetryBarrier`, `FallbackBarrier`) safety meaning.
Per spec Section 6, the residual risk of
`gateway-unavailable-during-payment` is exactly the branch probability
already reachable through the Event Tree — this supplement adds
classification, it never recomputes that number.

Unlike earlier revisions, three things here are not just declarative:

- `RetryBarrier`'s declared `sil: 2` is verified against its `FAILURE`
  branch's resolved probability (from the `GatewayFailure` Fault Tree) —
  outside the SIL 2 PFD band `[1e-3, 1e-2)` would be `E-133`.
- With `etdl.live-reliability` also declared and `GatewayFailure` listed
  under `x-live-reliability`, `RetryBarrier`'s own `FAILURE` branch
  condition is `safety.sil_maintained == false`: branch selection reacts
  live to the same Fault Tree's currently-tracked probability, not just
  the value resolved at compile time.
- `retry-barrier`/`fallback-gateway-barrier`'s mutual `independentOf`
  claim is checked against their *actual* Fault Tree structure (Section
  6.3), not only against each other — see `shared-cause.etdl` for the
  counter-example this would catch.

## Triggering each diagnostic

```yaml
# W-410: declared riskIndex doesn't match the risk matrix
# (severity: critical, likelihood: remote -> matrix value 2)
riskIndex: 4
```

```yaml
# E-131: consequenceRef doesn't resolve to a Consequence node
consequenceRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
```

```yaml
# E-131: failureOutcome doesn't match one of the Barrier node's own outcomes
failureOutcome: TIMEOUT
```

```yaml
# E-133: sil declared inconsistent with the resolved PFD (0.005 sits in
# SIL 2's band [1e-3, 1e-2); SIL 4's band is [1e-5, 1e-4))
sil: 4
```

```bash
# E-132: see contradictory-independence.etdl
etdl validate contradictory-independence.etdl

# E-134: see shared-cause.etdl
etdl validate shared-cause.etdl
```

```yaml
# E-135: safety.sil_maintained used without etdl.live-reliability declared
```

`etdl validate --json hazard-demo.etdl` reports the same diagnostics as the
plain-text form, machine-readably. `W-410` is a warning, not a rejection —
`etdl validate` still exits 0 with a mismatched `riskIndex`; every other
code above is an error and exits 1.

## Compatibility

Comment out the `supplements: [{id: etdl.safety, ...}]` block (leaving
`x-safety` in place) and re-run `etdl validate` — it stays valid with zero
`E-13x`/`W-410` diagnostics, proving `x-safety` is additive metadata, never
a precondition for parsing or validation (spec Section 7). Removing only
`etdl.live-reliability` (leaving `etdl.safety` declared) removes
`safety.sil_maintained`'s availability specifically (`E-135`) while SIL
&harr; PFD enforcement and empirical independence still apply — they need
nothing beyond `etdl.safety` itself.

## Runtime behavior

`etdl compile` itself doesn't demonstrate `safety.sil_maintained`-driven
branch selection reacting to a live probability drift — see
`etdl-compiler/tests/safety_codegen_test.rs` for a real, `cargo
run`-executed proof.
