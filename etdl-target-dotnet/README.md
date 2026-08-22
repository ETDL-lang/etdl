# etdl-target-dotnet

[![Crates.io](https://img.shields.io/crates/v/etdl-target-dotnet.svg)](https://crates.io/crates/etdl-target-dotnet)
[![Docs.rs](https://img.shields.io/docsrs/etdl-target-dotnet)](https://docs.rs/etdl-target-dotnet)

**The .NET/C# developer API for [ETDL](https://github.com/ETDL-lang/etdl).** Generates a C# `record`/`interface`/orchestration-class surface from a validated `.etdl` document, bound to the compiled Rust ETDL runtime via modern, source-generated P/Invoke (**`[LibraryImport]`** + **`[UnmanagedCallersOnly]`**, .NET 7+) against [`etdl-runtime-ffi`](https://crates.io/crates/etdl-runtime-ffi)'s C ABI. This crate is pure Rust: it never re-implements branch/SLA accounting, retry backoff, or ECEL evaluation in C# — every one of those calls through to the same implementation the Rust target itself uses.

## Building this crate never needs a .NET SDK

`etdl-target-dotnet` only emits C# source text (plus a `.csproj`) — it's a plain Rust library used by [`etdl-cli`](https://crates.io/crates/etdl-cli) (`etdl compile --target dotnet`) or directly via its `DotnetCodeGenerator`. A .NET SDK 8+ is needed only to build/run the *generated* code, and a built `etdl-runtime-ffi` is needed only for anything that touches `BranchMonitor`/`RetryPolicy`/`Condition`.

## What it generates

```
Etdl/Runtime/
    NativeMethods.cs      # [LibraryImport] bindings to libetdl_runtime_ffi
    BranchMonitor.cs        # thin facade — delegates to the native runtime
    RetryPolicy.cs             # thin facade — native attempt/backoff loop via [UnmanagedCallersOnly] + GCHandle
    Condition.cs                  # thin facade — native regex/set-membership evaluation (System.Text.Json for marshaling)
    WorkflowError.cs
    IPublisher.cs                    # developer-implemented interface (consequence: send)
    BackoffStrategy.cs
<Namespace>/
    Messages.cs                        # one record per referenced AsyncAPI message
    I<Tree>Handlers.cs                    # generated interface — implement this, don't edit it
    <Stem>Workflow.cs                        # generated orchestration + fault-tree probability constants
<Stem>.csproj                                   # targets net9.0 by default, self-contained
```

The `Etdl/Runtime/*.cs` and `<Stem>Workflow.cs`/`I<Tree>Handlers.cs` files are regenerated on every compile and marked `DO NOT EDIT DIRECTLY`. Your own code implements the generated interface and `IPublisher` in separate, hand-written classes that are never touched by regeneration.

## Usage

```bash
etdl compile order-fulfillment.etdl --target dotnet --out-dir ./generated
cargo build -p etdl-runtime-ffi --release   # the native runtime the generated code binds to

ETDL_RUNTIME_LIBRARY=/path/to/libetdl_runtime_ffi.so \
  dotnet run
```

The generated `.csproj` targets `net9.0` by default — targeting a framework version other than your installed SDK's own major version makes NuGet download that version's runtime packs on first build (slow, not incorrect); change the one line if your SDK differs.

## Verified, not just generated

This crate's own test suite compiles hand-authored interface implementations against generated output with `dotnet build`/`dotnet run`, executed against a real, compiled `etdl-runtime-ffi` — including the native retry callback (`[UnmanagedCallersOnly]` + `GCHandle`, the standard .NET idiom for a static native callback needing per-call managed state) and RE2 regex evaluation through `etdl_core::condition::matches`. See `tests/dotnet_runtime_integration.rs`.

Full architecture: [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md).

## License

Apache-2.0
