# ETDL Safety Supplement 1.0 (`etdl.safety`)

Sections marked **NORMATIVE** define required behavior any conforming
implementation must have. Sections marked **INFORMATIVE** are examples,
guidance, and rationale — not requirements. This document summarizes the
normative spec at `ETDL-Safety-Supplement.md` (in the `etdl-specification`
repository) as implemented by `etdl-compiler`; the spec itself is
authoritative if the two ever disagree.

## 1. Purpose (INFORMATIVE)

Classifies hazards and gives safety meaning — Safety Integrity Level,
independence, common-cause grouping — to structures ETDL Core already
defines: the Consequence node and the Barrier node. Defines no new
probability mathematics; residual risk is read from core's own Fault-Tree
evaluation, never recomputed here.

## 2. Scope (NORMATIVE)

This supplement defines:

- The Hazard Object and Safety Barrier Object data models (§4) and how a
  document declares them (`x-safety`, the same generic `x-*` extension
  mechanism every ETDL extension already uses — **no core language or
  parser change was made or is required**).
- Reference resolution: a Hazard's `consequenceRef` must resolve to a
  Consequence node; a Safety Barrier's `nodeRef` must resolve to a Barrier
  node, both already defined elsewhere in the same document.
- The Section 4.1 risk matrix (severity x likelihood -> Risk Index) and the
  contradiction check between mutual `independentOf` claims and a shared
  `commonCauseGroup` (§9).

This supplement does **not** define:

- Any new probability computation. A Hazard's residual risk is exactly the
  branch probability already reachable through the Event Tree (§6).
- Certification or derivation of a Safety Integrity Level — `sil` records
  an assignment, it does not compute or validate one against IEC 61508.
- Validation that an `independentOf`/`commonCauseGroup` claim is
  *empirically* true — only that the claims as declared are not
  self-contradictory (§9).

## 3. Terminology (NORMATIVE)

| Term | Meaning |
|---|---|
| Hazard | A Hazard Object (§4.1): a named source of harm, classified by severity and likelihood, tied to a specific Consequence |
| Severity | One of `catastrophic`, `critical`, `marginal`, `negligible` |
| Likelihood | One of `frequent`, `probable`, `occasional`, `remote`, `improbable` |
| Risk Index | An integer 1-4 read from the §4.1 risk matrix; lower is worse |
| Safety Barrier | A Safety Barrier Object (§4.2): a core Barrier node given a Safety Integrity Level and an independence declaration |
| Safety Integrity Level (SIL) | An integer 1-4 (IEC 61508 scale) a Safety Barrier is assigned — this supplement records the assignment, it does not certify or derive one |
| Common-Cause Group | A free-form string tag identifying a shared failure cause; two barriers sharing a tag are not independent regardless of an `independentOf` declaration |

## 4. Data model (NORMATIVE)

```rust
pub struct Hazard {
    pub id: String,
    pub description: String,
    pub severity: String,        // raw string; validated against the enum below
    pub likelihood: String,      // raw string; validated against the enum below
    pub risk_index: i64,         // riskIndex
    pub consequence_ref: String, // consequenceRef
}

pub struct SafetyBarrier {
    pub id: String,
    pub node_ref: String,               // nodeRef
    pub sil: i64,
    pub independent_of: Vec<String>,    // independentOf, default []
    pub common_cause_group: Option<String>, // commonCauseGroup
}
```

`id` is REQUIRED and unique within `x-safety.hazards`/`x-safety.barriers`
respectively. `severity`/`likelihood` are REQUIRED and must be one of the
enumerated values in §3. `riskIndex`/`sil` are REQUIRED integers in
`[1,4]`. `consequenceRef`/`nodeRef` are REQUIRED Internal References
(`^#/eventTrees/[^/]+/nodes/[^/]+$` — the node-level shape only, no
whole-tree alternative). `independentOf` and `commonCauseGroup` are
OPTIONAL on a Safety Barrier.

**Risk matrix** (severity x likelihood -> Risk Index):

| | frequent | probable | occasional | remote | improbable |
|---|---|---|---|---|---|
| **catastrophic** | 1 | 1 | 1 | 2 | 2 |
| **critical** | 1 | 1 | 2 | 2 | 3 |
| **marginal** | 1 | 2 | 3 | 3 | 4 |
| **negligible** | 2 | 3 | 4 | 4 | 4 |

## 5. Reference resolution (NORMATIVE)

Both `consequenceRef` and `nodeRef` are checked against the document's own
`eventTrees`, not a generic JSON-Pointer walk (the same manual-parse style
`performance::resolve_node_ref`/`validate::check_transfers` use): a
`consequenceRef` must name a node that is specifically a **Consequence**;
a `nodeRef` must name a node that is specifically a **Barrier**. Either
resolving to the wrong node kind, or to nothing at all, is `E-131`.

## 6. Residual risk — relationship to Fault-Tree evaluation (NORMATIVE)

A Hazard's `consequenceRef` typically names a Consequence reached through
an Operation whose failure path is protected by one or more Safety
Barriers, which in turn may derive their branch probability from a Fault
Tree's Top Event. This supplement defines no new computation over that
value: the residual probability of a hazard occurring, after its protecting
barriers, is exactly the core-computed branch probability already reachable
through the Event Tree — this supplement only adds the hazard
classification and barrier metadata an external safety-case tool needs to
interpret that number.

## 7. Compiler integration (NORMATIVE)

Implemented entirely in `etdl-compiler::safety` — a plain module, no
dedicated structural crate, for the same reason as
[Performance](performance-supplement.md): a Hazard/Safety Barrier only
cross-references the document's own existing `eventTrees` and has no
reusable structural model a third domain would independently consume.

**Registered, but not pipeline-special-cased (NORMATIVE for 1.0).** Like
Performance and unlike Tree Event/Reliability: `SafetyExtension` is
registered unconditionally in `extension::builtin_registry()` (so `etdl
capabilities`/`etdl supplement list`/E-108/W-407 all see it) *and*
separately seeded into `Compiler::new()`'s `extensions` list, so it runs
through the same generic, registry-driven `EtdlExtension::validate`/
`process` path a third-party `Compiler::with_extension` supplement uses —
not a special-cased direct function call anywhere in `lib.rs`.
`SafetyExtension::descriptor()` returns a `SupplementDescriptor` colocated
with `parse_and_validate_safety` in this same module, which `etdl
capabilities`/`etdl supplement list` read generically — see
[Performance](performance-supplement.md#7-compiler-integration-normative)
for the full mechanism.

## 8. `x-safety` example (INFORMATIVE)

```yaml
supplements:
  - id: etdl.safety
    version: "1.0"

x-safety:
  hazards:
    - id: gateway-unavailable-during-payment
      description: "payment cannot be captured while the gateway is down"
      severity: critical
      likelihood: remote
      riskIndex: 2
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/PaymentFailedConsequence"
  barriers:
    - id: retry-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 2
      independentOf: ["fallback-gateway-barrier"]
      commonCauseGroup: "primary-network-path"
    - id: fallback-gateway-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/FallbackBarrier"
      sil: 1
      independentOf: ["retry-barrier"]
      commonCauseGroup: "secondary-network-path"
```

See `examples/safety/hazard-demo.etdl` for a complete, runnable document,
and `examples/safety/contradictory-independence.etdl` for the `E-132`
counter-example (same two barriers, same `commonCauseGroup`).

## 9. Validation (NORMATIVE)

`safety::parse_and_validate_safety` checks, collecting every problem in one
pass:

1. `x-safety` is only processed when the document declares
   `supplements: [{id: etdl.safety, ...}]` — never merely because the field
   is present (§10).
2. A `hazards`/`barriers` key that is present but fails to deserialize as
   an array is `E-130` (the spec's own diagnostic table has no dedicated
   "manifest invalid" code, so this is folded into E-130's existing
   multi-condition bucket, the same interpretation Performance's
   `E-160` makes for its own manifest).
3. A duplicate `id` within `hazards` or within `barriers` is `E-130`
   (§4.1/4.2 both say "unique within ..." as a MUST, but §5's table has no
   dedicated code for it either).
4. A Hazard's `severity`/`likelihood` not one of §3's enumerated values, or
   `riskIndex` outside `[1,4]`, is `E-130`.
5. A Safety Barrier's `sil` outside `[1,4]` is `E-130`.
6. An unresolvable, or wrong-kind, `consequenceRef`/`nodeRef` (§5) is
   `E-131`.
7. Two Safety Barriers **mutually** listing each other in `independentOf`
   (both directions — a one-sided claim forms no edge) while sharing a
   non-empty `commonCauseGroup`, directly or transitively through further
   mutual pairs, is `E-132` — checked only among barriers that already
   passed rules 3/5/6.
8. A Hazard's declared `riskIndex` not matching the §4.1 matrix lookup for
   its (valid) `severity`/`likelihood` is `W-410` — a warning, not a
   rejection; the hazard remains in the accepted result. Not checked when
   `severity`/`likelihood`/`riskIndex` are themselves invalid (rule 4
   already covers that).

## 10. Compatibility (NORMATIVE)

Silently ignoring `x-safety` (core Section 11.1's baseline behavior) leaves
a document fully valid under core alone — hazard classification and SIL
assignment are additive metadata, never a precondition for parsing,
validation, or code generation. `examples/safety/README.md` demonstrates
this directly.

## 11. Versioning (NORMATIVE)

| Axis | Value |
|---|---|
| Schema | `etdl.safety/1.0` (`etdl_compiler::safety::SAFETY_SCHEMA`) |
| Supplement id/version (as declared via `supplements:`) | `etdl.safety` / `"1.0"`, checked by the same major-version-gate rule every supplement already uses |

A future `1.x` minor may add hazard/barrier fields (e.g. a
mitigation-tracking field); it must not change the meaning of `severity`,
`likelihood`, `sil`, or the risk matrix without a major bump.

## 20. Diagnostics (NORMATIVE)

| Code | Meaning |
|---|---|
| `E-130` | A Hazard's `severity`/`likelihood`/`riskIndex`, or a Safety Barrier's `sil`, is invalid; `hazards`/`barriers` failed to deserialize; or a duplicate `id` was declared |
| `E-131` | A Hazard's `consequenceRef`, or a Safety Barrier's `nodeRef`, does not resolve to a node of the required kind |
| `E-132` | Two Safety Barriers mutually claim `independentOf` each other (directly or transitively) while sharing a non-empty `commonCauseGroup` — self-contradictory |
| `W-410` | A Hazard's declared `riskIndex` does not equal the risk matrix value for its `severity`/`likelihood` pair |

`E-130`-`E-132`/`W-410` are scoped to this supplement's own namespace of
meaning; they do not collide with core Section 7's codes or with any other
supplement's codes.

## 21. CLI (INFORMATIVE)

No dedicated `etdl safety ...` subcommand exists, for the same reasoning
[Performance](performance-supplement.md#21-cli-informative) gives: no
extract-and-render use case beyond what `etdl validate --json`/`etdl
compile` diagnostics and `etdl capabilities` already surface.

```bash
etdl validate examples/safety/hazard-demo.etdl
etdl compile examples/safety/hazard-demo.etdl --out-dir ./generated
etdl capabilities --json | jq '.extensions[] | select(.id == "etdl.safety")'
```
