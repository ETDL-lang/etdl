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
evaluation, never recomputed here. Unlike earlier revisions, a declared
`sil` is now verified against the barrier's actual resolved failure
probability, that commitment is observable live at runtime via the ECEL
path `safety.sil_maintained` (when the document also declares
`etdl.live-reliability`), and a declared `independentOf` claim is verified
against real fault-tree structure, not only checked for self-consistency.

## 2. Scope (NORMATIVE)

This supplement defines:

- The Hazard Object and Safety Barrier Object data models (§4) and how a
  document declares them (`x-safety`, the same generic `x-*` extension
  mechanism every ETDL extension already uses — **no core language or
  parser change was made or is required**, including for
  `safety.sil_maintained` — see §6.2).
- Reference resolution: a Hazard's `consequenceRef` must resolve to a
  Consequence node; a Safety Barrier's `nodeRef` must resolve to a Barrier
  node, both already defined elsewhere in the same document; a Safety
  Barrier's `failureOutcome` must equal one of that Barrier node's own
  `branches[].outcome` values.
- The Section 4.1 risk matrix (severity x likelihood -> Risk Index) and the
  contradiction check between mutual `independentOf` claims and a shared
  `commonCauseGroup` (§9).
- Runtime meaning (§6) — a declared `sil` is verified against the
  `failureOutcome` branch's resolved probability; `safety.sil_maintained`
  makes that check observable live; `independentOf` is verified against
  real fault-tree structure via minimal-cut-set analysis.

This supplement does **not** define:

- Any new probability computation. A Hazard's residual risk, and a Safety
  Barrier's `failureOutcome` probability, are exactly the branch
  probabilities already reachable through the Event Tree.
- Certification of a Safety Integrity Level — `sil` remains an
  author-declared assignment; the compiler only verifies it is consistent
  with the document's own resolved numbers, it never derives or assigns
  one (that remains an external, human safety-case activity).
- Cross-service propagation of live SIL data beyond what the Live
  Reliability Supplement already propagates for the underlying Fault Tree
  — this supplement reads that same live-tracked value, it does not add a
  second channel.

## 3. Terminology (NORMATIVE)

| Term | Meaning |
|---|---|
| Hazard | A Hazard Object (§4.1): a named source of harm, classified by severity and likelihood, tied to a specific Consequence |
| Severity | One of `catastrophic`, `critical`, `marginal`, `negligible` |
| Likelihood | One of `frequent`, `probable`, `occasional`, `remote`, `improbable` |
| Risk Index | An integer 1-4 read from the §4.1 risk matrix; lower is worse |
| Safety Barrier | A Safety Barrier Object (§4.2): a core Barrier node given a Safety Integrity Level, a designated failure outcome, and an independence declaration |
| Safety Integrity Level (SIL) | An integer 1-4 (IEC 61508 scale) a Safety Barrier is assigned — verified against the `failureOutcome` branch's resolved probability via the §6.1 PFD bands; still not a certification |
| Failure Outcome | The `failureOutcome` branch's resolved probability is this barrier's probability of failure on demand (PFD) — the value §6.1/§6.2 check |
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
    pub failure_outcome: String,        // failureOutcome
    pub independent_of: Vec<String>,    // independentOf, default []
    pub common_cause_group: Option<String>, // commonCauseGroup
}
```

`id` is REQUIRED and unique within `x-safety.hazards`/`x-safety.barriers`
respectively. `severity`/`likelihood` are REQUIRED and must be one of the
enumerated values in §3. `riskIndex`/`sil` are REQUIRED integers in
`[1,4]`. `consequenceRef`/`nodeRef` are REQUIRED Internal References
(`^#/eventTrees/[^/]+/nodes/[^/]+$` — the node-level shape only, no
whole-tree alternative). `failureOutcome` is REQUIRED and must equal one
of the `branches[].outcome` values of the Barrier node named by
`nodeRef`. `independentOf` and `commonCauseGroup` are OPTIONAL on a Safety
Barrier.

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
`failureOutcome` is then checked against that resolved Barrier node's own
`branches[].outcome` values — no match is also `E-131`.

## 6. Runtime meaning — SIL enforcement, live monitoring, verified independence (NORMATIVE)

Where core's Fault-Tree evaluation computes a single probability, this
supplement gives three real consequences to the metadata layered on top
of it — not new probability mathematics (core's computed value is never
recomputed, re-derived, or overridden), but real verification and, where
the document opts in, real runtime behavior.

### 6.1 SIL &harr; PFD enforcement

A Safety Barrier's `failureOutcome` branch resolves to a probability
exactly the way any other branch does (core's existing
`probability`/`probabilitySource` mechanism, unchanged). That resolved
probability is verified against the IEC 61508 low-demand-mode
PFD-per-demand band the barrier's declared `sil` implies:

| SIL | PFD band |
|---|---|
| 1 | `[1e-2, 1e-1)` |
| 2 | `[1e-3, 1e-2)` |
| 3 | `[1e-4, 1e-3)` |
| 4 | `[1e-5, 1e-4)` |

A resolved probability outside the band for its declared `sil` is
`E-133`. This check (`safety::validate_sil_constraints`) runs from
`Compiler::validate_with_base`, not through the generic
`EtdlExtension::validate` path every other Safety rule uses — it is the
one documented exception (parallel to `tree_event`'s own pipeline
exception) because it needs the *resolved* fault-tree probabilities
`ExtensionContext`'s generic signature has no way to receive. The result:
`etdl validate` (not only `etdl compile`) catches a misrepresented SIL
before any code exists.

### 6.2 `safety.sil_maintained` (ECEL)

A Safety Barrier may use `safety.sil_maintained` in a branch condition,
written as a comparison against a boolean literal
(`safety.sil_maintained == true`) — reusing ECEL's existing Comparison
grammar, the same choice `reliability.in_range`/`performance.in_budget`
made for their own analogous paths. Unlike those two, this path depends
on **both** `etdl.safety` and `etdl.live-reliability` being declared: its
SIL band comes from `x-safety`, its live value comes from
`x-live-reliability`. Using it without both declared, with the wrong
shape, or nested inside `&&`/`||`/`!` instead of being the entire branch
condition, is `E-135`.

It resolves to whether the Fault Tree behind the barrier's own
`failureOutcome` branch's `probabilitySource` — which must also be
declared under `x-live-reliability` for this path to be usable at all —
is *currently* live-tracked within the same PFD band §6.1 checks at build
time (`etdl_core::live::current_probability`). With no live observations
yet, it resolves to `true` (fail-open — the same "insufficient data is
not an anomaly" convention `reliability.in_range`/`performance.in_budget`
both already use). A `failureOutcome` branch whose probability is a
static literal (no `probabilitySource`) has no Fault Tree to live-track,
so cannot be used with `safety.sil_maintained` — a codegen-time error,
`E-109`.

### 6.3 Verified independence

`independentOf` is no longer checked only for *self*-consistency (§9 rule
7's `E-132`, which only ever compares declared claims against each
other). Each declared `independentOf` entry is additionally verified
against the actual Fault Trees behind both barriers' `failureOutcome`
branches, when both resolve to a `probabilitySource`: computing each
Fault Tree's minimal cut sets (`fault_tree::enumerate_minimal_cut_sets`,
MOCUS) and checking whether their basic events overlap. A non-empty
intersection is real, structural evidence of a shared cause — `E-134` —
independent of whether `commonCauseGroup` was declared at all. The check
is one-directional: barrier A declaring `independentOf: [B]` is a
checkable claim about A and B regardless of whether B reciprocates,
unlike `E-132`'s mutual-claim scope.

A Fault Tree containing a `NOT`/`XOR` gate is non-coherent (minimal cut
sets are undefined for it); a pair involving one is skipped — neither
flagged as violating nor as satisfying independence.

## 7. Compiler integration (NORMATIVE)

Implemented entirely in `etdl-compiler::safety` — a plain module, no
dedicated structural crate, for the same reason as
[Performance](performance-supplement.md): a Hazard/Safety Barrier only
cross-references the document's own existing `eventTrees` and has no
reusable structural model a third domain would independently consume.

**Registered generically, with one documented pipeline exception.** Like
Performance: `SafetyExtension` is registered unconditionally in
`extension::builtin_registry()` (so `etdl capabilities`/`etdl supplement
list`/E-108/W-407 all see it) *and* separately seeded into
`Compiler::new()`'s `extensions` list, so `parse_and_validate_safety`
(hazards/barriers parsing, §9 rules 1-8, §6.3's `E-134`) runs through the
same generic, registry-driven `EtdlExtension::validate`/`process` path a
third-party `Compiler::with_extension` supplement uses.
`safety::validate_sil_constraints` (§6.1's `E-133`) is the **one
exception**: it is called directly from `Compiler::validate_with_base`
because it needs *resolved* fault-tree probabilities that
`ExtensionContext` cannot provide — the same kind of legitimate exception
`tree_event::parse_and_validate_trees` already is, for the same reason.
`SafetyExtension::descriptor()` returns a `SupplementDescriptor` colocated
with `parse_and_validate_safety` in this same module, which `etdl
capabilities`/`etdl supplement list` read generically — see
[Performance](performance-supplement.md#7-compiler-integration-normative)
for the full mechanism.

Codegen (`etdl-compiler::codegen::rust`) reads `safety::SafetyData` (parsed
once per `generate_all` call, `CodegenCtx.safety`, mirroring
`CodegenCtx.performance`/`CodegenCtx.live_reliability`) and renders
`safety.sil_maintained` (§6.2) in `render_condition`/
`try_render_safety_condition`, alongside the pre-existing
`reliability.in_range`/`performance.in_budget` rendering.

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
      failureOutcome: FAILURE
      independentOf: ["fallback-gateway-barrier"]
      commonCauseGroup: "primary-network-path"
    - id: fallback-gateway-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/FallbackBarrier"
      sil: 1
      failureOutcome: FAILURE
      independentOf: ["retry-barrier"]
      commonCauseGroup: "secondary-network-path"
```

See `examples/safety/hazard-demo.etdl` for a complete, runnable document
exercising all three of §6.1/6.2/6.3,
`examples/safety/contradictory-independence.etdl` for the `E-132`
counter-example (same two barriers, same `commonCauseGroup`), and
`examples/safety/shared-cause.etdl` for the `E-134` counter-example (two
fault-tree-backed barriers whose declared `independentOf` contradicts
their actual shared basic event).

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
6. An unresolvable, or wrong-kind, `consequenceRef`/`nodeRef` (§5), or a
   `failureOutcome` not matching one of the resolved Barrier node's own
   `branches[].outcome` values, is `E-131`.
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
9. Two Safety Barriers declaring `independentOf` each other (one-directional
   — see §6.3) whose `failureOutcome` branches' Fault Trees share at least
   one basic event is `E-134` — checked only among barriers that already
   passed rules 3/5/6, independently of rule 7's `commonCauseGroup` check.

Separately, outside `parse_and_validate_safety` (see §7): a `failureOutcome`
branch's resolved probability outside its declared `sil`'s PFD band (§6.1)
is `E-133`, checked from `Compiler::validate_with_base`. A branch
condition's `safety.*` path misuse (§6.2) is `E-135`, reported by
`typeck`.

## 10. Compatibility (NORMATIVE)

Silently ignoring `x-safety` (core Section 11.1's baseline behavior) leaves
a document fully valid under core alone — hazard classification and SIL
assignment are additive metadata, never a precondition for parsing,
validation, or code generation. A document declaring `etdl.safety` without
also declaring `etdl.live-reliability` is unaffected by §6.2 specifically
(no `safety.sil_maintained` availability) while §6.1/§6.3 still apply —
those two need nothing beyond this supplement itself.
`examples/safety/README.md` demonstrates the base compatibility case
directly.

## 11. Versioning (NORMATIVE)

| Axis | Value |
|---|---|
| Schema | `etdl.safety/1.0` (`etdl_compiler::safety::SAFETY_SCHEMA`) |
| Supplement id/version (as declared via `supplements:`) | `etdl.safety` / `"1.0"`, checked by the same major-version-gate rule every supplement already uses |

This supplement was extended in place, still at version `1.0` — the
entire specification remains `Status: Under Development — NOT YET
RELEASED`, so there is no released `1.0` behavior to protect against this
change. A future `1.x` minor may add hazard/barrier fields (e.g. a
mitigation-tracking field); it must not change the meaning of `severity`,
`likelihood`, `sil`, the risk matrix, the PFD bands, or
`safety.sil_maintained`'s resolution, without a major bump.

## 20. Diagnostics (NORMATIVE)

| Code | Meaning |
|---|---|
| `E-130` | A Hazard's `severity`/`likelihood`/`riskIndex`, or a Safety Barrier's `sil`, is invalid; `hazards`/`barriers` failed to deserialize; or a duplicate `id` was declared |
| `E-131` | A Hazard's `consequenceRef`, or a Safety Barrier's `nodeRef`, does not resolve to a node of the required kind; or a Safety Barrier's `failureOutcome` does not equal one of that Barrier node's own `branches[].outcome` values |
| `E-132` | Two Safety Barriers mutually claim `independentOf` each other (directly or transitively) while sharing a non-empty `commonCauseGroup` — self-contradictory |
| `E-133` | A Safety Barrier's `failureOutcome` branch resolves to a probability outside the IEC 61508 PFD-per-demand band its declared `sil` implies |
| `E-134` | Two Safety Barriers declare `independentOf` each other (one-directional) while their `failureOutcome` branches' Fault Trees share at least one basic event — the declared claim contradicts the actual fault-tree structure |
| `E-135` | A branch condition uses the `safety.*` ECEL path root without the document declaring `etdl.safety`, without also declaring `etdl.live-reliability`, or the path isn't exactly `safety.sil_maintained` |
| `W-410` | A Hazard's declared `riskIndex` does not equal the risk matrix value for its `severity`/`likelihood` pair |

`E-130`-`E-135`/`W-410` are scoped to this supplement's own namespace of
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

The generated code's own runtime behavior (`safety.sil_maintained`-driven
branch selection reacting to a live probability drift) is not something
`etdl compile` itself demonstrates — see
`etdl-compiler/tests/safety_codegen_test.rs` for a real, `cargo
run`-executed proof.
