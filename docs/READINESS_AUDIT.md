# ETDL Readiness Audit

**Repository:** `github.com/usamassem/etdl`
**Specification:** `github.com/usamassem/etdl-specification` (ETDL v1.0.0)
**Audit date:** 2026-08-13
**Audited version:** workspace 0.1.4 (crates `etdl-parser`, `etdl-compiler`, `etdl-core`, `etdl-cli`, `etdl-wasm`)

This document is the Phase 0 deliverable of the ETDL readiness mission. It is a
**fact-based audit**: every item cites evidence (file:line) or a test. Where the
audit identifies a deviation from the specification, that is flagged as a spec gap
or a conformance gap, never silently "fixed" in this document.

Severity legend:
- **P0** — blocker: prevents honest claims of production readiness, conformance, or safety
- **P1** — important: required for a professional release
- **P2** — improvement: meaningful but not blocking
- **P3** — optional/future

---

## 0. Executive summary

ETDL is a young but structurally sound project. The core architecture is clean
(parse → validate → resolve → generate → run), the AST is carefully modeled, the
spec and the compiler's worked example are byte-identical, and 45+ tests pass.
Since the previous milestone the project has also added span-aware parsing and
LSP-style endpoints (`etdl-parser/src/{semantic.rs,spanned.rs}`).

However, the project cannot currently support claims of **production readiness** or
**spec conformance**:

1. **Generated Rust does not compile.** `etdl-compiler/src/codegen/rust.rs`
   references `etdl_core::telemetry::BranchMonitor` (wrong path), an unimported
   `WorkflowError`, a `publish_to_channel` function that exists nowhere, and emits
   invalid Rust for the ECEL `in` and `matches` operators. There is no
   compile-check of generated output anywhere.
2. **Fault-tree probabilities are assigned to the wrong operation** when more than
   one fault tree exists: `find_fault_tree_prob` returns the first map entry,
   ignoring `onFailureProbabilitySource` (`codegen/rust.rs:128-134`).
3. **Multiple panic/crash paths on untrusted input** (ECEL index overflow, unbounded
   recursion over flat node chains, retry all-timeout panic, integer overflow in
   `binomial_coeff`/factorial).
4. **Two normative validation rules are dead or absent:** the branch-probability
   sum check (V-203) is unreachable dead code, and branch probability ranges [0,1]
   are never validated.
5. **No CI/CD, no conformance suite, no security policy, no changelog.**
6. **The runtime's W3C traceparent is malformed** (span-id length), SLA anomaly
   observation semantics are broken, and the chaos production-guard is weaker than
   the specification requires.

The remainder of this document is the complete P0–P3 inventory by area.

---

## 1. Repository & project structure

| Item | Current state | Evidence |
|---|---|---|
| Workspace | 5 crates, workspace root 0.1.4, edition 2021, Apache-2.0 | `Cargo.toml:12,15` |
| Repository metadata | repository/homepage set, readme inherited; per-crate READMEs absent | `etdl-*/Cargo.toml` |
| Git history | Clean, conventional commit messages | `git log` |
| CI/CD | **None** | no `.github/` |

Risk: **P1** (no CI means no gate). Required for 1.0: yes. Required for business: yes.
Open source (CI belongs in OSS).

---

## 2. etdl-parser

### Current state
- Full AST with manual `Deserialize` supporting legacy `eventTree`, `x-*`
  extensions, camelCase aliases (`ast.rs`).
- ECEL parser (`ecel.rs`), AsyncAPI registry (`asyncapi.rs`), JSON Pointer
  (`jsonptr.rs`).
- Span-aware parsing (`spanned.rs`) and LSP-style semantic endpoints
  (`semantic.rs`): `document_symbols`, `hover`, `goto_definition`,
  `find_references`, `complete`, `format`.
- 20+ unit tests across `ecel.rs` (6), `jsonptr.rs` (4), `semantic.rs` (8),
  `spanned.rs` (6).

### Missing / risks

| # | Item | Severity |
|---|---|---|
| P0-3a | **ECEL index overflow panic** — `s.parse::<usize>().unwrap()` on `[index]` (`ecel.rs:156`); 20+ digit index → `ParseIntError` → panic on untrusted input | P0 |
| P0-4 | **Branch probability range [0,1] never validated** (`ast.rs` effective probability; only basic-event ranges checked at compute time) | P0 |
| P0-5a | **`etdl` MAJOR version gate not implemented** (spec 10.1) — any `etdl:` string accepted | P0 |
| P0-5b | **Unknown non-`x-` fields not rejected** (spec Section 11) — serde default ignores unknown fields | P0 |
| P1-1 | `etdl-parser/tests/` empty (no integration tests in the crate; only `etdl-cli` tests) | P1 |
| P1-2 | No panic-safety tests / fuzzing | P1 |
| P3-1 | saphyr dependency added (0.0.11) — verify necessity/justification | P3 |

---

## 3. etdl-compiler

### Current state
- Clean pipeline: `Compiler::validate` / `Compiler::compile` (`lib.rs:58-95`).
- `validate.rs` implements ~30 E-/V-/W- diagnostics.
- `fault_tree.rs` computes AND/OR/NOT/XOR/VOTING/INHIBIT/PRIORITY_AND; MOCUS
  `enumerate_minimal_cut_sets` exists.
- `typeck.rs` type-checks ECEL against AsyncAPI schemas.
- `codegen/rust.rs` emits Rust with `CodeGenerator`/`RustCodeGenerator` trait.

### Missing / risks

| # | Item | Severity |
|---|---|---|
| P0-1 | **Generated code does not compile** (see §0). Affected: `BranchMonitor` import path, missing `WorkflowError` import, undefined `publish_to_channel`, invalid `in`/`matches` emission, empty operands for literal-left / path-right comparisons, unused `timeout` var | P0 |
| P0-2 | **`find_fault_tree_prob` returns first map entry** — wrong probability when >1 fault tree | P0 |
| P0-3b | **Unbounded recursion** in `check_dag`/`propagate_reachability`/`check_termination`/`dfs_gate` and codegen over flat node chains → stack overflow on large documents | P0 |
| P0-3c | **Integer overflow**: `binomial_coeff` (n≥66), PriorityAnd factorial (n≥21), VOTING polynomial growth | P0 |
| P0-4 | **V-203 branch-sum check is dead code** (`validate.rs:1303-1320` — `default_prob = (1.0 - sum).max(0.0)` then `< 0.0` never true) | P0 |
| P0-5c | **V-104 not implemented** (paths must terminate in a Consequence — spec 7.2) | P0 |
| P0-5d | **V-301 not implemented** (operation handler must be valid identifier — spec 7.4) | P0 |
| P0-5e | **Transfer target existence not verified** (only prefix check V-506) | P0 |
| P0-5f | **`undeveloped` boolean parsed but never read**; `eventType: undeveloped` conflicts with V-503 hard error | P0 |
| P0-5g | **`probabilitySource` precedence not enforced** (spec 5.15: source is authoritative) | P0 |
| P1-3 | Diagnostics taxonomy in docs contradicts code (`docs/architecture.md` V-3xx/V-5xx labels wrong; docs claim handler-existence check that doesn't exist) | P1 |
| P1-4 | Non-deterministic diagnostic ordering: V-404 emitted in `HashMap` iteration order (`validate.rs:819`) | P1 |
| P1-5 | Non-deterministic fault-tree topological sort seeding (`fault_tree.rs:224-278` iterates `HashMap`) | P1 |
| P1-6 | MOCUS unbounded (no size cap), dead code, no tests | P1 |
| P1-7 | `components` (spec 5.4) parsed but unused by validate/typeck/codegen | P1 |
| P1-8 | `typeck.rs` / `validate.rs` / `codegen` have zero direct unit tests; V-204 never exercised | P1 |
| P2-1 | E-102 BOM returned as `Err(String)` not a `Diagnostic` (inconsistent error channel) | P2 |

---

## 4. etdl-core (runtime)

### Current state
- `BranchMonitor`, `RetryPolicy`, `SlaTracker`, `ChaosController`, telemetry.
- Async only in `RetryPolicy::execute` (tokio `time` feature); rest is runtime-free.
- Chaos disabled by default; production guard + scope + seed.

### Missing / risks

| # | Item | Severity |
|---|---|---|
| P0-3d | **Retry panic**: `panic!("retry exhausted ...")` when all attempts time out (`retry.rs:63`); also `backoff_ms * 2^attempt` overflow (n>64) | P0 |
| P0-6 | **Malformed W3C traceparent**: span-id `{:016x}` of `nanos*17` can exceed 16 hex chars; not 16-char fixed; also time-derived trace-id (collision-prone, all-zero before epoch) (`telemetry.rs:46-74`) | P0 |
| P0-7 | **SLA semantics broken**: `BranchMonitor` passes `occurred=true` always (`monitor.rs:51,77`) so observed frequency is always 1.0; anomaly only fires for declared p < 0.9 | P0 |
| P0-8 | **Chaos production guard too weak**: exact-match only `production/prod/prd/live` (`chaos.rs:51-54`); misses `production-us-east`, etc.; falls back to "not production" when no env var set — a stray `ETDL_CHAOS=true` in an undetected prod env activates chaos | P0 |
| P1-9 | `retry.rs` and `telemetry.rs` have no tests | P1 |
| P1-10 | Env-var tests mutate globals with no synchronization (racy under parallel test) | P1 |
| P1-11 | `Mutex::lock().unwrap()` poison panics in monitor | P1 |
| P1-12 | Telemetry to stderr unconditionally (noisy in libraries); no vendor-neutral interface | P1 |
| P2-2 | `thiserror` unused dependency in etdl-core | P2 |
| P2-3 | `SlaTracker` counter map unbounded growth | P2 |

---

## 5. etdl-cli

| # | Item | Severity |
|---|---|---|
| P1-13 | No `--json` machine output | P1 |
| P1-14 | stdout/stderr inconsistent (validate diagnostics → stdout; compile → stderr) | P1 |
| P1-15 | No `--quiet`/`--verbose`, no directory input | P1 |
| P1-16 | No CLI subprocess tests (no assert_cmd); exit codes untested | P1 |
| P2-4 | `etdl analyze` / `etdl format` commands absent (roadmap) | P2 |
| P2-5 | `enumerate_minimal_cut_sets` not exposed via CLI | P2 |

---

## 6. etdl-wasm

| # | Item | Severity |
|---|---|---|
| P1-17 | No tests at all | P1 |
| P1-18 | Diagnostics positions: populated from spans, but core validation produces few positions; heuristic anchoring remains in the extension | P1 |
| P2-6 | `version()` returns crate version (0.1.4) not a semantic API version | P2 |
| P2-7 | WASM binary ~880 KB uncompressed (packaging consideration) | P2 |

---

## 7. Probability engine (fault-tree math)

| # | Item | Severity |
|---|---|---|
| P0-3c | Overflow in `binomial_coeff`, factorial, polynomial growth | P0 |
| P1-19 | NOT/XOR/VOTING probability math untested (only INHIBIT/PRIORITY_AND unit-tested) | P1 |
| P1-20 | VOTING heterogeneous-probability polynomial path untested | P1 |
| P1-21 | No numerical-stability / precision tests; rounding only via `{:.6}` at codegen | P1 |
| P2-8 | No documented probability semantics doc (`docs/PROBABILITY_SEMANTICS.md`) | P2 (mission requires) |

---

## 8. ECEL

| # | Item | Severity |
|---|---|---|
| P0-3a | Index overflow panic (`ecel.rs:156`) | P0 |
| P1-22 | `!=`, `>=`, `<=`, `<`, negatives, bool/null literals, quoted keys, error cases all untested | P1 |
| P1-23 | `matches` RE2 mandate (spec 6.5) not enforced — runtime regex semantics undefined; codegen emits invalid Rust | P1 |
| P2-9 | No `docs/ECEL.md` | P2 (mission requires) |

---

## 9. AsyncAPI integration

| # | Item | Severity |
|---|---|---|
| P0-5h | **`../` path traversal guard missing** (spec Section 12) — local imports resolve with no root containment | P0 |
| P1-24 | Schema introspection ignores `$ref`/`allOf`/`oneOf`/`enum` (typeck silently passes) | P1 |
| P1-25 | No size/`$ref`-depth limits on untrusted AsyncAPI docs (spec 12 SHOULD) | P1 |
| P2-10 | `get_schema_for_path` introspection untested | P2 |

---

## 10. Code generation (Rust target)

Covered by P0-1/P0-2 above. Additionally:

| # | Item | Severity |
|---|---|---|
| P1-26 | No compile-check of generated Rust (`cargo check`) anywhere | P1 |
| P1-27 | Const rounding `{:.6}` vs README's `0.012987` (should document/standardize) | P2 |
| P1-28 | `use etdl_core::telemetry::BranchMonitor` + `publish_to_channel` + `WorkflowError` must be resolved by the Publisher contract | P0 (part of P0-1) |

---

## 11. Tests & conformance

| # | Item | Severity |
|---|---|---|
| P0-9 | No conformance suite; no third-party-runnable corpus | P0 |
| P1-29 | No generated-code compile test; no `etdl-core` integration test dir | P1 |
| P1-30 | `typeck`, `validate`, `codegen` untested directly | P1 |
| P2-11 | No benchmarks (parsing/validation/codegen/FT-eval) | P2 (mission requires) |
| P2-12 | No fuzzing (proptest/cargo-fuzz) | P2 (mission requires) |

---

## 12. CI/CD, releases, packaging

| # | Item | Severity |
|---|---|---|
| P0-9 | No CI in either repo | P0 |
| P1-31 | No release process/changelog/conventions | P1 |
| P1-32 | Version strategy ad-hoc (0.1.x with no documented policy) | P1 |
| P2-13 | No `SECURITY.md`, no dependency auditing in CI | P1 |

---

## 13. VS Code extension

| # | Item | Severity |
|---|---|---|
| P1-33 | No language server in the extension (WASM endpoints exist in compiler but aren't wired; README claims features that don't exist) | P1 |
| P1-34 | Error navigation uses heuristic anchoring (falls back to line 0) | P1 |
| P1-35 | `repository.url` points to compiler repo (wrong); README documents 2 of 5 settings; marketplace badge placeholder | P1 |
| P2-14 | No click-to-jump visualization; Mermaid ignores layout direction | P2 |
| P2-15 | Mermaid gate highlighting only 5 gate types (grammar), INHIBIT/PRIORITY_AND unhighlighted | P2 |

---

## 14. Security

| # | Item | Severity |
|---|---|---|
| P0-5h | Path traversal in AsyncAPI imports | P0 |
| P0-3 | Panic-on-untrusted-input paths (ECEL, recursion, overflow) | P0 |
| P1-36 | No `SECURITY.md` / vulnerability reporting process | P1 |
| P1-37 | No dependency vulnerability audit (cargo-audit) | P1 |
| P2-16 | No recursion-depth / doc-size limits (DoS) | P2 |

---

## 15. Documentation

| # | Item | Severity |
|---|---|---|
| P1-3 | `docs/architecture.md` and `docs/reference/cli.md` contradict implementation | P1 |
| P1-38 | README "complete example" does not parse (missing required `description` fields) | P1 |
| P1-39 | No audience-trio docs (developer/architect/business) | P1 (mission requires) |
| P2-17 | Missing mission docs: `CONFORMANCE.md`, `VERSIONING.md`, `PROBABILITY_SEMANTICS.md`, `FAULT_TREE_ANALYSIS.md`, `EVENT_TREE_ANALYSIS.md`, `ECEL.md`, `ASYNCAPI_INTEGRATION.md`, `API_STABILITY.md`, `DIAGNOSTICS.md`, `RUNTIME.md`, `CLI.md`, `READINESS_SCORECARD.md` (positioning/business/ecosystem/certification docs are tracked privately) | P2 (mission requires) |

---

## 16. Specification gaps (owned by spec repo)

| # | Item | Severity |
|---|---|---|
| P0-10 | Appendices A–E (JSON Schema, diagnostic registry, reserved words, changelog, companion artifacts) referenced but absent | P0 |
| P0-10b | Contradictions: V-203 vs probabilitySource precedence; `in` right-operand array literal absent from grammar; `matches` RE2 mandate vs no runtime regex contract; number grammar no exponent; `any`/`all` quantifiers no grammar; 9.3 "percentage points" ill-defined for p<0.1 | P1 (spec) |
| P1-40 | `missionTime` unit "implementation-defined" but no document field to declare it | P1 (spec) |
| P1-41 | 9.4 "lower-probability path" undefined for >2 branches / ties | P1 (spec) |

---

## 17. Business / ecosystem readiness

| # | Item | Severity |
|---|---|---|
| P1-42 | No business-positioning docs, no business demos | P1 (mission requires) |
| P1-43 | No commercial-boundary / SaaS-requirements docs | P1 (mission requires) |
| P2-18 | No website structure proposal | P2 (mission requires) |

---

## 18. What is already strong (do not regress)

- Spec and compiler worked example byte-identical.
- Clean crate boundaries and pipeline; `Compiler` API is tidy.
- Span-aware AST + LSP endpoints (valuable for the extension).
- Chaos off by default, with a dedicated safety test.
- `BTreeMap` used throughout codegen for deterministic ordering.
- Transfers/INHIBIT/PRIORITY_AND/eventType recently added with tests.
- All 45+ tests currently pass.

---

## 19. Priority backlog (derived from the above)

### P0 — must fix before any production/conformance claim
1. P0-1 Generated Rust must compile (Publisher contract, imports, WorkflowError, `in`/`matches`).
2. P0-2 Correct fault-tree probability wiring (`find_fault_tree_prob`).
3. P0-3 Eliminate panics: ECEL index overflow; bounded recursion; retry all-timeout → Err; overflow-proof math.
4. P0-4 Validate branch probability range; revive V-203 sum check.
5. P0-5 Spec MUSTs: `etdl` MAJOR gate; reject unknown non-`x-` fields; V-104; V-301; transfer resolution; `undeveloped`/`eventType` semantics; `probabilitySource` precedence; AsyncAPI `../` guard.
6. P0-6 Fix malformed traceparent (span-id/trace-id).
7. P0-7 Fix SLA observation semantics (spec 9.3).
8. P0-8 Harden chaos production guard.
9. P0-9 CI/CD + conformance suite.
10. P0-10 (spec repo) Appendices A–E + contradiction resolutions.

### P1 — important (professional release)
Diagnostics taxonomy docs, deterministic ordering, MOCUS cap+exposure, typeck/validate/codegen tests, CLI `--json`/`--quiet`/directory/exit-code contract, WASM tests, runtime tests, security policy + dependency audit, extension language-server wiring + metadata fixes, business/ecosystem docs, mission documentation set.

### P2 — improvements
README example fixes, unused-dependency cleanup, benchmarks, fuzzing, doc housekeeping.

### P3 — optional/future
saphyr justification, WASM binary size, deeper DoS limits.

---

## 20. Scope of this mission

The remaining phases (1–11) implement the P0/P1 backlog plus the required
documentation set, conformance suite, CI, fuzzing, benchmarks, and business
readiness docs, per `docs/SPEC_IMPLEMENTATION_MATRIX.md`, `docs/CONFORMANCE.md`,
and `docs/VERSIONING.md`. No P2/P3 work will be started before all P0 items are
closed.
