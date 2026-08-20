> **2026-08-19 update:** the four P0 blockers this scorecard's evidence is
> keyed to (P0-A/B/C/D) are now fixed, with regression tests — see
> `docs/RELEASE_READINESS_1.0.md` for the current scorecard and evidence.
> The table below is the historical snapshot at the time it was written.

# ETDL Readiness Scorecard — Current Truth

Scored 0–5 (0 = absent, 1 = prototype, 2 = functional, 3 = reliable,
4 = production-ready, 5 = ecosystem/industry-ready). Every score has evidence
from `docs/CURRENT_READINESS_AUDIT.md`. Scores reflect the implementation as it
is, not as claimed.

| Area | Score | Evidence | Blocker? |
|---|---|---|---|
| Specification | 4 | Complete (all 5 appendices present), gate/eventType/transfer math consistent with compiler; worked example byte-identical; honest IEC framing. Residual: §7/Appendix B text drift, `any()/all()` grammar hole, house-event W-406 tension, missionTime units, §9.3/9.4 ambiguity. | P1-M, P1-N (spec) |
| Conformance | 3 | 10-case declarative suite + runner, CI-wired. Thin: no NOT/XOR/INHIBIT/PRIORITY_AND/hetero-VOTING/E-103/104/V-101/201/202/204/301/302/401-405/501/503/504/505/W-*. Doc describes corpus layout that doesn't exist. | P2-K |
| Parser | 4 | 39 tests; overflow-safe ECEL index; path-traversal guard; unknown top-level field rejection; span index robust (proptest). Nested unknown fields silently ignored; MAJOR-0 accepted. | P2-N |
| Compiler | 3 | Full pipeline; generated code compiles (harness); deterministic; 101 tests. But: V-204 dead (P0-B), negative λ unvalidated (P0-C), `in`/`matches` codegen broken (P0-D), unbounded recursion (P1-C). | P0-B/C/D |
| Probability engine | 3 | Formulas match spec; overflow-proof math; worked example exact. But negative/NaN λ flows un-clamped; AND/OR/XOR/INHIBIT not clamped; SLA failure anomaly always fires (P0-A). | P0-A, P0-C |
| Fault-tree analysis | 4 | All gates tested incl. hetero-VOTING, INHIBIT, PRIORITY_AND; MOCUS capped; deterministic; docs. Negative-λ gap lives in basic-event conversion (P0-C). | P0-C |
| Event-tree analysis | 4 | V-101..V-104, V-2xx/V-3xx enforced; termination/cycle checks; deterministic; tests. | — |
| ECEL | 2 | Grammar + parser + `in`/`matches` lowering exist; BUT type-checking (V-204) never fires for path operands (P0-B), and `in`/`matches` codegen doesn't compile (P0-D). The "type-checked contracts" claim is unmet. | P0-B, P0-D |
| AsyncAPI | 3 | Resolution, `../` guard, E-103/104, load_from_content; but schema introspection returns `None` for `message.payload.*` (drives P0-B), no `$ref`/`allOf`/`oneOf`/`enum` handling, no size/depth limits. | P0-B |
| Runtime | 3 | Retry panic-safe, traceparent valid, chaos safe-by-default, deterministic. But failure-SLA always alarms (P0-A), mutex poison panics (P1-D), chaos env snapshotted (P2-B), telemetry stderr-only. | P0-A |
| CLI | 4 | `--json`/`--quiet`/`--verbose`/`analyze`/directory; exit codes tested; deterministic. Non-recursive dir scan; analyze drops FT errors. | P2-C, P2-D |
| WASM | 3 | 12 exports incl. LSP endpoints; deterministic; version 0.2.0. 0 in-crate tests (P2-J); E-100 reuse for YAML errors (P1-J); no version-compat contract beyond `version()`. | P1-J, P2-J |
| VS Code | 2 | Diagnostics with real positions; RAAML+Mermaid viz (INHIBIT/PRIORITY_AND/house/undeveloped supported); 13 tests. BUT 5 of 6 IntelliSense features broken (P1-A), packaging likely broken (P1-F), `enable` setting inert (P1-G), perf (P2-F), no click-to-jump (P2-G). | P1-A, P1-F |
| Documentation | 3 | Broad and honest (developer/architect/business, semantics, do-not-overclaim). But README + several docs show removed APIs/versions and a non-validating example (P1-P, P2-M); type-check claim overstated (P0-B). | P1-P |
| Security | 3 | SECURITY.md, `../` guard, no-panic-on-input (ECEL), cargo-audit in CI. Residual: unbounded recursion (P1-C), no depth/size DoS limits (P3-E), negative-λ invalid output (P0-C). | P1-C |
| Performance | 3 | Baselines in `docs/PERFORMANCE.md` (~77µs parse, ~6µs validate). No large-doc scaling bench; recursive DAG traversal risk (P1-C). | P1-C |
| Developer Experience | 3 | 10-minute path documented; business demos; CLI clean; generated code compiles. But extension IntelliSense broken, generated `in`/`matches` fails compile, README example doesn't validate. | P0-D, P1-A |
| Open Source | 4 | Apache-2.0; spec CC BY 4.0; conformance public; company strategy out of public repo; contributing/security docs. | — |
| Ecosystem | 2 | Extension points documented (CodeGenerator trait, WASM endpoints, conformance). No third-party runners/backends; WASM LSP shape undocumented for consumers (the extension itself is the only consumer and it's broken). | P1-A |
| Business Positioning | 3 | Honest positioning + do-not-overclaim; 10 demos; commercial boundary private. Business value story is sound but reliability numbers can be silently wrong (P0-A/C) and type-check claim unmet (P0-B) — weakens trust basis. | P0-A/B/C |

**Overall: 3/5 — reliable prototype, not production-ready.**

## Honest bottom line

- **Strong:** parser, fault-tree/event-tree math correctness, determinism,
  runtime panic-safety, CLI, docs breadth, honesty posture, open-source hygiene.
- **Blocking production readiness:** failure-SLA false alarms (P0-A), dead
  ECEL type-checking (P0-B), negative-probability emission (P0-C), uncompilable
  `in`/`matches` codegen (P0-D).
- **Blocking developer-acquisition readiness:** 5 broken IntelliSense features
  (P1-A) and likely-broken extension packaging (P1-F).
- **Not yet credible for:** "catches type errors at build time," "probability-
  driven SLA anomaly detection," "compiles any valid model" — until the P0s are
  closed with tests.
