# ETDL API Stability

This document defines the **public** API surface of the ETDL crates and the
compatibility guarantees that apply to it. It separates PUBLIC API from INTERNAL
implementation details so that consumers know what they can rely on.

## Versioning policy

All ETDL crates share the workspace version (currently 0.2.x, pre-1.0). Under
Cargo semver semantics for 0.x:

- A **minor** bump (0.2 → 0.3) may include breaking changes, but the project
  treats it as the correct place for deliberate API evolution.
- A **patch** bump (0.2.1 → 0.2.2) is backward-compatible.

This is distinct from the ETDL *language* version (the `etdl:` field in a
document, currently `1.0.0` — see `docs/VERSIONING.md`), which is already
stable and unrelated to the crate version. A future 1.0.0 crate release will
freeze the public API described here.

## Crate dependency graph

```
etdl-parser   (no etdl deps)
etdl-compiler -> etdl-parser
etdl-cli      -> etdl-parser, etdl-compiler
etdl-wasm     -> etdl-parser, etdl-compiler
etdl-core     (no etdl deps)  [runtime; depends on serde/serde_json/tokio/regex]
```

## etdl-parser — PUBLIC API

| Item | Status |
|---|---|
| `parse_document(&str) -> Result<EtlDocument, String>` | STABLE |
| `parse_document_from_file(&Path) -> Result<EtlDocument, String>` | STABLE |
| `load_asyncapi_imports(&EtlDocument, &Path) -> Result<AsyncApiRegistry, String>` | STABLE |
| `ast::EtlDocument`, `Info`, `EventTree`, `InitiatingEvent`, `Node`, `Barrier`, `Branch`, `Operation`, `Consequence`, `FaultTree`, `TopEvent`, `Gate`, `GateType`, `BasicEvent`, `BasicEventType`, `TransferNode`, `RetryPolicy`, `BackoffStrategy` | STABLE (serialize/deserialize shapes) |
| `ecel::{parse_condition, Condition, Comparison, Operand, PathExpr, PathSegment, Comparator, Literal}` | STABLE |
| `asyncapi::AsyncApiRegistry::{new, load, load_from_content, resolve, resolve_ref, get_schema_for_path}` | STABLE |
| `jsonptr::resolve_json_pointer` | STABLE |
| `semantic::{document_symbols, hover, goto_definition, find_references, complete, format}` | EXPERIMENTAL (LSP endpoints) |
| `spanned::{parse_document_with_spans, build_span_index, inject_spans, detect_duplicate_ids, Span, SpanKey, ...}` | EXPERIMENTAL (IDE support) |

## etdl-compiler — PUBLIC API

| Item | Status |
|---|---|
| `Compiler::{new, validate, compile}` | STABLE |
| `CompilationResult { diagnostics, rust_output }` | STABLE |
| `validate::Diagnostic`, `DiagnosticSeverity` | STABLE |
| `validate::{validate_document, resolve_probability_links, validate_probability_sums}` | STABLE |
| `fault_tree::{resolve_fault_trees, enumerate_minimal_cut_sets, MAX_CUT_SET_ROWS}` | STABLE (`enumerate_minimal_cut_sets` is a SHOULD-level tool) |
| `codegen::{CodeGenerator, RustCodeGenerator}` | STABLE |
| `typeck::type_check_conditions` | INTERNAL (private module) |

## etdl-core — PUBLIC API

| Item | Status |
|---|---|
| `BranchMonitor` | STABLE |
| `RetryPolicy`, `BackoffStrategy`, `RetryError` | STABLE |
| `SlaTracker` | STABLE |
| `ChaosController` | STABLE |
| `Publisher`, `NoopPublisher`, `ChannelCapturingPublisher`, `PublishError` | STABLE |
| `condition::{contains, matches}` | STABLE |
| `telemetry::{inject_traceparent, Error, WorkflowError, attach_node_span_attribute, emit_anomaly_event}` | STABLE (interface), EXPERIMENTAL (stderr sink) |
| `serde_json` re-export | STABLE (for generated code) |

## Stability statuses

| Status | Meaning |
|---|---|
| STABLE | API is relied upon; changes only via minor/breaking releases with migration notes. |
| EXPERIMENTAL | May change without notice; used by the IDE/WASM layer. |
| DEPRECATED | Keep working ≥ 1 major cycle; prefer replacement (e.g. `eventTree`). |
| INTERNAL | Not part of the public contract; can change at any time. |

## Language-level stability (the `.etdl` format)

The language version in a document's `etdl` field is governed by
`docs/VERSIONING.md`. The parser:

- accepts any document whose MAJOR matches the compiler's supported MAJOR (1),
- rejects unimplemented future MAJORs (`E-100`),
- preserves `x-*` extension fields,
- rejects unknown non-`x-` fields at parse time.

## Backward compatibility notes

- Adding a field to an AST struct is a MINOR (additive) change when it has a
  `#[serde(default)]`.
- Changing the Rust backend's generated-code contract (e.g. the handler
  signature) is a MINOR/breaking change tracked in the changelog; the generated
  output is validated by the compile-check harness.
- Diagnostic codes are stable within a MAJOR version (see `DIAGNOSTICS.md`).
