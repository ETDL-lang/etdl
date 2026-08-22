# etdl-target-java

[![Crates.io](https://img.shields.io/crates/v/etdl-target-java.svg)](https://crates.io/crates/etdl-target-java)
[![Docs.rs](https://img.shields.io/docsrs/etdl-target-java)](https://docs.rs/etdl-target-java)

**The Java developer API for [ETDL](https://github.com/ETDL-lang/etdl).** Generates a Java `record`/`interface`/orchestration-class surface from a validated `.etdl` document, bound to the compiled Rust ETDL runtime via **Java 21's `java.lang.foreign`** (the Foreign Function & Memory API) — no JNI glue code, no per-language native crate. This crate is pure Rust: it never re-implements branch/SLA accounting, retry backoff, or ECEL evaluation in Java — every one of those calls through to [`etdl-runtime-ffi`](https://crates.io/crates/etdl-runtime-ffi), the same implementation the Rust target itself uses.

## Building this crate never needs a JDK

`etdl-target-java` only emits Java source text — it's a plain Rust library used by [`etdl-cli`](https://crates.io/crates/etdl-cli) (`etdl compile --target java`) or directly via its `JavaCodeGenerator`. A JDK 21+ is needed only to compile/run the *generated* Java, and a built `etdl-runtime-ffi` is needed only to run anything that touches `BranchMonitor`/`RetryPolicy`/`Condition`.

## What it generates

```
etdl/runtime/
    EtdlNative.java        # java.lang.foreign MethodHandle bindings to libetdl_runtime_ffi
    BranchMonitor.java      # thin facade — delegates to the native runtime
    RetryPolicy.java        # thin facade — native attempt/backoff loop via an upcall stub
    Condition.java          # thin facade — native regex/set-membership evaluation
    WorkflowError.java
    Publisher.java           # developer-implemented interface (consequence: send)
    BackoffStrategy.java
<package>/
    <Type>.java              # one record per referenced AsyncAPI message
    <Tree>Handlers.java       # generated interface — implement this, don't edit it
    <Stem>Workflow.java        # generated orchestration + fault-tree probability constants
```

The `etdl/runtime/*.java` and `<Stem>Workflow.java`/`<Tree>Handlers.java` files are regenerated on every compile and marked `DO NOT EDIT DIRECTLY`. Your own code implements the generated `<Tree>Handlers` interface and `Publisher` in separate, hand-written classes that are never touched by regeneration.

## Usage

```bash
etdl compile order-fulfillment.etdl --target java --out-dir ./generated
cargo build -p etdl-runtime-ffi --release   # the native runtime the generated code binds to

javac --release 21 --enable-preview -d out $(find ./generated -name '*.java')
java --enable-preview --enable-native-access=ALL-UNNAMED \
     -Detdl.runtime.library=/path/to/libetdl_runtime_ffi.so \
     -cp out your.package.Main
```

`--enable-preview` is only required on JDK 21 (the FFM API is finalized, flag-free, from JDK 22 onward).

## Verified, not just generated

This crate's own test suite compiles hand-authored `Handlers`/`Publisher` implementations against generated output and runs it against a real, compiled `etdl-runtime-ffi` — including the native retry callback and RE2 regex evaluation through `etdl_core::condition::matches`. See `tests/java_runtime_integration.rs`.

Full architecture: [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md).

## License

Apache-2.0
