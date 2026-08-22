# etdl-wasm

[![Crates.io](https://img.shields.io/crates/v/etdl-wasm.svg)](https://crates.io/crates/etdl-wasm)
[![Docs.rs](https://img.shields.io/docsrs/etdl-wasm)](https://docs.rs/etdl-wasm)

**Browser and Node.js bindings for [ETDL](https://github.com/ETDL-lang/etdl)**, via `wasm-bindgen` — validate, parse, and get editor-grade language intelligence for `.etdl` documents (event trees per IEC 62502, fault trees per IEC 61025) without a CLI, a server, or a network round-trip.

## What it exposes

| Function | Purpose |
|---|---|
| `validate_etdl(content, asyncapi_files_json)` | Full structural + semantic validation, same diagnostics `etdl-cli` produces |
| `parse_for_diagram(content)` | A diagram-ready graph structure (nodes/edges) for interactive event-tree/fault-tree visualization |
| `parse_for_raaml(content)` | RAAML-shaped output for SysML/safety-analysis tooling interop |
| `parse_with_spans` / `find_span` | Source-position-aware parsing for editor tooling |
| `complete`, `hover`, `goto_definition`, `find_references`, `document_symbols`, `format` | Editor language-server-style operations — autocomplete, hover info, go-to-definition, references, symbol outline, formatting |
| `version()` | The compiled binary's version, for cache-busting / compatibility checks |

## Why WASM, specifically

`etdl-wasm` links [`etdl-parser`](https://crates.io/crates/etdl-parser) and [`etdl-compiler`](https://crates.io/crates/etdl-compiler) directly and compiles them to a `cdylib` — the same parsing/validation logic the CLI uses, running client-side. This is what powers the [ETDL VS Code extension](https://github.com/ETDL-lang/etdl-vscode)'s live validation and diagram view: instant feedback with no CLI installed, no server round-trip.

It's also the closest existing template for a future browser-based JavaScript/TypeScript code-generation target — see [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md)'s "Future targets" section.

## Building

```bash
wasm-pack build --target web    # or --target nodejs
```

## License

Apache-2.0
