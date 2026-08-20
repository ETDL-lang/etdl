# ETDL Readiness Backlog

Every finding from `docs/CURRENT_READINESS_AUDIT.md`, classified P0–P3 with
problem, evidence, business/technical impact, proposed solution, tests, docs,
and dependencies. This is the implementation plan; nothing here is done yet.

---

## P0 — Blockers (production-readiness claims are unsupportable until closed)

### P0-A — Failure SLA anomaly always fires
- **Problem:** `BranchMonitor::record_failure` records only failures into the
  `{op}.failure` window with `occurred=true`, so observed frequency is always
  1.0; after `MIN_OBSERVATIONS` (10) failures, `is_anomaly` fires unconditionally.
- **Evidence:** `etdl-core/src/monitor.rs:67-82`; `etdl-core/src/sla.rs:74-84`.
- **Business impact:** false SLA alarms; an operation that fails 10+ times with
  declared p=0.02 will alarm as an anomaly even when behavior is exactly as
  designed. Undermines the "probability-driven SLA" value proposition.
- **Technical impact:** the failure-denominator must count *evaluations*
  (attempts), not only failures; `record_failure` needs to feed the node's
  evaluation denominator, mirroring the branch path.
- **Proposed solution:** track a per-node evaluation count for failures (attempts
  vs failures) so `observed_frequency("{op}.failure") = failures / attempts`;
  or record `occurred` = whether the attempt failed, with successes recorded on
  the success path. Add a `record_attempt`/`record_success` seam.
- **Tests required:** SLA edge tests: all-fail, mixed fail/success, threshold
  equality, <10 observations, empty window.
- **Docs required:** `RUNTIME.md` §4, `PROBABILITY_SEMANTICS.md` §retry-interaction.
- **Dependencies:** none.

### P0-B — ECEL type-checking (V-204) is dead for path operands
- **Problem:** `typeck::resolve_operand_type` passes full path segments to
  `get_schema_for_path`, which resolves `message.payload.X` against the
  already-unwrapped payload → always `None` → `EcelType::Unknown` → V-204 never
  fires. The "type-checked contracts" claim is not delivered.
- **Evidence:** `etdl-compiler/src/typeck.rs:145-170`; `etdl-parser/src/asyncapi.rs:125-164`.
- **Business impact:** the flagship "catch `qty > "three"` at build time"
  capability is a marketing claim with no enforcement; architects cannot trust
  schema-validated conditions.
- **Technical impact:** fix schema-path resolution (skip `message` and `payload`
  segments, then resolve against properties); add literal-vs-schema type
  comparison; make Unknown conservative-but-visible (warn) rather than silently
  pass when a schema is resolvable.
- **Tests required:** V-204 unit tests (path operand vs schema, literal vs path,
  ordering on non-number, `in` non-array, `matches` non-string, missing field,
  unresolvable schema → skip).
- **Docs required:** `docs/ECEL.md` type-checking section, `README.md` claim
  corrected only after tests prove it.
- **Dependencies:** P1-L schema gap awareness (schema vs typeck are separate).

### P0-C — Negative `failureRate`/`missionTime` produce negative probabilities
- **Problem:** no λ≥0 / t>0 validation; negative λ yields P<0 that flows
  un-clamped through AND/OR/XOR/INHIBIT into emitted constants.
- **Evidence:** `fault_tree.rs:94-107` (no sign check), `:110-177` (no clamp on
  AND/OR/XOR/INHIBIT); verified P=−1.718, OR=−0.359 at runtime.
- **Business impact:** silent invalid reliability numbers in generated code;
  "deterministic chain of reasoning" breaks.
- **Technical impact:** add `V-503`-adjacent range checks (λ≥0, t>0) or a new
  diagnostic; clamp all gate outputs to [0,1] as documented.
- **Tests required:** negative λ, NaN λ, inf λ, negative t, and clamp regression
  for every gate.
- **Docs required:** `PROBABILITY_SEMANTICS.md` §4.4 accuracy; `DIAGNOSTICS.md`
  new code if added.
- **Dependencies:** none.

### P0-D — Generated code for ECEL `in`/`matches` does not compile
- **Problem:** `contains(&vec!["A","B"], &message.payload.status)` mixes
  `&[&str]`/`&String` → E0308; `matches(String, "…")` passes `String` for `&str`.
- **Evidence:** `codegen/rust.rs:507-516`; rustc repro confirmed E0308.
- **Business impact:** any model using `in`/`matches` on a string field generates
  uncompilable Rust — a hard break of "your event tree becomes your code."
- **Technical impact:** lower `in` to `.iter().any(|v| v == &x)` with an
  `AsRef<str>`/generic helper, or `contains(items.as_ref(), value.as_ref())`;
  lower `matches` to `matches(value.as_str(), "…")`; extend the compile-check
  fixture to cover `in` (string literal + string path) and `matches`.
- **Tests required:** extend `gencheck` fixture with `in`/`matches` conditions;
  assert compile; unit-test the generated expression strings.
- **Docs required:** `docs/ECEL.md` lowering table already exists — correct it.
- **Dependencies:** none.

---

## P1 — Important (professional release)

| # | Finding | Evidence | Proposed solution |
|---|---|---|---|
| P1-A | Extension IntelliSense broken (5/6) — WASM LSP shape vs `lsp.ts` expectation mismatch | live WASM output; `lsp.ts:72,91,109,125,153,175` | Rewrite `lsp.ts` to consume `range`/`symbols`/`items`/`contents`; add unit tests that call the real WASM; drop `find_span`/`parse_with_spans` dead declarations |
| P1-B | AND/OR/XOR/INHIBIT not clamped to [0,1] | `fault_tree.rs:116-138,169-174` | Clamp at each gate output; test all gates |
| P1-C | Unbounded recursion in DAG checks + codegen | `validate.rs:389,482,511,994`; `codegen/rust.rs:192` | Add explicit node-chain depth cap (e.g. 10k) returning an error; test deep chain |
| P1-D | Monitor mutex poison panics | `monitor.rs:39,46,71,89` | Replace `.unwrap()` with `.unwrap_or_else(|e| …)` or document panic; correct `RUNTIME.md:39` |
| P1-E | `onFailureProbabilitySource` unresolved silently ignored | `validate.rs:1328-1333` | Emit diagnostic (reuse V-401 or new); test |
| P1-F | Extension packaging: dagre unbundled + node_modules excluded + `--no-dependencies` | `package.json:146-149`, `.vscodeignore:4`, `ci.yml:37` | Bundle with esbuild (or move dagre logic to devDependency + vendor), or package with `--dependencies`; verify vsix activates |
| P1-G | `etdl.validation.enable` inert | `package.json:95` vs `src/` | Wire the setting; test |
| P1-H | V-001 vs W-001 duplicate-id code mismatch | `cli/main.rs:156`, `wasm/lib.rs:165`, `DIAGNOSTICS.md:79` | Pick one (recommend W-001 per class), update all docs |
| P1-I | E-101 semantic drift | `DIAGNOSTICS.md:22` vs implementation | Emit E-101 for reference-grammar mismatch or re-document as import-file error; align matrix |
| P1-J | WASM E-100 reuse for YAML errors | `wasm/lib.rs:125` | Use E-101/E-102-appropriate code; document |
| P1-K | Schema `x-*` vs §11 contradiction; root unknown fields accepted | `schemas/etdl.schema.json` | Add root `additionalProperties` pattern for `x-*`; allow `x-*` on nested objects; align with §11 |
| P1-L | Schema misses MUST-level constraints | see P1-K file | Add: branch probability presence, `outcome`, `action`, send channel/message, exactly-one prob/failureRate, VOTING `k`, INHIBIT `inhibitCondition`, per-type arity (via `if/then`), non-empty eventTrees, strict SemVer pattern |
| P1-M | §7 vs Appendix B code-text drift (V-203, E-103); E-100 only in registry | spec lines 577-638 vs 1026-1094 | Unify §7 tables and Appendix B |
| P1-N | `any()`/`all()` documented but no grammar | spec §6.4 vs §6.2 | Add grammar production or mark not-in-1.0.0 |
| P1-O | Extension CSP interpolation bug | `visualization.ts:10,14` | Use template literal `${webview.cspSource}` |
| P1-P | Stale README/docs (old codegen sample, non-validating example, removed APIs) | `README.md:72-96,146-154`; docs/* | Update README code sample to Publisher signature; make example validate; sweep docs |

---

## P2 — Improvements

| # | Finding | Evidence |
|---|---|---|
| P2-A | Unused imports in generated code (Serialize, contains/matches) | `codegen/rust.rs:64-68` |
| P2-B | ChaosController env snapshot at construction | `chaos.rs:16` |
| P2-C | `etdl analyze` drops FT probability errors | `main.rs:456` |
| P2-D | Non-recursive directory scan vs doc claim | `main.rs:115-119`, `CLI.md:11` |
| P2-E | V-506 doesn't require topEvent target | `validate.rs:897-974` |
| P2-F | Extension perf: no viz debounce, global diag timer, full re-parse per LSP call, no doc-size guard | `visualization.ts:211-217`, `diagnostics.ts:73-82`, `lsp.ts` |
| P2-G | Diagnostic ranges single-point; no click-to-jump; Mermaid ignores direction/color; no quick fixes; no workspace validation; no AsyncAPI revalidation | `diagnostics.ts:124`, `mermaid.ts:184` |
| P2-H | Grammar omits INHIBIT/PRIORITY_AND highlight | `syntaxes/etdl.tmLanguage.json:44` |
| P2-I | `thiserror` unused in all crates | all `Cargo.toml` |
| P2-J | etdl-wasm has 0 tests | `etdl-wasm/src/lib.rs` |
| P2-K | Conformance suite thin (10 cases) | `conformance/conformance.rs` |
| P2-L | `RUNTIME.md:39` "fail closed" inaccurate | `RUNTIME.md:39` |
| P2-M | Version citations stale (0.1.x) | `API_STABILITY.md:9`, `reference/crates.md` |
| P2-N | MAJOR-0 accepted vs doc | `validate.rs:116` |
| P2-O | W-406 vs §5.14 house-probability contradiction | `validate.rs:795`, spec §5.14 |

---

## P3 — Future

| # | Finding |
|---|---|
| P3-A | `any()`/`all()` grammar + nested-wildcard semantics |
| P3-B | missionTime unit declaration + mixed-unit rule |
| P3-C | §9.3 percentage-points → probability-space threshold; §9.4 lower-probability-path (multi-branch/ties); retry jitter |
| P3-D | Third-party conformance runner; TS/Go backends |
| P3-E | DoS limits (doc size, `$ref` depth, node-chain depth) |
| P3-F | OTLP export behind vendor-neutral seam |
| P3-G | Component `$ref` substitution semantics |

---

## Recommended implementation order

1. **P0-A, P0-B, P0-C, P0-D** (correctness — silent/wrong behavior).
2. **P1-A** (extension IntelliSense — flagship DX) and **P1-F** (packaging).
3. **P1-B..P1-E, P1-G..P1-J** (validation/runtime correctness + diagnostics).
4. **P1-K..P1-P** (schema, spec consistency, docs sweep).
5. **P2** (perf, hygiene, conformance breadth).
6. **P3** (future language/ecosystem items).
