# Changelog

All notable changes to the ETDL reference implementation are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/); versioning follows
`docs/VERSIONING.md` (language `etdl` 1.x) and Cargo semver (crates 0.x).

## [Unreleased]

### Fixed — Unbounded hang in `build_span_index` on truncated directives

`etdl_parser::spanned::build_span_index` (used by `etdl-cli` and `etdl-wasm`
to power span-aware tooling — hover, goto-definition, duplicate-id
diagnostics, etc.) could hang forever and grow memory without bound on
certain malformed input, most simply a document whose entire content is a
single `%` character.

Root cause is in the `saphyr-parser` dependency (present in the latest
0.0.12 release too, not just the 0.0.11 pinned here): `is_yaml_non_break`
excludes `is_break` (`\n`/`\r`) but not `is_breakz`, so it fails to
recognize `'\0'` — the end-of-stream sentinel `BufferedInput` (the
char-iterator-backed `Input` impl used by `saphyr::MarkedYaml::load_from_str`)
returns once its source iterator is exhausted. A token scan that reaches
true EOF while still mid-token (e.g. a directive-name scan for a `%` with
nothing after it) never recognizes end-of-input and loops forever appending
`'\0'` to an unbounded buffer.

`build_span_index` now drives saphyr via `saphyr_parser::Parser::new_from_str`
(the `&str`-backed `StrInput`, which bounds-checks against the real buffer
length and isn't affected) instead of `MarkedYaml::load_from_str` — the same
pattern `detect_duplicate_ids` already used. `parse_document` (the main
compile path) was never affected. New regression test:
`etdl-parser/tests/robustness.rs::span_builder_bare_directive_no_hang`.

### Added — Third-party supplement registration

`Compiler` now accepts additional, non-built-in `EtdlExtension` implementations
via a new `Compiler::with_extension(Box<dyn EtdlExtension>)` builder method —
closing a gap where `EtdlExtension`/`ExtensionRegistry`/`builtin_registry()`
were already public API in shape, but nothing let a caller actually wire an
extension instance into `compile()`/`validate()`: `run_extensions` was
hard-coded to the two built-in extensions only, and the earlier
"is this supplement implemented" check (`validate::validate_supplements`,
governing the E-108/W-407 diagnostics) had no way to know about a
caller-registered extension either, so a document declaring one would have
been incorrectly reported as unimplemented even after registration. Both are
now fixed:

- `ExtensionResult` gained a `basic_event_overrides()` default method (empty
  by default) so a generically-registered extension can contribute fault-tree
  probability overrides the same way the built-in reliability extension's
  own hard-coded path already does, without `run_extensions` needing to know
  the extension's concrete result type.
- `validate::validate_document_with_extensions` (a new function;
  `validate_document` is unchanged and delegates to it with an empty list)
  threads registered-extension ids through to `validate_supplements`, so
  E-108/W-407 correctly recognize a caller-registered extension as
  implemented.
- New test `etdl-compiler/tests/third_party_extension_test.rs` proves the
  full path end-to-end: a non-built-in extension's `validate()` diagnostic
  appears in `compile()`'s output, and its `process()`-resolved probability
  override is embedded in the generated Rust, replacing the document's own
  declared value.

This is the entry point a non-core supplement (specification Section
11.4/11.5) — for example, a third-party `etdl.chain` (Blockchain,
Attestation and Provenance) implementation maintained in its own repository —
registers itself through.

### Fixed — ETDL 1.0 release-readiness pass

Four correctness defects, first flagged in `docs/CURRENT_READINESS_AUDIT.md`
(2026-08-13) as P0 blockers, were independently re-verified against current
code (none had been touched by the intervening standard-library/supplement/
conformance work) and found still live. All four are fixed, each with a
regression test that reproduces the original failure mode. See
`docs/RELEASE_READINESS_1.0.md` §3 for full detail.

- **SLA false-alarm bug**: `BranchMonitor::record_failure` recorded every
  call into a `"{op}.failure"`-only rolling window, so its observed
  frequency was permanently `1.0` regardless of an operation's actual
  overall failure rate — any operation with a declared failure probability
  below the alarm threshold would eventually trigger a permanent false
  anomaly. Added `BranchMonitor::record_success`, wired into generated
  code's `Ok` arm alongside the existing `Err` arm's `record_failure` call
  (`etdl-core/src/monitor.rs`, `etdl-compiler/src/codegen/rust.rs`).
- **V-204 (ECEL type-checking) was dead for every `message.payload.*`
  path**: `resolve_schema_path` stripped a leading `message` root segment
  but never the following `payload` segment, even though the schema was
  already unwrapped to the payload schema one level up — every payload
  path therefore resolved to `Unknown` and silently skipped type-checking
  (`etdl-parser/src/asyncapi.rs`).
- **New diagnostic `V-507`**: basic-event `probability`/`failureRate`/
  `missionTime` numeric validity (finite, non-negative rate/duration,
  `probability` in `[0,1]`) was previously unvalidated — a negative
  `failureRate` flowed unclamped into emitted fault-tree probability
  constants (`etdl-compiler/src/validate.rs`; registered in
  `docs/DIAGNOSTICS.md`).
- **Generated code for ECEL `in`/`matches` did not compile**:
  `etdl_core::condition::contains` required both operands to share one
  Rust type, but generated code always compared a `&[&str]` literal
  against an owned `String` field; `matches`'s codegen call site passed
  its path operand unborrowed. `contains` now takes two independent type
  parameters (`T: PartialEq<U>`), and the `matches` call site borrows its
  path operand (`etdl-core/src/condition.rs`,
  `etdl-compiler/src/codegen/rust.rs`). New dedicated compile-check
  fixture `etdl-cli/tests/fixtures/in-matches-check.etdl`, checked
  alongside the existing fixture by `codegen_test.rs`.
- Stale crate-version references corrected in `docs/API_STABILITY.md` and
  `docs/reference/crates.md` (cited `0.1.x`; workspace is `0.2.2` and now
  twelve crates, not four).
- `docs/CURRENT_READINESS_AUDIT.md` and `docs/READINESS_SCORECARD.md`
  annotated with pointers to `docs/RELEASE_READINESS_1.0.md`, the current
  authoritative status — their own P0/P1/P2 findings are kept as historical
  record, not rewritten.

### Added — ETDL Conformance, Verification & Validation 1.0

- **New crate `etdl-conformance`** — a cross-layer conformance framework
  answering "how do we know an ETDL implementation actually implements the
  specification and supplements correctly?" Extends, does not replace, the
  pre-existing `conformance/conformance.rs` (Level 0/1: 12 parser/compiler/
  fault-tree cases, unchanged) and `docs/CONFORMANCE.md`.
- **Conformance levels 0-7** matching this workspace's own layering
  (`etdl_conformance::vector::Level`): syntax, semantic, standard library,
  supplement, artifact, runtime, WASM, full. See
  `docs/reference/conformance-framework.md`.
- **`ConformanceVector`** schema (`src/vector.rs`): stable id (e.g.
  `PRED-001`), level, spec/doc reference, one-sentence requirement,
  suite version, status (Active/Experimental/Deprecated) — identity and
  traceability metadata attached to each `#[test]`, never a duplicated
  input/output payload.
- **An independent mathematical reference oracle** (`src/reference.rs`):
  exponential and Weibull survival/hazard/cumulative-hazard, binomial
  PMF/CDF via direct summation, the two-sided exact binomial test via the
  same "doubling" definition the implementation documents but via a
  different algorithm (no regularized incomplete beta function), and basic
  probability composition — coded directly from mathematical definitions,
  never calling into `etdl-probability-core`/`etdl-reliability-core`/
  `etdl-reliability`'s own formulas. This is what makes the numerical
  conformance vectors a real check rather than a self-certification loop
  (task §73/§74).
- **59 conformance vectors** across 8 test files: `ARCH-*` (7, dependency
  graph/architecture — see below), `LIB-PROB-*` (9, `std.probability`
  invariants and composition), `TREE-*` (10, Generic Tree Event structural
  invariants + a stack-safety vector), `REL-*` (5, reliability estimate
  invariants), `PRED-*` (8, Predictive Reliability vs. the independent
  oracle), `CAL-*` (4, calibration vs. the independent oracle + a
  deterministic calibration vector + non-mutation), `ART-*` (7, artifact
  round-trip/schema/identity/malformed-input rejection).
- **Dependency-graph / architecture checker** (`src/depgraph.rs`): shells
  out to `cargo metadata --format-version=1 --no-deps` (no new
  dependency), parses only normal (non-dev, non-build) edges, and checks
  the invariants this workspace's docs have claimed in prose since the
  Standard Probability Library and Generic Tree Event Supplement tasks:
  zero dependency from `etdl-probability-core`/`etdl-tree-core` onto any
  reliability crate, the one-directional `etdl-reliability -> {probability,
  tree}` edge, optional-not-just-conventional reliability-family
  dependencies in `etdl-compiler`/`etdl-cli`, and an acyclic workspace
  graph.
  - **Found and fixed a real bug this way**: `etdl-tree-core`'s
    reachability/cycle-detection walk, `descendants`, and `postorder` were
    recursive (one Rust function call per tree node); a 5,000-deep-but-
    valid tree crashed the process with a stack overflow (`TREE-010`,
    caught on first run). Rewrote all three as iterative, explicit-stack
    implementations producing byte-identical output — verified against
    all 27 pre-existing `etdl-tree-core` tests, which pass unchanged.
  - **Found and documented a real architectural fact**: `etdl-wasm`
    transitively depends on `etdl-reliability-core` (via `etdl-compiler`'s
    default-on `reliability` feature) — not previously stated anywhere.
    `etdl-reliability-core` is a pure serde-typed crate, confirmed
    WASM-safe by the existing `wasm` CI job, so this is not a defect; it
    means the WASM validator surfaces E-110/111/112 diagnostics for
    documents declaring `x-reliability`. Documented in
    `docs/reference/crates.md`; the conformance vector was narrowed to the
    invariant that actually matters (no dependency on the *heavy*
    `etdl-reliability` engine/ontology/discovery crates).
  - **Fixed `etdl-conformance`'s own `Cargo.toml`** to pin
    `default-features = false` on its `etdl-compiler` dependency (with
    `reliability` feature-unified through its own `reliability` feature) —
    without this, the crate would have leaked the reliability engine into
    any lean build depending on it, the exact class of bug `ARCH-004`/
    `ARCH-007` exist to catch.
- **`ConformanceManifest`** (`src/manifest.rs`) and **`report::
  area_statuses`** (`src/report.rs`): machine-readable manifest (ETDL
  language version, implementation version, suite version, supported
  supplements/libraries/targets/artifact schemas) and objective per-area
  PASS/PARTIAL/UNSUPPORTED/FAILED status (task §49: "no marketing claims,
  only objective states"), computed from compile-time feature flags —
  never probing anything at runtime.
- **CLI**: `etdl conformance status` and `etdl conformance manifest`
  (`--json` supported on both) — no new command ecosystem beyond these
  two. `etdl-cli` now always depends on `etdl-conformance`
  (`default-features = false`, feature-unified through `etdl-cli`'s own
  `reliability` feature), verified to build and behave correctly both with
  and without `--no-default-features`.
- **CI**: new `conformance` job in `.github/workflows/ci.yml` (full +
  lean `cargo test -p etdl-conformance`); `scripts/feature-matrix.sh`
  gained two entries (`H`: `etdl-conformance` lean, `I`: `etdl-cli` fully
  lean) — all 9 combinations verified passing.
- New docs: `docs/reference/conformance-framework.md` ("ETDL Conformance
  Guide 1.0" — levels, vector schema, no-self-certification-loop
  methodology, numerical tolerance policy, WASM conformance profile,
  optional-library conformance, CI, release gates, known gaps),
  `docs/conformance/supplement-traceability-matrix.md` (the companion to
  `docs/SPEC_IMPLEMENTATION_MATRIX.md`, which only covers the core spec,
  for every supplement). `docs/CONFORMANCE.md` and
  `docs/SPEC_IMPLEMENTATION_MATRIX.md` extended additively (a pointer
  section and a version-header bump respectively), not rewritten.
- **Repository-split decision**: kept in-repo (`etdl-conformance` crate,
  not a separate `etdl-conformance` GitHub repository) for 1.0 — reasoned
  out in the conformance guide's "Where the suite lives" section; the
  Level 0/1 suite already documents how a third-party implementation would
  port it, but Levels 2-7 are written directly against this workspace's
  own Rust APIs with no second implementation yet to serve, so splitting
  now would be motion without value.
- **Known, documented gaps**: no `cargo-fuzz` targets yet (parser, module
  loader, artifact decoder, tree validator — recommended next step);
  security testing limited to a handful of illustrative malformed-artifact
  cases, not a full corpus; no migration testing (no migration mechanism
  exists to test — current behavior is a hard schema-mismatch rejection,
  documented, not silent rewriting); no dedicated Level-5 runtime
  conformance harness beyond the calibration-specific vectors; the
  `std.reliability` ETDL-source facade still does not exist (carried
  forward from the Predictive Reliability task).

### Added — ETDL Predictive Reliability Supplement 1.0

- **New module `etdl-reliability::predictive`** — extends "what is the
  estimated probability?" to "what is the predicted probability/reliability
  over a specified future time/exposure?", cleanly distinct from
  `ProbabilityEstimate` (no time horizon) and `AggregateObservation` (a
  record of what happened). Feature-gated with the rest of
  `etdl-reliability` (`reliability` Cargo feature); a lean
  `--no-default-features` `etdl-cli` build stays healthy.
- **`TimeToFailureModel` trait** (`predictive::models`) with `survival`,
  `hazard`, `cumulative_hazard`, `density`, `failure_probability`, `mean`,
  `quantile`, `descriptor` — every method total over all finite `t >= 0`,
  no panics at `t = 0` or very large `t`.
  - `ExponentialModel` (constant hazard): a thin wrapper reusing
    `etdl_probability_core::distribution::Exponential` directly — no
    reimplementation of its math.
  - `WeibullModel` (shape/scale): a genuinely new implementation (not
    present in the domain-neutral `std.probability`), covering
    infant-mortality (shape < 1), constant-hazard (shape = 1, verified
    equivalent to `ExponentialModel`), and wear-out/aging (shape > 1)
    behavior. `H(t)` is computed directly from the closed form rather than
    via `-ln(S(t))`, staying accurate as `S(t) -> 0`. Its own `Gamma(x)`
    (Lanczos approximation) is a fresh, independent reimplementation —
    `etdl_probability_core::numerics::log_gamma` exists but its containing
    module is private (`mod numerics;`, not `pub mod`) and unreachable
    from outside that crate, matching this workspace's existing
    "reimplement, don't share across the crate boundary" pattern.
- **`MissionTime`, `PredictiveQuantity`, `ModelDescriptor`,
  `PredictiveProvenance`, `PredictiveResult`** (`predictive` root module) —
  `PredictiveQuantity` is a closed enum (`Survival`/`Reliability`/
  `FailureProbability`/`Hazard`/`CumulativeHazard`/`Density`), never a bare
  `f64`. `PredictiveResult::new` computes an `extrapolated` flag from
  `ModelDescriptor.valid_range`, defaulting to `false` (not `true`) when no
  range was declared — absence of a declared range is not evidence either
  way.
- **`CensoredObservation`/`CensoringKind`** (`predictive::censoring`) —
  minimal, purely additive right/left/interval-censoring representation,
  deliberately kept separate from the existing
  `etdl-reliability::observations::AggregateObservation` (still the type
  the binomial calibration pipeline consumes, unmodified). Construction
  only in 1.0; censored-data parameter fitting (MLE, Kaplan-Meier, ...) is
  explicitly out of scope.
- **`exponential_model_from_artifact`** (`predictive::calibration_adapter`)
  — the only supported bridge from an existing, calibrated
  `ReliabilityArtifact`'s `FailureRate` estimate to a predictive model.
  Read-only: does not call `calibrate()` or touch `dataset`/`observation`
  — Predictive Reliability consumes the existing
  observe → analyze → review → publish new artifact → rebuild loop, it
  does not add a second one.
- **`evaluate_failure_probability_at`** (`predictive::tree`) — evaluates a
  Generic Tree Event's root failure probability at a mission time by
  computing each leaf's `F(t)` from its own `TimeToFailureModel` and
  calling `tree_adapter::evaluate_assuming_independence` **unchanged** — no
  new tree-composition or gate logic; the tree supplement and its
  reliability adapter are untouched.
- **CLI**: `etdl capabilities` gains a `predictive_reliability` block
  (`available`, `schema`, `models`, `quantities`, `sampling`,
  `censored_data_fitting`) — no new command ecosystem, per the task's own
  scope. Reported via a small `#[cfg(feature = "reliability")]`-gated
  helper so the lean build never references the optional crate.
- 13 reference/edge-case integration tests
  (`etdl-reliability/tests/predictive_reliability.rs`), including the
  task's own acceptance criterion (`lambda=0.001/hour, t=100h => R(t) =
  exp(-0.1)`), Weibull hazard-direction tests for each shape regime, the
  extrapolation flag in both its declared and undeclared states, and a
  full `predict -> observe -> calibrate -> new artifact -> new prediction`
  loop that asserts (via JSON-snapshot comparison, since
  `ReliabilityArtifact` has no `PartialEq`) that the original artifact and
  prediction are byte-for-byte unchanged throughout.
- New docs: `docs/reference/predictive-reliability-supplement.md`. New
  example: `etdl-reliability/examples/predictive_reliability.rs` (mission
  reliability, Weibull aging, the calibration-loop discipline).
- **Known, documented gap carried forward:** a standalone `std.reliability`
  ETDL-source facade was never built; this module builds directly on the
  existing `etdl-reliability` engine, which plays that role today. Still
  recommended future work.

### Added — ETDL Generic Tree Event Supplement 1.0

- **New crate `etdl-tree-core`** — domain-neutral tree-of-events structural
  model. Zero dependency on any reliability or probability crate (checkable
  directly in its `Cargo.toml`), WASM-safe. `Tree`/`TreeNode`/`GateKind`
  (AND/OR/NOT/XOR/K_OF_N), full structural validation (`Tree::validate`
  collects every problem in one pass: empty tree, missing/unknown root,
  invalid gate arity, missing child references, cycles via DFS with an
  explicit on-path stack, and — the key 1.0 decision — **shared nodes are
  rejected, not silently treated as a DAG**: every non-root node must have
  exactly one parent), and traversal (`children`/`leaves`/`ancestors`/
  `descendants`/`depth`/`preorder`/`postorder`).
- Node identity is the `BTreeMap` key `Tree::nodes` stores it under, with
  no redundant `id` field on the node value — matching this workspace's
  existing convention (`FaultTree::basic_events`) rather than introducing
  a second identity mechanism. (An earlier draft kept both a map key and a
  `TreeNode.id` field; this broke `serde_yaml`'s `Value`-based
  deserialization in a way whose error message pointed at the wrong
  struct, caught and fixed before release — see the crate's test
  `node_identity_is_the_map_key_not_a_redundant_field`.)
- **Compiler integration** (`etdl-compiler::tree_event`): trees are
  declared under `x-tree-event` — the same generic `x-*` extension
  mechanism `x-reliability` already uses, so **zero parser/AST changes**
  were needed. Gated behind `supplements: [{id: etdl.tree-event, ...}]`,
  exactly like the reliability supplement's own opt-in discipline.
  Registered **unconditionally** in `builtin_registry()` (not behind the
  `reliability` Cargo feature — domain-neutral, built-in infrastructure).
  Three new diagnostics extending the existing `E-1xx` family: `E-120`
  (invalid manifest), `E-121` (structural validation failure, wrapping any
  `TreeError`), `E-122` (duplicate tree id). Wired into the ordinary
  `etdl validate`/`etdl compile` pipeline (previously `run_extensions` only
  ever invoked the reliability extension directly rather than iterating
  the registry generically; tree-event validation is called alongside it,
  additively, without changing that existing reliability code path).
- CLI: `etdl tree validate <file>`, `etdl tree inspect <file>`. `etdl
  capabilities` reports `tree_event: { available, schema, gates,
  structure }`.
- **`etdl-reliability::tree_adapter`** (new, purely additive) — one
  reliability interpretation of a generic tree: `evaluate_assuming_independence`
  combines leaf probabilities (`std.probability::Probability`, always
  caller-supplied, never inferred or defaulted) through each gate under an
  **explicit** independence assumption, verified against
  `etdl-probability-core`'s own `Binomial::cdf` for the `K_OF_N` case and
  against hand-derived values for AND/OR/NOT/XOR and a nested tree.
  `etdl-reliability/tests/tree_integration.rs` proves the full chain:
  `Tree` -> `tree_adapter` -> `std.probability` -> the *existing*,
  unmodified `ReliabilityArtifact`/`ArtifactResolver`.
- `examples/tree-event/generic.etdl` (no reliability vocabulary anywhere),
  `examples/tree-event/reliability-consumer.etdl` +
  `etdl-reliability/examples/tree_to_artifact.rs` (the same structural
  shape, interpreted by reliability), `examples/tree-event/future-safety-sketch.md`
  (documented, not implemented: the identical `Tree` type consumed by a
  hypothetical safety domain, requiring zero `etdl-tree-core` changes).
  Verified: the reliability-consumer example validates correctly even
  under an `etdl-cli` build compiled `--no-default-features` (no
  `reliability` feature) — tree structural validation never requires the
  reliability crate.
- `docs/reference/generic-tree-event-supplement.md` — the normative/
  informative specification (scope, terminology, tree/node/gate model,
  validation, identity, the structure/evaluation/domain-interpretation
  separation, reliability integration, ontology classification, built-in
  vs. optional, versioning, diagnostics).
- 47 tests across the feature (27 in `etdl-tree-core`, 5 in
  `etdl-compiler::tree_event`, 7 in `etdl-reliability::tree_adapter`, 2 in
  `etdl-reliability/tests/tree_integration.rs`, 6 pre-existing CLI tests
  re-verified) — structural test vectors for every gate kind, nested
  gates, every validation failure mode, full serde round-trip, and
  cross-validated reliability-interpretation arithmetic.
- Purely additive: no existing reliability/calibration/observation/artifact
  code was rewritten or moved; the full existing workspace test suite
  passes unchanged; WASM stays healthy on the real
  `wasm32-unknown-unknown` target for both new crates.

### Added — ETDL Standard Probability Library 1.0

- **New crate `etdl-probability-core`** — `std.probability`'s native layer.
  Zero dependency on any reliability crate (checkable directly in its
  `Cargo.toml`), WASM-safe (`cargo check --target wasm32-unknown-unknown`
  passes). `Probability` (validated `[0,1]`, rejects — never clamps —
  invalid values), `Rate` (distinct type, no `From`/`Into` to
  `Probability`), and explicit composition: `complement`,
  `independent_and`/`independent_and_n`, `independent_or`/`independent_or_n`,
  `mutually_exclusive_or` (kept structurally distinct from independent OR —
  rejects a sum exceeding 1 rather than silently producing an invalid
  result), `conditional` (requires the joint probability explicitly, never
  derives it by assuming independence), `bayes`.
- **Five foundational distributions**, each validated construction (no
  silent parameter repair) with correctly-named operations (PMF for
  discrete, PDF for continuous, CDF for all, survival function named as
  such on `Exponential`): `Bernoulli`, `Binomial` (log-space PMF avoiding
  overflow at `n` in the millions; CDF via the regularized incomplete beta
  function, the same identity `etdl-reliability`'s calibration module
  already uses independently), `Beta` (pdf/cdf/quantile/mean/variance —
  verified against the exact Beta-Binomial posterior mean
  `etdl-reliability`'s own estimator already computes for the same
  inputs), `Exponential` (numerically stable CDF via `expm1`, matching
  `etdl-reliability`'s existing exponential estimator's technique),
  `Normal` (CDF via Abramowitz & Stegun 7.1.26; quantile via Peter
  Acklam's rational approximation — an earlier draft mixed coefficients
  from two different published approximations and silently produced wrong
  results outside the central region, caught by a known-reference-value
  test, not just round-trip consistency).
- **`std.probability`** (ETDL source, `stdlib/probability/lib.etdl`) — the
  honestly-scoped pure-ETDL half: three reusable probability constants
  (`Certain`, `Impossible`, `EvenOdds`) as basic events. The compositional
  math and distributions are deliberately *not* ETDL source — ETDL has no
  expression/function-call syntax to express them; this is documented, not
  worked around with a fake syntax.
- **`etdl-reliability::probability_adapter`** (new, purely additive) —
  `estimate_from_probability`/`probability_from_estimate` convert between
  `etdl-probability-core::Probability` and the *unchanged*
  `etdl-reliability-core::ProbabilityEstimate`. Two cross-validation tests
  assert `etdl-probability-core`'s independently-implemented Binomial CDF
  and Beta posterior mean agree with `etdl-reliability`'s own,
  independently-implemented math for the same inputs — proving
  mathematical compatibility without moving or rewriting the existing,
  tested estimator code.
- `etdl capabilities` reports `std_probability: { available, schema, kind,
  distributions, sampling }` — `sampling` explicitly `"unavailable"`
  (deterministic math only; no RNG anywhere in this crate). No new CLI
  command ecosystem was added.
- `examples/probability/basic.etdl` (the ETDL-source half, end to end
  through `validate`/`analyze`), `etdl-probability-core/examples/{composition,distributions}.rs`
  (the Rust-API half — runnable via `cargo run -p etdl-probability-core
  --example ...`), `etdl-reliability/tests/probability_integration.rs`
  (a validated `Probability` flowing into the *existing*, unmodified
  `ReliabilityArtifact`/`ArtifactResolver`).
- `docs/reference/standard-probability-library.md` — full module
  reference: types (and why `Probability`/`ProbabilityEstimate` stay
  distinct types, not merged), every composition formula, all five
  distributions, numerical tolerance policy, determinism/sampling scope
  (and why sampling is out of scope here), units (mirroring the existing
  `std.units` deferral), built-in-vs-optional rationale, the reliability
  adapter, and explicit future hooks (hazard/survival, credible intervals,
  tree-event domains) that this task deliberately does not implement.
- 77 tests in `etdl-probability-core` (69 unit + 8 property/boundary:
  CDF bounds and monotonicity across all five distributions, complement
  involution, AND/OR bounds over a value grid) plus 6 in
  `etdl-reliability`'s adapter and 3 in its integration test — all passing,
  with known-reference-value assertions (not just internal consistency)
  for every composition operation and distribution.
- Purely additive: no existing reliability/calibration/observation/artifact
  code was rewritten or moved; the full existing workspace test suite
  passes unchanged; WASM stays healthy (`etdl-wasm` and
  `etdl-probability-core` both check clean on the real
  `wasm32-unknown-unknown` target).

### Added — ETDL Standard Library Core 1.0

- **`std.logic`** — the standard library's second module: named boolean
  composition patterns (`AnyOf`/OR, `AllOf`/AND, `MajorityOf`/VOTING k=2,
  `ExactlyOneOf`/XOR) built from ETDL's existing native gate types over
  three overridable placeholder signals (`SignalA`/`SignalB`/`SignalC`, no
  probability of their own). Source-only, embedded like `std.events`.
  Reuse pattern: override a placeholder's qualified id locally to
  repurpose a pattern (no template/parameter primitive — documented as a
  proposed future core addition, not implemented).
- **Qualified-id splicing generalized to gates.** `stdlib::expand_libraries`
  previously spliced only library-provided basic events; it now resolves a
  fixpoint over both basic events *and* gates, so a library-provided gate
  whose own inputs reference further qualified ids (its own placeholders,
  or another library) resolves transitively. Everything downstream (type
  checking, fault-tree evaluation, codegen) remains unaware libraries
  exist — a spliced gate is an ordinary gate to them.
- **`std.events` extended, non-destructively.** Four new, genuinely
  domain-neutral entries with no probability/failure_rate/mission_time at
  all — `Occurred`, `StateChanged`, `ConditionMet`, `SignalReceived` — sit
  alongside (not replacing) the five pre-existing illustrative
  failure-mechanism entries, which are unchanged for non-regression. An
  event no longer automatically implies failure.
- `--library-path` extended to `etdl validate` and `etdl analyze` (was
  previously only on `etdl compile` and `etdl library resolve`) — a real
  gap this task's own future-domain-library example exposed and closed.
- `examples/standard-library/generic-composition.etdl` — `std.events` +
  `std.logic` composed together with zero reliability involvement (no
  `supplements:` block at all).
- `examples/standard-library/future-domain-sketch/` — an illustrative
  optional library, `acme.signals` (not part of this repository's
  standard library), declaring `dependsOn: [{name: std.events, ...}]` and
  composing its generic identities — proving a future domain library can
  build on `std.*` without this repository implementing that domain.
- `examples/standard-library/units-limitation.md` — `std.units` is **not**
  implemented in this version; this document demonstrates the silent
  unit-confusion problem directly and explains why shipping named
  constants without real unit-checking would be exactly the "unsafe
  implicit behavior" this task says not to build. `std.collections` is
  similarly deferred (ETDL's type system has no generic/collection type to
  build one from). Both deferrals are documented with a concrete proposed
  core primitive in `docs/reference/standard-library.md`, not silently
  dropped.
- `docs/reference/standard-library.md` extended: full module reference
  (purpose/public constructs/examples/limitations/stability per module),
  a public-vs-internal stdlib API convention (nothing hidden appears under
  `components:`), the dependency-direction rule (`stdlib` must never
  depend on reliability, verified against actual crate dependencies, not
  merely asserted), and a "future tree-event domains" section showing the
  intended `std.events -> std.logic -> Tree Event Supplement -> Reliability
  Tree / Safety Tree / ...` shape (not implemented).
- Five new tests: a non-cyclic dependency chain (stdlib A depends on
  stdlib B, both resolve), gate splicing with transitive placeholder
  resolution, gate-splicing override flow-through, an end-to-end
  `std.logic` compile (correctly rejected by the *existing* V-503 rule
  until its placeholders are overridden — no special-casing), and the
  updated pure-ETDL check covering both built-in modules.
- Non-regression, verified: the reliability crates' full test suite
  (evidence, estimation, `ReliabilityArtifact`, dependency/CCF, uncertainty/
  sensitivity/importance, runtime observation, `ObservationDataset`,
  predicted-vs-observed calibration, build-manifest provenance) passes
  unchanged; no reliability source file was modified by this task. WASM
  builds clean on the real `wasm32-unknown-unknown` target.

### Added — ETDL Standard Library 1.0

- **`libraries:`** — a new top-level `EtlDocument` field (`LibraryImport`:
  `{name, version, required}`, deliberately shaped like `Supplement`) lets a
  document declare reusable ETDL-source libraries by dotted name, e.g.
  `std.events`. Resolution happens once, before structural validation,
  producing a new expanded document with referenced qualified ids
  (`<library-name>.<short-name>`) spliced into the fault trees that use
  them — nothing downstream (type checking, fault-tree evaluation, code
  generation) was modified; a qualified id is just another basic-event id
  to them.
- **`etdl-parser::ast::LibraryDocument`** — a library's own schema (`etdl`,
  `library: {name, version, description, dependsOn}`, `components`), parsed
  via the new `parse_library_document()`, reusing the exact same
  `Components`/`BasicEvent`/`Gate` types an ordinary document's
  `components:` block already uses. A library has no event trees or fault
  trees of its own — it's a component catalog, not a system.
- **`etdl-compiler::stdlib`** — the resolver: `LibraryResolver`,
  `expand_libraries()`, cycle detection (DFS with an explicit
  currently-resolving stack), major-version compatibility checking (the
  same rule already used for `doc.etdl` and `Supplement::version`), and a
  hard partition (not a precedence rule) protecting the reserved `std.*`
  namespace from being shadowed by an optional or user library, even if a
  same-named directory happens to exist on a search path.
- **`std.events`** — the standard library's first module: five reusable,
  illustrative basic-event definitions (`NetworkTimeout`,
  `ConnectionRefused`, `ProcessCrash`, `DiskFull`, `ConfigurationMissing`).
  Source-only (no native component): `stdlib/events/lib.etdl`, embedded
  into the compiler binary via `include_str!` (offline, no network, no
  manual copying — and available inside `etdl-wasm` for free, since
  embedding needs no filesystem).
- Six new diagnostics extending the existing `E-1xx`/`W-4xx` families:
  `E-113` invalid library name, `E-114` incompatible/unparseable version,
  `E-115` invalid library manifest, `E-116` required-but-unresolvable (or a
  shadowed `std.*` name), `E-117` cyclic dependency, `W-409`
  optional-but-unresolvable.
- CLI: `etdl library list`, `etdl library resolve <file>`, and
  `etdl compile --library-path <dir>` (repeatable, for optional-library
  search paths). `etdl validate`/`etdl analyze`/`etdl compile` resolve
  `libraries:` automatically — no flag needed for the built-in standard
  library or a project-local `lib/<name>/lib.etdl`. `etdl capabilities`
  reports `standard_library: { available, schema, builtin_libraries }`.
- `etdl-stdlib-manifest.json` — written next to generated code whenever at
  least one library resolved, independent of the `reliability` feature and
  of the existing `etdl-build-manifest.json` (reliability provenance
  semantics are unchanged).
- `docs/reference/standard-library.md` — the five-layer architecture, the
  core-vs-library rule, resolution/versioning/diagnostics reference, and
  what's intentionally not implemented yet (package registry, dependency
  solver beyond major-version gating, native-component compilation, a
  second stdlib module, gate/barrier/operation splicing).
- `examples/standard-library/` — a minimal worked example with captured,
  verified CLI output.
- This is purely additive: no existing reliability API was renamed, no
  reliability artifact/calibration/observation semantics changed, no
  existing Rust implementation moved, and ordinary documents that declare
  no `libraries:` are byte-for-byte unaffected (`cargo test --workspace`
  unchanged elsewhere).

### Added — Reliability Runtime Feedback & Calibration 1.0

- **Runtime observation path completed.** `BranchMonitor::record_branch` and
  `record_failure` in `etdl-core` now emit a `ReliabilityObservation` through
  the configured `ObservationSink` (previously they updated only the SLA
  tracker; no sink was ever reached). `ReliabilityObservation` gains
  `service_version` and `build_ref` fields — the software version and a
  stable reference to the compiled reliability artifact/build that produced
  the observation, without embedding the whole artifact. The runtime records
  data only; it runs no statistics, Monte Carlo, or reliability library.
- `etdl_core::observation::JsonlSink` — a lightweight file sink (append +
  flush per observation, no buffering that could lose data on exit), plus
  `generate_observation_id()` and `now_rfc3339()` helpers (no new
  dependency: reuses the existing `getrandom` fallback pattern).
- **Observation identity.** `AggregateObservation` gains an `id: Option<String>`
  field (`#[serde(default)]`, backward compatible); an `ObservationDataset`
  requires every member to carry one and rejects duplicates — including
  after reordering, since identity is never array position.
- `etdl-reliability::dataset` — `ObservationDataset`: versioned
  (`id`, `version`), immutable (a new observation is always a new dataset
  value, never an in-place edit), with schema, source, collection period,
  conditions and provenance. `aggregate_across()` sums observations for one
  failure mode across one or more datasets **only** when their exposure unit
  and conditions match exactly, refusing to silently combine incompatible
  observations; the result carries full `AggregationProvenance` (every
  contributing dataset and observation id, sorted for determinism).
- `Evidence::to_aggregate_observation()` — bridges raw per-occurrence
  evidence into the counted `AggregateObservation` form the estimators and
  calibration consume, reusing the existing `Evidence` outcome-matching
  convention rather than re-deriving it.
- **`etdl-reliability::calibration`** — predicted vs. observed comparison.
  `calibrate()` takes `&ReliabilityArtifact` (never `&mut`) and one
  `AggregateObservation`, and returns a `CalibrationResult` with expected
  failures (`n*p`), difference, ratio, and an **exact** two-sided binomial
  test p-value (reusing `analysis::estimator::regularized_beta`, the same
  machinery behind Beta-Binomial credible intervals — no normal
  approximation). Refuses the comparison (`unsupported_comparison`) when
  metric, conditions, or time basis don't match between prediction and
  observation, rather than comparing incompatible things. Five statuses:
  `consistent`, `potential_deviation`, `significant_deviation`,
  `insufficient_data`, `unsupported_comparison`. `is_drift()` is true only
  for `significant_deviation` under matching conditions — never merely
  `observed != predicted`. Five diagnostic codes `RC001`-`RC005`.
- CLI: `etdl reliability calibrate <artifact> <event> --dataset <ds>...
  [--alpha] [--strict-alpha] [--min-exposure] [--output]`. Exit code `0` for
  every computed status including drift (a report for engineering review,
  not a tool failure); `1` only for actual errors.
- `etdl capabilities` reports `runtime_feedback`/`calibration` availability
  and method (`binomial-two-sided-exact`) truthfully per build.
- `docs/reliability/runtime-feedback-calibration.md` — the full pipeline,
  the exact binomial test derivation, calibration statuses, and documented
  limitations (rate-based metrics and correlated uncertainty are explicitly
  out of scope for this version, not silently unsupported).
- `examples/reliability-runtime-feedback/` — a worked example showing the
  same observed data producing two different, individually correct verdicts
  depending on whether the artifact being checked is stale or current.
- `etdl-reliability/tests/calibration.rs` — reproduces the worked example's
  exact numbers (including the binomial p-values) as assertions, so the
  documentation and the implementation cannot silently diverge.
- The feedback loop is strictly **observe -> analyze -> review -> publish a
  new artifact -> rebuild**. Nothing in `calibration`/`dataset` can mutate an
  artifact, fault tree, or generated code; `calibrate()`'s only artifact
  parameter is `&ReliabilityArtifact`.

### Added — Reliability Analysis: uncertainty, sensitivity and importance 1.0

- **Uncertainty propagation that actually propagates.** `propagate()` samples
  each basic event from its declared uncertainty law and evaluates the
  dependency-aware fault tree per sample. Reports mean, median, interpolated
  quantiles (R type-7), standard deviation, standard error, and batch quantile
  stability.
- `PropagationSemantics` — the output interval states what it is:
  `propagated-credible-interval`, `propagated-quantile-interval-from-confidence-inputs`,
  `propagated-quantile-interval`, or `no-propagated-uncertainty`. A propagated
  interval is never described as a confidence interval.
- `Uncertainty::Interval` in `etdl-reliability-core` — a plain range with no
  coverage claim, for vendor min/max and engineering bounds. `level()` returns
  `None` for it. Plus `UncertaintyKind` with per-kind `interpretation()`.
- `InputUncertainty` — converts a declared `Uncertainty` into a sampling law
  (`deterministic`, `uniform`, `beta`, `normal`, `lognormal`,
  `normal-from-interval`). One-sided bounds and unsupported distributions are
  refused, not approximated.
- `sampling` module — documented `xorshift64star` PRNG with algorithm and
  version constants, Marsaglia-Tsang Gamma, and exact Beta via `X/(X+Y)`.
  Validated against closed-form moments including `Beta(1, 1e6)`.
- **Fussell-Vesely and criticality importance**, alongside the existing
  Birnbaum/RAW/RRW. Computed by exact conditioning, dependency-aware, and
  withheld with diagnostic `RA008` for non-coherent trees. The compiler's
  existing MOCUS is not duplicated.
- `ImportanceResult` / `SensitivityResult` — full results with method identity,
  measure names, assumptions and diagnostics, replacing bare floats.
- **Two-sided sensitivity** with elasticity `(dP/P)/(dq/q)`. Both directions are
  always evaluated; symmetry is reported, never assumed. A leg that cannot move
  is reported as not applied. Elasticity is withheld with `RA006` when a
  denominator is degenerate — no epsilon is substituted.
- **Uncertainty contribution ranking** (`variance-freeze-one-at-a-time/common-random-numbers`)
  — which probability estimate deserves more evidence. Explicitly labelled as
  not an importance measure and not a variance decomposition that sums to one.
- Analysis result artifact extended: `schema_version`, content-hashed
  `analysis_id`, `model_id`/`model_version`, `AnalysisInputs` snapshot with
  artifact refs, `AnalysisProvenance` (analyzer, method, sampler, seed, samples,
  level, ontology version), and diagnostics.
- `compare()` and `AnalysisComparison` — before/after with input, assumption,
  method and importance-rank changes. Refuses causal attribution unless exactly
  one input changed in exactly one way.
- Thirteen stable diagnostic codes `RA001`-`RA013`.
- CLI: `etdl analyze --uncertainty --level --perturbation --uncertainty-ranking
  --no-importance --no-sensitivity --output`; new `etdl reliability compare`.
- `etdl capabilities` now reports statistical estimation, uncertainty analysis,
  Monte Carlo (with sampler identity), importance (with measure list),
  sensitivity (with method), uncertainty ranking and analysis comparison
  separately. Correlated parameter uncertainty and conditional probability
  evaluation are reported as **unsupported** in every build.
- `docs/reliability/uncertainty-importance-sensitivity.md` — formulas,
  assumptions, interpretation and limitations for every metric.
- `examples/reliability-analysis/` — the complete worked example, including
  before/after mitigation.
- Test suites `importance.rs`, `sensitivity_analysis.rs`,
  `uncertainty_analysis.rs`, `end_to_end.rs`, each checking hand-derived
  closed-form results rather than plausible ranges.

### Fixed

- `analyze()` passed an empty interval map to Monte Carlo, so every propagation
  run sampled basic events at their point values and produced a zero-width
  interval. Propagation now samples declared laws.
- Declared conditional probabilities and `depends-on` / `conditional-on`
  dependency edges were validated but never used in evaluation — a silent
  independence fallback. `DependencyEvaluator::check_supported()` now runs before
  every evaluation and refuses such models with `RA001`. Importance, sensitivity
  and propagation all inherit the refusal.
- Common-cause importance forced the *affected leaves* to 0/1, which the
  conditioning loop then overrode, discarding the residual independent failure
  paths. It now conditions on the cause itself via a renormalised state sum, so
  `P(top | C absent)` correctly keeps each affected leaf at its residual
  probability `(q - p_C)/(1 - p_C)`.
- OR gates are evaluated as `-expm1(sum log1p(-q_i))`. The previous
  `1 - prod(1 - q_i)` form kept roughly four significant digits at `q ~ 1e-12`;
  the log-space form is accurate to about one ulp across the rare-event range.

### Changed

- `ImportanceMetric` extended with `FussellVesely`, `Criticality`,
  `RiskReductionWorth`, and the reserved-but-unimplemented `Diagnostic`,
  `Differential`, `Structural`. Gains `name()`, `is_implemented()` and
  `requires_coherence()`.
- `analysis::sensitivity::SensitivityResult` (the flat OR contribution ratio) is
  documented as legacy; new work should use the dependency-aware results.
- `MonteCarloConfig::validate()` rejects a zero sample count and an interval
  level outside `(0, 1)`.

### Notes

- **No ontology changes.** Uncertainty, importance and sensitivity are
  analysis-result metadata, not canonical failure-domain concepts, and
  `EntryKind::Dependency` already covers common causes. All ontology entries
  classify as *unchanged*: none extended, none new, none deprecated.
- **No specification changes.** Reliability Supplement 1.0 section 20 already
  names `UncertaintyResult`, `ImportanceResult`, `SensitivityResult` as analysis
  outputs, and section 11 already defines the uncertainty representation.
  Analysis artifacts remain external.
- Deterministic compilation is unchanged. Monte Carlo never runs during
  compilation; `etdl-core` and `etdl-parser` gained no dependencies.

## [0.2.0] — 2026-08-13

### Added
- Conformance suite (`conformance/`) with declarative valid/invalid/probability
  cases and a runner (`etdl-compiler/tests/conformance_test.rs`).
- `etdl analyze` CLI command; `--json`, `--quiet`, `--verbose` global flags;
  directory input for `validate`.
- `etdl-core::publisher` module (`Publisher`, `NoopPublisher`,
  `ChannelCapturingPublisher`) — generated handlers now take a `&dyn Publisher`.
- `etdl-core::condition::{contains, matches}` for ECEL `in` / `matches` lowering.
- Generated-code compile check (`etdl-compiler/tests/codegen_test.rs`).
- Proptest robustness suite (`etdl-parser/tests/robustness.rs`) — no panic on
  malformed/untrusted input.
- Criterion benchmarks + documented baselines (`docs/PERFORMANCE.md`).
- GitHub Actions CI (fmt, clippy, test, WASM build, `cargo audit`, docs) in
  both the compiler and VS Code repos.
- `SECURITY.md`, `CHANGELOG.md` (this file).
- Readiness docs: `READINESS_AUDIT.md`, `SPEC_IMPLEMENTATION_MATRIX.md`,
  `PROBABILITY_SEMANTICS.md`, `FAULT_TREE_ANALYSIS.md`, `EVENT_TREE_ANALYSIS.md`,
  `ECEL.md`, `ASYNCAPI_INTEGRATION.md`, `API_STABILITY.md`, `DIAGNOSTICS.md`,
  `RUNTIME.md`, `CLI.md`, `CONFORMANCE.md`, `PERFORMANCE.md`,
  `READINESS_SCORECARD.md`, `POSITIONING.md`, `DO_NOT_OVERCLAIM.md`,
  `COMMERCIAL_BOUNDARY.md`, `SaaS_REQUIREMENTS.md`, `ECOSYSTEM.md`,
  `CERTIFICATION.md`, developer/architect/business guides, 10 business demos.
- VS Code extension: language-server features (go-to-definition, references,
  hover, outline, completion, format) backed by WASM endpoints.

### Fixed
- Generated Rust no longer references `etdl_core::telemetry::BranchMonitor` (wrong
  path), unimported `WorkflowError`, or the undefined `publish_to_channel`;
  ECEL `in`/`matches` emit valid runtime calls. (P0-1)
- Fault-tree constant wiring honors `onFailureProbabilitySource` instead of
  taking the first tree. (P0-2)
- ECEL `[index]` no longer panics on overflow; saturates to `usize::MAX`. (P0-3a)
- `RetryPolicy::execute` returns `RetryError::{Exhausted,TimedOut}` instead of
  panicking; exponential backoff saturates. (P0-3d)
- W3C `traceparent` span-id/trace-id are now correctly sized and random. (P0-6)
- SLA observed-frequency now uses a per-node denominator (meaningful vs declared).
  (P0-7)
- Chaos production guard matches qualified env names (`production-us-east`, ...).
  (P0-8)
- Branch probability sum (V-203) is live again and checks the [0,1] range.
- Language version MAJOR gate (E-100), V-104 (non-terminating paths), V-301
  (handler identifier), transfer-target resolution (V-506). (P0-5)
- Event-tree cycle detection no longer flags a consequence revisited from a
  second branch. (P0-5 / V-102)
- Overflow-proof fault-tree math: f64 binomial (n≥66), log-space factorial
  (n≥21), MOCUS row cap. (P0-3c)
- Deterministic gate evaluation and V-404 diagnostic ordering.
- **UTF-8 char-boundary panic in the span index** (found by proptest): byte
  slicing now clamps to char boundaries. (P0-3)

## [0.1.4] — language-server endpoints

### Added
- `etdl-parser`: span-aware parsing (`spanned.rs`), LSP-style endpoints
  (`semantic.rs`), duplicate-id detection (`V-001`).
- `etdl-wasm`: `parse_with_spans`, `find_span`, `complete`, `hover`,
  `goto_definition`, `find_references`, `document_symbols`, `format`.

## [0.1.3] — gate/event-type extensions

### Added
- `INHIBIT` (with `inhibitCondition`) and `PRIORITY_AND` gates; `eventType`
  (`house`/`undeveloped`/`conditional`); fault-tree `transfers`.
- `etdl-wasm::parse_for_raaml` emitting `voting_params`.

## [0.1.2] — WASM + editor split

### Added
- `etdl-wasm` crate; `parse_for_diagram`; AsyncAPI `load_from_content`.

### Changed
- VS Code extension moved to `github.com/ETDL-lang/etdl-vscode`.

## [0.1.1] — metadata & docs

### Added
- SEO metadata, README rewrite, docs tree, CITATION.cff.

## [0.1.0] — initial release

### Added
- `etdl-parser`, `etdl-compiler`, `etdl-core`, `etdl-cli`; IEC 62502/61025
  event/fault tree modeling; ECEL; AsyncAPI resolution; Rust codegen; runtime
  (monitor, retry, SLA, chaos, telemetry).
