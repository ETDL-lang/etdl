# Crates Reference

This repository's workspace (`Cargo.toml`) has eleven path members, each
versioning and documenting itself independently (own `README.md`, own
`version` — see `docs/API_STABILITY.md`, which is the crate/SemVer axis,
separate from the ETDL *language* version, currently `1.0.0`). The richer
reliability engine and every non-`rust` code-generation target moved to
their own repositories and are pulled in as `git` dependencies (see the
comment block at the top of `Cargo.toml`); they're listed at the bottom of
this page for completeness. Publish order for the in-workspace crates
(respecting the dependency graph): `etdl-core`, `etdl-probability-core`,
`etdl-tree-core`, `etdl-reliability-core` → `etdl-parser` → `etdl-compiler`
→ `etdl-cli`, `etdl-wasm`, `etdl-conformance`, `etdl-supplement-sdk`,
`etdl-runtime-ffi`.

| Crate | Purpose |
|---|---|
| [etdl-parser](https://crates.io/crates/etdl-parser) | Parse `.etdl` documents, ECEL expressions, and AsyncAPI 3.0 references |
| [etdl-compiler](https://crates.io/crates/etdl-compiler) | Semantic validation, fault tree resolution, standard-library resolution, code generation |
| [etdl-core](https://crates.io/crates/etdl-core) | Runtime library for generated code (`BranchMonitor`, retry, SLA, chaos, telemetry, ECEL `in`/`matches` helpers) |
| [etdl-cli](https://crates.io/crates/etdl-cli) | `etdl` binary (compile/validate/analyze/discover/reliability/library/tree/supplement/conformance/capabilities) |
| [etdl-wasm](https://crates.io/crates/etdl-wasm) | WASM bindings (validate, AST extraction, LSP endpoints) for editor extensions |
| `etdl-probability-core` | `std.probability`'s native layer — see below |
| `etdl-tree-core` | Generic Tree Event Supplement's native layer — see below |
| `etdl-reliability-core` | Built-in reliability types (`ProbabilityEstimate`, `ReliabilityArtifact`) the compiler depends on |
| `etdl-conformance` | Conformance, verification & validation framework — see `docs/reference/conformance-framework.md` |
| `etdl-supplement-sdk` | SDK for authoring third-party supplement plugins as sandboxed `.wasm` modules — see below and [supplement-plugins.md](supplement-plugins.md) |
| `etdl-runtime-ffi` | Stable, versioned C ABI over `etdl-core`; what every non-Rust `--target` binding actually calls. No toolchain of its own to build; `cargo build -p etdl-runtime-ffi --release` |

Crates that used to live in this workspace and now live elsewhere, pulled in
by `etdl-cli`/`etdl-conformance` as git dependencies (each still Apache 2.0
licensed, still independently versioned):

| Crate | Repository | Purpose |
|---|---|---|
| [etdl-reliability](https://crates.io/crates/etdl-reliability) | [etdl-reliability](https://github.com/ETDL-lang/etdl-reliability) | The richer, optional reliability engine (analysis, calibration, predictive reliability) — the `reliability` Cargo feature |
| [etdl-reliability-ontology](https://crates.io/crates/etdl-reliability-ontology) | [etdl-reliability](https://github.com/ETDL-lang/etdl-reliability) | Canonical failure taxonomy and ontology versioning |
| [etdl-failure-discovery](https://crates.io/crates/etdl-failure-discovery) | [etdl-reliability](https://github.com/ETDL-lang/etdl-reliability) | Source-code failure/event discovery, mapped to the ontology — the `discovery` Cargo feature |
| `etdl-target-java`/`-python`/`-go`/`-dotnet` | one repo per target, e.g. [etdl-target-java](https://github.com/ETDL-lang/etdl-target-java) | Optional `--target` code-generation bindings — see [Target Architecture](../architecture/targets.md) |

This page does not assert current publication status on crates.io for any
crate — verify directly before relying on it.

`etdl-probability-core` — `std.probability`'s native layer: `Probability`,
`Rate`, composition math (complement, independent AND/OR, conditional,
Bayes), and five distributions (Bernoulli, Binomial, Beta, Exponential,
Normal). Zero dependency on any reliability crate by construction. See
[standard-probability-library.md](standard-probability-library.md).

`etdl-tree-core` — the Generic Tree Event Supplement's native layer:
`Tree`/`TreeNode`/`GateKind` (AND/OR/NOT/XOR/K_OF_N), structural
validation (cycles, arity, shared nodes, reachability), traversal,
serialization. Zero dependency on any reliability or probability crate.
See [generic-tree-event-supplement.md](generic-tree-event-supplement.md).

`etdl-reliability::predictive` — the Predictive Reliability Supplement:
`TimeToFailureModel` trait with `ExponentialModel`/`WeibullModel`,
`MissionTime`/`PredictiveResult`/`PredictiveQuantity`, censored-observation
representation, a read-only calibration adapter from `ReliabilityArtifact`,
and predictive tree evaluation reusing `tree_adapter` unchanged. Feature-gated
with the rest of `etdl-reliability` (`reliability` cargo feature). See
[predictive-reliability-supplement.md](predictive-reliability-supplement.md).

## etdl-parser

- `ast::EtlDocument` — full AST with manual `Deserialize` supporting `eventTrees`/`eventTree` and `x-*` extension fields
- camelCase aliases (`maxAttempts`, `backoffMs`, `backoffStrategy`, `probabilitySource`, `onFailure`, `retryPolicy`, `timeoutMs`, `failureRate`, `missionTime`)
- `ecel` — ECEL parser (nom-based), `Literal::Array` literals, wildcard/index/quoted-key paths
- `asyncapi` — AsyncAPI 3.0 document loading and registry with schema introspection
- `jsonptr` — RFC 6901 JSON Pointer resolution

## etdl-compiler

- `validate` — all E-1xx, V-1xx..V-5xx, W-4xx diagnostics
- `fault_tree` — exact top-event probability evaluation (AND/OR/NOT/XOR/VOTING, exponential failure model), MOCUS `enumerate_minimal_cut_sets`
- `typeck` — ECEL type checking against AsyncAPI schemas
- `codegen::{CodeGenerator, RustCodeGenerator}` — the generator trait and the Rust backend
- `stdlib` — ETDL Standard Library resolution (`libraries:` imports, built-in
  `std.*` embedded from `etdl-compiler/stdlib/`, optional/user library search paths,
  cycle/version diagnostics). See [standard-library.md](standard-library.md).
- `extension::{EtdlExtension, ExtensionContext, ExtensionRegistry, ExtensionResult}` —
  the generic supplement-extension mechanism (mirrors the ETDL specification's
  Section 11.3 validate/process lifecycle). Six built-in extensions are
  registered in `extension::builtin_registry()`: `etdl.reliability` and
  `etdl.tree-event` are each additionally wired into `Compiler` internally
  via their own special-cased direct call; `etdl.performance`, `etdl.safety`,
  `etdl.diagnostics`, and `etdl.security` instead run through the same
  generic, registry-driven path `Compiler::with_extension` uses
  (`Compiler::new()` seeds `Compiler::extensions` with all four) — the
  preferred shape for a new core supplement going forward, since it needs no
  bespoke pipeline code of its own. See each module's own docs,
  [performance-supplement.md](performance-supplement.md),
  [safety-supplement.md](safety-supplement.md),
  [diagnostics-supplement.md](diagnostics-supplement.md), and
  [security-supplement.md](security-supplement.md). A caller registers an
  *additional*, non-built-in extension — for example, a third-party,
  non-core supplement (specification Section 11.4) such as a future
  `etdl.chain` implementation — with
  `Compiler::new().with_extension(Box::new(my_extension))`; its
  `validate`/`process` then run during `Compiler::validate`/`compile` exactly
  like these four's, gated the same way (only for a document that declares
  the extension's id under `supplements:`). An extension that resolves
  external values into fault-tree probabilities implements
  `ExtensionResult::basic_event_overrides`. See
  `etdl-compiler/tests/third_party_extension_test.rs` for a complete,
  runnable example proving both phases actually execute and an override
  actually reaches generated code, and each supplement's own
  `etdl-compiler/tests/*_wiring_test.rs` for the same proof per supplement —
  including tests proving multiple generically-registered supplements run
  together in one document without interfering, and that a warning-only
  diagnostic isn't duplicated by `process()` re-running validation
  (`run_extensions` only skips `process()` after an *error*, not a warning).
- `performance` — the Performance Supplement (`etdl.performance`): declared
  latency percentile budgets and throughput expectations against existing
  Operation/Event Tree nodes, purely declarative (no runtime enforcement, no
  probability math). See [performance-supplement.md](performance-supplement.md).
- `safety` — the Safety Supplement (`etdl.safety`): hazard classification
  against a fixed severity/likelihood risk matrix, and Safety Integrity
  Level/independence declarations on existing core Barrier nodes; no new
  probability mathematics. See [safety-supplement.md](safety-supplement.md).
- `diagnostics` — the Diagnostics Supplement (`etdl.diagnostics`): declared
  telemetry-span-to-Fault-Tree-cause correlations and monitored-node
  anomaly rules; purely structural metadata, no automated inference. See
  [diagnostics-supplement.md](diagnostics-supplement.md).
- `security` — the Security Supplement (`etdl.security`): STRIDE-classified
  attack trees (reusing `etdl.tree-event`'s `Tree` structure — the one
  built-in supplement with a real cross-supplement dependency) and Controls
  mapped onto core Barrier nodes. See [security-supplement.md](security-supplement.md).

## etdl-core

- `monitor::BranchMonitor` — branch/failure recording with declared probabilities
- `retry::{RetryPolicy, BackoffStrategy}` — async retry with exponential/fixed backoff and timeout
- `sla::SlaTracker` — rolling-window anomaly detection (`ETDL_SLA_WINDOW`, `ETDL_SLA_THRESHOLD`)
- `chaos::ChaosController` — seeded, scoped failure injection, production guard (`ETDL_CHAOS`, `ETDL_CHAOS_SEED`, `ETDL_CHAOS_SCOPE`, `ETDL_ENV`)
- `telemetry` — `inject_traceparent` W3C trace context, anomaly events, node span attributes

## etdl-cli

The `etdl` binary — see [CLI reference](../CLI.md).

## etdl-wasm

WASM bindings compiled with `wasm-bindgen` for the [VS Code extension](https://github.com/ETDL-lang/etdl-vscode):

`etdl-wasm` depends on `etdl-compiler` with default features, so it
transitively includes `etdl-reliability-core` (the compiler's `reliability`
feature is on by default) — a pure serde-typed crate with no filesystem/
thread/IO code, confirmed WASM-safe by the `wasm` CI job. This means
`validate_etdl` surfaces E-110/111/112 reliability diagnostics for documents
declaring `x-reliability` even in the WASM build. `etdl-wasm` never depends
on the richer `etdl-reliability` engine, `etdl-reliability-ontology`, or
`etdl-failure-discovery` — checked by `etdl-conformance`'s `ARCH-005` vector
(`etdl-conformance/tests/architecture.rs`).

- `validate_etdl(content, asyncapi_files_json)` — returns diagnostics as JSON (no filesystem access; AsyncAPI contents are passed in). Every diagnostic carries 0-based `line`/`column`/`end_line`/`end_column` when a source position is known, plus `V-001` warnings for duplicate ids under `nodes`/`gates`/`basicEvents`.
- `parse_for_diagram(content)` — returns the AST as JSON for diagram rendering
- `parse_for_raaml(content)` — like `parse_for_diagram` with RAAML enrichments (e.g. `voting_params`)
- `parse_with_spans(content)` — like `parse_for_raaml` but attaches a `span` (0-based offsets/line/column) to every element; scalar leaves with spans are wrapped as `{ "value", "span" }`
- `find_span(content, offset)` — the deepest semantic element (`kind`/`name`/`field`/`tree`/`span`) containing a 0-based character offset, or `null`
- `complete(content, offset)` / `hover(content, offset)` / `goto_definition(content, offset)` / `find_references(content, offset)` / `document_symbols(content)` / `format(content)` — LSP-style semantic endpoints (JSON payloads modeled on LSP types)
- `version()` — crate version

`AsyncApiRegistry::load_from_content` was added to `etdl-parser` so imports can be provided as string contents rather than read from disk.

Source-position support lives in `etdl-parser::spanned` (a `SpanIndex` built with a position-aware YAML parse) and the semantic endpoints in `etdl-parser::semantic`. The validator attaches structured locators (`SpanKey`) to every diagnostic; the WASM layer resolves them against the span index.

## etdl-supplement-sdk

SDK for writing ETDL supplement plugins — dynamically loaded, sandboxed
`wasm32-unknown-unknown` modules that `etdl-cli` (built with its optional
`plugins` Cargo feature) runs via `etdl supplement install`. A Rust author
implements the `Supplement` trait and wraps it with the `etdl_supplement!`
macro, which generates the six-export wire ABI (`etdl_alloc`/
`etdl_dealloc`/`etdl_supplement_id`/`etdl_supplement_version`/
`etdl_supplement_validate`/`etdl_supplement_process`) so the plugin author
never implements it by hand:

```rust
use etdl_supplement_sdk::{Supplement, SupplementContext, SupplementDiagnostic};

#[derive(Default)]
struct MyAudit;

impl Supplement for MyAudit {
    fn id(&self) -> &str { "etdl.mycompany-audit" }
    fn version(&self) -> &str { "1.0" }

    fn validate(&self, _doc: &serde_json::Value, _ctx: &SupplementContext) -> Vec<SupplementDiagnostic> {
        Vec::new()
    }
}

etdl_supplement_sdk::etdl_supplement!(MyAudit);
```

```bash
cargo build --target wasm32-unknown-unknown --release
etdl supplement install target/wasm32-unknown-unknown/release/my_audit.wasm
```

The plugin sees the parsed document as `serde_json::Value`, not
`etdl_parser::ast::EtlDocument`, precisely so this crate never depends on
`etdl-parser`. Non-Rust plugin authors (or anyone implementing the ABI by
hand) should read [supplement-plugins.md](supplement-plugins.md) instead,
which documents the raw wire contract this macro generates.

