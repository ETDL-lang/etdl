# Worked examples: ETDL Security Supplement 1.0

| File | Proves |
|---|---|
| `attack-tree-demo.etdl` | one attack tree under `x-tree-event`, a Threat Model classifying two of its three leaves against STRIDE, and one Control mapping a core Barrier node to a mitigated threat — also exercises `W-411` (a mitigated-but-uncategorized threat) |
| `control-threshold-demo.etdl` | bypass-threshold enforcement (Section 7.1) and `security.control_effective` (Section 7.2), since the Control's `bypassOutcome` branch is backed by a real, live-tracked Fault Tree rather than a static literal |

## `attack-tree-demo.etdl`

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

## What's declared

`x-security` (the same generic `x-*` extension mechanism every other
supplement already uses — **zero parser or AST changes** were needed here
either), gated by `supplements: [{id: etdl.security, ...}]`. It defines no
new tree structure of its own: `gateway-compromise` is validated entirely
by the Tree Event Supplement's own machinery (`etdl.tree-event`'s
`E-120`/`E-121`/`E-122`) before Security ever reads it — Security only
reinterprets an already-valid `Tree`'s leaves under STRIDE and maps Controls
onto core Barrier nodes. Per spec Section 6.3, this supplement still
records *that* a control is claimed to mitigate a threat without verifying
the claim empirically, does not validate `controlId` against
`NIST-800-53`'s actual catalog, and performs no automated threat analysis.

Unlike earlier revisions, a Control's declared `maxBypassProbability` (an
optional pair with `bypassOutcome` — see `control-threshold-demo.etdl`
below) *is* verified against the document's own resolved numbers, and can
be checked live via `security.control_effective` — spec Section 7.

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

```yaml
# W-416: see control-threshold-demo.etdl's SlowRequestPacing leaf, which
# is deliberately left uncategorized *and* unmitigated (only
# DistributedRequestFlood is both).
```

## `control-threshold-demo.etdl`

```bash
etdl validate control-threshold-demo.etdl
etdl compile control-threshold-demo.etdl --out-dir ./generated
```

```text
$ etdl validate control-threshold-demo.etdl
document 'control-threshold-demo.etdl' is valid (0 errors, 0 warnings)
```

`RateLimitBarrier`'s `FAILURE` branch is backed by a real Fault Tree
(`RateLimitBypassFailure`) rather than a static literal, specifically so
it can be live-tracked. The Control mapped onto it declares `bypassOutcome:
FAILURE`/`maxBypassProbability: 0.05` — verified at compile time against
the resolved 0.01 probability (well under the ceiling), and checked live
via the branch condition `security.control_effective == false`
(`etdl.live-reliability` is also declared, the ECEL path's required
second supplement).

Triggering `E-141`/`E-142`/`E-143`:

```yaml
# E-142: sil declared inconsistent with the resolved bypass probability
# (0.01 sits well under 0.05; a ceiling below the resolved value fails)
maxBypassProbability: 0.001
```

```yaml
# E-141: bypassOutcome and maxBypassProbability must both be declared,
# or neither
bypassOutcome: "FAILURE"
# maxBypassProbability omitted
```

```bash
# E-143: security.control_effective used without etdl.live-reliability
# declared — comment out that supplement's line and re-validate.
```

The generated code's own runtime behavior
(`security.control_effective`-driven branch selection reacting to a live
probability drift) is not something `etdl compile` itself demonstrates —
see `etdl-compiler/tests/security_codegen_test.rs` for a real, `cargo
run`-executed proof.

## Compatibility

Comment out the `supplements: [{id: etdl.security, ...}]` block (leaving
`etdl.tree-event` declared and both `x-tree-event`/`x-security` in place)
and re-run `etdl validate` — it stays valid with zero security-related
diagnostics, and compiling produces byte-for-byte identical generated Rust
to a version with `x-security` removed entirely, proving `x-security` is
additive metadata (spec Section 7). Removing only `etdl.live-reliability`
from `control-threshold-demo.etdl` (leaving `etdl.security` declared)
removes `security.control_effective`'s availability specifically
(`E-143`) while bypass-threshold enforcement (`E-142`) still applies — it
needs nothing beyond `etdl.security` itself.
