# ETDL Security Supplement 1.0 (`etdl.security`)

Sections marked **NORMATIVE** define required behavior any conforming
implementation must have. Sections marked **INFORMATIVE** are examples,
guidance, and rationale — not requirements. This document summarizes the
normative spec at `ETDL-Security-Supplement.md` (in the
`etdl-specification` repository) as implemented by `etdl-compiler`; the
spec itself is authoritative if the two ever disagree.

## 1. Purpose (INFORMATIVE)

Classifies threats and maps mitigating controls. Defines no new tree
structure of its own: an attack tree is structurally identical to any Tree
Event Supplement (`etdl.tree-event`) tree, so this supplement reuses that
supplement's already-validated Tree under a security interpretation (a
STRIDE category per leaf), and separately maps mitigating controls onto
core Barrier nodes — the same "give existing core structure a domain
meaning" pattern [Safety](safety-supplement.md) uses for the same node
under a different interpretation. Unlike earlier revisions, a Control's
declared `maxBypassProbability` is now verified against the resolved
probability of its `bypassOutcome` branch, and a Control can validate its
bypass rate live via the ECEL path `security.control_effective` when the
document also declares `etdl.live-reliability`.

## 2. Scope (NORMATIVE)

This supplement defines:

- The Threat Model Object and Control Object data models (§4) and how a
  document declares them (`x-security`, the same generic `x-*` extension
  mechanism every ETDL extension already uses — **no core language or
  parser change was made or is required**, including for
  `security.control_effective` — see §7.2).
- STRIDE classification of a Tree Event Supplement tree's leaves.
- Mapping of Controls onto core Barrier nodes and the threats they mitigate.
- Runtime meaning (§7) — a declared `maxBypassProbability` is verified
  against the `bypassOutcome` branch's resolved probability;
  `security.control_effective` makes that check observable live.

This supplement does **not** define:

- Any new tree structure — attack-tree validity (cycles, arity, reachability)
  is entirely `etdl.tree-event`'s own responsibility, unmodified.
- Whether a Control's claim to mitigate a threat is actually true beyond
  §7.1's numeric check, or validation of `controlId` against its named
  `framework`'s real catalog (this specification is not the authority for
  NIST 800-53, ISO 27001, or any other external standard).
- Any automated, formal, or AI-assisted threat analysis.

## 3. Terminology (NORMATIVE)

| Term | Meaning |
|---|---|
| Threat Model | A Threat Model Object (§4.1): an `etdl.tree-event` Tree reinterpreted as an attack tree, with a STRIDE category assigned to each leaf that has one |
| STRIDE Category | One of `spoofing`, `tampering`, `repudiation`, `information-disclosure`, `denial-of-service`, `elevation-of-privilege` |
| Control | A Control Object (§4.2): a core Barrier node given a security-control identity, a list of the threats it mitigates, and optionally a designated bypass outcome and bypass-probability ceiling |
| Bypass Outcome | The `bypassOutcome` branch's resolved probability is this control's probability of being bypassed — the value §7.1/§7.2 check |
| Framework | A free-form string naming the control catalog a `controlId` is drawn from — this specification does not own or validate any such catalog's contents |

## 4. Data model (NORMATIVE)

```rust
pub struct ThreatModel {
    pub id: String,
    pub tree_ref: String,                       // treeRef
    pub leaf_categories: BTreeMap<String, String>, // leafCategories; raw strings, validated against §3
}

pub struct Control {
    pub id: String,
    pub node_ref: String,       // nodeRef
    pub framework: Option<String>,
    pub control_id: String,     // controlId
    pub mitigates: Vec<String>, // REQUIRED, non-empty
    pub bypass_outcome: Option<String>,          // bypassOutcome
    pub max_bypass_probability: Option<f64>,     // maxBypassProbability
}
```

`id`/`treeRef`/`leafCategories` are REQUIRED on a Threat Model and unique
within `x-security.threatModels`. `id`/`nodeRef`/`controlId`/`mitigates`
are REQUIRED on a Control and unique within `x-security.controls`; not
every leaf needs a `leafCategories` entry — an uncategorized leaf is not
itself an error. `bypassOutcome`/`maxBypassProbability` are OPTIONAL and
co-required (declaring one without the other is `E-141`) — a wholly new,
additive capability, not a retrofit of an existing mandatory field.

## 5. The `etdl.tree-event` dependency (NORMATIVE)

**This is the one built-in supplement with a real cross-supplement
dependency.** `treeRef` names the `id` of a Tree declared under this same
document's `x-tree-event.trees` — resolved by calling
[`crate::tree_event::parse_and_validate_trees`] directly (a pure function;
calling it again here is additional-but-harmless, the same
"each supplement independently re-derives its own inputs" shape
`validate()`/`process()` already use within a single supplement).

The spec's own worked example declares the dependency formally via
`supplements:`'s `metadata.x-requires` field:

```yaml
supplements:
  - id: etdl.security
    version: "1.0"
    metadata:
      x-requires:
        - id: etdl.tree-event
          range: ">=1.0 <2.0"
  - id: etdl.tree-event
    version: "1.0"
```

**This module does not parse or separately enforce that `x-requires`
metadata** — no generic supplement-dependency-declaration mechanism exists
anywhere in this codebase, and this implementation does not add one. The
dependency is instead a natural *consequence* of how `treeRef` resolves:
`parse_and_validate_trees` self-gates on `etdl.tree-event` also being
declared under `supplements:`, so a document declaring `etdl.security`
without `etdl.tree-event` sees zero trees, and every `treeRef` correctly
fails to resolve (`E-140`) — the practical effect the dependency
declaration asks for. See `examples/security/README.md` for a
demonstration.

Separately, `security.control_effective` (§7.2) has a second, narrow,
*optional* dependency on the Live Reliability Supplement
(`etdl.live-reliability`) — needed only for that one ECEL path, not for
anything else in this module.

## 6. Reference resolution (NORMATIVE)

A Threat Model's `leafCategories` **keys** are checked against that
specific Threat Model's own resolved tree's leaves (`E-141` if not a
leaf). A Control's `mitigates` entries are checked against the **union**
of every successfully-resolved Threat Model's tree's leaves — the spec's
field description ("Leaf node ids from *some* Threat Model's `treeRef`
tree") does not name a specific one when more than one Threat Model is
declared. A Control's `nodeRef` is checked against the document's own
`eventTrees`, Barrier kind only (`^#/eventTrees/[^/]+/nodes/[^/]+$` — the
node-level shape only, same as Safety's). A declared `bypassOutcome` is
then checked against that resolved Barrier node's own `branches[].outcome`
values — no match is also `E-141`.

## 7. Runtime meaning — bypass-threshold enforcement and live monitoring (NORMATIVE)

Where core's Fault-Tree evaluation computes a single probability, this
supplement gives two real consequences to a Control's declared
`bypassOutcome`/`maxBypassProbability` — not new probability mathematics
(core's computed value is never recomputed, re-derived, or overridden),
but real verification and, where the document opts in, real runtime
behavior.

### 7.1 Bypass-threshold enforcement

A Control's `bypassOutcome` branch resolves to a probability exactly the
way any other branch does (core's existing `probability`/
`probabilitySource` mechanism, unchanged). That resolved probability is
verified against the control's declared `maxBypassProbability`, when both
fields are declared: a resolved probability exceeding the ceiling is
`E-142`. This check (`security::validate_control_thresholds`) runs from
`Compiler::validate_with_base`, not through the generic
`EtdlExtension::validate` path every other Security rule uses — it is a
documented exception (parallel to Safety's own SIL↔PFD exception)
because it needs the *resolved* fault-tree probabilities
`ExtensionContext`'s generic signature cannot provide. The result: `etdl
validate` (not only `etdl compile`) catches a control whose real bypass
rate exceeds its declared ceiling before any code exists.

### 7.2 `security.control_effective` (ECEL)

A Control may use `security.control_effective` in a branch condition,
written as a comparison against a boolean literal
(`security.control_effective == true`) — reusing ECEL's existing
Comparison grammar, the same choice `safety.sil_maintained`/
`reliability.in_range`/`performance.in_budget` made for their own
analogous paths. Like `safety.sil_maintained`, this path depends on
**both** `etdl.security` and `etdl.live-reliability` being declared: its
ceiling comes from `x-security`, its live value comes from
`x-live-reliability`. Using it without both declared, with the wrong
shape, or nested inside `&&`/`||`/`!` instead of being the entire branch
condition, is `E-143`.

It resolves to whether the Fault Tree behind the control's own
`bypassOutcome` branch's `probabilitySource` — which must also be
declared under `x-live-reliability` for this path to be usable at all —
is *currently* live-tracked at or under the declared
`maxBypassProbability` ceiling (`etdl_core::live::current_probability`).
With no live observations yet, it resolves to `true` (fail-open — the
same "insufficient data is not an anomaly" convention
`reliability.in_range`/`performance.in_budget`/`safety.sil_maintained`
all already use). A `bypassOutcome` branch whose probability is a static
literal (no `probabilitySource`) has no Fault Tree to live-track, so
cannot be used with `security.control_effective` — a codegen-time error,
`E-109`.

## 8. Compiler integration (NORMATIVE)

Implemented entirely in `etdl-compiler::security` — no dedicated structural
crate; an attack tree's structure is entirely `etdl-tree-core`'s
responsibility already.

**Registered generically, with one documented pipeline exception.** Same
shape as Performance/Safety/Diagnostics: `SecurityExtension` is registered
unconditionally in `extension::builtin_registry()` and separately seeded
into `Compiler::new()`'s `extensions` list, so `parse_and_validate_security`
(structural checks, §6/§9 rules) runs through the generic
`EtdlExtension::validate`/`process` path rather than a special-cased direct
call in `lib.rs`. `security::validate_control_thresholds` (§7.1's `E-142`)
is the **one exception**: it is called directly from
`Compiler::validate_with_base` because it needs *resolved* fault-tree
probabilities `ExtensionContext` cannot provide — the same kind of
legitimate exception `safety::validate_sil_constraints` already is.
`SecurityExtension::descriptor()` returns a `SupplementDescriptor`
(including `requires: &["etdl.tree-event"]`) colocated with
`parse_and_validate_security` in this same module, which `etdl
capabilities`/`etdl supplement list` read generically — see
[Performance](performance-supplement.md#7-compiler-integration-normative)
for the full mechanism.

Codegen (`etdl-compiler::codegen::rust`) reads `security::SecurityData`
(parsed once per `generate_all` call, `CodegenCtx.security`, mirroring
`CodegenCtx.safety`) and renders `security.control_effective` (§7.2) in
`render_condition`/`try_render_security_condition`, alongside the
pre-existing `safety.sil_maintained` rendering.

## 9. `x-security` example (INFORMATIVE)

```yaml
supplements:
  - id: etdl.security
    version: "1.0"
  - id: etdl.tree-event
    version: "1.0"

x-tree-event:
  trees:
    - id: "gateway-compromise"
      version: "1"
      root: "GatewayCompromised"
      nodes:
        CredentialStuffing: { kind: leaf }
        ApiKeyLeak: { kind: leaf }
        GatewayCompromised:
          kind: gate
          gate: OR
          children: ["CredentialStuffing", "ApiKeyLeak"]

x-security:
  threatModels:
    - id: payment-gateway-attack-tree
      treeRef: "gateway-compromise"
      leafCategories:
        CredentialStuffing: spoofing
        ApiKeyLeak: information-disclosure
  controls:
    - id: gateway-rate-limiter
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      framework: "NIST-800-53"
      controlId: "SC-5"
      mitigates: ["CredentialStuffing"]
      bypassOutcome: "FAILURE"
      maxBypassProbability: 0.02
```

See `examples/security/attack-tree-demo.etdl` for a complete, runnable
document (including an intentional `W-411` to demonstrate that
diagnostic), and `examples/security/control-threshold-demo.etdl` for
§7.1/§7.2's bypass-threshold enforcement and live ECEL check.

## 10. Validation (NORMATIVE)

`security::parse_and_validate_security` checks, collecting every problem in
one pass:

1. `x-security` is only processed when the document declares
   `supplements: [{id: etdl.security, ...}]` (§11).
2. A `threatModels`/`controls` key that is present but fails to deserialize
   is `E-140` (for Threat Models) or `E-141` (for Controls) — no dedicated
   "manifest invalid" code exists here either.
3. A duplicate `id` within `threatModels` is `E-140`; within `controls` is
   `E-141` (folded into each object type's own bucket — neither is
   explicit in the spec's diagnostic table).
4. An unresolvable `treeRef`, or a `leafCategories` value not one of §3's
   six STRIDE categories, is `E-140`.
5. A `leafCategories` key not a leaf of its own Threat Model's tree, an
   empty or unresolvable-entry `mitigates` list, an unresolvable/
   wrong-kind Control `nodeRef`, exactly one of `bypassOutcome`/
   `maxBypassProbability` declared, or a `bypassOutcome` not matching one
   of the resolved Barrier's own branch outcomes, is `E-141`.
6. A `mitigates` entry that *is* a valid leaf (§6) but that no declared
   Threat Model's `leafCategories` assigns a category to (whether or not
   that category's own STRIDE value was itself valid — a key existing is
   what "assigns a category" means for this rule) is `W-411`.
7. A `leafCategories` entry (a leaf some Threat Model categorized) that
   zero declared Controls' `mitigates` targets anywhere is `W-416` — the
   inverse of rule 6, checked once over the fully-parsed document rather
   than inline in either parsing loop.

Separately, outside `parse_and_validate_security` (see §8): a Control's
`bypassOutcome` branch resolving to a probability exceeding its declared
`maxBypassProbability` (§7.1) is `E-142`, checked from
`Compiler::validate_with_base`. A branch condition's `security.*` path
misuse (§7.2) is `E-143`, reported by `typeck`.

## 11. Compatibility (NORMATIVE)

Silently ignoring `x-security` (core Section 11.1's baseline behavior)
leaves a document fully valid under core and `etdl.tree-event` alone —
threat classification and control mapping are additive metadata, never a
precondition for parsing, validation, or code generation. A document
declaring `etdl.security` without also declaring `etdl.live-reliability`
is unaffected by §7.2 specifically (no `security.control_effective`
availability) while §7.1 still applies — that check needs nothing beyond
this supplement itself. `examples/security/README.md` demonstrates the
base compatibility case directly.

## 12. Versioning (NORMATIVE)

| Axis | Value |
|---|---|
| Schema | `etdl.security/1.0` (`etdl_compiler::security::SECURITY_SCHEMA`) |
| Supplement id/version (as declared via `supplements:`) | `etdl.security` / `"1.0"`, checked by the same major-version-gate rule every supplement already uses |

This supplement was extended in place, still at version `1.0` — the
entire specification remains `Status: Under Development — NOT YET
RELEASED`, so there is no released `1.0` behavior to protect against this
change. A future `1.x` minor may add fields to the Threat Model or
Control Object; it must not change the meaning of the six STRIDE
categories, or §7's enforcement/resolution semantics, without a major
bump.

## 20. Diagnostics (NORMATIVE)

| Code | Meaning |
|---|---|
| `E-140` | A Threat Model's `leafCategories` value is not a STRIDE category, `treeRef` doesn't resolve, `threatModels` failed to deserialize, or a duplicate Threat Model `id` was declared |
| `E-141` | A `leafCategories` key or `mitigates` entry is not a leaf of the relevant tree, a Control's `nodeRef` doesn't resolve to a Barrier, `mitigates` is empty, exactly one of `bypassOutcome`/`maxBypassProbability` is declared, a `bypassOutcome` doesn't match a branch outcome, `controls` failed to deserialize, or a duplicate Control `id` was declared |
| `E-142` | A Control's `bypassOutcome` branch resolves to a probability exceeding its declared `maxBypassProbability` |
| `E-143` | A branch condition uses the `security.*` ECEL path root without the document declaring `etdl.security`, without also declaring `etdl.live-reliability`, or the path isn't exactly `security.control_effective` |
| `W-411` | A `mitigates` entry is a genuine leaf but no declared Threat Model categorizes it |
| `W-416` | A Threat Model categorizes a leaf that zero declared Controls' `mitigates` targets |

## 21. CLI (INFORMATIVE)

No dedicated `etdl security ...` subcommand exists, for the same reasoning
[Performance](performance-supplement.md#21-cli-informative) gives.

```bash
etdl validate examples/security/attack-tree-demo.etdl
etdl compile examples/security/attack-tree-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.security")'
```

The generated code's own runtime behavior
(`security.control_effective`-driven branch selection reacting to a live
probability drift) is not something `etdl compile` itself demonstrates —
see `etdl-compiler/tests/security_codegen_test.rs` for a real, `cargo
run`-executed proof.
