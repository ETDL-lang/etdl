# etdl-conformance

[![Crates.io](https://img.shields.io/crates/v/etdl-conformance.svg)](https://crates.io/crates/etdl-conformance)
[![Docs.rs](https://img.shields.io/docsrs/etdl-conformance)](https://docs.rs/etdl-conformance)

**ETDL Conformance, Verification & Validation 1.0** — answers *"how do we know an ETDL implementation actually implements the specification and supplements correctly?"* with explicit conformance levels, normative test vectors, a mathematical reference oracle independent of the implementation under test, and a machine-readable conformance manifest/report.

## What it provides

- **`vector::Level`** — explicit conformance levels (0/1: parser + compiler + fault-tree probability; 2–7: standard library, supplements — Generic Tree Event, Reliability, Predictive Reliability, Runtime Feedback & Calibration —, artifact/serialization, runtime behavior, WASM).
- **`vector::ConformanceVector`** — normative test vectors per level.
- **`reference`** — an oracle coded **independently** of every crate under test. A vector that only re-asserts what the implementation itself computed proves nothing; every numerical vector here compares against either `reference`'s own computation or a hand-derived textbook constant, never a second call into the same formula.
- **`manifest::ConformanceManifest`** / **`report`** — machine-readable output consumed by [`etdl-cli`](https://crates.io/crates/etdl-cli)'s `etdl conformance status`/`etdl conformance manifest`.
- **`depgraph`** — models which conformance areas depend on which, so a gap in one area's status is traceable to its root cause.

## This extends, not replaces

Levels 0/1 (parser + compiler + fault-tree probability, 12 cases) predate this crate and live in `etdl-compiler/tests/conformance_test.rs`, unchanged. This crate adds levels 2–7 plus the manifest/report/dependency-graph infrastructure none of the existing suites provided.

## No self-certification loop

`reference` never calls back into [`etdl-compiler`](https://crates.io/crates/etdl-compiler), [`etdl-core`](https://crates.io/crates/etdl-core), or any of the reliability crates it's checking — its numbers come from an independently-coded implementation of the same math, so a bug shared between the implementation and its own test oracle can't hide.

Full methodology: [`docs/reference/conformance-framework.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/reference/conformance-framework.md); how this relates to the pre-existing Level 0/1 suite: [`docs/CONFORMANCE.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/CONFORMANCE.md).

## License

Apache-2.0
