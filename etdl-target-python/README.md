# etdl-target-python

[![Crates.io](https://img.shields.io/crates/v/etdl-target-python.svg)](https://crates.io/crates/etdl-target-python)
[![Docs.rs](https://img.shields.io/docsrs/etdl-target-python)](https://docs.rs/etdl-target-python)

**The Python developer API for [ETDL](https://github.com/ETDL-lang/etdl).** Generates a Python `@dataclass`/`ABC`/orchestration-function surface from a validated `.etdl` document, bound to the compiled Rust ETDL runtime via the **standard library's `ctypes`** — no third-party package, no compiled Python extension module required. This crate is pure Rust: it never re-implements branch/SLA accounting, retry backoff, or ECEL evaluation in Python — every one of those calls through to [`etdl-runtime-ffi`](https://crates.io/crates/etdl-runtime-ffi), the same implementation the Rust target itself uses.

## Building this crate never needs Python

`etdl-target-python` only emits Python source text — it's a plain Rust library used by [`etdl-cli`](https://crates.io/crates/etdl-cli) (`etdl compile --target python`) or directly via its `PythonCodeGenerator`. Python 3.9+ is needed only to run the *generated* code, and a built `etdl-runtime-ffi` is needed only for anything that touches `BranchMonitor`/`RetryPolicy`/`Condition`.

## What it generates

```
etdl/runtime/
    native.py           # ctypes CDLL bindings to libetdl_runtime_ffi
    branch_monitor.py    # thin facade — delegates to the native runtime
    retry_policy.py       # thin facade — native attempt/backoff loop via a CFUNCTYPE callback
    condition.py           # thin facade — native regex/set-membership evaluation (stdlib json for marshaling)
    errors.py
    publisher.py            # developer-implemented ABC (consequence: send)
<package>/
    messages.py               # one @dataclass per referenced AsyncAPI message
    <tree>_handlers.py          # generated ABC — implement this, don't edit it
    workflow.py                   # generated orchestration + fault-tree probability constants
```

The `etdl/runtime/*.py` and `workflow.py`/`*_handlers.py` files are regenerated on every compile and marked `DO NOT EDIT DIRECTLY`. Your own code subclasses the generated handler ABC and `Publisher` in separate, hand-written modules that are never touched by regeneration.

## Usage

```bash
etdl compile order-fulfillment.etdl --target python --out-dir ./generated
cargo build -p etdl-runtime-ffi --release   # the native runtime the generated code binds to

ETDL_RUNTIME_LIBRARY=/path/to/libetdl_runtime_ffi.so \
  python3 -c "import your_package.main; your_package.main.main()"
```

## Verified, not just generated

This crate's own test suite runs hand-authored handler/publisher implementations against generated output, executed by a real `python3` against a real, compiled `etdl-runtime-ffi` — including the native retry callback (ctypes transparently reacquires the GIL for the C-to-Python call, the standard documented pattern) and RE2 regex evaluation through `etdl_core::condition::matches`. See `tests/python_runtime_integration.rs`.

Full architecture: [`docs/architecture/targets.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/targets.md).

## License

Apache-2.0
