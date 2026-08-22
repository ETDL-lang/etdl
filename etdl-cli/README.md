# etdl-cli

[![Crates.io](https://img.shields.io/crates/v/etdl-cli.svg)](https://crates.io/crates/etdl-cli)
[![Docs.rs](https://img.shields.io/docsrs/etdl-cli)](https://docs.rs/etdl-cli)

**The `etdl` command-line tool** — compile, validate, analyze, and inspect [ETDL](https://github.com/ETDL-lang/etdl) (Event Tree Definition Language) documents: event tree analysis (IEC 62502) and fault tree analysis (IEC 61025) with build-time-resolved probabilities, generating a native Rust runtime by default or a thin developer binding in Java, Python, Go, or C# on request.

## Install

```bash
cargo install etdl-cli
```

## Commands

| Command | Purpose |
|---|---|
| `etdl compile <FILE> [--target <TARGET>]` | Compile to `rust` (default), or `java`/`python`/`go`/`dotnet` (comma-separated for several at once) |
| `etdl validate <FILE...>` | Validate one or more documents/directories, no code generation |
| `etdl analyze <FILE>` | Reliability summary: fault-tree/branch counts, resolved probabilities |
| `etdl discover <PATH>` | Static analysis producing candidate failure modes mapped to the reliability ontology (`discovery` feature) |
| `etdl reliability {resolve,validate,inspect,estimate,trace}` | Resolve external reliability probabilities and their provenance (`reliability` feature) |
| `etdl stdlib {list,resolve}` | Inspect the ETDL Standard Library (built-in/optional/user resolution) |
| `etdl tree-event {validate,summarize}` | Inspect Generic Tree Event Supplement (`x-tree-event`) trees |
| `etdl capabilities` | Report which optional features this specific binary was compiled with |
| `etdl conformance {status,manifest}` | Objective per-area conformance status and the machine-readable conformance manifest |

## Optional targets, on by default

`--target java`/`python`/`go`/`dotnet` each generate a thin binding to the same compiled Rust ETDL runtime ([`etdl-runtime-ffi`](https://crates.io/crates/etdl-runtime-ffi)) — never a per-language reimplementation of branch/SLA accounting, retry backoff, or ECEL evaluation. Each is gated by its own Cargo feature (`target-java`, `target-python`, `target-go`, `target-dotnet`; all default-on, since none of the underlying generator crates need their language's toolchain *installed* merely to build this CLI). `rust` always remains available in a normal install.

```bash
etdl compile order-fulfillment.etdl --target java --out-dir ./generated
etdl compile order-fulfillment.etdl --target rust,java,python,go,dotnet --out-dir ./generated
```

Full target architecture: [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md).

## Reliability, on by default

The `reliability` feature wires in `etdl reliability ...` subcommands against the built-in [`etdl-reliability-core`](https://crates.io/crates/etdl-reliability-core) layer. The `discovery` feature adds `etdl discover`, backed by [`etdl-failure-discovery`](https://crates.io/crates/etdl-failure-discovery). Both can be disabled with `--no-default-features` for a minimal, dependency-light install.

Full command reference: [`docs/CLI.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/CLI.md).

## License

Apache-2.0
