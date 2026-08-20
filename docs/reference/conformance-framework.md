# ETDL Conformance Guide 1.0

This is the guide for **ETDL Conformance, Verification & Validation 1.0**:
the cross-layer framework answering "how do we know an ETDL implementation
actually implements the ETDL specification and supplements correctly?"

It **extends** the conformance material that already existed before this
guide — [`docs/CONFORMANCE.md`](../CONFORMANCE.md) (what "conformant" means
per the core specification, §2.3) and `conformance/conformance.rs` (12
declarative parser/compiler/fault-tree-probability cases, unchanged) — with
the layers those never covered: the standard library, every supplement
(Generic Tree Event, Reliability, Predictive Reliability, Runtime Feedback &
Calibration), artifact/serialization, a dependency-graph checker, and a
machine-readable manifest/status report. Nothing pre-existing was rewritten,
redesigned, or replaced.

## Where the suite lives

**In-repo, in a new crate (`etdl-conformance`), not a separate GitHub
repository.** Task §57/§89 ask for a repository-split decision; this guide's
answer is: keep it here for 1.0. The reasoning:

- `etdl-specification` (a separate repo) already exists as the normative
  spec authority — that split is correct and untouched.
- A third repo (`etdl-conformance`) is explicitly valuable only if it lets
  a *different* ETDL implementation run the same corpus independently. That
  value is real for the existing Level 0/1 suite (`docs/CONFORMANCE.md`
  already documents exactly how a third party would port it — declarative
  YAML cases + a small runner). It is not yet real for Levels 2-7: those
  vectors are written directly against this workspace's own Rust APIs
  (`etdl-probability-core`, `etdl-tree-core`, `etdl-reliability::
  predictive`, ...), because no second ETDL implementation exists to port
  them to. Splitting a single-implementation test suite into its own repo
  before there is a second implementation to serve would be motion without
  value — exactly what task §89 warns against ("only create a third
  repository if the separation provides real value").
- Creating a new GitHub repository is also an action with real,
  hard-to-reverse consequences (a public namespace claim, a second
  place contributors have to know about) that should be a deliberate,
  confirmed decision, not a side effect of one task.

**Recommended trigger for the split**: the day a second ETDL implementation
(or a certification body, per `docs/CONFORMANCE.md`'s "Future
certification" section) wants to run these vectors against itself. At that
point, `etdl-conformance`'s `tests/*.rs` files are already close to
portable — each vector is self-contained data plus an assertion — and the
crate should move to its own repository essentially unchanged.

## Conformance levels

Matches this workspace's own layering, not an invented hierarchy:

| Level | Name | What it checks | Suite |
|---|---|---|---|
| 0 | Syntax | `.etdl` parses (or is correctly rejected) | `conformance/conformance.rs` (existing) |
| 1 | Semantic | Validation diagnostics, fault-tree resolution, type checking | `conformance/conformance.rs` (existing) |
| 2 | Standard Library | `libraries:` import resolution, `std.probability` invariants | `etdl-conformance/tests/stdlib_probability.rs` |
| 3 | Supplement | Generic Tree Event, Reliability, Predictive Reliability, Runtime Feedback & Calibration | `tests/tree_event.rs`, `tests/reliability.rs`, `tests/predictive_reliability.rs`, `tests/calibration.rs` |
| 4 | Artifact | `ReliabilityArtifact` serialization, schema, identity, provenance | `tests/artifact.rs` |
| 5 | Runtime | `BranchMonitor`, observation emission, calibration behavior | `tests/calibration.rs` (CAL-003/004) |
| 6 | WASM | The WASM-safe subset builds and matches the documented profile | see "WASM conformance profile" below |
| 7 | Full | Cross-layer invariants (dependency graph, architecture boundaries) | `tests/architecture.rs` |

A vector's `Level` field ([`etdl_conformance::vector::Level`]) records which
of these it belongs to; `docs/architecture.md` is the source of truth for
the layering itself.

## Normative test vectors

Every `#[test]` in `etdl-conformance/tests/` is paired with a
`ConformanceVector` (`etdl-conformance/src/vector.rs`):

```rust
pub struct ConformanceVector {
    pub id: &'static str,        // e.g. "PRED-001" — stable, never reused
    pub level: Level,
    pub spec_ref: &'static str,  // which doc section is the authority
    pub requirement: &'static str, // the semantic requirement, one sentence
    pub version: &'static str,   // suite version this vector was introduced in
    pub status: VectorStatus,    // Active / Experimental / Deprecated
}
```

A vector carries **no input/expected-output payload of its own** — those
vary too widely (ETDL source text, a numeric formula, an artifact JSON
document) to usefully force into one generic field. Each test function owns
its case data directly; the vector is identity/traceability metadata
attached to that test, printed in assertion failure messages so a failing
vector always names itself (`ARCH-001`, `PRED-003`, ...) rather than
failing anonymously.

**Naming convention** (task §66): `<AREA>-<NNN>`. Areas in use: `ARCH`
(dependency graph / architecture), `LIB-PROB` (std.probability), `TREE`
(Generic Tree Event), `REL` (Reliability estimates), `PRED` (Predictive
Reliability), `CAL` (Calibration), `ART` (Artifacts). IDs are never reused
for a different requirement once published — the same discipline
`docs/DIAGNOSTICS.md` already applies to diagnostic codes.

## Specification as authority; resolving disagreements

Per task §4, when implementation behavior differs from what a vector
expected, the order of operations is: (1) is the implementation wrong, (2)
is the specification/doc incomplete, (3) is the behavior intentionally
implementation-defined, (4) document the resolution — never silently loosen
the vector to make it pass. This guide records the two resolutions this
task's own vectors produced, as a template for future ones:

1. **`ARCH-005` (etdl-wasm's dependency graph)** — an initial vector
   asserted `etdl-wasm` has zero dependency on *any* reliability crate. It
   failed: `etdl-wasm` depends on `etdl-compiler` with default features,
   and `etdl-compiler`'s `reliability` feature (default-on) pulls in
   `etdl-reliability-core`. Resolution: the implementation is not wrong —
   `etdl-reliability-core` is a pure serde-typed crate confirmed WASM-safe
   by the `wasm` CI job, and its presence means the WASM validator can
   surface reliability diagnostics for documents declaring
   `x-reliability`, plausibly a feature rather than a bug for the VS Code
   extension. The vector was narrowed to the invariant that actually
   matters (no dependency on the *heavy* `etdl-reliability` engine,
   ontology, or failure discovery), and the finding was documented in
   `docs/reference/crates.md`'s `etdl-wasm` section rather than silently
   asserted away.
2. **`TREE-010` (deep-tree stack safety)** — a new vector (not present
   before this task) constructing a 5,000-deep-but-valid tree crashed the
   process with a stack overflow. Resolution: the implementation *was*
   wrong per this task's own §43/44 (resource limits, stack/recursion
   safety) — `etdl-tree-core`'s cycle-detection walk, `descendants`, and
   `postorder` were recursive (one Rust function call per tree node).
   Fixed by converting all three to iterative, explicit-stack
   implementations that produce byte-identical output (verified: all 27
   pre-existing `etdl-tree-core` tests pass unchanged) without the
   process-stack depth risk. See `etdl-tree-core/src/tree.rs` and
   `traverse.rs` for the rewritten functions' doc comments.

## No self-certification loop

Task §73/§74/§39/§40: a test architecture where the implementation
generates its own expected result is not a conformance check. Every
numerical conformance vector in this crate compares the implementation's
output to `etdl-conformance::reference` — a small module coded directly
from mathematical definitions using only `std`'s floating-point primitives,
**never** calling into `etdl-probability-core`, `etdl-reliability-core`, or
`etdl-reliability`'s own formulas. Two concrete examples:

- `LIB-PROB-003`/`PRED-002`/`PRED-003` compare `etdl-probability-core`'s
  regularized-incomplete-beta-based `Binomial::cdf` (and
  `etdl-reliability::predictive`'s exponential/Weibull models) against
  `reference::binomial_cdf`/`reference::exponential_*`/`reference::
  weibull_*` — direct PMF summation and closed-form evaluation, a
  different algorithm computing the same quantity.
- `CAL-001`/`CAL-002` compare `etdl-reliability::calibration::
  binomial_test_two_sided` (which uses the regularized incomplete beta
  function) against `reference::binomial_test_two_sided` (direct PMF
  summation) — both implement the *same documented statistical
  definition* (the standard "doubling" two-sided exact binomial test,
  `min(2*min(P(X<=k), P(X>=k)), 1)`), via genuinely different code paths.
  This is important: the oracle was deliberately written to match the
  implementation's own *documented* definition, not a different
  (also-valid) one like the "minlike" method some statistics packages
  default to — matching a different definition would be "silently
  redefining the language" (task §4), not conformance checking.

Textbook constants are used directly where available instead of a coded
oracle (`LIB-PROB-005`'s standard-normal 97.5th-percentile check against
the well-known value `1.9599639845...`, reproduced from statistical
tables) — the simplest, most independent oracle of all.

## Numerical tolerance policy

No exact floating-point equality is used for any algorithm involving
transcendental functions (`exp`, `ln`, `powf`) or iterative numerical
methods. Tolerances are chosen per vector, stated explicitly, and fall into
three bands:

- **`1e-12`–`1e-9`**: closed-form identities that should hold to near
  machine precision (e.g. `S(t) + F(t) = 1`, De Morgan's identity over
  probability composition, the exponential reference vs. implementation
  comparison).
- **`1e-6`**: cross-algorithm comparisons against the independent
  oracle where the two code paths accumulate floating-point error
  differently (e.g. `Binomial::cdf` via regularized-incomplete-beta vs.
  direct PMF summation; the calibration p-value cross-check).
- **A documented qualitative bound instead of a fixed epsilon**, when the
  quantity itself is expected to be extremely small or approach a limit
  (e.g. `PRED-008`'s "`S(t)` at `t=10^6` must be finite, non-negative, and
  `< 1e-30`" rather than comparing to one specific tiny float).

No vector anywhere uses exact `==` for a computed `f64`.

## WASM conformance profile

**Explicitly WASM-required** (built and smoke-tested by the existing `wasm`
CI job): `etdl-parser`, `etdl-compiler` (including its default `reliability`
feature — see the `ARCH-005` resolution above), `etdl-tree-core`,
`etdl-wasm`'s own bindings (`validate_etdl`, `parse_for_diagram`,
`parse_for_raaml`, `parse_with_spans`, LSP-style semantic endpoints — see
`docs/reference/crates.md#etdl-wasm`).

**Explicitly native-only** (never linked into `etdl-wasm`, and the `wasm`
job would fail loudly if that ever changed, since `ARCH-005` runs as part
of the ordinary native `cargo test` suite, not inside the WASM build
itself): the rich `etdl-reliability` engine (analysis, calibration,
predictive models), `etdl-reliability-ontology`, `etdl-failure-discovery`,
and this crate itself (`etdl-conformance`, which is a native-only
dev/testing tool, not compiled to WASM). A caller attempting to use a
native-only feature from a WASM binding gets a compile-time absence (the
symbol does not exist), not a confusing runtime failure — there is no
"unavailable at runtime" WASM code path to test, because the unavailable
functionality was never linked in.

## Optional-library conformance

`std.probability` is built-in (always available, unconditionally). The
standard library's optional/user library search path (`--library-path`,
`etdl-compiler::stdlib::LibraryResolver`) is exercised by the existing
`docs/reference/standard-library.md` test suite (unchanged by this task);
this framework's own contribution is `ARCH-004`/`ARCH-007`, which verify at
the dependency-graph level that a library's *optionality* is real —
i.e. that disabling the `reliability` cargo feature genuinely removes
`etdl-reliability-core` from the build graph rather than it sneaking in
transitively (this exact class of bug is what `ARCH-005` found and this
task's own `etdl-conformance/Cargo.toml` had to be fixed to avoid — see
its comment on the `etdl-compiler` dependency entry).

## Dependency graph / architecture invariants

`etdl-conformance::depgraph` shells out to `cargo metadata --format-version=1
--no-deps` (already installed; no new dependency) and parses only **normal**
(non-dev, non-build) dependency edges — a dev-dependency (e.g.
`etdl-failure-discovery`'s test-only use of `etdl-compiler`/
`etdl-reliability`) is not a runtime architectural coupling and is
correctly excluded. `tests/architecture.rs`'s 7 `ARCH-*` vectors check:
zero dependency from `etdl-probability-core`/`etdl-tree-core` onto any
reliability crate; the one-directional `etdl-reliability -> {probability,
tree}` edge; `etdl-compiler`'s and `etdl-cli`'s reliability-family
dependencies are genuinely optional (not just conventionally treated as
such); no dependency cycles anywhere in the workspace.

## CLI: `etdl conformance`

```
etdl conformance status     # objective PASS/PARTIAL/UNSUPPORTED per area
etdl conformance manifest   # machine-readable manifest (JSON with --json)
```

No new command ecosystem beyond these two — matching the same restraint
`std.probability` and Predictive Reliability's own CLI additions already
applied (`docs/reference/standard-probability-library.md`,
`docs/reference/predictive-reliability-supplement.md`). `status` reports
**compiled-in capability from cargo feature flags**, computed once
(`cfg!(feature = "reliability")`) and passed into
`etdl_conformance::report::area_statuses` — it does not run the actual test
suite (that remains `cargo test -p etdl-conformance`), so it stays fast and
side-effect-free, the same "capabilities never probes anything, only
reports what was compiled in" discipline `etdl capabilities` already
established.

## CI integration and release gates

The `conformance` job in `.github/workflows/ci.yml` runs
`cargo test -p etdl-conformance --all-targets` as a normal, fast job
alongside the existing `test`/`clippy`/`wasm`/`features` jobs — it is not a
separate slow tier, because every vector in this crate runs in well under a
second (verified: the full suite, including the `cargo metadata` subprocess
call in `tests/architecture.rs`, completes in well under 1 second). Fuzzing
(task §41) is deliberately **not** wired into this job or any other CI job
in this task — see "Known gaps" below.

**Release gate**: a release should not claim Predictive
Reliability/Runtime Feedback/Artifact conformance if `cargo test -p
etdl-conformance --features reliability` fails; it should not claim any
conformance claim at all if `cargo test -p etdl-conformance
--no-default-features` fails, since that is the lean-build floor every
build configuration must clear.

## Versioning

`etdl_conformance::CONFORMANCE_SUITE_VERSION` ("1.0.0") is distinct from
the ETDL language version (`etdl_conformance::ETDL_LANGUAGE_VERSION`), the
workspace crate version, and any individual supplement's own version.
Bumped when vectors are added, materially changed, or deprecated. A vector
is never silently deleted when it becomes obsolete — its `status` field
moves to `Deprecated` (task §19/§20's backward-compatibility testing
depends on deprecated vectors staying runnable, not disappearing).

## Known gaps (stated honestly, not hidden)

- **Fuzzing (task §41)**: no `cargo-fuzz` targets exist yet for the
  parser, module loader, artifact decoder, or tree validator. This guide
  recommends `cargo-fuzz` targets seeded from
  `etdl-conformance`'s own negative-test corpus (`ART-004`'s malformed
  JSON strings, `TREE-002`/`TREE-003`'s malformed tree shapes) as the
  concrete next step — deferred because it requires a nightly toolchain
  and a fuzzing corpus this task's scope did not include setting up.
- **Security testing beyond illustrative cases (task §42)**: `ART-004`
  covers a handful of malformed-artifact-JSON shapes (crash/panic
  resistance, not a security audit). No dedicated resource-exhaustion or
  path-traversal corpus exists yet.
- **Migration testing (task §79)**: no artifact migration mechanism exists
  in the implementation, so there is nothing to test — this is the
  explicit "if migration does not exist, document explicit compatibility
  behavior" case the task itself anticipates. Current behavior: loading an
  artifact with a schema version other than `ARTIFACT_SCHEMA` is a hard
  rejection (`SchemaVersionMismatch`, `ART-003`), never a silent rewrite.
- **No dedicated runtime conformance harness**: `BranchMonitor`/observation
  emission has existing unit/integration test coverage (`etdl-core`,
  `etdl-reliability`), but no `etdl-conformance`-owned Level-5 vectors
  beyond the calibration-specific `CAL-003`/`CAL-004`. Reported as
  `Partial` in `etdl conformance status`.
- **The `std.reliability` ETDL-source facade** (carried forward from the
  Predictive Reliability task's own final report): still not built.
  Predictive Reliability's conformance vectors test `etdl-reliability::
  predictive` directly, consistent with the rest of the ecosystem building
  on that crate in the facade's absence.
- **No separate `etdl-conformance` repository yet** — see "Where the suite
  lives" above for the reasoning and trigger condition.
