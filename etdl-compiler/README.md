# etdl-compiler

[![Crates.io](https://img.shields.io/crates/v/etdl-compiler.svg)](https://crates.io/crates/etdl-compiler)
[![Docs.rs](https://img.shields.io/docsrs/etdl-compiler)](https://docs.rs/etdl-compiler)

**The [ETDL](https://github.com/ETDL-lang/etdl) compiler pipeline** — semantic validation, IEC 61025 fault-tree probability resolution, ECEL type-checking, and the target-agnostic `CodeGenerator` trait (plus the built-in `rust` target) that turns a validated `.etdl` document into generated code.

## Pipeline

1. **`validate::validate_document`** — structural and semantic diagnostics: **E-1xx** document structure, **V-1xx** info integrity, **V-2xx** type checking/probability ranges/branch sums, **V-3xx** event-tree topology, **V-4xx** fault-tree correctness (gate arity, cycles), **V-5xx** codegen preconditions, **W-4xx** warnings.
2. **`fault_tree::resolve_fault_trees`** — exact top-event probability evaluation (AND/OR/NOT/XOR/K-of-N gates, exponential failure model, MOCUS minimal cut sets) — a fault tree's failure probability becomes a build-time-computed constant, not a runtime guess.
3. **`typeck`** — ECEL condition type-checking against resolved AsyncAPI message schemas.
4. **`codegen::CodeGenerator`** — the pluggable target trait every code-generation backend implements (see below).

## The target trait

```rust
pub trait CodeGenerator {
    fn target_name(&self) -> &'static str;
    fn generate_all(&self, doc: &EtlDocument, fault_tree_probs: &BTreeMap<String, f64>,
                     registry: &AsyncApiRegistry, stem: &str, diagnostics: &mut Vec<Diagnostic>)
        -> Result<Vec<GeneratedFile>, String>;
}
```

Every target consumes the *same* validated document + resolved fault-tree probabilities computed once, here — no target re-parses `.etdl`, re-validates ECEL conditions, or re-evaluates fault trees. This crate ships `RustCodeGenerator` (the `rust` target: `async fn handle_<event>` functions built on [`etdl-core`](https://crates.io/crates/etdl-core)); [`etdl-target-java`](https://crates.io/crates/etdl-target-java), [`etdl-target-python`](https://crates.io/crates/etdl-target-python), [`etdl-target-go`](https://crates.io/crates/etdl-target-go), and [`etdl-target-dotnet`](https://crates.io/crates/etdl-target-dotnet) implement the same trait as separate, optional crates.

## Reliability

The `reliability` Cargo feature (default-on) wires in [`etdl-reliability-core`](https://crates.io/crates/etdl-reliability-core), the small, deterministic, WASM-compatible reliability layer this compiler depends on directly. Richer reliability engineering (statistical estimation, Bayesian analysis, evidence, ontology, failure discovery) lives in separate optional crates this compiler does *not* depend on — see [`etdl-reliability`](https://crates.io/crates/etdl-reliability).

## Example

```rust
use etdl_compiler::Compiler;
use etdl_parser::{parse_document_from_file, load_asyncapi_imports};
use std::path::Path;

let base = Path::new(".");
let doc = parse_document_from_file(&base.join("order-fulfillment.etdl"))?;
let registry = load_asyncapi_imports(&doc, base)?;
let result = Compiler::new().compile(&doc, &registry);
assert!(result.diagnostics.iter().all(|d| !d.is_error()));
assert!(result.rust_output.is_some());
# Ok::<(), String>(())
```

Full architecture (pipeline, codegen contract, target registry): [`docs/architecture.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture.md) and [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md).

## License

Apache-2.0
