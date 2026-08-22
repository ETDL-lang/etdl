# ETDL Current Readiness Audit

**Repository:** `github.com/ETDL-lang/etdl` (compiler, v0.2.0)
**Specification:** `github.com/ETDL-lang/etdl-specification` (ETDL 1.0.0)
**Extension:** `github.com/ETDL-lang/etdl-vscode` (0.2.0)
**Audit date:** 2026-08-13

> **2026-08-19 update:** all four §3 P0 blockers below (P0-A/B/C/D) were
> independently re-verified against current code (workspace v0.2.2, after the
> standard library / Generic Tree Event / Reliability / Predictive
> Reliability / Runtime Feedback / Conformance work landed) and were found
> **still live** — none had been fixed in the intervening work. All four are
> now fixed, with regression tests, as part of the ETDL 1.0 release-readiness
> pass. See `docs/RELEASE_READINESS_1.0.md` for the current, authoritative
> status; the P0/P1/P2 findings below are kept as the historical record of
> what was found and are no longer all accurate as "open" — do not treat this
> file as current without cross-checking the update doc.

This is a fresh, evidence-based audit. It does **not** re-litigate the previous
review; it records what changed, what the implementation actually does now, and
what that means for production/standards/ecosystem/business readiness. Every
finding cites file:line or a runtime verification.

Severity: **P0** = blocker, **P1** = important, **P2** = improvement,
**P3** = future.

---

## 1. What changed since the previous review

### Compiler repo (`ETDL-lang/etdl`) — history 3c0cac3 → 49eb0db
The previous readiness phases (Phases 1–11) landed, plus:

| Change | Evidence | Verified? |
|---|---|---|
| Codegen Publisher trait (`&dyn Publisher` handler param) + generated-code compile-check harness | `codegen/rust.rs:152-155`, `tests/codegen_test.rs` | ✅ generated fixture compiles (test passes) |
| Fault-tree constant wiring honors `onFailureProbabilitySource` (was first-map-entry) | `codegen/rust.rs:113-136` | ✅ unit tests pass |
| `RetryError::{Exhausted,TimedOut}` (no panic on timeout) | `etdl-core/src/retry.rs:20-26` | ✅ tests |
| W3C-valid traceparent (getrandom) | `etdl-core/src/telemetry.rs:64-109` | ✅ tests |
| Per-node SLA observed-frequency (shared denominator) | `etdl-core/src/sla.rs:88-100` | ✅ tests |
| Hardened chaos production guard (qualified env names) | `etdl-core/src/chaos.rs:45-111` | ✅ tests |
| ECEL `[index]` saturating parse (no overflow panic) | `etdl-parser/src/ecel.rs:152-156` | ✅ tests |
| V-203 branch sum/range revived; E-100 MAJOR gate; V-104, V-301, V-506 | `etdl-compiler/src/validate.rs` | ✅ tests |
| Overflow-proof FT math: f64 binomial, log-space factorial, MOCUS cap | `fault_tree.rs:175-250,324` | ✅ tests |
| Proptest robustness suite (found + fixed UTF-8 span-index panic) | `etdl-parser/tests/robustness.rs` | ✅ 7 pass |
| Conformance suite (10 cases) + runner | `conformance/conformance.rs` | ✅ runs in CI |
| CLI `--json`/`--quiet`/`--verbose`, `analyze`, directory input | `etdl-cli/src/main.rs` | ✅ subprocess tests |
| CI: fmt/clippy/test/wasm/audit/docs | `.github/workflows/ci.yml` | — |
| Docs set + 10 business demos + positioning/honesty docs | `docs/` | — |
| Company-strategy docs moved to private repo, history rewritten | `git log` | ✅ |
| Gap docs removed; spec changes applied directly | — | ✅ |

**Workspace version:** 0.2.0 (`Cargo.toml:21`). **Tests:** 101 passing across
the workspace (`cargo test --workspace --tests`).

### Spec repo (`ETDL-lang/etdl-specification`) — history f3dbef0 → 01b5766
- Appendices A–E added directly to `ETDL-Specification.md` (grammar, diagnostic
  registry, reserved words, changelog, JSON Schema reference).
- New `schemas/etdl.schema.json` companion artifact.
- §5.11.1 Transfer Object, §5.13 INHIBIT/PRIORITY_AND gates, §5.14 eventType,
  §5.16 formulas, §6.2 array literals, §7.6/7.7 new diagnostics
  (V-505/V-506/W-405/W-406).
- `SPEC_GAPS_ADDENDUM.md` removed (changes applied directly).

### Extension repo (`ETDL-lang/etdl-vscode`) — history b447029 → e31681d
- WASM LSP endpoints wired into native providers (`src/lsp.ts`).
- WASM `pkg/` synced to 0.2.0; `repository.url` fixed; README/config updated.
- `docs/raaml-compiler-gaps.md` removed.
- CI added (`test` + `package` jobs).
- **13 tests passing** (`npm run test:unit`).

---

## 2. Verified strengths (do not regress)

1. **Generated code compiles** — the order-fulfillment fixture generates Rust
   that passes `cargo check` against a Publisher harness (`tests/codegen_test.rs`).
2. **Runtime is panic-safe on the audited paths** — retry exhaustion returns
   `RetryError`, backoff saturates, ECEL index parse saturates.
3. **Deterministic** — BTreeMap ordering throughout codegen; sorted gate
   evaluation; sorted V-404 emission.
4. **Chaos is safe by default** — off unless `ETDL_CHAOS` truthy; production
   guard handles qualified env names; tested.
5. **Probability formulas match the spec** — AND/OR/NOT/XOR/VOTING/INHIBIT/
   PRIORITY_AND all match `docs/PROBABILITY_SEMANTICS.md` and §5.16; the
   0.012987 worked example checks out.
6. **Spec and worked example are byte-identical** across spec/compiler/vscode
   copies.
7. **Honesty** — spec explicitly refuses "certified PRA"; the project's
   do-not-overclaim posture is documented (tracked privately); IEC alignment is
   framed as "adapted," not certified.
8. **Conformance suite is real and CI-wired** (10 cases).
9. **Company strategy is out of the public repo** (history rewritten).

---

## 3. P0 — Blockers

| # | Finding | Evidence | Verification |
|---|---|---|---|
| P0-A | **Failure SLA anomaly always fires.** `record_failure` pushes `occurred=true` into a `{op}.failure`-only window, so observed frequency is always 1.0; after ≥10 failures the anomaly triggers unconditionally (`|1.0 − declared| > threshold` for any declared ≤ 0.9). | `monitor.rs:67-82`, `sla.rs:74-84` | ✅ read + reasoned; no test covers it |
| P0-B | **V-204 (ECEL type-checking) is effectively dead for path operands.** `resolve_operand_type` passes full path segments to `get_schema_for_path`, which resolves `message.payload.X` against the already-unwrapped payload schema → always `None` → `Unknown` → passes. The README/spec claim "catches `qty > "three"` before it ships" is not delivered. | `typeck.rs:145-170`, `asyncapi.rs:125-164` | ✅ read; no test triggers V-204 |
| P0-C | **`failureRate`/`missionTime` have no sign/NaN validation.** Negative λ yields negative probability that flows un-clamped through AND/OR/XOR/INHIBIT into emitted constants. Verified: λ=−0.1,t=10 → P=−1.718, OR(·,0.5)=−0.359. Contradicts `PROBABILITY_SEMANTICS.md:19` (λ≥0) and the clamp claim. | `fault_tree.rs:94-107,110-177` | ✅ runtime test |
| P0-D | **Generated code for ECEL `in` (String field) and `matches` does not compile.** `contains(&vec!["A","B"], &message.payload.status)` mixes `&[&str]`/`&String` → E0308; `matches(String, "…")` passes `String` where `&str` required. The compile-check harness only exercises `qty > 0`. | `codegen/rust.rs:507-516`, unit tests only substring-assert | ✅ rustc repro |

## 4. P1 — Important

| # | Finding | Evidence |
|---|---|---|
| P1-A | **VS Code IntelliSense broken (5 of 6 features).** WASM returns LSP shapes (`range`/`symbols`/`items`/`contents`); `lsp.ts` expects `span`/`references`/bare-array/string → go-to-def, references, hover, outline, completion all return null or throw; only Format works. | verified against live WASM output; `lsp.ts:72,91,109,125,153,175` |
| P1-B | **AND/OR/XOR/INHIBIT do not clamp results to [0,1]**; only VOTING and PRIORITY_AND clamp. Out-of-range values propagate to emitted constants. | `fault_tree.rs:116-138,169-174` vs `:148,166,185` |
| P1-C | **Unbounded recursion (P0-3b) still open** in validate DAG checks and codegen (`validate.rs:389,482,511,994`; `codegen/rust.rs:192`) — no depth cap on flat node chains; the 0.2.0 changelog only fixed the UTF-8 char-boundary issue. |
| P1-D | **Monitor mutex poison panics** — `.lock().unwrap()` at `monitor.rs:39,46,71,89`; `RUNTIME.md:39` claims "fail closed" which is not what a panic is. |
| P1-E | **`onFailureProbabilitySource` unresolved target silently ignored** (no diagnostic; codegen records `None`). `validate.rs:1328-1333`. |
| P1-F | **Extension packaging bug:** `dagre` is a runtime dependency, extension is unbundled, `node_modules/**` is excluded, CI packages with `--no-dependencies` → shipped vsix likely fails to load RAAML (require("dagre")). `package.json:146-149`, `.vscodeignore:4`, `ci.yml:37`. |
| P1-G | **`etdl.validation.enable` setting is documented but inert** — never read in `src/`. |
| P1-H | **Duplicate-id warning code mismatch:** emitted as `V-001` (`cli/main.rs:156`, `wasm/lib.rs:165`) but documented as `W-001` (`DIAGNOSTICS.md:79`); spec/matrix disagree. |
| P1-I | **E-101 semantic drift:** documented as "reference grammar mismatch" but only emitted by WASM for a missing AsyncAPI file; the compiler never emits it. `parse_reference` unused. |
| P1-J | **WASM `E-100` reuse for YAML parse errors** contradicts its documented meaning (language version). `wasm/lib.rs:125`. |
| P1-K | **Schema/§11 contradiction:** `x-*` on nested objects rejected by `additionalProperties:false`; unknown non-`x-` root fields accepted (root has no `additionalProperties:false`). `schemas/etdl.schema.json`; spec §11. |
| P1-L | **Schema gaps:** does not enforce branch-must-have-probability, `outcome`, `action`, send-consequence channel/message, exactly-one probability/failureRate, VOTING `k`, INHIBIT `inhibitCondition`, per-type arity, non-empty eventTrees, valid SemVer. |
| P1-M | **V-203 text drift:** §7.3 defines sum-only; Appendix B adds range; E-103 text differs between §7.1 and Appendix B. |
| P1-N | **`any()`/`all()` (spec §6.4 MAY) have no grammar production** — a conforming doc using them fails §6.2. |
| P1-O | **Extension CSP interpolation bug** — literal `${webview.cspSource}` not interpolated. `visualization.ts:10,14`. |
| P1-P | **Stale docs/README:** README generated-code sample still shows old signature + `publish_to_channel`; "complete example" doesn't validate; docs/{examples,concepts,architecture,matrix,reference/cli,crates} reference removed APIs or wrong versions. |

## 5. P2 — Improvements

| # | Finding |
|---|---|
| P2-A | Generated code emits unused imports (`Serialize`, `contains`/`matches`) when operators unused — warnings under `-D warnings` in consumer builds. |
| P2-B | `ChaosController` snapshots env at construction; env changes not re-probed. |
| P2-C | `etdl analyze` drops FT probability errors (throwaway `Vec::new()`). `main.rs:456`. |
| P2-D | `etdl validate` directory scan is non-recursive (`read_dir`); `docs/CLI.md:11` claims recursive. |
| P2-E | V-506 does not verify target points at a `topEvent` (any sub-path of an existing tree passes). |
| P2-F | Extension: visualization re-parse not debounced (full parse per keystroke); diagnostics use one global debounce timer; LSP providers re-parse whole doc each call; no doc-size guard. |
| P2-G | Extension: diagnostic ranges collapsed to single points (end_line/end_column dropped from `DiagnosticJson`); no click-to-jump; Mermaid ignores direction/colorScheme; no quick fixes; no workspace-wide validation; no AsyncAPI file-change revalidation. |
| P2-H | Grammar highlight list omits INHIBIT/PRIORITY_AND. |
| P2-I | `thiserror` declared in all 5 crates but unused anywhere. |
| P2-J | `etdl-wasm` has 0 in-crate tests. |
| P2-K | Conformance suite coverage is thin (10 cases; no NOT/XOR/INHIBIT/PRIORITY_AND/hetero-VOTING/E-103/104/V-101/201/202/204/301/302/401-405/501/503/504/505/W-*). |
| P2-L | `RUNTIME.md:39` "fail closed" wording inaccurate (it's a panic). |
| P2-M | `API_STABILITY.md` and `docs/reference/crates.md` cite 0.1.x; workspace is 0.2.0. |
| P2-N | MAJOR-0 documents accepted (code) vs "matches supported MAJOR (1)" (doc). |
| P2-O | W-406 flags house events declaring probability — contradicts §5.14's own "supplied" semantics. |

## 6. P3 — Future

| # | Finding |
|---|---|
| P3-A | `any()`/`all()` quantifier grammar, nested-wildcard semantics. |
| P3-B | missionTime unit declaration field; mixed-unit consistency rule. |
| P3-C | §9.3 percentage-points threshold → precise probability-space definition; §9.4 lower-probability-path for >2 branches/ties; retry jitter definition. |
| P3-D | Third-party conformance runner; TypeScript/Go codegen backends. |
| P3-E | DoS limits: doc-size, `$ref` depth, node-chain depth caps. |
| P3-F | OpenTelemetry export behind the vendor-neutral seam. |
| P3-G | Component `$ref` substitution semantics (§5.4). |

---

## 7. Category scores (see READINESS_SCORECARD.md for the full table)

Strongest: Parser (4), Probability formula correctness (4), Fault-tree math (4),
Event-tree validation (4), CLI (4), Runtime panic-safety (4), Determinism (4),
Docs breadth (4), Conformance runner (3), CI (4).

Weakest: ECEL type-checking (V-204 dead → effectively 2), VS Code IntelliSense
(2, 5 of 6 features broken), SLA failure observability (2, always-alarm),
probability input sanitization (2, negative λ), spec/schema/§11 consistency (2),
extension packaging (2).

---

## 8. IEC claim audit (summary)

| IEC concept | ETDL support | Evidence | Limitations |
|---|---|---|---|
| Constant-failure-rate exponential model | Faithful | `P=1−e^(−λt)` in §5.16 + `fault_tree.rs:99` | λ sign not validated |
| Gates AND/OR/NOT/XOR/VOTING | Implemented | §5.13/5.16 + `fault_tree.rs` | independence assumed |
| INHIBIT, PRIORITY_AND | Adapted | §5.13/5.16 | INHIBIT = product of 2; PAND = `(∏p)/n!` approximation |
| Transfer symbols | Adapted (nav only) | §5.11.1 | no subtree splicing into computation |
| MOCUS cut sets | Implemented (coherent only) | §8.6 + `fault_tree.rs:326` | NOT/XOR refused |
| Event tree barriers/sequences | Adapted | §5.8/5.10 | N-ary guarded branches, not IEC binary |
| Certified PRA | **Refused** | §1.4, §12 | explicit |

**Conclusion:** ETDL is "aligned with / inspired by" IEC 61025/62502 — it does
**not** claim certified compliance, and the spec says so. See
`docs/READINESS_SCORECARD.md` for the evidence-backed table.

---

## 9. Bottom line

The 0.2.0 release genuinely delivered its headline P0 fixes (codegen
compilability, FT wiring, retry safety, traceparent, V-203, chaos guard,
determinism, conformance). The project is **structurally sound and well-tested
(101 tests)** and its honesty posture is a genuine strength.

However, four production-critical defects remain open and are either silent or
worse-than-useless in operation: **(P0-A) failure SLA alarms always fire**,
**(P0-B) ECEL type-checking is dead**, **(P0-C) negative failureRate yields
negative probabilities**, and **(P0-D) `in`/`matches` codegen doesn't compile**.
Five of the extension's six IntelliSense features are broken against the real
WASM output, and the extension packaging will likely fail in the marketplace.
The project is **not yet production-ready**; it is a credible, well-engineered
0.2.0 prototype with clear P0/P1 work remaining.
