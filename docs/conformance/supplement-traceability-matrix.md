# Supplement Traceability Matrix

Companion to [`docs/SPEC_IMPLEMENTATION_MATRIX.md`](../SPEC_IMPLEMENTATION_MATRIX.md),
which covers only the core `etdl-specification` sections (2-11) and predates
every supplement built since. This matrix covers the supplements: each has
no upstream normative section in `etdl-specification` — its own reference
doc in this repository *is* its authority, per how each was designed (see
`docs/reference/standard-library.md`'s "extensibility" framing). Status
legend matches `SPEC_IMPLEMENTATION_MATRIX.md`'s.

## Standard Library / `std.probability`

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| `libraries:` import resolution, qualified-id splicing | IMPLEMENTED + TESTED | `etdl-compiler::stdlib` | `etdl-compiler/tests/` | — |
| `std.*` namespace protected from shadowing | IMPLEMENTED + TESTED | `etdl-compiler::stdlib` | existing | — |
| `Probability` bounded `[0,1]`, rejects (never clamps) invalid values | IMPLEMENTED + TESTED | `etdl-probability-core::probability` | existing + conformance | `LIB-PROB-001` |
| Composition ops (complement, AND/OR, conditional, Bayes) | IMPLEMENTED + TESTED | `etdl-probability-core::probability` | existing + conformance | `LIB-PROB-006`-`009` |
| Distributions (Bernoulli/Binomial/Beta/Exponential/Normal) | IMPLEMENTED + TESTED | `etdl-probability-core::distribution` | existing + conformance | `LIB-PROB-002`-`005` |
| `std.units`/`std.collections` | NOT IMPLEMENTED (documented) | — | — | out of scope, see `standard-library.md` |

## Generic Tree Event Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| One root, resolvable | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-001`, `TREE-004` |
| No cycles | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-002` |
| Valid child references | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-003` |
| Gate arity (AND/OR/NOT/XOR/K_OF_N) | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-005`, `TREE-006` |
| Strict tree (shared nodes rejected, not a DAG) | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-007` |
| Reachability (orphaned nodes rejected) | IMPLEMENTED + TESTED | `etdl-tree-core::tree` | existing + conformance | `TREE-008` |
| Deterministic traversal | IMPLEMENTED + TESTED | `etdl-tree-core::traverse` | existing + conformance | `TREE-009` |
| Stack-safe traversal on deep trees | IMPLEMENTED + TESTED (fixed by this task) | `etdl-tree-core::tree`/`traverse` | conformance | `TREE-010` |
| Zero dependency on Reliability/Probability | IMPLEMENTED + TESTED | `etdl-tree-core/Cargo.toml` | conformance | `ARCH-002` |
| Compiler integration (`x-tree-event`, supplement gating) | IMPLEMENTED + TESTED | `etdl-compiler::tree_event` | existing | — |

## Performance Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| Only processed when `supplements:` declares `etdl.performance` | IMPLEMENTED + TESTED | `etdl-compiler::performance` | `document_without_x_performance_has_no_diagnostics`, `document_not_declaring_performance_is_unaffected` | — |
| `budgets` optional at top level; malformed manifest -> E-160 | IMPLEMENTED + TESTED | `etdl-compiler::performance` | `missing_budgets_key_is_not_an_error`, `malformed_budgets_produces_e160` | — |
| Duplicate budget `id` -> E-160 | IMPLEMENTED + TESTED | `etdl-compiler::performance` | `duplicate_budget_id_produces_e160` | — |
| `nodeRef` resolves to an Event Tree or Operation node (not Barrier/Consequence) -> else E-160 | IMPLEMENTED + TESTED | `etdl-compiler::performance` | `valid_budget_operation_node_ref_has_no_diagnostics`, `valid_budget_whole_tree_node_ref_has_no_diagnostics`, `unresolvable_node_ref_produces_e160`, `node_ref_at_barrier_is_rejected_produces_e160` | — |
| Non-positive/non-finite percentile or `maxConcurrency`/`expectedRatePerSecond` -> E-160 | IMPLEMENTED + TESTED | `etdl-compiler::performance` | `non_positive_percentile_produces_e160_without_spurious_e161` | — |
| Percentile ordering (`p50<=p95<=p99`) -> else E-161 | IMPLEMENTED + TESTED | `etdl-compiler::performance` | `bad_percentile_ordering_produces_e161` | — |
| Duplicate `nodeRef` across budgets -> W-413 (warning only, both kept) | IMPLEMENTED + TESTED | `etdl-compiler::performance` | `duplicate_node_ref_produces_w413_and_keeps_both_budgets`, `warning_only_diagnostic_is_not_duplicated_by_process` | — |
| Diagnostics surface through the real `Compiler::validate`/`compile` entry points | IMPLEMENTED + TESTED | `etdl-compiler::performance` (registered generically, not pipeline-special-cased) | `etdl-compiler/tests/performance_wiring_test.rs` | — |
| No effect on generated code, no fault-tree overrides | IMPLEMENTED + TESTED | `etdl-compiler::performance` (`basic_event_overrides` default, unused) | `process_returns_typed_result_with_correct_extension_id` | — |

No dedicated `PERF-*` conformance vector file exists yet — this supplement
is a single-file, single-function piece of `etdl-compiler` (no separate
structural crate, unlike Tree Event) already fully exercised by the unit
and wiring tests listed above; a `PERF-*` file would substantially duplicate
that coverage under a different harness. Revisit if an external conformance
audit specifically requires vector-numbered traceability for these codes.

## Safety Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| Only processed when `supplements:` declares `etdl.safety` | IMPLEMENTED + TESTED | `etdl-compiler::safety` | `document_without_x_safety_has_no_diagnostics`, `document_not_declaring_safety_is_unaffected` | — |
| `hazards`/`barriers` optional at top level; malformed manifest -> E-130 | IMPLEMENTED + TESTED | `etdl-compiler::safety` | `missing_hazards_and_barriers_keys_are_not_an_error`, `malformed_hazards_produces_e130` | — |
| Duplicate hazard/barrier `id` -> E-130 | IMPLEMENTED + TESTED | `etdl-compiler::safety` | `duplicate_hazard_id_produces_e130`, `duplicate_barrier_id_produces_e130` | — |
| Hazard `severity`/`likelihood` enumerated, `riskIndex` in `[1,4]` -> else E-130 | IMPLEMENTED + TESTED | `etdl-compiler::safety` | `invalid_severity_produces_e130`, `invalid_likelihood_produces_e130`, `out_of_range_risk_index_produces_e130` | — |
| Barrier `sil` in `[1,4]` -> else E-130 | IMPLEMENTED + TESTED | `etdl-compiler::safety` | `out_of_range_sil_produces_e130` | — |
| `consequenceRef`/`nodeRef` resolve to the required node kind -> else E-131 | IMPLEMENTED + TESTED | `etdl-compiler::safety` | `consequence_ref_at_wrong_node_kind_produces_e131`, `barrier_node_ref_at_wrong_node_kind_produces_e131`, `unresolvable_node_ref_produces_e131` | — |
| Mutual `independentOf` + shared `commonCauseGroup` (direct or transitive) -> E-132 | IMPLEMENTED + TESTED | `etdl-compiler::safety` | `mutual_independent_of_with_shared_common_cause_group_produces_e132`, `one_sided_independent_of_does_not_produce_e132`, `mutual_independent_of_with_different_common_cause_groups_does_not_produce_e132` | — |
| `riskIndex` mismatched against the §4.1 risk matrix -> W-410 (warning only, hazard kept) | IMPLEMENTED + TESTED | `etdl-compiler::safety` | `mismatched_risk_index_produces_w410`, `matching_risk_index_has_no_w410` | — |
| Diagnostics surface through the real `Compiler::validate`/`compile` entry points | IMPLEMENTED + TESTED | `etdl-compiler::safety` (registered generically, not pipeline-special-cased — same shape as Performance) | `etdl-compiler/tests/safety_wiring_test.rs`, including a test proving Performance and Safety run together without interference | — |
| No effect on generated code, no fault-tree overrides | IMPLEMENTED + TESTED | `etdl-compiler::safety` (`basic_event_overrides` default, unused) | `process_returns_typed_result_with_correct_extension_id` | — |

No dedicated `SAFE-*` conformance vector file exists yet, for the same
reasoning as Performance's `PERF-*` decision above.

## Diagnostics Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| Only processed when `supplements:` declares `etdl.diagnostics` | IMPLEMENTED + TESTED | `etdl-compiler::diagnostics` | `document_without_x_diagnostics_has_no_diagnostics`, `document_not_declaring_diagnostics_is_unaffected` | — |
| `correlations`/`anomalyRules` optional at top level; malformed manifest -> E-150 | IMPLEMENTED + TESTED | `etdl-compiler::diagnostics` | `missing_correlations_and_anomaly_rules_keys_are_not_an_error`, `malformed_correlations_produces_e150` | — |
| `causeRef` resolves to a Gate or Basic Event -> else E-150 | IMPLEMENTED + TESTED | `etdl-compiler::diagnostics` | `unresolvable_cause_ref_produces_e150`, `cause_ref_at_undeclared_gate_produces_e150` | — |
| `monitors` resolves to any node kind -> else E-150 | IMPLEMENTED + TESTED | `etdl-compiler::diagnostics` | `unresolvable_monitors_produces_e150` | — |
| Duplicate Correlation/Anomaly Rule `id` (own collection only) -> E-151 | IMPLEMENTED + TESTED | `etdl-compiler::diagnostics` | `duplicate_correlation_id_produces_e151`, `duplicate_anomaly_rule_id_produces_e151`, `correlation_and_anomaly_rule_may_share_an_id` | — |
| Monitored Operation with no correlated cause -> W-412 (warning only, rule kept) | IMPLEMENTED + TESTED | `etdl-compiler::diagnostics` | `monitored_operation_with_no_probability_source_produces_w412`, `monitored_operation_with_uncorrelated_probability_source_produces_w412`, `monitored_operation_with_correlated_probability_source_has_no_w412` | — |
| Diagnostics surface through the real `Compiler::validate`/`compile` entry points | IMPLEMENTED + TESTED | `etdl-compiler::diagnostics` (registered generically, not pipeline-special-cased) | `etdl-compiler/tests/diagnostics_wiring_test.rs` | — |
| No effect on generated code, no fault-tree overrides | IMPLEMENTED + TESTED | `etdl-compiler::diagnostics` (`basic_event_overrides` default, unused) | `process_returns_typed_result_with_correct_extension_id` | — |

No dedicated `DIAG-*` conformance vector file exists yet, for the same
reasoning as Performance's `PERF-*` decision above.

## Security Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| Only processed when `supplements:` declares `etdl.security` | IMPLEMENTED + TESTED | `etdl-compiler::security` | `document_without_x_security_has_no_diagnostics` | — |
| `threatModels`/`controls` optional at top level; malformed manifest -> E-140/E-141 | IMPLEMENTED + TESTED | `etdl-compiler::security` | `malformed_threat_models_produces_e140`, `missing_threat_models_and_controls_keys_are_not_an_error` | — |
| `treeRef` resolves against `etdl.tree-event`'s parsed trees -> else E-140 (natural consequence when `etdl.tree-event` isn't declared) | IMPLEMENTED + TESTED | `etdl-compiler::security` (calls `tree_event::parse_and_validate_trees` directly — the one built-in cross-supplement dependency) | `unresolvable_tree_ref_produces_e140`, `security_without_tree_event_declared_has_unresolvable_tree_refs`, `etdl-compiler/tests/security_wiring_test.rs::tree_event_cross_dependency_resolves_through_the_real_pipeline` | — |
| `leafCategories` value is a STRIDE category -> else E-140 | IMPLEMENTED + TESTED | `etdl-compiler::security` | `invalid_stride_category_produces_e140` | — |
| `leafCategories` key / `mitigates` entry is a genuine leaf, Control `nodeRef` resolves to a Barrier, `mitigates` non-empty -> else E-141 | IMPLEMENTED + TESTED | `etdl-compiler::security` | `leaf_categories_key_at_non_leaf_produces_e141`, `control_node_ref_at_wrong_node_kind_produces_e141`, `empty_mitigates_produces_e141`, `mitigates_entry_not_a_leaf_produces_e141` | — |
| Duplicate Threat Model `id` -> E-140; duplicate Control `id` -> E-141 | IMPLEMENTED + TESTED | `etdl-compiler::security` | `duplicate_threat_model_id_produces_e140`, `duplicate_control_id_produces_e141` | — |
| Uncategorized leaf is not itself an error; but a Control mitigating one -> W-411 (warning only, control kept) | IMPLEMENTED + TESTED | `etdl-compiler::security` | `uncategorized_leaf_is_not_an_error`, `mitigates_entry_uncategorized_leaf_produces_w411` | — |
| Diagnostics surface through the real `Compiler::validate`/`compile` entry points | IMPLEMENTED + TESTED | `etdl-compiler::security` (registered generically, not pipeline-special-cased) | `etdl-compiler/tests/security_wiring_test.rs` | — |
| No effect on generated code, no fault-tree overrides | IMPLEMENTED + TESTED | `etdl-compiler::security` (`basic_event_overrides` default, unused) | `process_returns_typed_result_with_correct_extension_id` | — |

No dedicated `SEC-*` conformance vector file exists yet, for the same
reasoning as Performance's `PERF-*` decision above.

## Reliability Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| `0 <= P(E) <= 1` for probability-like metrics | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | existing + conformance | `REL-001` |
| NaN/infinity always rejected | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | existing + conformance | `REL-002` |
| `Unknown` never resolves to a scalar | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | existing + conformance | `REL-003` |
| Rate metrics not bounded to `[0,1]` (non-negative only) | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | conformance | `REL-004` |
| No implicit metric conversion | IMPLEMENTED + TESTED | `etdl-reliability-core::estimate` | existing + conformance | `REL-005` |
| Artifact round-trip (JSON/YAML), schema check | IMPLEMENTED + TESTED | `etdl-reliability-core::artifact` | existing + conformance | `ART-001`-`003` |
| Malformed artifact rejected, not panicking | IMPLEMENTED + TESTED | `etdl-reliability-core::artifact` | conformance | `ART-004` |
| Identity is event id, not array position | IMPLEMENTED + TESTED | `etdl-reliability-core::artifact` | conformance | `ART-005` |
| Duplicate estimate rejected | IMPLEMENTED + TESTED | `etdl-reliability-core::artifact` | existing + conformance | `ART-006` |

## Predictive Reliability Supplement 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| Exponential model: S(t)/h(t)/H(t) vs. independent reference | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | existing + conformance (independent oracle) | `PRED-001`, `PRED-002` |
| Weibull model: S(t)/h(t)/H(t), all shape regimes, vs. independent reference | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | existing + conformance (independent oracle) | `PRED-003` |
| `0 <= S(t) <= 1`, non-increasing | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | conformance | `PRED-004` |
| `0 <= F(t) <= 1` | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | conformance | `PRED-005` |
| `S(t) + F(t) = 1` | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | conformance | `PRED-006` |
| Parameter validity enforced at construction | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | existing + conformance | `PRED-007` |
| Numerical stability near `S(t) -> 0` | IMPLEMENTED + TESTED | `etdl-reliability::predictive::models` | existing + conformance | `PRED-008` |
| Extrapolation flag (declared vs. undeclared range) | IMPLEMENTED + TESTED | `etdl-reliability::predictive` | existing | — |
| Censoring representation (construction only, no fitting) | IMPLEMENTED + TESTED (fitting explicitly deferred) | `etdl-reliability::predictive::censoring` | existing | — |
| Calibration adapter (read-only, from `ReliabilityArtifact`) | IMPLEMENTED + TESTED | `etdl-reliability::predictive::calibration_adapter` | existing | — |
| Tree integration reuses `tree_adapter` unchanged | IMPLEMENTED + TESTED | `etdl-reliability::predictive::tree` | existing | — |
| Requires Reliability + Probability (one-directional) | IMPLEMENTED + TESTED | `etdl-reliability/Cargo.toml` | conformance | `ARCH-003` |
| `std.reliability` ETDL-source facade | NOT IMPLEMENTED (documented gap) | — | — | recommended future work |

## Runtime Feedback & Calibration 1.0

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| Two-sided exact binomial test vs. independent reference | IMPLEMENTED + TESTED | `etdl-reliability::calibration` | existing + conformance (independent oracle) | `CAL-001` |
| Deterministic calibration vector (fixed inputs -> fixed outputs) | IMPLEMENTED + TESTED | `etdl-reliability::calibration` | conformance | `CAL-002` |
| `calibrate()` never mutates the input artifact | IMPLEMENTED + TESTED | `etdl-reliability::calibration` | existing + conformance | `CAL-003` |
| Insufficient-exposure flag | IMPLEMENTED + TESTED | `etdl-reliability::calibration` | existing + conformance | `CAL-004` |
| Full loop (predict -> observe -> calibrate -> new artifact) never mutates the original | IMPLEMENTED + TESTED | `etdl-reliability::predictive` | existing (`predictive_reliability.rs` integration test) | — |

## Cross-cutting: dependency graph / architecture

| Requirement | Implemented? | Where | Test? | Conformance vector |
|---|---|---|---|---|
| `etdl-probability-core` zero dependency on reliability | IMPLEMENTED + TESTED | `etdl-probability-core/Cargo.toml` | conformance | `ARCH-001` |
| Generic Tree must NOT require Reliability | IMPLEMENTED + TESTED | `etdl-tree-core/Cargo.toml` | conformance | `ARCH-002` |
| Predictive Reliability requires Reliability + Probability | IMPLEMENTED + TESTED | `etdl-reliability/Cargo.toml` | conformance | `ARCH-003` |
| Compiler's reliability dependency is optional | IMPLEMENTED + TESTED | `etdl-compiler/Cargo.toml` | conformance | `ARCH-004` |
| WASM excludes the heavy reliability engine | IMPLEMENTED + TESTED (finding documented, see conformance guide) | `etdl-wasm/Cargo.toml` | conformance | `ARCH-005` |
| Workspace dependency graph is acyclic | IMPLEMENTED + TESTED | whole workspace | conformance | `ARCH-006` |
| CLI's reliability-family dependencies are all optional | IMPLEMENTED + TESTED | `etdl-cli/Cargo.toml` | conformance | `ARCH-007` |
