# ETDL Specification ↔ Implementation Matrix

**Specification:** ETDL v1.0.0 (`github.com/ETDL-lang/etdl-specification`)
**Implementation:** workspace 0.2.2 (`github.com/ETDL-lang/etdl`)
**Status:** Phase 0 audit (core specification only — see
[`docs/conformance/supplement-traceability-matrix.md`](conformance/supplement-traceability-matrix.md)
for everything built since as a supplement rather than a core-spec change)

Status legend:
- **IMPLEMENTED + TESTED** — implemented and covered by an automated test
- **IMPLEMENTED** — implemented; coverage to be added
- **PARTIAL** — partially implemented (noted)
- **NOT IMPLEMENTED** — absent
- **NOT IMPLEMENTED (documented)** — explicitly documented as out of scope
- **SPEC GAP** — the specification itself is ambiguous/absent; needs a spec change

Severity: P0/P1/P2 (see `READINESS_AUDIT.md`).

---

## 2. Conformance & notation

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| 2.3 Conforming Document validates against Appendix E JSON Schema | SPEC GAP | — | — | Appendix E absent from spec | JSON Schema must be authored | P0 |
| 2.3 Conforming Parser accepts/rejects per Section 7 | PARTIAL | `validate.rs` | partial | ~30 codes implemented; see rows below | many | P0 |
| 2.3 Conforming Compiler satisfies 8.2/8.3 for ≥1 language | NOT IMPLEMENTED | `codegen/rust.rs` | no | generated Rust does not compile | P0-1 | P0 |

## 4. File format

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| 4.1 `.etdl` is YAML 1.2 (JSON included) | IMPLEMENTED | `parse_document` (serde_yaml) | `integration_test` | — | — | — |
| 4.1 duplicate YAML keys rejected (SHOULD) | NOT IMPLEMENTED | — | no | serde_yaml default silently keeps last | SHOULD | P1 |
| 4.3 UTF-8 without BOM; reject BOM with E-102 | PARTIAL | `lib.rs:52-61` | no | BOM rejected but as `Err(String)`, not a `Diagnostic` | E-102 channel | P2 |

## 5. Document structure

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| 5.1 `etdl`, `info`, `asyncapi_imports` REQUIRED | IMPLEMENTED | `ast.rs` manual Deserialize | parse test | missing → parse error | — | — |
| 5.1 exactly one of `eventTrees`/`eventTree` | IMPLEMENTED | `ast.rs:80-107` | parse test | both → error; legacy accepted silently | deprecation advisory absent | P2 |
| 5.2 `domain` MUST match `^[A-Za-z][A-Za-z0-9]*$` | IMPLEMENTED | `ast.rs:141-163` | no | enforced at deserialize | — | P1 (test) |
| 5.3 External/Internal reference resolution | IMPLEMENTED | `validate.rs`, `jsonptr.rs` | partial | E-103/E-104/E-105 | — | — |
| 5.3.3 Internal refs other than `#/faultTrees/<id>/topEvent` | PARTIAL | `validate.rs:214` | — | treated as error per E-105 | spec tension (5.3.2 "no defined meaning" vs E-105) | P2 (spec) |
| 5.4 `components` object | PARTIAL | parsed only | no | never used by validate/typeck/codegen | feature gap | P1 |
| 5.5 Event Tree: `initiatingEvent`, `nodes` | IMPLEMENTED | `ast.rs:178` | parse test | — | — | — |
| 5.6 InitiatingEvent REQUIRED fields | IMPLEMENTED | `ast.rs:187` | parse test | — | — | — |
| 5.7 Node IDs unique within a tree | IMPLEMENTED | `spanned.rs` detect_duplicate_ids → V-001 | spanned test | V-001 (Warning) | severity is Warning not Error (spec: MUST be unique) | P1 |
| 5.8.1 Branch must supply one of probability/probabilityOfSuccess/probabilityOfFailure/probabilitySource | IMPLEMENTED | `validate.rs:607` (V-203 no-prob) | integration | V-203 | — | — |
| 5.8.1 `probabilitySource` authoritative when present | NOT IMPLEMENTED | — | no | `probability` silently wins | spec 5.15 | P0 |
| 5.8.1 at most one `default` branch, evaluated last | IMPLEMENTED | `validate.rs:571,585` (V-202) | no | — | — | P1 (test) |
| 5.8.1 branch probabilities sum to 1.0 ±0.0001 | NOT IMPLEMENTED (dead code) | `validate.rs:1303-1320` | no | dead code; never fires | P0-4 | P0 |
| 5.9 Operation REQUIRED fields; `onFailure` omitted → propagate typed error | IMPLEMENTED | `codegen/rust.rs:414-418` | no | generates `Err(WorkflowError::new(...))` | generated code doesn't compile (P0-1) | P0 |
| 5.9.1 retryPolicy semantics | IMPLEMENTED | `retry.rs`, `codegen` | partial | Fixed/Exponential; default 1 attempt | panic on all-timeout | P0 |
| 5.10 `send` consequence requires channel+message | IMPLEMENTED | `validate.rs:664` (V-302) | no | — | — | P1 (test) |
| 5.11 FaultTree `topEvent`+`basicEvents` REQUIRED | IMPLEMENTED | `ast.rs:361` | parse test | — | — | — |
| 5.11 gate/basic-event IDs share namespace, no collision | IMPLEMENTED | `validate.rs:712` (V-402) | advanced-FT test | — | — | — |
| 5.12 `rootCause` MUST resolve | IMPLEMENTED | `validate.rs:772` (V-401) | integration | — | — | — |
| 5.13 gate arity (V-501) | IMPLEMENTED | `validate.rs:986-1120` | advanced-FT test | AND/OR≥2, NOT=1, XOR=2, VOTING≥2, INHIBIT=2, PAND≥2 | — | — |
| 5.13 VOTING `1 ≤ k ≤ n` (V-502) | IMPLEMENTED | `validate.rs:1053-1069` | no | — | — | P1 (test) |
| 5.14 BasicEvent REQUIRED fields + exactly one probability/failureRate | IMPLEMENTED | `validate.rs:1150,1164` (V-503), `:1180` (V-504) | no | — | undeveloped semantics conflict (V-503 vs W-407) | P0 |
| 5.15 FaultTree must not reference an EventTree | IMPLEMENTED | one-way resolution | — | — | — | — |
| 5.16 bottom-up topological evaluation | IMPLEMENTED | `fault_tree.rs` | integration | — | non-deterministic topo-seed order | P1 |

## 6. ECEL

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| 6.2 grammar (comparison, path-expr, operators) | IMPLEMENTED | `ecel.rs` | 6 tests | — | — | — |
| 6.2 string literals: no escapes | IMPLEMENTED | `ecel.rs:225-234` | no | — | — | P1 (test) |
| 6.3 `message.payload` / `message.headers` roots | PARTIAL | `typeck.rs` | no | schema introspection on payload only; headers unhandled | headers | P1 |
| 6.4 wildcard = universal (`all`) semantics | IMPLEMENTED | `codegen` `.iter().all()` | mermaid/gen test | — | — | — |
| 6.4 `any`/`all` quantifiers (MAY) | NOT IMPLEMENTED (documented) | — | — | grammar absent | optional per spec | P2 |
| 6.5 `==`/`!=` same runtime type | IMPLEMENTED | `typeck.rs:72-91` (V-204) | no | — | — | P1 |
| 6.5 ordering ops require number | IMPLEMENTED | `typeck.rs:95-103` (V-204) | no | — | — | P1 |
| 6.5 `in` right operand array | IMPLEMENTED | `typeck.rs:108-118` (V-204) | no | element-type compat not checked | — | P1 |
| 6.5 `matches` left string, right RE2 | PARTIAL | `typeck.rs:122-132`; `codegen` emits `matches` (invalid) | no | RE2 not enforced at runtime; codegen invalid | RE2 runtime | P0 |
| 6.7 no implicit coercion; cross-type = V-204 | IMPLEMENTED | `typeck.rs` | no | — | — | P1 |
| 6.8 pure/deterministic/bounded/sandboxed evaluator | NOT IMPLEMENTED | — | — | no runtime ECEL evaluator exists (only compile-time) | runtime eval is TS/Rust-gen concern | P1 |

## 7. Validation rules

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| E-101 grammar mismatch | IMPLEMENTED | `validate.rs` | no | — | — | P1 |
| E-102 BOM | PARTIAL | `lib.rs:57` | no | `Err(String)` not Diagnostic | — | P2 |
| E-103 unknown alias | IMPLEMENTED | `validate.rs:214` | integration | — | code reused for two conditions | P1 |
| E-104 pointer unresolvable | IMPLEMENTED | `validate.rs:228` | wasm test | — | — | — |
| E-105 internal ref mismatch | IMPLEMENTED | `validate.rs` | no | — | — | P1 |
| V-101 dangling next/onFailure | IMPLEMENTED | `validate.rs:271,330` | integration | — | — | — |
| V-102 event-tree cycle | IMPLEMENTED | `validate.rs:385` | no | — | — | P1 |
| V-103 unreachable node | IMPLEMENTED | `validate.rs:433` | no | — | — | P1 |
| V-104 path not terminating in Consequence | **NOT IMPLEMENTED** | — | no | absent | P0-5c | P0 |
| V-201 barrier <2 branches | IMPLEMENTED | `validate.rs:545` | no | — | — | P1 |
| V-202 default rules | IMPLEMENTED | `validate.rs:571,585` | no | — | — | P1 |
| V-203 branch sum != 1.0 | **NOT IMPLEMENTED (dead)** | `validate.rs:1303-1320` | no | dead code | P0-4 | P0 |
| V-204 ECEL type mismatch / missing field | IMPLEMENTED | `typeck.rs` | no | — | no tests | P1 |
| V-301 handler not valid identifier | **NOT IMPLEMENTED** | — | no | absent | P0-5d | P0 |
| V-302 send without channel/message | IMPLEMENTED | `validate.rs:664` | no | — | — | P1 |
| V-401 input/rootCause unresolved | IMPLEMENTED | `validate.rs:772,796` | integration | — | — | — |
| V-402 gate/basic-event ID collision | IMPLEMENTED | `validate.rs:712` | advanced-FT | — | — | — |
| V-403 fault-tree cycle | IMPLEMENTED | `validate.rs:942` | no | — | wrapped into V-401 when caught in resolver | P1 |
| V-404 unreachable gate/basic-event | IMPLEMENTED | `validate.rs:834` | advanced-FT | non-deterministic order | P1 |
| V-501 gate arity | IMPLEMENTED | `validate.rs` | advanced-FT | — | — | — |
| V-502 VOTING k range | IMPLEMENTED | `validate.rs:1053-1069` | no | — | — | P1 |
| V-503 both/neither prob+failureRate | IMPLEMENTED | `validate.rs:1150-1180` | no | conflicts with W-407 for undeveloped | P0 | P0 |
| V-504 failureRate w/o missionTime | IMPLEMENTED | `validate.rs:1180` | no | — | — | P1 |
| W-401 no onFailure advisory | IMPLEMENTED | `validate.rs:636` | no | — | — | P1 |
| W-402 cached probability drift | IMPLEMENTED | `validate.rs:1218` | no | — | — | P1 |

## 8. Compiler & code generation

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| 8.1 pipeline (8 ordered stages) | IMPLEMENTED | `Compiler::compile` | integration | — | — | — |
| 8.2 entry-point per initiatingEvent (Rust `handle_`+snake_case) | IMPLEMENTED | `codegen/rust.rs:143` | no | — | — | P1 |
| 8.2 BranchMonitor per barrier with recordBranch(outcome, effectiveProbability) | IMPLEMENTED | `codegen` | no | — | — | P1 |
| 8.2 recordFailure per op failure w/ linked probability | IMPLEMENTED | `codegen:366-388` | no | wrong probability (P0-2) | P0 | P0 |
| 8.2 retryPolicy/timeoutMs faithful reproduction | IMPLEMENTED | `codegen:311-333` | no | unused `timeout` var when no retry | P0-1 | P0 |
| 8.2 W3C traceparent into every consequence message | NOT IMPLEMENTED | — | no | `publish_to_channel` undefined | P0-1 | P0 |
| 8.2 typed error path | PARTIAL | `codegen:147` | no | `WorkflowError` unimported | P0-1 | P0 |
| 8.3 header comment | IMPLEMENTED | `codegen:34-37` | no | `// AUTOGENERATED BY ETDL COMPILER v1.0.0` | version hardcoded (not `v{version}`) | P2 |
| 8.3 no runtime network I/O; probabilities as literals | IMPLEMENTED | `codegen` consts | no | — | — | P1 |
| 8.3 deterministic (SHOULD) | PARTIAL | BTreeMap codegen | no | topo-seed + V-404 order nondeterministic | P1 | P1 |
| 8.6 refuse cut sets for NOT/XOR; document refusal | IMPLEMENTED | `fault_tree.rs:280` | no | returns Err | no CLI/test | P1 |

## 9. Runtime library contract

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| 9.1 BranchMonitor API | IMPLEMENTED | `monitor.rs` | 1 smoke | — | — | — |
| 9.2 inject traceparent on consequence send | PARTIAL | `telemetry.rs` | no | malformed span-id | P0-6 | P0 |
| 9.2 attach node id as span attribute (`etdl.node.id`) | IMPLEMENTED | `telemetry.rs:26-28` | no | — | — | P1 |
| 9.3 compare declared vs observed; anomaly on divergence; increment counter | PARTIAL | `sla.rs`, `monitor.rs` | sla tests | observed always 1.0 | P0-7 | P0 |
| 9.4 chaos deterministic lower-probability routing; ignored in production | PARTIAL | `chaos.rs` | chaos tests | hash-parity 50/50, not probability-based; prod-guard weak | P0-8 | P0 |

## 10. Versioning & compatibility

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| 10.1 accept same MAJOR; reject unimplemented future MAJOR | **NOT IMPLEMENTED** | — | no | any `etdl:` accepted | P0-5a | P0 |
| 10.2 `eventTree` ≡ `eventTrees: { default: ... }` | IMPLEMENTED | `ast.rs` | parse test | — | no advisory | P2 |
| 10.2 probabilityOfSuccess/Failure aliases | IMPLEMENTED | `ast.rs:228-235` | no | — | — | P1 |
| 10.3 deprecated fields accepted ≥1 MAJOR | IMPLEMENTED | `ast.rs` | — | — | — | — |
| 10.3 SHOULD advise on deprecated fields | NOT IMPLEMENTED | — | — | — | — | P2 |

## 11. Extensibility & security

| Spec requirement | Implemented? | Where | Test? | Exact behavior | Gap | Priority |
|---|---|---|---|---|---|---|
| 11 x- fields preserved; unknown non-x- rejected | PARTIAL | `ast.rs` x- preserved | parse test | unknown non-x- NOT rejected | P0-5b | P0 |
| 12 no scripting escape hatch | IMPLEMENTED (n/a) | ECEL grammar | — | no eval | — | — |
| 12 local imports must not escape project root | **NOT IMPLEMENTED** | `asyncapi.rs` resolve_location | no | no containment | P0-5h | P0 |
| 12 runtime detects production, ignores ETDL_CHAOS | PARTIAL | `chaos.rs` | 1 test | exact-match only | P0-8 | P0 |
| 12 must not cite computed probabilities as safety certification | N/A (documentation) | — | — | to be covered in docs | — | P2 |

---

## Summary of gaps by class

- **P0 implementation gaps:** 5.8.1 sum (V-203), 5.8.1 probabilitySource precedence, 5.16-n/a (V-104), 7.4 (V-301), 7.5 transfer resolution, 5.14 undeveloped semantics, 10.1 MAJOR gate, 11 unknown-field rejection, 12 path containment, 8.2/8.3 codegen compilability, 9.2 traceparent, 9.3 SLA observation, 9.4 chaos guard.
- **Spec-side changes applied:** Appendices A–E now present (incl. `schemas/etdl.schema.json`); `in` array-literal grammar added; INHIBIT/PRIORITY_AND/eventType/transfers incorporated (see `etdl-spec` repo).
- **Test gaps (P1):** typeck, validate, codegen, retry, telemetry, WASM, CLI-subprocess, VOTING/NOT/XOR math, introspection, duplicate-keys, `domain` regex.
