# Worked example: ETDL Security Supplement 1.0

`attack-tree-demo.etdl` declares one attack tree under `x-tree-event`
(`etdl.security` has a genuine dependency on `etdl.tree-event` — both
supplements are declared), a Threat Model classifying two of its three
leaves against STRIDE, and one Control mapping a core Barrier node to a
mitigated threat.

```bash
etdl validate attack-tree-demo.etdl
etdl compile attack-tree-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.security")'
```

```text
$ etdl validate attack-tree-demo.etdl
[WARNING] W-411: x-security: control 'gateway-rate-limiter': mitigates entry
'RateLimitBypass' is not categorized by any declared threat model's
leafCategories
document 'attack-tree-demo.etdl' is valid (0 errors, 1 warnings)
```

That `W-411` is intentional — `RateLimitBypass` is a genuine leaf of the
attack tree (the Control correctly mitigates it) but is deliberately left
out of the Threat Model's `leafCategories` in this example, to demonstrate
the warning. Add `RateLimitBypass: denial-of-service` to `leafCategories`
to clear it.

## What's declared, and why no formal-verification language appears

`x-security` (the same generic `x-*` extension mechanism every other
supplement already uses — **zero parser or AST changes** were needed here
either), gated by `supplements: [{id: etdl.security, ...}]`. It defines no
new tree structure of its own: `gateway-compromise` is validated entirely
by the Tree Event Supplement's own machinery (`etdl.tree-event`'s
`E-120`/`E-121`/`E-122`) before Security ever reads it — Security only
reinterprets an already-valid `Tree`'s leaves under STRIDE and maps Controls
onto core Barrier nodes. Per spec Section 6, this supplement records *that*
a control is claimed to mitigate a threat; it does not verify the claim,
validate `controlId` against `NIST-800-53`'s actual catalog, or perform any
automated threat analysis.

## The cross-supplement dependency

Try removing the `- id: etdl.tree-event` line from `supplements:` (leaving
`etdl.security` and both `x-tree-event`/`x-security` blocks in place) and
re-validating:

```bash
etdl validate attack-tree-demo.etdl
```

Every `treeRef` now fails to resolve (`E-140`) — not because this
supplement parses and enforces the `x-requires` dependency metadata itself
(it doesn't; no generic supplement-dependency mechanism exists anywhere in
this compiler), but because `etdl.tree-event`'s own extension self-gates on
being separately declared, so Security's internal call into it sees zero
trees. The practical effect is the same as an enforced dependency.

## Triggering the other diagnostics

```yaml
# E-140: leafCategories value is not one of the six STRIDE categories
leafCategories:
  CredentialStuffing: not-a-real-category
```

```yaml
# E-141: leafCategories key (or a Control's mitigates entry) is not a
# leaf of the tree, or a Control's nodeRef doesn't resolve to a Barrier
leafCategories:
  GatewayCompromised: spoofing   # a gate, not a leaf
```

## Compatibility

Comment out the `supplements: [{id: etdl.security, ...}]` block (leaving
`etdl.tree-event` declared and both `x-tree-event`/`x-security` in place)
and re-run `etdl validate` — it stays valid with zero security-related
diagnostics, and compiling produces byte-for-byte identical generated Rust
to a version with `x-security` removed entirely, proving `x-security` is
additive metadata (spec Section 7).
