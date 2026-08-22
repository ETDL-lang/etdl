# etdl-target-go

[![Crates.io](https://img.shields.io/crates/v/etdl-target-go.svg)](https://crates.io/crates/etdl-target-go)
[![Docs.rs](https://img.shields.io/docsrs/etdl-target-go)](https://docs.rs/etdl-target-go)

**The Go developer API for [ETDL](https://github.com/ETDL-lang/etdl).** Generates a Go `struct`/`interface`/orchestration-function surface from a validated `.etdl` document, bound to the compiled Rust ETDL runtime via **`cgo`** against [`etdl-runtime-ffi`](https://crates.io/crates/etdl-runtime-ffi)'s C ABI. This crate is pure Rust: it never re-implements branch/SLA accounting, retry backoff, or ECEL evaluation in Go — every one of those calls through to the same implementation the Rust target itself uses.

## Building this crate never needs a Go toolchain

`etdl-target-go` only emits Go source text (plus a `go.mod`) — it's a plain Rust library used by [`etdl-cli`](https://crates.io/crates/etdl-cli) (`etdl compile --target go`) or directly via its `GoCodeGenerator`. A Go toolchain with `cgo` enabled is needed only to build/run the *generated* code, and a built `etdl-runtime-ffi` (library + `include/etdl_runtime.h`) is needed only for anything that touches `BranchMonitor`/`RetryPolicy`/`Condition`.

## What it generates

```
etdl/runtime/
    native.go           # cgo bindings to libetdl_runtime_ffi (+ include/etdl_runtime.h)
    branch_monitor.go    # thin facade — delegates to the native runtime
    retry_policy.go        # thin facade — native attempt/backoff loop via runtime/cgo.Handle
    condition.go             # thin facade — native regex/set-membership evaluation
    errors.go
    publisher.go               # developer-implemented interface (consequence: send)
<package>/
    messages.go                   # one struct per referenced AsyncAPI message
    <tree>_handlers.go              # generated interface — implement this, don't edit it
    workflow.go                        # generated orchestration + fault-tree probability constants
go.mod                                    # self-contained, buildable standalone
```

The `etdl/runtime/*.go` and `workflow.go`/`*_handlers.go` files are regenerated on every compile and marked `DO NOT EDIT DIRECTLY`. Your own code implements the generated handler interface and `Publisher` in separate, hand-written files that are never touched by regeneration.

## Usage

```bash
etdl compile order-fulfillment.etdl --target go --out-dir ./generated
cargo build -p etdl-runtime-ffi --release   # the native runtime the generated code binds to

CGO_CFLAGS="-I/path/to/etdl-runtime-ffi/include" \
CGO_LDFLAGS="-L/path/to/target/release" \
LD_LIBRARY_PATH="/path/to/target/release" \
  go build ./...
```

## Known limitation

This target's generated `cgo` code follows well-documented idioms (`runtime/cgo.Handle` for the retry callback, a small C shim so cgo never has to marshal an anonymous C function-pointer type) but **has not been compiled against a real `go` toolchain** in this project's own development environment — no Go toolchain was available. The `go_build_*` tests in `tests/go_generation.rs` exist to verify it the moment one is; they currently skip with an explicit message rather than silently pass. See the crate's module-level doc comment for the specific line most likely to need a one-word fix (`C.bool` vs. the documented fallback `C._Bool`).

Full architecture: [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md).

## License

Apache-2.0
