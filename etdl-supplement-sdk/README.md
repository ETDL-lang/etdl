# etdl-supplement-sdk

SDK for writing ETDL supplement plugins — dynamically loaded, sandboxed
`wasm32-unknown-unknown` modules `etdl-cli` runs via `etdl supplement
install` (see `etdl-compiler`'s `WasmExtension` host adapter).

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

A plugin runs sandboxed: no filesystem, network, or clock access, and
under a `wasmtime` fuel limit. It sees the parsed document as
`serde_json::Value` (not `etdl_parser::ast::EtlDocument`) precisely so
this crate never depends on `etdl-parser` — see `src/lib.rs`'s module doc
for why.

For non-Rust plugin authors, the raw wire ABI this macro generates is
documented in `docs/reference/supplement-plugins.md` in the main `etdl`
repository.
