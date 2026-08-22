# Crates Reference

ETDL's workspace has grown to twelve crates as the standard library and
supplements were added. All twelve share one workspace version, currently
`0.2.2` (see `docs/API_STABILITY.md` — this is the crate/SemVer version, a
separate axis from the ETDL *language* version, currently `1.0.0`). Publish
order (respecting the dependency graph): `etdl-core`, `etdl-probability-core`,
`etdl-tree-core`, `etdl-reliability-core` → `etdl-parser` → `etdl-compiler`,
`etdl-reliability-ontology` → `etdl-reliability`, `etdl-failure-discovery` →
`etdl-cli`, `etdl-wasm`, `etdl-conformance`.

| Crate | Purpose |
|---|---|
| [etdl-parser](https://crates.io/crates/etdl-parser) | Parse `.etdl` documents, ECEL expressions, and AsyncAPI 3.0 references |
| [etdl-compiler](https://crates.io/crates/etdl-compiler) | Semantic validation, fault tree resolution, standard-library resolution, code generation |
| [etdl-core](https://crates.io/crates/etdl-core) | Runtime library for generated code (`BranchMonitor`, retry, SLA, chaos, telemetry, ECEL `in`/`matches` helpers) |
| [etdl-cli](https://crates.io/crates/etdl-cli) | `etdl` binary (compile/validate/analyze/discover/reliability/library/tree/conformance/capabilities) |
| [etdl-wasm](https://crates.io/crates/etdl-wasm) | WASM bindings (validate, AST extraction, LSP endpoints) for editor extensions |
| `etdl-probability-core` | `std.probability`'s native layer — see below |
| `etdl-tree-core` | Generic Tree Event Supplement's native layer — see below |
| `etdl-reliability-core` | Built-in reliability types (`ProbabilityEstimate`, `ReliabilityArtifact`) the compiler depends on |
| `etdl-reliability` | The richer, optional reliability engine (analysis, calibration, predictive reliability) |
| `etdl-reliability-ontology` | Canonical failure taxonomy and ontology versioning |
| `etdl-failure-discovery` | Source-code failure/event discovery, mapped to the ontology |
| `etdl-conformance` | Conformance, verification & validation framework — see `docs/reference/conformance-framework.md` |

All crates are Apache 2.0 licensed. This table does not assert current
publication status on crates.io — verify directly before relying on it.

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
  Section 11.3 validate/process lifecycle). The two built-in extensions
  (`etdl.reliability`, `etdl.tree-event`) are wired into `Compiler`
  internally and unaffected by this. A caller registers an *additional*,
  non-built-in extension — for example, a third-party, non-core supplement
  (specification Section 11.4) such as a future `etdl.chain` implementation —
  with `Compiler::new().with_extension(Box::new(my_extension))`; its
  `validate`/`process` then run during `Compiler::validate`/`compile` exactly
  like the built-in ones, gated the same way (only for a document that
  declares the extension's id under `supplements:`). An extension that
  resolves external values into fault-tree probabilities implements
  `ExtensionResult::basic_event_overrides`. See
  `etdl-compiler/tests/third_party_extension_test.rs` for a complete,
  runnable example proving both phases actually execute and an override
  actually reaches generated code.

## etdl-core

- `monitor::BranchMonitor` — branch/failure recording with declared probabilities
- `retry::{RetryPolicy, BackoffStrategy}` — async retry with exponential/fixed backoff and timeout
- `sla::SlaTracker` — rolling-window anomaly detection (`ETDL_SLA_WINDOW`, `ETDL_SLA_THRESHOLD`)
- `chaos::ChaosController` — seeded, scoped failure injection, production guard (`ETDL_CHAOS`, `ETDL_CHAOS_SEED`, `ETDL_CHAOS_SCOPE`, `ETDL_ENV`)
- `telemetry` — `inject_traceparent` W3C trace context, anomaly events, node span attributes

## etdl-cli

The `etdl` binary — see [CLI reference](cli.md).

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

