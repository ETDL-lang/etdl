# etdl-runtime-ffi

[![Crates.io](https://img.shields.io/crates/v/etdl-runtime-ffi.svg)](https://crates.io/crates/etdl-runtime-ffi)
[![Docs.rs](https://img.shields.io/docsrs/etdl-runtime-ffi)](https://docs.rs/etdl-runtime-ffi)

**The stable C ABI over `etdl-core`.** This is the one native surface every non-Rust [ETDL](https://github.com/ETDL-lang/etdl) language target — Java, Python, Go, .NET, and future targets — binds to. It is not a second runtime: every exported function is a thin, panic-safe wrapper directly around the same `etdl_core::BranchMonitor`, `RetryPolicy`, and `condition` module the Rust code generator's `async fn handle_<event>` output has always used.

## Why this crate exists

ETDL compiles reliability models (event trees per IEC 62502, fault trees per IEC 61025) into code. The Rust target links `etdl-core` directly — no FFI needed, it's already Rust. Every other language target needs a way to call into that *same* authoritative implementation instead of re-implementing branch/SLA accounting, retry backoff sequencing, and ECEL `matches`/`in` evaluation in each new language. `etdl-runtime-ffi` is that boundary: build once, bind from anywhere with a C-compatible FFI mechanism.

## What it exposes

| Function family | Purpose |
|---|---|
| `etdl_branch_monitor_{new,free,record_branch,record_success,record_failure}` | Branch/SLA observation — the FFI face of `etdl_core::BranchMonitor` |
| `etdl_retry_policy_{new,free,delay_ms,execute}` | Authoritative attempt-count/backoff sequencing, driven via a callback so the retry *loop* is computed once, in Rust, no matter which language is retrying |
| `etdl_condition_{matches,contains}` | ECEL `matches` (RE2-compatible regex) / `in` (set membership) — identical semantics in every binding, because it's the same regex engine either way |
| `etdl_runtime_{version,abi_version}` | Diagnostics + ABI-compatibility check (`ETDL_RUNTIME_ABI_VERSION`) |
| `etdl_last_error_message`, `etdl_string_free`, `etdl_set_log_callback` | Error reporting and a panic-safe one-way notification callback |

Design rules the whole surface follows: opaque handles only (no Rust memory layout crosses the boundary), every function is wrapped in `catch_unwind` (no Rust panic can ever cross into C/Java/Python/Go/.NET), and the smallest set of operations a real binding actually needs — nothing spec'd speculatively.

## Building

```bash
cargo build -p etdl-runtime-ffi --release
```

Produces `libetdl_runtime_ffi.{so,dylib}` / `etdl_runtime_ffi.dll` (dynamic), `libetdl_runtime_ffi.a` (static), and regenerates `include/etdl_runtime.h` via `cbindgen` on every build — the header can never drift from the actual exported functions. No JDK/Python/Go/.NET SDK is needed to build this crate; those are only needed to build/run code that *binds* to it.

## Who binds to this

- [`etdl-target-java`](https://crates.io/crates/etdl-target-java) — `java.lang.foreign`
- [`etdl-target-python`](https://crates.io/crates/etdl-target-python) — `ctypes`
- [`etdl-target-go`](https://crates.io/crates/etdl-target-go) — `cgo`
- [`etdl-target-dotnet`](https://crates.io/crates/etdl-target-dotnet) — `LibraryImport`/`UnmanagedCallersOnly`

Full design (callback contract, ownership/threading/panic-safety model, ABI versioning) in [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md).

## License

Apache-2.0
