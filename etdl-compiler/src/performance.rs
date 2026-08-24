//! Compiler integration for the ETDL Performance Supplement
//! (`etdl.performance`).
//!
//! Reads a document's `x-performance` extension field (the same generic
//! `x-*` mechanism every extension already uses — zero parser/AST changes
//! were needed), deserializes it into [`Budget`] values, and validates
//! them. Purely declarative: per spec Section 6, a Budget's percentile
//! targets are never enforced at runtime and never translated into a
//! fault-tree probability override — `process()` uses the trait's default
//! (empty) `basic_event_overrides()`.
//!
//! Unlike the Tree Event and Reliability supplements, this extension is
//! **not** special-cased with a direct function call anywhere in
//! `Compiler`'s pipeline (`lib.rs`). It is registered into
//! [`crate::extension::builtin_registry`] (so `etdl capabilities`/`etdl
//! supplement list`/the E-108 "is this supplement supported" check all see
//! it) *and* pushed into `Compiler::new()`'s `extensions` list, so it runs
//! through the same generic, registry-driven `EtdlExtension::validate`/
//! `process` path (`Compiler::run_extensions`) that a third-party
//! `Compiler::with_extension` supplement uses — built-in only in the sense
//! that it ships compiled into the binary and is auto-registered, not in
//! the sense of having bespoke pipeline code of its own. See
//! `docs/reference/performance-supplement.md`.

use std::collections::BTreeSet;

use etdl_parser::ast::{EtlDocument, Node};

use crate::validate::Diagnostic;

const PERFORMANCE_SUPPLEMENT: &str = "etdl.performance";
pub const PERFORMANCE_SCHEMA: &str = "etdl.performance/1.0";

/// One Budget Object under `x-performance.budgets`
/// (`ETDL-Performance-Supplement.md` Section 4.1). Structure is via serde;
/// range/ordering/reference rules are hand-checked in
/// [`parse_and_validate_budgets`], the same "structure via serde, rules via
/// explicit checks" split every other supplement in this compiler uses (no
/// JSON-Schema-validation engine is used anywhere in this codebase).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Budget {
    pub id: String,
    #[serde(rename = "nodeRef")]
    pub node_ref: String,
    #[serde(rename = "p50Ms")]
    pub p50_ms: f64,
    #[serde(rename = "p95Ms")]
    pub p95_ms: f64,
    #[serde(rename = "p99Ms")]
    pub p99_ms: f64,
    /// `i64`, not `u64`: a negative value must deserialize successfully and
    /// then be rejected by an explicit `<= 0` check with a clear E-160
    /// message, rather than becoming an opaque serde type error.
    #[serde(default, rename = "maxConcurrency")]
    pub max_concurrency: Option<i64>,
    #[serde(default, rename = "expectedRatePerSecond")]
    pub expected_rate_per_second: Option<f64>,
}

/// Read every Budget Object declared under `x-performance.budgets` in the
/// document. Returns `(budgets, diagnostics)`: a budget that failed any
/// check is omitted from `budgets` but always produces a diagnostic (except
/// `W-413`, whose budget is still returned — a duplicate `nodeRef` is a
/// warning, not a rejection).
pub fn parse_and_validate_budgets(doc: &EtlDocument) -> (Vec<Budget>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut budgets = Vec::new();

    // `x-performance` is only processed when the document explicitly opts
    // in via `supplements:`, never merely because the extension field
    // happens to be present — the same gate every other supplement uses.
    if !crate::validate::declares_supplement(doc, PERFORMANCE_SUPPLEMENT) {
        return (budgets, diagnostics);
    }

    let Some(ext) = doc.extensions.get("x-performance") else {
        return (budgets, diagnostics);
    };

    // Unlike Tree Event's `trees`, `budgets` is OPTIONAL at the top level
    // (no `"required"` array in the JSON Schema) — a missing key is not an
    // error.
    let Some(raw_budgets) = ext.get("budgets") else {
        return (budgets, diagnostics);
    };

    let candidates: Vec<Budget> = match serde_yaml::from_value(raw_budgets.clone()) {
        Ok(b) => b,
        Err(e) => {
            // The spec's diagnostic table (Section 5) has no dedicated
            // "manifest invalid" code (unlike Tree Event's E-120) — E-160 is
            // already a multi-condition catch-all for this object, so a
            // malformed manifest is folded into it.
            diagnostics.push(Diagnostic::error(
                "E-160",
                format!("x-performance: invalid budget manifest: {e}"),
            ));
            return (budgets, diagnostics);
        }
    };

    let mut seen_ids = BTreeSet::new();
    let mut seen_node_refs = BTreeSet::new();

    for budget in candidates {
        let mut has_error = false;

        // Spec Section 4.1 marks `id` REQUIRED and unique within
        // `x-performance.budgets` (a MUST) but Section 5 has no dedicated
        // code for a duplicate id, so — like the malformed-manifest case
        // above — it is folded into E-160.
        if !seen_ids.insert(budget.id.clone()) {
            diagnostics.push(Diagnostic::error(
                "E-160",
                format!("x-performance: duplicate budget id '{}'", budget.id),
            ));
            has_error = true;
        }

        if !resolve_node_ref(doc, &budget.node_ref) {
            diagnostics.push(Diagnostic::error(
                "E-160",
                format!(
                    "x-performance: budget '{}': nodeRef '{}' does not resolve to an Event Tree or an Operation node",
                    budget.id, budget.node_ref
                ),
            ));
            has_error = true;
        }

        for (field, value) in [
            ("p50Ms", budget.p50_ms),
            ("p95Ms", budget.p95_ms),
            ("p99Ms", budget.p99_ms),
        ] {
            if !value.is_finite() || value <= 0.0 {
                diagnostics.push(Diagnostic::error(
                    "E-160",
                    format!(
                        "x-performance: budget '{}': {field} must be a positive, finite number (got {value})",
                        budget.id
                    ),
                ));
                has_error = true;
            }
        }

        if let Some(max_concurrency) = budget.max_concurrency {
            if max_concurrency <= 0 {
                diagnostics.push(Diagnostic::error(
                    "E-160",
                    format!(
                        "x-performance: budget '{}': maxConcurrency must be positive (got {max_concurrency})",
                        budget.id
                    ),
                ));
                has_error = true;
            }
        }
        if let Some(expected_rate) = budget.expected_rate_per_second {
            if !expected_rate.is_finite() || expected_rate <= 0.0 {
                diagnostics.push(Diagnostic::error(
                    "E-160",
                    format!(
                        "x-performance: budget '{}': expectedRatePerSecond must be a positive, finite number (got {expected_rate})",
                        budget.id
                    ),
                ));
                has_error = true;
            }
        }

        // Checked unconditionally alongside the E-160 checks above (not
        // gated on them passing first): comparisons against a non-finite or
        // non-positive value simply won't produce a spurious ordering
        // violation, so no extra "only check ordering if otherwise valid"
        // branch is needed.
        if budget.p50_ms > budget.p95_ms || budget.p95_ms > budget.p99_ms {
            diagnostics.push(Diagnostic::error(
                "E-161",
                format!(
                    "x-performance: budget '{}': percentile ordering violated (p50Ms={}, p95Ms={}, p99Ms={})",
                    budget.id, budget.p50_ms, budget.p95_ms, budget.p99_ms
                ),
            ));
            has_error = true;
        }

        // Not an error: the second budget declaring a given `nodeRef` is
        // still valid and still included in the result — only flagged as
        // not meaningfully authoritative for that node.
        if !seen_node_refs.insert(budget.node_ref.clone()) {
            diagnostics.push(Diagnostic::warning(
                "W-413",
                format!(
                    "x-performance: nodeRef '{}' is declared by more than one budget; only one is meaningfully authoritative",
                    budget.node_ref
                ),
            ));
        }

        if !has_error {
            budgets.push(budget);
        }
    }

    (budgets, diagnostics)
}

/// Resolve a Budget's `nodeRef` against the document's own `eventTrees`,
/// matching the JSON Schema's `^#/eventTrees/[^/]+(/nodes/[^/]+)?$` pattern
/// by hand (no regex dependency) via shape-matching on `split('/')` — the
/// same manual-parse style `check_transfers` (fault-tree transfer target
/// resolution, `validate.rs`) uses for internal cross-references; there is
/// no generic JSON-Pointer resolver for same-document references in this
/// codebase (`etdl_parser::jsonptr` is only used for AsyncAPI E-104).
/// Additionally enforces the spec's restriction that a node-level `nodeRef`
/// must specifically name an Operation node, not a Barrier or Consequence —
/// a restriction the pattern alone cannot express.
fn resolve_node_ref(doc: &EtlDocument, node_ref: &str) -> bool {
    let rest = node_ref.trim_start_matches('#');
    let Some(after) = rest.strip_prefix("/eventTrees/") else {
        return false;
    };
    match after.split('/').collect::<Vec<_>>().as_slice() {
        [tree_id] if !tree_id.is_empty() => doc.event_trees.contains_key(*tree_id),
        [tree_id, "nodes", node_id] if !tree_id.is_empty() && !node_id.is_empty() => doc
            .event_trees
            .get(*tree_id)
            .and_then(|t| t.nodes.get(*node_id))
            .is_some_and(|n| matches!(n, Node::Operation(_))),
        _ => false,
    }
}

/// The built-in Performance Supplement extension.
#[derive(Debug, Default)]
pub struct PerformanceExtension;

impl PerformanceExtension {
    pub fn new() -> Self {
        PerformanceExtension
    }
}

/// The typed result of the performance extension's processing step: every
/// budget that parsed and validated successfully. Uses
/// [`crate::extension::ExtensionResult`]'s default (empty)
/// `basic_event_overrides()` — a Budget never resolves into a fault-tree
/// probability (spec Section 6).
pub struct PerformanceResult {
    pub budgets: Vec<Budget>,
}

impl crate::extension::ExtensionResult for PerformanceResult {
    fn extension_id(&self) -> &str {
        PERFORMANCE_SUPPLEMENT
    }
}

impl crate::extension::EtdlExtension for PerformanceExtension {
    fn id(&self) -> &str {
        PERFORMANCE_SUPPLEMENT
    }

    fn version(&self) -> &str {
        "1.0"
    }

    fn descriptor(&self) -> crate::extension::SupplementDescriptor {
        crate::extension::SupplementDescriptor {
            summary: "Declared latency-percentile budgets (p50Ms/p95Ms/p99Ms) and an optional \
                      throughput expectation for an Operation or a whole Event Tree; declarative \
                      only, never enforced at runtime.",
            schema: Some(PERFORMANCE_SCHEMA),
            diagnostic_codes: &["E-160", "E-161", "W-413"],
            requires: &[],
        }
    }

    fn validate(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let (_budgets, budget_diagnostics) = parse_and_validate_budgets(doc);
        diagnostics.extend(budget_diagnostics);
    }

    /// Deliberately does **not** extend `diagnostics` again: `run_extensions`
    /// only skips `process()` after an *error* from `validate()` (warnings
    /// don't block it — see `Compiler::run_extensions`), so a `process()`
    /// that re-ran the same diagnostic-producing checks would duplicate
    /// every warning (e.g. W-413) every time this extension actually runs
    /// through the real pipeline. `validate()` already reported everything
    /// there is to report; this just recomputes the same deterministic
    /// `budgets` list for [`PerformanceResult`].
    fn process(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn crate::extension::ExtensionResult + '_> {
        let (budgets, _budget_diagnostics) = parse_and_validate_budgets(doc);
        Box::new(PerformanceResult { budgets })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::{builtin_registry, EtdlExtension, ExtensionContext};

    /// `eventTrees` includes one Operation node (`ProcessPaymentOperation`)
    /// and one Barrier node (`RetryBarrier`), so tests can exercise both
    /// valid `nodeRef` shapes and the Operation-only restriction.
    fn doc_with_budgets(x_performance_yaml: &str) -> EtlDocument {
        let yaml = format!(
            r#"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
supplements:
  - id: etdl.performance
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: {{ id: I, message: "a#/m", next: RetryBarrier }}
    nodes:
      RetryBarrier:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: default
            probability: 1.0
            next: ProcessPaymentOperation
      ProcessPaymentOperation:
        type: operation
        action: execute
        handler: "h"
        next: C
      C: {{ type: consequence, operation: terminate }}
x-performance:
{x_performance_yaml}
"#
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn performance_extension_is_registered_and_built_in() {
        let registry = builtin_registry();
        assert!(registry.contains(PERFORMANCE_SUPPLEMENT));
        assert!(registry.list().contains(&PERFORMANCE_SUPPLEMENT));
    }

    #[test]
    fn document_without_x_performance_has_no_diagnostics() {
        let yaml = r#"
etdl: "1.0.0"
info: { title: "T", version: "1.0.0", domain: "D" }
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
"#;
        let doc: EtlDocument = serde_yaml::from_str(yaml).unwrap();
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(budgets.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn valid_budget_operation_node_ref_has_no_diagnostics() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: op-budget
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
      p50Ms: 150
      p95Ms: 800
      p99Ms: 2000
"##,
        );
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(budgets.len(), 1);
    }

    #[test]
    fn valid_budget_whole_tree_node_ref_has_no_diagnostics() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: e2e-budget
      nodeRef: "#/eventTrees/OrderFulfillment"
      p50Ms: 400
      p95Ms: 2500
      p99Ms: 5000
      maxConcurrency: 200
      expectedRatePerSecond: 50
"##,
        );
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(budgets.len(), 1);
    }

    #[test]
    fn missing_budgets_key_is_not_an_error() {
        let doc = doc_with_budgets("  {}");
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(budgets.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn malformed_budgets_produces_e160() {
        let doc = doc_with_budgets("  budgets: \"oops\"");
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(budgets.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-160"));
    }

    #[test]
    fn bad_percentile_ordering_produces_e161() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: bad-order
      nodeRef: "#/eventTrees/OrderFulfillment"
      p50Ms: 900
      p95Ms: 800
      p99Ms: 2000
"##,
        );
        let (_budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-161"));
        assert!(!diagnostics.iter().any(|d| d.code == "E-160"));
    }

    #[test]
    fn duplicate_node_ref_produces_w413_and_keeps_both_budgets() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: first
      nodeRef: "#/eventTrees/OrderFulfillment"
      p50Ms: 100
      p95Ms: 200
      p99Ms: 300
    - id: second
      nodeRef: "#/eventTrees/OrderFulfillment"
      p50Ms: 100
      p95Ms: 200
      p99Ms: 300
"##,
        );
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert_eq!(budgets.len(), 2);
        assert!(diagnostics.iter().any(|d| d.code == "W-413"));
        assert!(!diagnostics.iter().any(|d| d.is_error()));
    }

    #[test]
    fn unresolvable_node_ref_produces_e160() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: dangling
      nodeRef: "#/eventTrees/DoesNotExist"
      p50Ms: 100
      p95Ms: 200
      p99Ms: 300
"##,
        );
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(budgets.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-160"));
    }

    #[test]
    fn node_ref_at_barrier_is_rejected_produces_e160() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: at-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      p50Ms: 100
      p95Ms: 200
      p99Ms: 300
"##,
        );
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(budgets.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-160"));
    }

    #[test]
    fn non_positive_percentile_produces_e160_without_spurious_e161() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: negative-p50
      nodeRef: "#/eventTrees/OrderFulfillment"
      p50Ms: -5
      p95Ms: 200
      p99Ms: 300
"##,
        );
        let (budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(budgets.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-160"));
        assert!(!diagnostics.iter().any(|d| d.code == "E-161"));
    }

    #[test]
    fn duplicate_budget_id_produces_e160() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: dup
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
      p50Ms: 100
      p95Ms: 200
      p99Ms: 300
    - id: dup
      nodeRef: "#/eventTrees/OrderFulfillment"
      p50Ms: 100
      p95Ms: 200
      p99Ms: 300
"##,
        );
        let (_budgets, diagnostics) = parse_and_validate_budgets(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-160" && d.message.contains("duplicate budget id")));
    }

    #[test]
    fn process_returns_typed_result_with_correct_extension_id() {
        let doc = doc_with_budgets(
            r##"  budgets:
    - id: op-budget
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
      p50Ms: 150
      p95Ms: 800
      p99Ms: 2000
"##,
        );
        let ext = PerformanceExtension::new();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        let result = ext.process(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(result.extension_id(), PERFORMANCE_SUPPLEMENT);
        assert!(result.basic_event_overrides().is_empty());
    }
}
