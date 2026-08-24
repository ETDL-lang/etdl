# Worked examples: ETDL Safety Supplement 1.0

| File | Proves |
|---|---|
| `hazard-demo.etdl` | one Hazard classified against the Section 4.1 risk matrix, and two Safety Barriers on genuinely independent network paths — validates cleanly |
| `contradictory-independence.etdl` | the same two barriers, but sharing a `commonCauseGroup` while mutually claiming `independentOf` each other — `E-132` |

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

## What's declared, and why no probability/enforcement language appears

`x-safety` (the same generic `x-*` extension mechanism `x-reliability`,
`x-tree-event`, and `x-performance` already use — **zero parser or AST
changes** were needed for this supplement either), gated by `supplements:
[{id: etdl.safety, ...}]`. `severity`/`likelihood`/`riskIndex` classify the
Hazard; `sil`/`independentOf`/`commonCauseGroup` give the two core Barrier
nodes (`RetryBarrier`, `FallbackBarrier`) safety meaning. Per spec Section
6, the residual risk of `gateway-unavailable-during-payment` is exactly the
branch probability already reachable through the Event Tree — this
supplement adds classification, it never recomputes that number.

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

```bash
# E-132: see contradictory-independence.etdl
etdl validate contradictory-independence.etdl
```

`etdl validate --json hazard-demo.etdl` reports the same diagnostics as the
plain-text form, machine-readably. `W-410` is a warning, not a rejection —
`etdl validate` still exits 0 with a mismatched `riskIndex`; `E-131`/`E-132`
are errors and exit 1.

## Compatibility

Comment out the `supplements: [{id: etdl.safety, ...}]` block (leaving
`x-safety` in place) and re-run `etdl validate` — it stays valid with zero
safety-related diagnostics, proving `x-safety` is additive metadata, never a
precondition for parsing or validation (spec Section 7).

## Proving it's purely declarative

Compiling `hazard-demo.etdl` with `x-safety`/`supplements:` present produces
byte-for-byte identical generated Rust to a version with both removed — the
same check `examples/performance/README.md` demonstrates for the Performance
Supplement.
