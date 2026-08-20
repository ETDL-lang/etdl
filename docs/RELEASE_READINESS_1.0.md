# ETDL 1.0 Release Readiness

**Audit date:** 2026-08-19
**Workspace version:** 0.2.2 (crate/SemVer axis — see "Versioning" below)
**ETDL language version:** 1.0.0 (unchanged; already stable — see `docs/VERSIONING.md`)
**Conformance suite version:** 1.0.0 (`etdl-conformance`)

This document is the authoritative, current release-readiness record. It
supersedes the P0/P1/P2 findings in `docs/CURRENT_READINESS_AUDIT.md` and
`docs/READINESS_SCORECARD.md` (2026-08-13, workspace v0.2.0) where they
overlap — those files are kept as historical record with pointers here, not
rewritten.

## 1. What this pass covered

Per the task's own framing, this was **not** a new-feature pass. It was:
audit the full ecosystem as it actually exists (not as prior summaries
claimed), verify the four still-open P0 blockers from the 2026-08-13 audit
against current code, fix what was still live with regression tests,
complete the Conformance, Verification & Validation framework, and produce
an honest release decision. Nothing described below as "STABLE" or "fixed"
was accepted on a prior session's word — everything was re-verified by
reading the current source or running the current test suite.

## 2. Gap matrix (Part A)

| Area | Status | Release Blocker? | Action Taken |
|---|---|---|---|
| Core parser/compiler/codegen | STABLE, but had 2 live P0 defects | Was YES | Fixed (§3) |
| Runtime (`etdl-core`) | STABLE, but had 1 live P0 defect | Was YES | Fixed (§3) |
| Standard Library (`libraries:`, `std.events`/`std.logic`) | STABLE | No | Verified via existing + conformance tests |
| `std.probability` | STABLE | No | Verified via existing + `LIB-PROB-*` conformance vectors, all against an independent oracle |
| Generic Tree Event Supplement | STABLE, had 1 live robustness defect | Was YES (stack safety) | Fixed (§3), `TREE-*` conformance vectors added |
| Reliability Supplement | STABLE | No | Verified via existing + `REL-*` conformance vectors |
| Predictive Reliability Supplement | STABLE | No | Verified via existing + `PRED-*` conformance vectors against an independent oracle |
| Runtime Feedback & Calibration | STABLE | No | Verified via existing + `CAL-*` conformance vectors against an independent oracle; non-mutation re-verified |
| Artifacts (`ReliabilityArtifact`) | STABLE | No | `ART-*` conformance vectors added (round-trip, schema check, malformed-input rejection, identity, duplicate rejection) |
| Service-code failure discovery | STABLE | No | Verified: `CandidateStatus` never auto-reaches `Accepted` in production code paths (§7) |
| Conformance framework | Was INCOMPLETE | Was YES (this task's own primary goal) | Completed: Levels 0-7, 59 vectors, independent oracle, dependency-graph checker, manifest/CLI, docs |
| Ontology | STABLE, no accidental duplicates found | No | Audited (§6) |
| WASM | STABLE for what it claims | No | `cargo check --target wasm32-unknown-unknown` clean; CI `wasm` job unchanged |
| Docs | Had stale version references | No (documentation-only) | `API_STABILITY.md`, `docs/reference/crates.md` corrected |
| License/repo hygiene | Clean | No | Verified: no secrets, no tracked build artifacts, `LICENSE` present |
| Versioning | Consistent but not at 1.0.0 | No, but see §11 | Documented, not silently bumped |
| Canonical end-to-end example | PARTIAL | No | See §9 |

## 3. Fixed release blockers — re-verified live, then fixed

The 2026-08-13 audit (`docs/CURRENT_READINESS_AUDIT.md` §3) listed four P0
findings. Before touching anything, each was independently re-verified
against current code (not assumed fixed, not assumed still-broken). All
four were **still live**, unaffected by the intervening standard-library/
supplement/conformance work, because none of that work touched the affected
files. All four are now fixed, each with a regression test that would have
caught the original bug.

### P0-A — Failure-SLA anomaly always fired

**Confirmed live:** `BranchMonitor::record_failure` (`etdl-core/src/
monitor.rs`) pushed every call into a `"{operation_id}.failure"`-only SLA
window with `occurred = true`; nothing ever recorded `occurred = false` for
that same key. `observed_frequency` for that key was therefore permanently
`1.0`, so any operation with a declared failure probability below roughly
`1.0 - threshold` would, after a handful of failures over its lifetime,
trigger a permanent false anomaly — regardless of its actual overall
failure rate.

**Fix:** added `BranchMonitor::record_success` (`etdl-core/src/
monitor.rs`), which records `occurred = false` on the same key. Wired into
generated code's `Ok(_result) =>` arm (`etdl-compiler/src/codegen/
rust.rs`), mirroring exactly the existing `Err(err) =>` arm's
`on_failure_probability_source` handling.

**Regression test:** `etdl-core/src/monitor.rs::tests::
record_success_keeps_observed_frequency_meaningful_not_permanently_one` —
19 successes + 1 failure at a 5% declared rate now observes `~0.05`, not
`1.0`.

### P0-B — V-204 (ECEL type-checking) dead for every `message.payload.*` path

**Confirmed live:** `resolve_schema_path` (`etdl-parser/src/asyncapi.rs`)
stripped a leading `message` path segment as a root marker but never
stripped the following `payload` segment — even though its only caller,
`get_schema_for_path`, had *already* unwrapped the schema down to the
payload schema before calling it. Every `message.payload.<field>` path
(the standard ECEL root) therefore tried to resolve a literal field named
`payload` inside the payload schema, which essentially never exists,
always returned `None`, and the caller (`typeck.rs`) treated that as
`Unknown` — silently skipping V-204 type-checking for every path operand,
for as long as this code has existed.

**Fix:** `resolve_schema_path` now also strips a `payload` segment
following `message`, for the same reason `message` itself is stripped.
`message.headers.*` is a separate, already-documented gap
(`SPEC_IMPLEMENTATION_MATRIX.md` §6.3) and is out of scope here.

**Regression test:** new conformance case `invalid-payload-type-mismatch`
in `conformance/conformance.rs` — `message.payload.ok > 0` (boolean
compared with an ordering operator) now correctly produces `V-204`.

### P0-C — `failureRate`/`missionTime`/`probability` had no sign/range/NaN validation

**Confirmed live:** `fault_tree.rs`'s `1.0 - (-failure_rate *
mission_time).exp()` had no upstream validation; a negative `failureRate`
produced a negative "probability" that flowed unclamped through AND/OR/XOR
gate math into emitted constants. `check_basic_event_rules`
(`validate.rs`) checked presence/exclusivity of `probability`/`failureRate`
but never their numeric validity.

**Fix:** new diagnostic `V-507` (`docs/DIAGNOSTICS.md`) — rejects
non-finite (NaN/infinity) values for any of the three fields, negative
`failureRate`/`missionTime`, and out-of-`[0,1]` `probability`.

**Regression tests:** two new conformance cases,
`invalid-negative-failure-rate` and `invalid-out-of-range-probability`.

### P0-D — Generated code for ECEL `in`/`matches` did not compile

**Confirmed live, reproduced directly:** compiling a minimal fixture with
`x in ["A","B"]` and `x matches "pattern"` conditions and inspecting the
generated Rust showed exactly the reported shape:
`etdl_core::condition::contains(&vec!["A","B"], &message.payload.status)`
— a `&[&str]` vs. `&String` type mismatch, since `contains<T: PartialEq>`
required both sides to share one type — and
`etdl_core::condition::matches(message.payload.status, "^A.*$")` — an
owned `String` passed where `&str` was required, with no `&` at all.
Neither compiled. The existing compile-check harness (`etdl-compiler/
tests/codegen_test.rs`) only ever exercised `qty > 0`, so this was never
caught.

**Fix:** `contains` now takes two independent type parameters
(`T: PartialEq<U>` instead of `T: PartialEq<T>`), which compiles for
`&str` vs. `String` via `std`'s own `impl PartialEq<String> for &str` (the
same impl that makes `"foo" == some_string` ergonomic) — same-type calls
are unaffected (`T: PartialEq<T>` is the trivial case). The `matches`
codegen call site now borrows its path operand
(`&message.payload.status`), letting `&String -> &str` deref coercion
apply.

**Regression test:** new fixture `etdl-cli/tests/fixtures/
in-matches-check.etdl` (reuses the existing `orders_api` AsyncAPI schema
and gencheck message stubs — no new stub types needed), compiled and
`cargo check`-verified end-to-end by `codegen_test.rs`'s
`generated_code_compiles` test, alongside the original fixture.

### Net effect

All four fixes are additive/corrective at the smallest scope that resolves
the actual defect — no redesign of the monitor, type-checker, validator, or
codegen architecture. Full workspace regression (`cargo test --workspace`,
all crates, all existing + new tests) passes with zero failures after all
four fixes combined. `cargo clippy --workspace --all-targets` with the
project's exact CI flags (`-D warnings`) is clean.

## 4. Architectural freeze classification (Part B)

Following `docs/API_STABILITY.md`'s existing STABLE/EXPERIMENTAL/INTERNAL/
DEPRECATED framework (unchanged, corrected for the version reference only):

- **STABLE**: everything in `API_STABILITY.md`'s tables, plus (newly, by
  the same criteria — a documented, tested, intentionally public surface):
  `etdl-probability-core`'s public API, `etdl-tree-core`'s public API,
  `etdl-reliability::predictive`'s public API, `etdl-conformance`'s
  `vector`/`reference`/`manifest`/`report`/`depgraph` modules.
- **EXPERIMENTAL** (unchanged): `etdl-parser::semantic` (LSP endpoints),
  `etdl-parser::spanned` (IDE support).
- **INTERNAL** (unchanged, plus): `etdl-compiler::typeck` (private module);
  `etdl-conformance::reference` is public API of a dev/test-tooling crate,
  not of the language — it is not meant to be depended on by ETDL programs.
- **DEPRECATED**: `eventTree` (singular; use `eventTrees`),
  `probabilityOfSuccess`/`probabilityOfFailure` (use `probability`) — both
  unchanged from before this pass, both still accepted per
  `docs/CONFORMANCE.md`'s compatibility rules.
- **FUTURE**: nothing in this pass was implemented as "future-flagged" —
  see §10 for what is explicitly deferred instead.

No experimental API was found accidentally treated as normative, and no
internal implementation detail was found accidentally exposed as a stable
API surface, during this audit.

## 5. Specification audit (Parts C/D) — what belongs in `etdl-specification`

`etdl-specification` was not available as a local checkout in this
environment (only a standalone downloaded snapshot of
`ETDL-Specification.md` was found, not a git clone) — this section could
not directly edit that repository. It reports, as requested, concrete gaps
between what the specification currently defines (§1-13: Introduction,
Conformance & Notation, Terminology, File Format, Document Structure,
ECEL, Semantic Validation Rules, Compiler & Codegen Semantics, Runtime
Library Contract, Versioning & Compatibility, Extensibility, Security,
Worked Example) and what this implementation now has, for a human to act
on directly in that repository:

1. **Register `V-507`** in the diagnostic registry (§7 / Appendix B) —
   the specification's own validation-rules section should state the
   numeric-validity requirement this diagnostic enforces (finite,
   non-negative rate/duration, `probability` in `[0,1]`), not just leave
   it to implementations to discover independently.
2. **Specify `message.payload`/`message.headers` path resolution
   precisely** (§6.3) — the specification is currently silent on exactly
   how a `message.payload.<field>` ECEL path resolves against an AsyncAPI
   message schema. That silence is exactly what let this implementation's
   root-stripping bug (P0-B) go undetected across at least two prior
   audits: nothing in the spec pins down the expected behavior precisely
   enough to test against independently. `message.headers.*` schema
   introspection is also still unspecified/unimplemented — the spec
   should either normatively require it or explicitly mark it optional.
3. **No supplement is currently defined in the specification at all.**
   Every one of Standard Library, Generic Tree Event Supplement,
   Reliability Supplement, Predictive Reliability Supplement, and Runtime
   Feedback & Calibration exists **only** as this implementation
   repository's own `docs/reference/*.md` files — none has a normative
   home in `etdl-specification`. Concretely needed:
   - A **§11 Extensibility** amendment (or new top-level section) formally
     defining what a "supplement" is: the `supplements:` declaration
     mechanism, `x-*` extension field convention, the built-in-extension
     registry concept, and namespace protection rules — currently 100%
     implementation convention, 0% normative text.
   - A **Generic Tree Event Supplement appendix**: schema (`Tree`/
     `TreeNode`/`GateKind`), the "tree, not DAG" decision (shared nodes
     rejected), validation rules, traversal order guarantees.
   - A **Reliability Supplement appendix**: `ProbabilityEstimate` shape,
     `ReliabilityArtifact` schema/versioning, the estimation methods
     (empirical/Wilson, Beta-Binomial Bayesian, exponential), uncertainty/
     importance/sensitivity semantics.
   - A **Predictive Reliability Supplement appendix**: `S(t)`/`F(t)`/
     `h(t)`/`H(t)` definitions, the exponential/Weibull model families,
     the prediction-vs-estimate-vs-observation distinction, extrapolation
     semantics.
   - A **Runtime Feedback & Calibration Supplement appendix**: the
     observe → analyze → review → publish-new-artifact → rebuild
     discipline as a *normative* (not just implementation-convention)
     requirement — this is a strong, deliberate design invariant this
     implementation enforces structurally, and multiple independent
     implementations should be required to preserve it, not merely
     encouraged to.
   - Canonical **ontology concept definitions** (Event, Failure,
     Probability, Observation, Evidence, Uncertainty, Reliability,
     Prediction, Tree Event, Artifact, Calibration, External Value) —
     currently defined only in Rust types (`etdl-reliability-core`,
     `etdl-reliability-ontology`, `etdl-tree-core`), not in specification
     prose. A second implementation has no normative text to agree with.
4. **Appendix E JSON Schema** — already flagged as a pre-existing spec gap
   (`SPEC_IMPLEMENTATION_MATRIX.md` line 10: "pending"); still pending.
   This blocks a literal reading of the spec's own §2.3 "Conforming
   Document" definition, which requires validating against it.
5. **§5.8.1 `probabilitySource` authoritative-when-present rule** — per
   `SPEC_IMPLEMENTATION_MATRIX.md`, still not implemented; the spec asserts
   a precedence rule the compiler does not enforce. Either implement it or
   mark it explicitly non-normative until it is.
6. **§6.4 `any()`/`all()` quantifiers** — spec marks them MAY; no grammar
   production exists. Either add the grammar or downgrade the spec text so
   a conforming parser genuinely can omit them without appearing to
   violate §6.2.
7. **Conformance levels 2-7** (standard library, supplements, artifacts,
   runtime, WASM, full) have no normative counterpart in the
   specification's own §2.3 "conformance targets" — only "Conforming
   Document/Parser/Compiler/Runtime" are defined there. If multi-level
   conformance claims are meant to be portable across implementations
   (not just this one's own `etdl-conformance` crate), §2.3 needs
   equivalent target definitions for the supplement/artifact/WASM layers.

None of these were resolved by changing the implementation to match a
silent spec assumption — per this task's own instruction, they are
reported for a human to resolve in the specification repository directly.

## 6. Ontology audit (Part E)

Read every type in `etdl-reliability-ontology/src/*.rs` and
`etdl-reliability-core/src/*.rs`. Classification:

| Concept | Type | Classification | Notes |
|---|---|---|---|
| Event/Failure taxonomy entry | `OntologyEntry`, `EntryKind` (`etdl-reliability-ontology`) | UNCHANGED | Pre-existing |
| Failure lifecycle status | `FailureStatus` (`etdl-reliability-ontology`) | UNCHANGED | Pre-existing |
| Probability | `ProbabilityEstimate`, `ProbabilityMetric`, `ProbabilityState` (`etdl-reliability-core`) | UNCHANGED | Pre-existing |
| Probability (domain-neutral) | `Probability`, `Rate` (`etdl-probability-core`) | NEW (this session) | Deliberately distinct from `ProbabilityEstimate` — no duplicate concept, a different layer (raw math vs. engineering estimate with provenance) |
| Observation | `ReliabilityObservation`, `AggregateObservation` | UNCHANGED | Pre-existing |
| Evidence | `Evidence` (reliability), `Evidence` (failure-discovery, `candidate.rs`) | UNCHANGED (two, deliberately distinct) | Reliability's `Evidence` supports an estimate; discovery's `Evidence` supports a *candidate* — different life-cycle stage, not a duplicate |
| Uncertainty | `Uncertainty`, `ConfidenceInterval` | UNCHANGED | Pre-existing |
| Reliability | `ReliabilityArtifact`, `ArtifactResolver` | UNCHANGED | Pre-existing |
| Prediction | `PredictiveResult`, `PredictiveQuantity`, `ModelDescriptor`, `PredictiveProvenance` (`etdl-reliability::predictive`) | NEW (prior session) | Deliberately distinct from `ProbabilityEstimate` (a prediction always carries a time horizon; an estimate never does) |
| Tree Event | `Tree`, `TreeNode`, `GateKind` (`etdl-tree-core`) | NEW (prior session) | Domain-neutral; not an ontology concept in the reliability sense, a structural one |
| Artifact | `ReliabilityArtifact` (reliability), `candidate_only_artifact` output (discovery) | UNCHANGED (two, deliberately distinct) | Discovery's output explicitly self-identifies as `"kind": "discovery-output"` specifically so it is never mistaken for a `.rprob` reliability artifact — see `bridge.rs:91-112` |
| Calibration | `CalibrationResult`, `CalibrationStatus` (`etdl-reliability::calibration`) | UNCHANGED | Pre-existing |
| External Value | `SuppliedEstimate` (`etdl-failure-discovery::bridge`) | UNCHANGED | Pre-existing |
| Candidate (failure discovery) | `CandidateStatus`, `FailureClassification`, `DiscoveryCandidate` | UNCHANGED | Pre-existing; see §7 |

**No accidental duplicate concepts found.** Every place two similarly-named
types exist (`Evidence` × 2, `Artifact`-shaped output × 2), the duplication
is deliberate and load-bearing — each pair represents a different stage or
domain that must not be silently merged (e.g., merging discovery's
candidate `Evidence` into reliability's estimate `Evidence` would let an
unreviewed source-code pattern masquerade as engineering evidence for a
probability estimate). This matches the ontology's own stated discipline.

## 7. Service-code analysis validation (Part R)

`etdl-failure-discovery::candidate::CandidateStatus` is
`{Candidate, Accepted, Rejected, Ignored, Mapped}`; its own doc comment
states "Discovery only ever produces `Candidate`; engineering review moves
it forward." Verified directly: the only place `CandidateStatus::Accepted`
is constructed anywhere in the crate's non-test code is nowhere —
`accepted_candidates_to_artifact` (`bridge.rs`) only ever *filters* on
`status == Accepted` and requires an externally-supplied `SuppliedEstimate`
per candidate (never derives a probability from discovery confidence); the
only place a candidate is *constructed* with `Accepted` status is a test
fixture (`bridge.rs::tests::sample_candidate`) simulating an
already-human-reviewed candidate to test the filter. No code path lets an
observed exception become an authoritative engineering failure without an
explicit, external step. This requirement is met.

## 8. Conformance system (Parts F/G/H/I/J)

Completed in this pass — see `docs/reference/conformance-framework.md`
(the full guide) and `docs/conformance/supplement-traceability-matrix.md`
(requirement -> implementation -> test -> status per supplement). Summary:

- **Levels**: 0 (Syntax) / 1 (Semantic) — pre-existing, unchanged
  (`conformance/conformance.rs`, now 15 cases after 3 added this pass for
  the P0-B/P0-C regressions). 2 (Standard Library) / 3 (Supplement) / 4
  (Artifact) / 5 (Runtime) / 6 (WASM) / 7 (Full) — new, `etdl-conformance`
  crate, 59 vectors across 8 test files, all passing (`cargo test -p
  etdl-conformance` and `cargo test -p etdl-conformance
  --no-default-features` both clean).
- **Independent oracle**: `etdl-conformance::reference` — coded directly
  from mathematical definitions, never calling into the implementation
  crates' own formulas. Used to cross-check `Binomial::cdf`, the
  exponential/Weibull predictive models, and the two-sided binomial
  calibration test.
- **Dependency-graph checker**: `etdl-conformance::depgraph` (7 `ARCH-*`
  vectors) — found and fixed one real gap in its own crate's
  `Cargo.toml` (§3 note in the conformance guide) and one real,
  previously-undocumented architectural fact about `etdl-wasm`'s
  dependency graph (documented, not silently asserted away — see the
  conformance guide's "No self-certification loop" section for the
  resolution reasoning).
- **CLI**: `etdl conformance status` / `etdl conformance manifest`, both
  verified working in this pass with and without `--no-default-features`.

## 9. Examples (Parts AI/AJ)

Existing per-topic examples were spot-checked as present:
`examples/{business,probability,reliability,reliability-analysis,
reliability-external,reliability-runtime-feedback,standard-library,
tree-event}` plus Rust example binaries
(`etdl-reliability/examples/{tree_to_artifact,predictive_reliability}.rs`,
`etdl-probability-core/examples/{composition,distributions}.rs`). The
`predictive_reliability.rs` example alone already demonstrates most of the
canonical chain (mission reliability -> Weibull aging ->
predict/observe/review/republish/new-prediction).

**Gap, honestly reported, not fixed in this pass:** no single example
chains literally through *every* stage of Engineering Model -> ETDL ->
Reliability -> Predictive Reliability -> Artifact -> Runtime Observation ->
Calibration -> New Artifact -> Future Prediction in one file — the stages
exist across several examples, not one narrative. Recommended future work,
not a release blocker (every individual stage is independently
demonstrated and tested); see §10.

## 10. Deferred / future work (Part BD) — explicitly NOT done in this pass

Per the release-freeze instruction (no new major features, models,
supplements, or redesigns):

- The single canonical end-to-end example (§9).
- `std.reliability` ETDL-source facade (carried forward from the
  Predictive Reliability task's own final report — still not built;
  `etdl-reliability` plays that role today).
- `cargo-fuzz` targets for the parser/module loader/artifact decoder/tree
  validator (conformance guide's own documented gap).
- A dedicated security-testing corpus beyond `ART-004`'s illustrative
  malformed-artifact cases.
- A dedicated Level-5 (Runtime) conformance harness beyond the
  calibration-specific `CAL-003`/`CAL-004` vectors.
- Every item already listed as deferred in the Predictive Reliability
  Supplement's own docs (repairable systems, availability, renewal
  processes, advanced degradation/physics-of-failure, advanced Bayesian
  models, additional tree-based domains) — none required by the current
  specification.
- Bumping crate versions to `1.0.0` — a release-engineering decision
  reported as a remaining human action (§12), not performed automatically.
- `git push` / `cargo publish` — explicitly not performed; see §12.

## 11. Versioning (Part AM)

- **ETDL language version**: `1.0.0` — already stable, unaffected by this
  pass.
- **Conformance suite version**: `1.0.0`.
- **Crate/SemVer version**: `0.2.2` across all twelve workspace crates,
  unchanged by this pass. `docs/API_STABILITY.md` and
  `docs/reference/crates.md` corrected to state this accurately (they
  previously cited `0.1.x`/`0.1.1`, stale by two minor versions).
  Whether to bump to `1.0.0` now is a release-engineering decision for a
  human to make explicitly (see §12) — not something this pass changed
  unilaterally, since a `1.0.0` crate version is itself a public
  commitment (`API_STABILITY.md`'s own framework: "the 1.0.0 release will
  freeze the public API").
- **Migration**: no migration mechanism exists in the implementation, and
  none is required — this is a new release built additively on the
  existing 0.x line, not a breaking cutover. Artifact schema mismatches
  are a hard, explicit rejection (`SchemaVersionMismatch`), never a silent
  reinterpretation — verified by `ART-003`/`ART-004`.

## 12. Release scorecard (Part AV)

| Area | Status | Blocker | Evidence |
|---|---|---|---|
| Core | PASS | No | Full regression clean; 4 P0s fixed + regression-tested (§3) |
| Specification | PARTIAL | No (implementation gate; spec authorship is out of this repo) | §5 — supplements have no normative home yet |
| Compiler | PASS | No | §3 (P0-B, P0-D fixed), clippy clean |
| Runtime | PASS | No | §3 (P0-A fixed) |
| Standard Library | PASS | No | Existing + `LIB-PROB-*` |
| Built-in Libraries | PASS | No | `std.events`/`std.logic`/`std.probability` always available, verified |
| Optional Libraries | PASS | No | `ARCH-004`/`ARCH-007` verify genuine optionality |
| Generic Tree | PASS | No | §3 (stack-safety fixed), `TREE-*` |
| Reliability | PASS | No | `REL-*`, `ART-*` |
| Predictive Reliability | PASS | No | `PRED-*` vs. independent oracle |
| Runtime Feedback | PASS | No | `CAL-*` vs. independent oracle, non-mutation verified |
| Artifacts | PASS | No | `ART-*` |
| Provenance | PASS | No | `PredictiveProvenance`/`CalibrationProvenance` verified populated in existing tests |
| External Values | PASS | No | `SuppliedEstimate` requires explicit external supply, never derived |
| CLI | PASS | No | `--version`/`--help`/all subcommands smoke-tested |
| WASM | PASS (for what it claims) | No | `cargo check --target wasm32-unknown-unknown` clean; heavy-reliability exclusion verified (`ARCH-005`) |
| Security | PARTIAL | No (no unresolved critical issue found) | No secrets/artifacts tracked; malformed-input rejection tested (`ART-004`); fuzzing deferred (§10) |
| Performance | NOT RE-AUDITED THIS PASS | No | Out of this pass's scope per task's own "do not make benchmarks part of semantic conformance" instruction; existing `docs/PERFORMANCE.md` baselines unchanged |
| Documentation | PASS | No | Stale version refs fixed (§0); `CURRENT_READINESS_AUDIT.md`/`READINESS_SCORECARD.md` corrected with pointers |
| Examples | PARTIAL | No | §9 — per-topic examples all present; single canonical chain missing |
| Conformance | PASS | No | §8 |
| Packaging | PARTIAL | No | Crate versions still 0.2.2 (§11); no `cargo publish` performed |
| Compatibility | PASS | No | No migration needed; artifact mismatch is a hard, explicit rejection |

## 13. Release gate (Part AU)

| # | Condition | Met? |
|---|---|---|
| 1 | All release-blocking conformance tests pass | YES |
| 2 | Specification is internally consistent | PARTIAL — core spec is internally consistent; supplements have no normative text at all yet (§5), which is a specification *completeness* gap, not an internal *contradiction* |
| 3 | Compiler conforms | YES |
| 4 | Standard library conforms | YES |
| 5 | Supplements conform | YES (against this implementation's own reference docs, which are the only authority they currently have) |
| 6 | Artifacts conform | YES |
| 7 | Built-in libraries work | YES |
| 8 | Optional libraries work | YES |
| 9 | WASM profile passes | YES |
| 10 | Security audit has no unresolved critical issue | YES (with fuzzing explicitly deferred, not hidden) |
| 11 | Documentation is usable | YES |
| 12 | Installation works from a clean environment | YES (`cargo build`/`cargo test` from a clean checkout, verified this pass) |
| 13 | Examples work | PARTIAL (§9) |
| 14 | Versioning is consistent | YES (both axes — language 1.0.0, crates 0.2.2 — are internally consistent; not yet bumped to a 1.0.0 crate release, a deliberate human decision, §12) |
| 15 | Provenance works | YES |
| 16 | Compatibility behavior is documented | YES |
| 17 | Release artifacts can be reproduced or explained | PARTIAL — no `Cargo.lock` is committed (gitignored); recommended before a binary release for bit-reproducible builds, not currently a blocker since no release artifact has been cut yet |
| 18 | No known critical semantic defect remains | YES — the four known P0s are fixed; none of this pass's own audit work surfaced a new one |

## 14. Final decision (Part BC)

**ETDL 1.0 RELEASE READY**, for the implementation-conformance and
correctness scope this task covers, with the following **exact remaining
human actions** (not further engineering work — decisions and
one-directional operations this assistant will not perform unilaterally):

1. **Decide on and perform the crate version bump.** All twelve crates are
   currently `0.2.2`. Bumping to `1.0.0` is a public API-freeze commitment
   (`docs/API_STABILITY.md`) — a human should make this call explicitly,
   not have it happen as a side effect of a bug-fix pass.
2. **Commit `Cargo.lock`** (or explicitly decide not to) before cutting a
   release artifact, for reproducible builds (§13 item 17).
3. **`git push`** the branch containing this pass's work. Not done in this
   pass — flagged mid-task and intentionally held for explicit
   confirmation, since the work included real correctness fixes that
   deserved to be tested to completion first (now done).
4. **`cargo publish`** the twelve crates, in dependency order (§0's
   publish order), to crates.io. **Not done in this pass** — this is
   irreversible (crates.io has no true unpublish, only "yank," which does
   not remove the version) and requires publishing credentials this
   environment's access to was never confirmed. This should be a
   deliberate, separately-confirmed action once the version-bump decision
   (item 1) is made.
5. **Act on the `etdl-specification` gap list (§5)** in that repository
   directly — this environment did not have a writable local checkout of
   it.
6. **Optional, recommended, non-blocking:** build the single canonical
   end-to-end example (§9); add `cargo-fuzz` targets (§10).

## 15. Full test result summary (Part BB)

- `cargo test --workspace --jobs 2`: **all tests pass**, zero failures,
  across every crate (unit + integration + doc tests), after all four P0
  fixes combined.
- `cargo clippy --workspace --all-targets --jobs 2 -- -D warnings` (project's
  exact CI flags): **zero warnings**.
- `cargo check -p etdl-wasm --target wasm32-unknown-unknown --jobs 2`:
  clean.
- `cargo build -p etdl-cli --no-default-features --jobs 2` and default:
  both clean; `etdl conformance status`/`manifest` verified correct in
  both configurations.
- `cargo test -p etdl-conformance` and `--no-default-features`: both
  clean, 59 conformance vectors + unit tests, all passing.
- `bash scripts/feature-matrix.sh --check`: all 9 documented feature
  combinations pass (2 added this pass: `etdl-conformance` lean,
  `etdl-cli` fully lean).
