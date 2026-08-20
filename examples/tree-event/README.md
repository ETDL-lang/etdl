# Worked examples: ETDL Generic Tree Event Supplement 1.0

Three examples proving the tree supplement is genuinely domain-neutral:

| File | Proves |
|---|---|
| `generic.etdl` | a tree with no reliability (or any domain) vocabulary at all |
| `reliability-consumer.etdl` + `../../etdl-reliability/examples/tree_to_artifact.rs` | the *same* structural shape, interpreted by reliability, feeding the existing `ReliabilityArtifact` |
| `future-safety-sketch.md` | documented (not implemented): the same `Tree` type consumed by a hypothetical safety domain, requiring zero changes to `etdl-tree-core` |

## `generic.etdl`

```bash
etdl validate generic.etdl
etdl tree validate generic.etdl
etdl tree inspect generic.etdl
```

```text
$ etdl tree inspect generic.etdl
tree: operational-monitoring v1 (etdl.tree-event/1.0)
  root: AnyConditionObserved
  nodes: 3
  leaves: ConditionA, ConditionB
  preorder: AnyConditionObserved -> ConditionA -> ConditionB
```

Note what's declared: `x-tree-event` (the same generic `x-*` extension
mechanism `x-reliability` already uses — **zero parser or AST changes**
were needed for this supplement), gated by `supplements: [{id:
etdl.tree-event, ...}]` exactly like the reliability supplement.
`kind: leaf` / `kind: gate` / `gate: OR` are the only vocabulary here —
no "failure," no "top event," no probability anywhere in this file.

## `reliability-consumer.etdl`

```bash
etdl tree inspect reliability-consumer.etdl
cargo run -p etdl-reliability --example tree_to_artifact
```

```text
$ etdl tree inspect reliability-consumer.etdl
tree: payment-gateway-availability v1 (etdl.tree-event/1.0)
  root: GatewayUnavailable
  nodes: 3
  leaves: DatabaseFailure, NetworkTimeout
  preorder: GatewayUnavailable -> NetworkTimeout -> DatabaseFailure

$ cargo run -p etdl-reliability --example tree_to_artifact
tree 'payment-gateway-availability' is structurally valid
  root: GatewayUnavailable
  leaves: DatabaseFailure, NetworkTimeout
P(GatewayUnavailable), assuming independence = 0.0012997000000000147
ReliabilityArtifact 'payment-gateway-availability' now has 1 estimate(s)
  payment-gateway-availability.GatewayUnavailable = 0.0012997000000000147
```

`0.0012997...` is `1 - (1-0.001)(1-0.0003)` — the same OR-of-two-leaves
math the earlier `std.events`/`std.logic` examples already demonstrated,
now reached through a *tree structure* instead of a fault-tree gate
declared directly in `basicEvents:`/`gates:`.

**The compiler's tree-event validation (`etdl tree validate`, or ordinary
`etdl validate` — both surface the same diagnostics) is entirely
structural.** It never runs the OR-as-probability interpretation shown
above — that only happens in `etdl-reliability::tree_adapter`, called
explicitly from the Rust example. Nothing about parsing or validating
`reliability-consumer.etdl` requires `etdl-reliability` at all; try
`etdl tree validate reliability-consumer.etdl` with a build compiled
`--no-default-features` (no `reliability` feature) and it still works.

## Why the reliability interpretation isn't itself an `.etdl` file

Evaluating a tree (combining leaf probabilities through its gates) is
computation, not declaration — the same reason `std.probability`'s
composition operations are a Rust API rather than ETDL YAML (see
`docs/reference/standard-probability-library.md`). `tree_to_artifact.rs`
is the honest form this example can take.

## `future-safety-sketch.md`

Not runnable — a sketch, per the task's own instruction not to build the
safety domain now. See the file itself.
