# Contributing to ETDL

## Building from source

Prerequisites:

- A stable Rust toolchain (see `.github/workflows/ci.yml` for what CI uses —
  no pinned MSRV; `dtolnay/rust-toolchain@stable` tracks current stable)
- `wasm32-unknown-unknown` target, only if you're touching `etdl-wasm` or
  `etdl-supplement-sdk` (`rustup target add wasm32-unknown-unknown`)

Clone and build:

```bash
git clone https://github.com/ETDL-lang/etdl.git
cd etdl
cargo build --release -p etdl-cli
```

The `etdl` binary lands at `target/release/etdl`.

### Feature flags

`etdl-cli`'s default features are `reliability`, `discovery`,
`target-java`, `target-python`, `target-go`, `target-dotnet`. Dynamic,
sandboxed `.wasm` supplement plugins (`etdl install`, `etdl supplement
list/remove`) are always compiled in — not feature-gated, so no flag is
needed for them, and they pull in `wasmtime` (a full Cranelift JIT) on
every build:

```bash
# A minimal build advertising only the always-available `rust` target:
cargo build --release -p etdl-cli --no-default-features --features reliability,discovery
```

See `etdl-cli/Cargo.toml`'s `[features]` table for the full set, and
`scripts/feature-matrix.sh` for the combinations CI checks.

## Workspace layout

The workspace (`Cargo.toml`) has eleven path members. The core, always-built
crates:

| Crate | Role |
|---|---|
| `etdl-parser` | `.etdl`/ECEL/AsyncAPI parsing |
| `etdl-compiler` | Validation, fault-tree resolution, code generation |
| `etdl-core` | Runtime library generated code depends on |
| `etdl-cli` | The `etdl` binary |
| `etdl-wasm` | WASM bindings for the VS Code extension |
| `etdl-probability-core`, `etdl-tree-core`, `etdl-reliability-core` | Native layers for `std.probability`, the Generic Tree Event Supplement, and built-in reliability |
| `etdl-conformance` | The conformance / verification suite |
| `etdl-supplement-sdk` | SDK for authoring dynamic `.wasm` supplement plugins (see [docs/reference/supplement-plugins.md](docs/reference/supplement-plugins.md)) |
| `etdl-runtime-ffi` | C ABI over `etdl-core` that every non-Rust `--target` binding calls |

See [docs/reference/crates.md](docs/reference/crates.md) for the full
breakdown, including the richer reliability engine and language targets,
which have moved to their own repositories and are pulled in as git
dependencies (see the comment block in `Cargo.toml`).

## Running tests

```bash
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo test -p etdl-conformance --all-targets                        # full conformance suite
cargo test -p etdl-conformance --no-default-features --all-targets  # lean suite, no reliability feature
```

`cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets
--no-deps` must also pass — see `.github/workflows/ci.yml` for the exact
CI invocation.

## Contributing

Open an issue or pull request at
[github.com/ETDL-lang/etdl](https://github.com/ETDL-lang/etdl). Changes to
ETDL language semantics (as opposed to this reference implementation)
belong in [etdl-specification](https://github.com/ETDL-lang/etdl-specification)
instead.
