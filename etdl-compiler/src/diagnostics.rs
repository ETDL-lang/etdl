//! Compiler integration for the ETDL Diagnostics Supplement
//! (`etdl.diagnostics`).
//!
//! Reads a document's `x-diagnostics` extension field (the same generic
//! `x-*` mechanism every extension already uses — zero parser/AST changes
//! were needed), deserializes it into [`Correlation`]/[`AnomalyRule`]
//! values, and validates them. It declares which runtime telemetry
//! attribute a document's author expects to correlate with which
//! Fault-Tree cause. It still performs no automated inference of its own —
//! a Correlation is always author-declared, never computed — but
//! generated code now *surfaces* an already-declared Correlation
//! alongside an SLA anomaly independently detected at a matching node
//! (Section 6; `codegen/rust.rs`'s `diagnostics_correlation_for`,
//! `etdl_core::monitor::BranchMonitor::record_branch_with_cause` and its
//! `record_failure_with_cause`/`record_success_with_cause` siblings). This
//! is reference resolution and presentation, not inference: this module
//! never decides *whether* something is anomalous (`etdl_core::sla`
//! already does that, unchanged), only which already-declared metadata to
//! attach once an anomaly independently fires.
//!
//! Registered like [`crate::performance::PerformanceExtension`] and
//! [`crate::safety::SafetyExtension`] — no bespoke direct call anywhere in
//! `Compiler`'s pipeline (`lib.rs`). See
//! `docs/reference/diagnostics-supplement.md`.
//!
//! ## Interpretations beyond the literal diagnostic table
//!
//! - A `correlations`/`anomalyRules` manifest that fails to deserialize at
//!   all is folded into **E-150** (no dedicated "manifest invalid" code
//!   exists here either, matching Performance's/Safety's own precedent).
//! - **E-152** only checks a Correlation's `spanValue` against real node
//!   ids when its `spanAttribute` is exactly `"etdl.node.id"` — the only
//!   attribute the reference runtime ever emits
//!   (`etdl_core::telemetry::attach_node_span_attribute`). Any other
//!   `spanAttribute` value is left unchecked (spec Section 4.1: both
//!   fields are free-form), since this specification does not own or
//!   interpret third-party telemetry attribute names.
//! - **W-412**'s prose ("an Operation with neither `onFailureProbabilitySource`
//!   nor any Fault Tree reachable from this document that a Correlation
//!   Object's `causeRef` could plausibly connect it to") is implemented as:
//!   the monitored node is an Operation, AND either (a) it has no
//!   `onFailureProbabilitySource` at all, or (b) it does, but no declared
//!   Correlation's `causeRef` targets the *same* Fault Tree that source
//!   points into. A `monitors` node that is a Barrier or Consequence is
//!   never checked by this rule — the spec's text names Operations only.

use std::collections::BTreeSet;

use etdl_parser::ast::{EtlDocument, Node};

use crate::validate::Diagnostic;

const DIAGNOSTICS_SUPPLEMENT: &str = "etdl.diagnostics";
pub const DIAGNOSTICS_SCHEMA: &str = "etdl.diagnostics/1.0";

/// One Correlation Object under `x-diagnostics.correlations` (Section 4.1).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Correlation {
    pub id: String,
    #[serde(rename = "spanAttribute")]
    pub span_attribute: String,
    #[serde(rename = "spanValue")]
    pub span_value: String,
    #[serde(rename = "causeRef")]
    pub cause_ref: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// One Anomaly Rule Object under `x-diagnostics.anomalyRules` (Section
/// 4.2).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AnomalyRule {
    pub id: String,
    pub monitors: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Every Correlation/Anomaly Rule that parsed and validated successfully.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsData {
    pub correlations: Vec<Correlation>,
    pub anomaly_rules: Vec<AnomalyRule>,
}

/// Read and validate every Correlation/Anomaly Rule declared under
/// `x-diagnostics` in the document. An object that failed any check is
/// omitted from the returned [`DiagnosticsData`] but always produces a
/// diagnostic.
pub fn parse_and_validate_diagnostics(doc: &EtlDocument) -> (DiagnosticsData, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut data = DiagnosticsData::default();

    if !crate::validate::declares_supplement(doc, DIAGNOSTICS_SUPPLEMENT) {
        return (data, diagnostics);
    }

    let Some(ext) = doc.extensions.get("x-diagnostics") else {
        return (data, diagnostics);
    };

    if let Some(raw) = ext.get("correlations") {
        match serde_yaml::from_value::<Vec<Correlation>>(raw.clone()) {
            Ok(candidates) => {
                let mut seen_ids = BTreeSet::new();
                for correlation in candidates {
                    let mut has_error = false;

                    if !seen_ids.insert(correlation.id.clone()) {
                        diagnostics.push(Diagnostic::error(
                            "E-151",
                            format!("x-diagnostics: duplicate correlation id '{}'", correlation.id),
                        ));
                        has_error = true;
                    }

                    if !resolve_cause_ref(doc, &correlation.cause_ref) {
                        diagnostics.push(Diagnostic::error(
                            "E-150",
                            format!(
                                "x-diagnostics: correlation '{}': causeRef '{}' does not resolve to a Gate or Basic Event",
                                correlation.id, correlation.cause_ref
                            ),
                        ));
                        has_error = true;
                    }

                    // E-152: only checked for `etdl.node.id` specifically —
                    // the *only* attribute the reference runtime ever emits
                    // (`etdl_core::telemetry::attach_node_span_attribute`).
                    // `spanAttribute`/`spanValue` are otherwise free-form
                    // (spec Section 4.1), so this stays a targeted
                    // cross-check for the one attribute this codebase's own
                    // runtime actually produces, not a blanket requirement.
                    if correlation.span_attribute == "etdl.node.id"
                        && !node_id_exists_anywhere(doc, &correlation.span_value)
                    {
                        diagnostics.push(Diagnostic::error(
                            "E-152",
                            format!(
                                "x-diagnostics: correlation '{}': spanAttribute is 'etdl.node.id' but spanValue '{}' does not name a node in any declared eventTree",
                                correlation.id, correlation.span_value
                            ),
                        ));
                        has_error = true;
                    }

                    if !has_error {
                        data.correlations.push(correlation);
                    }
                }
            }
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E-150",
                    format!("x-diagnostics: invalid correlation manifest: {e}"),
                ));
            }
        }
    }

    if let Some(raw) = ext.get("anomalyRules") {
        match serde_yaml::from_value::<Vec<AnomalyRule>>(raw.clone()) {
            Ok(candidates) => {
                let mut seen_ids = BTreeSet::new();
                for rule in candidates {
                    let mut has_error = false;

                    if !seen_ids.insert(rule.id.clone()) {
                        diagnostics.push(Diagnostic::error(
                            "E-151",
                            format!("x-diagnostics: duplicate anomaly rule id '{}'", rule.id),
                        ));
                        has_error = true;
                    }

                    let node = resolve_monitors_ref(doc, &rule.monitors);
                    if node.is_none() {
                        diagnostics.push(Diagnostic::error(
                            "E-150",
                            format!(
                                "x-diagnostics: anomaly rule '{}': monitors '{}' does not resolve to a node",
                                rule.id, rule.monitors
                            ),
                        ));
                        has_error = true;
                    }

                    // Only meaningful once `monitors` itself resolved —
                    // otherwise this would double-report the same
                    // underlying problem E-150 already covers.
                    if let Some(Node::Operation(op)) = node {
                        if operation_lacks_correlated_cause(op, &data.correlations) {
                            diagnostics.push(Diagnostic::warning(
                                "W-412",
                                format!(
                                    "x-diagnostics: anomaly rule '{}': monitored Operation '{}' has no correlated cause on record",
                                    rule.id, rule.monitors
                                ),
                            ));
                        }
                    }

                    if !has_error {
                        data.anomaly_rules.push(rule);
                    }
                }
            }
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E-150",
                    format!("x-diagnostics: invalid anomaly rule manifest: {e}"),
                ));
            }
        }
    }

    (data, diagnostics)
}

/// Resolve a Correlation's `causeRef` against the document's own
/// `faultTrees` (the same manual-parse style `performance`/`safety` use —
/// no generic JSON-Pointer resolver exists in this codebase for
/// same-document references).
fn resolve_cause_ref(doc: &EtlDocument, cause_ref: &str) -> bool {
    let rest = cause_ref.trim_start_matches('#');
    let Some(after) = rest.strip_prefix("/faultTrees/") else {
        return false;
    };
    match after.split('/').collect::<Vec<_>>().as_slice() {
        [tree_id, "gates", gate_id] if !tree_id.is_empty() && !gate_id.is_empty() => doc
            .fault_trees
            .as_ref()
            .and_then(|fts| fts.get(*tree_id))
            .and_then(|ft| ft.gates.as_ref())
            .is_some_and(|gates| gates.contains_key(*gate_id)),
        [tree_id, "basicEvents", event_id] if !tree_id.is_empty() && !event_id.is_empty() => doc
            .fault_trees
            .as_ref()
            .and_then(|fts| fts.get(*tree_id))
            .is_some_and(|ft| ft.basic_events.contains_key(*event_id)),
        _ => false,
    }
}

/// Whether `node_id` names a node in *any* of the document's `eventTrees`
/// (any tree, any node kind) — needed for E-152, which is deliberately
/// document-wide, not scoped to one tree (a Correlation's `spanValue` for
/// `etdl.node.id` has no tree context of its own to narrow the search).
fn node_id_exists_anywhere(doc: &EtlDocument, node_id: &str) -> bool {
    doc.event_trees.values().any(|t| t.nodes.contains_key(node_id))
}

/// Resolve an Anomaly Rule's `monitors` against the document's own
/// `eventTrees` — unlike `performance`'s `nodeRef` (Operation or whole
/// tree) and `safety`'s (Barrier only), `monitors` accepts **any** node
/// kind (spec Section 4.2: "any node kind, core Section 5.7").
fn resolve_monitors_ref<'a>(doc: &'a EtlDocument, node_ref: &str) -> Option<&'a Node> {
    let rest = node_ref.trim_start_matches('#');
    let after = rest.strip_prefix("/eventTrees/")?;
    match after.split('/').collect::<Vec<_>>().as_slice() {
        [tree_id, "nodes", node_id] if !tree_id.is_empty() && !node_id.is_empty() => {
            doc.event_trees.get(*tree_id).and_then(|t| t.nodes.get(*node_id))
        }
        _ => None,
    }
}

/// See the module doc comment's "Interpretations" section for the exact
/// reading of W-412 this implements.
fn operation_lacks_correlated_cause(op: &etdl_parser::ast::Operation, correlations: &[Correlation]) -> bool {
    let Some(prob_source) = &op.on_failure_probability_source else {
        return true;
    };
    let Some(ft_id) = fault_tree_id_from_pointer(&prob_source.pointer) else {
        return true;
    };
    !correlations
        .iter()
        .any(|c| fault_tree_id_from_pointer(&c.cause_ref) == Some(ft_id))
}

/// The leading `<id>` segment of a `#/faultTrees/<id>/...` pointer, shared
/// by `Operation::on_failure_probability_source` and a Correlation's
/// `causeRef` — both use the same pointer shape.
fn fault_tree_id_from_pointer(pointer: &str) -> Option<&str> {
    let rest = pointer.trim_start_matches('#');
    let after = rest.strip_prefix("/faultTrees/")?;
    after.split('/').next()
}

/// The built-in Diagnostics Supplement extension.
#[derive(Debug, Default)]
pub struct DiagnosticsExtension;

impl DiagnosticsExtension {
    pub fn new() -> Self {
        DiagnosticsExtension
    }
}

/// The typed result of the diagnostics extension's processing step. Uses
/// [`crate::extension::ExtensionResult`]'s default (empty)
/// `basic_event_overrides()` — a Correlation/Anomaly Rule never resolves
/// into a fault-tree probability; this supplement performs no automated
/// correlation of its own (module docs).
pub struct DiagnosticsResult {
    pub correlations: Vec<Correlation>,
    pub anomaly_rules: Vec<AnomalyRule>,
}

impl crate::extension::ExtensionResult for DiagnosticsResult {
    fn extension_id(&self) -> &str {
        DIAGNOSTICS_SUPPLEMENT
    }
}

impl crate::extension::EtdlExtension for DiagnosticsExtension {
    fn id(&self) -> &str {
        DIAGNOSTICS_SUPPLEMENT
    }

    fn version(&self) -> &str {
        "1.0"
    }

    fn descriptor(&self) -> crate::extension::SupplementDescriptor {
        crate::extension::SupplementDescriptor {
            summary: "Declared telemetry-span-to-Fault-Tree-cause correlations and \
                      monitored-node anomaly rules; structural metadata only, no automated \
                      correlation or inference.",
            schema: Some(DIAGNOSTICS_SCHEMA),
            diagnostic_codes: &["E-150", "E-151", "E-152", "W-412"],
            requires: &[],
        }
    }

    fn validate(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let (_data, extra) = parse_and_validate_diagnostics(doc);
        diagnostics.extend(extra);
    }

    /// Deliberately does **not** extend `diagnostics` again — same reason
    /// as `performance`/`safety`: `run_extensions` only skips `process()`
    /// after an *error*, so W-412 (a warning) would otherwise be reported
    /// twice every time this extension actually runs through the real
    /// pipeline.
    fn process(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn crate::extension::ExtensionResult + '_> {
        let (data, _extra) = parse_and_validate_diagnostics(doc);
        Box::new(DiagnosticsResult {
            correlations: data.correlations,
            anomaly_rules: data.anomaly_rules,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::{builtin_registry, EtdlExtension, ExtensionContext};

    /// `eventTrees` includes a fault-tree-linked Operation
    /// (`ProcessPaymentOperation`, `onFailureProbabilitySource` pointing at
    /// `PaymentGatewayFailure`) and a plain Barrier (`RetryBarrier`, no
    /// probability source of any kind — used for the "monitors a
    /// non-Operation, W-412 never applies" test). `faultTrees` includes one
    /// gate and one basic event.
    fn doc_with_diagnostics(x_diagnostics_yaml: &str) -> EtlDocument {
        let yaml = format!(
            r##"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
supplements:
  - id: etdl.diagnostics
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
        onFailureProbabilitySource: "#/faultTrees/PaymentGatewayFailure/topEvent"
      C: {{ type: consequence, operation: terminate }}
faultTrees:
  PaymentGatewayFailure:
    topEvent: {{ id: Top, description: "t", rootCause: GatewayUnreachable }}
    basicEvents:
      GatewayUnreachable: {{ description: "d", probability: 0.01 }}
x-diagnostics:
{x_diagnostics_yaml}
"##
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn diagnostics_extension_is_registered_and_built_in() {
        let registry = builtin_registry();
        assert!(registry.contains(DIAGNOSTICS_SUPPLEMENT));
        assert!(registry.list().contains(&DIAGNOSTICS_SUPPLEMENT));
    }

    #[test]
    fn document_without_x_diagnostics_has_no_diagnostics() {
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
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(data.correlations.is_empty());
        assert!(data.anomaly_rules.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn valid_correlation_and_correlated_anomaly_rule_have_no_diagnostics() {
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: gateway-timeout-correlation
      spanAttribute: "etdl.node.id"
      spanValue: "ProcessPaymentOperation"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
  anomalyRules:
    - id: payment-operation-anomaly
      monitors: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
"##,
        );
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(data.correlations.len(), 1);
        assert_eq!(data.anomaly_rules.len(), 1);
    }

    #[test]
    fn missing_correlations_and_anomaly_rules_keys_are_not_an_error() {
        let doc = doc_with_diagnostics("  {}");
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(data.correlations.is_empty());
        assert!(data.anomaly_rules.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn malformed_correlations_produces_e150() {
        let doc = doc_with_diagnostics("  correlations: \"oops\"");
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(data.correlations.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-150"));
    }

    #[test]
    fn unresolvable_cause_ref_produces_e150() {
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: c1
      spanAttribute: "etdl.node.id"
      spanValue: "x"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/DoesNotExist"
"##,
        );
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(data.correlations.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-150"));
    }

    #[test]
    fn cause_ref_at_undeclared_gate_produces_e150() {
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: c1
      spanAttribute: "etdl.node.id"
      spanValue: "x"
      causeRef: "#/faultTrees/PaymentGatewayFailure/gates/DoesNotExist"
"##,
        );
        // No gates declared on PaymentGatewayFailure in the fixture -> unresolvable.
        let (_data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-150"));
    }

    #[test]
    fn unresolvable_monitors_produces_e150() {
        let doc = doc_with_diagnostics(
            r##"  anomalyRules:
    - id: r1
      monitors: "#/eventTrees/OrderFulfillment/nodes/DoesNotExist"
"##,
        );
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(data.anomaly_rules.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-150"));
    }

    #[test]
    fn duplicate_correlation_id_produces_e151() {
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: dup
      spanAttribute: "a"
      spanValue: "x"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
    - id: dup
      spanAttribute: "b"
      spanValue: "y"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-151" && d.message.contains("duplicate correlation id")));
    }

    #[test]
    fn duplicate_anomaly_rule_id_produces_e151() {
        let doc = doc_with_diagnostics(
            r##"  anomalyRules:
    - id: dup
      monitors: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
    - id: dup
      monitors: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-151" && d.message.contains("duplicate anomaly rule id")));
    }

    #[test]
    fn correlation_and_anomaly_rule_may_share_an_id() {
        // E-151 only forbids a duplicate *within* correlations or *within*
        // anomalyRules — the two collections are independent namespaces.
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: shared
      spanAttribute: "a"
      spanValue: "x"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
  anomalyRules:
    - id: shared
      monitors: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(!diagnostics.iter().any(|d| d.code == "E-151"));
    }

    #[test]
    fn monitored_operation_with_no_probability_source_produces_w412() {
        let doc = doc_with_diagnostics(
            r##"  anomalyRules:
    - id: r1
      monitors: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
"##,
        );
        // RetryBarrier is a Barrier, not an Operation -> W-412 never applies.
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert_eq!(data.anomaly_rules.len(), 1);
        assert!(!diagnostics.iter().any(|d| d.code == "W-412"));
    }

    #[test]
    fn monitored_operation_with_uncorrelated_probability_source_produces_w412() {
        let doc = doc_with_diagnostics(
            r##"  anomalyRules:
    - id: r1
      monitors: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
"##,
        );
        // ProcessPaymentOperation DOES have onFailureProbabilitySource, but
        // no Correlation Object targets PaymentGatewayFailure -> W-412.
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        // W-412 is a warning: the anomaly rule is still accepted.
        assert_eq!(data.anomaly_rules.len(), 1);
        assert!(diagnostics.iter().any(|d| d.code == "W-412"));
        assert!(!diagnostics.iter().any(|d| d.is_error()));
    }

    #[test]
    fn monitored_operation_with_correlated_probability_source_has_no_w412() {
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: c1
      spanAttribute: "etdl.node.id"
      spanValue: "ProcessPaymentOperation"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
  anomalyRules:
    - id: r1
      monitors: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    }

    #[test]
    fn process_returns_typed_result_with_correct_extension_id() {
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: c1
      spanAttribute: "etdl.node.id"
      spanValue: "ProcessPaymentOperation"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
"##,
        );
        let ext = DiagnosticsExtension::new();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        let result = ext.process(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(result.extension_id(), DIAGNOSTICS_SUPPLEMENT);
        assert!(result.basic_event_overrides().is_empty());
    }

    #[test]
    fn etdl_node_id_span_value_not_a_real_node_produces_e152() {
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: c1
      spanAttribute: "etdl.node.id"
      spanValue: "DoesNotExist"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
"##,
        );
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(data.correlations.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-152"), "got {diagnostics:?}");
    }

    #[test]
    fn etdl_node_id_span_value_matching_a_real_node_has_no_e152() {
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: c1
      spanAttribute: "etdl.node.id"
      spanValue: "ProcessPaymentOperation"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
"##,
        );
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(data.correlations.len(), 1);
    }

    #[test]
    fn non_etdl_node_id_span_attribute_is_never_checked_against_nodes() {
        // spanAttribute/spanValue are otherwise free-form (spec Section
        // 4.1) — E-152 only ever fires for the one attribute the reference
        // runtime actually emits.
        let doc = doc_with_diagnostics(
            r##"  correlations:
    - id: c1
      spanAttribute: "some.other.attribute"
      spanValue: "DoesNotExistEither"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
"##,
        );
        let (data, diagnostics) = parse_and_validate_diagnostics(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(data.correlations.len(), 1);
    }
}
