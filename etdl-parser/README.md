# etdl-parser

[![Crates.io](https://img.shields.io/crates/v/etdl-parser.svg)](https://crates.io/crates/etdl-parser)
[![Docs.rs](https://img.shields.io/docsrs/etdl-parser)](https://docs.rs/etdl-parser)

**The `.etdl` document parser for [ETDL](https://github.com/ETDL-lang/etdl)** (Event Tree Definition Language) — turns a YAML `.etdl` document into a typed AST, parses ECEL (Event-tree Condition Expression Language) barrier-branch conditions, and resolves AsyncAPI 3.0 message/channel references.

## What it does

- **Document parsing**: `serde`-based deserialization of the ETDL document schema (event trees per [IEC 62502](https://github.com/ETDL-lang/etdl-specification), fault trees per [IEC 61025](https://github.com/ETDL-lang/etdl-specification)) into `ast::EtlDocument`, with a manual `Deserialize` normalizing legacy field names and preserving `x-*` extension fields.
- **ECEL parsing**: a small `nom`-based parser for barrier-branch conditions — comparisons, `in` (set membership), `matches` (RE2-compatible regex), wildcard array quantification — producing `ecel::Condition`.
- **AsyncAPI 3.0 resolution**: loads `asyncapi_imports` documents and resolves both External References (`alias#/components/messages/Foo`) and Internal References (`#/components/messages/Foo`) via RFC 6901 JSON Pointer, normalizing both into the same `{name, payload, headers}` envelope shape so downstream code doesn't need to branch on which kind of reference it's looking at.

## Where this sits

```
.etdl document + AsyncAPI 3.0 imports
              |
              v
        etdl-parser          <- this crate: parse, don't validate semantics
              |
              v
       etdl-compiler          (structural/semantic validation, fault-tree
              |                resolution, ECEL type-checking, code generation)
              v
   etdl-cli / etdl-wasm / language targets
```

This crate deliberately stops at syntax and structure — it does not check that a referenced handler exists, that probabilities sum to 1.0, or that an ECEL condition type-checks against a message schema. That's `etdl-compiler`'s job, one layer up, so every consumer (CLI, WASM bindings, every language target) shares one parser and one semantic-validation pipeline instead of each re-deriving it.

## Example

```rust
use etdl_parser::{parse_document_from_file, load_asyncapi_imports};
use std::path::Path;

let base = Path::new(".");
let doc = parse_document_from_file(&base.join("order-fulfillment.etdl"))?;
let registry = load_asyncapi_imports(&doc, base)?;
# Ok::<(), String>(())
```

Full ETDL architecture and pipeline: [`docs/architecture.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture.md).

## License

Apache-2.0
