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
under a different interpretation.

## 2. Scope (NORMATIVE)

This supplement defines:

- The Threat Model Object and Control Object data models (§4) and how a
  document declares them (`x-security`, the same generic `x-*` extension
  mechanism every ETDL extension already uses — **no core language or
  parser change was made or is required**).
- STRIDE classification of a Tree Event Supplement tree's leaves.
- Mapping of Controls onto core Barrier nodes and the threats they mitigate.

This supplement does **not** define:

- Any new tree structure — attack-tree validity (cycles, arity, reachability)
  is entirely `etdl.tree-event`'s own responsibility, unmodified.
- Whether a Control's claim to mitigate a threat is actually true, or
  validation of `controlId` against its named `framework`'s real catalog
  (this specification is not the authority for NIST 800-53, ISO 27001, or
  any other external standard).
- Any automated, formal, or AI-assisted threat analysis.

## 3. Terminology (NORMATIVE)

| Term | Meaning |
|---|---|
| Threat Model | A Threat Model Object (§4.1): an `etdl.tree-event` Tree reinterpreted as an attack tree, with a STRIDE category assigned to each leaf that has one |
| STRIDE Category | One of `spoofing`, `tampering`, `repudiation`, `information-disclosure`, `denial-of-service`, `elevation-of-privilege` |
| Control | A Control Object (§4.2): a core Barrier node given a security-control identity and a list of the threats it mitigates |
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
}
```

`id`/`treeRef`/`leafCategories` are REQUIRED on a Threat Model and unique
within `x-security.threatModels`. `id`/`nodeRef`/`controlId`/`mitigates`
are REQUIRED on a Control and unique within `x-security.controls`; not
every leaf needs a `leafCategories` entry — an uncategorized leaf is not
itself an error.

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

## 6. Reference resolution (NORMATIVE)

A Threat Model's `leafCategories` **keys** are checked against that
specific Threat Model's own resolved tree's leaves (`E-141` if not a
leaf). A Control's `mitigates` entries are checked against the **union**
of every successfully-resolved Threat Model's tree's leaves — the spec's
field description ("Leaf node ids from *some* Threat Model's `treeRef`
tree") does not name a specific one when more than one Threat Model is
declared. A Control's `nodeRef` is checked against the document's own
`eventTrees`, Barrier kind only (`^#/eventTrees/[^/]+/nodes/[^/]+$` — the
node-level shape only, same as Safety's).

## 7. Compiler integration (NORMATIVE)

Implemented entirely in `etdl-compiler::security` — no dedicated structural
crate; an attack tree's structure is entirely `etdl-tree-core`'s
responsibility already.

**Registered, but not pipeline-special-cased (NORMATIVE for 1.0).** Same
shape as Performance/Safety/Diagnostics: `SecurityExtension` is registered
unconditionally in `extension::builtin_registry()` and separately seeded
into `Compiler::new()`'s `extensions` list, running through the generic
`EtdlExtension::validate`/`process` path rather than a special-cased direct
call in `lib.rs`. `SecurityExtension::descriptor()` returns a
`SupplementDescriptor` (including `requires: &["etdl.tree-event"]`)
colocated with `parse_and_validate_security` in this same module, which
`etdl capabilities`/`etdl supplement list` read generically — see
[Performance](performance-supplement.md#7-compiler-integration-normative)
for the full mechanism.

## 8. `x-security` example (INFORMATIVE)

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
```

See `examples/security/attack-tree-demo.etdl` for a complete, runnable
document (including an intentional `W-411` to demonstrate that diagnostic).

## 9. Validation (NORMATIVE)

`security::parse_and_validate_security` checks, collecting every problem in
one pass:

1. `x-security` is only processed when the document declares
   `supplements: [{id: etdl.security, ...}]` (§10).
2. A `threatModels`/`controls` key that is present but fails to deserialize
   is `E-140` (for Threat Models) or `E-141` (for Controls) — no dedicated
   "manifest invalid" code exists here either.
3. A duplicate `id` within `threatModels` is `E-140`; within `controls` is
   `E-141` (folded into each object type's own bucket — neither is
   explicit in the spec's diagnostic table).
4. An unresolvable `treeRef`, or a `leafCategories` value not one of §3's
   six STRIDE categories, is `E-140`.
5. A `leafCategories` key not a leaf of its own Threat Model's tree, an
   empty or unresolvable-entry `mitigates` list, or an unresolvable/
   wrong-kind Control `nodeRef`, is `E-141`.
6. A `mitigates` entry that *is* a valid leaf (§6) but that no declared
   Threat Model's `leafCategories` assigns a category to (whether or not
   that category's own STRIDE value was itself valid — a key existing is
   what "assigns a category" means for this rule) is `W-411`.

## 10. Compatibility (NORMATIVE)

Silently ignoring `x-security` (core Section 11.1's baseline behavior)
leaves a document fully valid under core and `etdl.tree-event` alone —
threat classification and control mapping are additive metadata, never a
precondition for parsing, validation, or code generation.
`examples/security/README.md` demonstrates this directly.

## 11. Versioning (NORMATIVE)

| Axis | Value |
|---|---|
| Schema | `etdl.security/1.0` (`etdl_compiler::security::SECURITY_SCHEMA`) |
| Supplement id/version (as declared via `supplements:`) | `etdl.security` / `"1.0"`, checked by the same major-version-gate rule every supplement already uses |

## 20. Diagnostics (NORMATIVE)

| Code | Meaning |
|---|---|
| `E-140` | A Threat Model's `leafCategories` value is not a STRIDE category, `treeRef` doesn't resolve, `threatModels` failed to deserialize, or a duplicate Threat Model `id` was declared |
| `E-141` | A `leafCategories` key or `mitigates` entry is not a leaf of the relevant tree, a Control's `nodeRef` doesn't resolve to a Barrier, `mitigates` is empty, `controls` failed to deserialize, or a duplicate Control `id` was declared |
| `W-411` | A `mitigates` entry is a genuine leaf but no declared Threat Model categorizes it |

## 21. CLI (INFORMATIVE)

No dedicated `etdl security ...` subcommand exists, for the same reasoning
[Performance](performance-supplement.md#21-cli-informative) gives.

```bash
etdl validate examples/security/attack-tree-demo.etdl
etdl compile examples/security/attack-tree-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.security")'
```
