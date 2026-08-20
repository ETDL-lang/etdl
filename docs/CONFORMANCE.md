# ETDL Conformance

This document defines what it means for an implementation to be ETDL-conformant,
what the conformance suite covers, and how a third-party implementation can use
the same corpus.

## What conformance means

ETDL §2.3 defines four independent conformance targets:

1. **Conforming Document** — a `.etdl` document that validates against the
   (future) Appendix E JSON Schema and satisfies every MUST-level rule in §7.
2. **Conforming Parser** — accepts every Conforming Document, rejects every
   MUST-level §7 violation with the corresponding diagnostic code, and resolves
   references per §5.3.
3. **Conforming Compiler** — a Conforming Parser that additionally satisfies §8.2
   and §8.3 for at least one target language.
4. **Conforming Runtime** — implements the §9 contract (BranchMonitor, SLA,
   chaos, traceparent) including the §12 production safeguard.

An implementation may satisfy any subset (e.g. a parser only, or a full
compiler + runtime). A "Conforming Compiler" claim implies the parser claims.

## The conformance suite

The reference conformance suite lives in `conformance/` of the ETDL compiler
repository:

```
conformance/
  conformance.rs        # declarative cases + runner (used by the integration test)
  valid/                # must-validate documents (future static corpus)
  invalid/              # must-reject documents with expected codes
  probability/          # expected top-event probabilities
  ...
```

Every case is a self-contained ETDL document plus expectations:

- `expected_valid: true/false`
- `expected_codes: [E-xxx, V-xxx, ...]` for invalid cases
- `expected_probability: (fault_tree, p)` for probability cases

The runner (`etdl-compiler/tests/conformance_test.rs`) loads a stub AsyncAPI
registry so reference resolution and type-checking stages execute.

## Using the suite in a third-party implementation

The suite is intentionally declarative and runner-agnostic. To run it against a
different implementation:

1. Copy `conformance/conformance.rs` (or re-implement its `Case` table in your
   language).
2. For each case:
   - parse the document (reject/accept per `expected_valid`),
   - assert the emitted diagnostic codes equal `expected_codes`,
   - assert the resolved top-event probability matches `expected_probability`.
3. Provide a stub AsyncAPI document exposing `components.messages.m` with a
   boolean `ok` payload (as the reference runner does).

Because the cases are data (YAML strings + expectations), porting the corpus is
mechanical.

## Diagnostic expectations

A conforming implementation MUST emit the diagnostic codes defined in the
specification (§7) and `docs/DIAGNOSTICS.md`. Codes are stable within a MAJOR
version; implementations must not reuse codes for different conditions.

## Compatibility rules

- `.etdl` documents are YAML 1.2 (JSON included).
- Deprecated fields (`eventTree`, `probabilityOfSuccess/Failure`) must be
  accepted for at least one MAJOR cycle and SHOULD surface an advisory.
- Unknown non-`x-` fields MUST be rejected; `x-*` fields MUST be preserved.
- The `etdl` version field follows SemVer; a parser accepts matching MAJOR and
  rejects unimplemented future MAJORs.

## Versioning

- Conformance definitions track the specification version (currently 1.0.0).
- A MINOR spec change that adds optional features does not invalidate existing
  conformant documents; it may add conformance cases.
- A MAJOR spec change may remove/replace rules; conformance claims are tied to
  the spec MAJOR.

## Future certification

Formal "ETDL 1.0 Compliant" certification is **future work** and is not yet
offered (the certification program is planned privately). The conformance suite
is the mechanism a future certification body would use; the runner reports
pass/fail per case so results can be published.

## Beyond the core specification: supplement conformance (2.0 additions)

Everything above this section describes conformance to the core
`etdl-specification` (§2.3's four conformance targets) and is unchanged by
the work below. **ETDL Conformance, Verification & Validation 1.0** extends
this with conformance Levels 2-7 — standard library, every supplement
(Generic Tree Event, Reliability, Predictive Reliability, Runtime Feedback
& Calibration), artifact/serialization, and a dependency-graph checker —
none of which the original four conformance targets or the `conformance/`
suite above covered. See:

- [`docs/reference/conformance-framework.md`](reference/conformance-framework.md)
  — the full guide (levels, vector schema, independent-oracle methodology,
  numerical tolerance policy, WASM profile, CI, release gates).
- [`docs/conformance/supplement-traceability-matrix.md`](conformance/supplement-traceability-matrix.md)
  — requirement-by-requirement traceability for each supplement, the
  companion to `docs/SPEC_IMPLEMENTATION_MATRIX.md` (which only covers the
  core spec).
- The `etdl-conformance` crate (`etdl-conformance/tests/*.rs`) — the
  vectors themselves. Run with `cargo test -p etdl-conformance`.
- `etdl conformance status` / `etdl conformance manifest` — objective
  per-area status and the machine-readable manifest.

## Current status

- Reference compiler: runs the suite (see `conformance/conformance.rs`).
- Third-party runners: not yet provided (documented for implementers).
- Appendix E JSON Schema: pending (spec gap; see spec repo addendum).
