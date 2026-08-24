//! Compiler integration for the ETDL Security Supplement (`etdl.security`).
//!
//! Reads a document's `x-security` extension field (the same generic `x-*`
//! mechanism every extension already uses — zero parser/AST changes were
//! needed), deserializes it into [`ThreatModel`]/[`Control`] values, and
//! validates them. Defines no new tree structure of its own: an attack
//! tree is structurally identical to any other Tree Event Supplement
//! (`etdl.tree-event`) tree, so this supplement reuses that supplement's
//! already-validated [`etdl_tree_core::Tree`] under a security
//! interpretation (a STRIDE category per leaf) rather than redefining tree
//! structure, and separately maps mitigating Controls onto core Barrier
//! nodes — the same "give existing core structure a domain meaning"
//! pattern [`crate::safety`] uses for the same Barrier node under a
//! different interpretation.
//!
//! **This is the one built-in supplement with a real cross-supplement
//! dependency**, unlike Performance/Safety/Diagnostics: it reads
//! `etdl.tree-event`'s already-parsed [`etdl_tree_core::Tree`] values
//! directly, via [`crate::tree_event::parse_and_validate_trees`] (a pure
//! function — calling it again here is additional-but-harmless, the same
//! "each supplement independently re-derives its own inputs" shape
//! `validate()`/`process()` already use within a single supplement).
//!
//! Registered like [`crate::performance::PerformanceExtension`] — no
//! bespoke direct call anywhere in `Compiler`'s pipeline (`lib.rs`). See
//! `docs/reference/security-supplement.md`.
//!
//! ## Interpretations beyond the literal diagnostic table
//!
//! - **The `etdl.tree-event` dependency (spec Section 1's `x-requires`
//!   metadata) is not separately parsed or enforced by this module.**
//!   No generic supplement-dependency-declaration mechanism exists
//!   anywhere in this codebase (confirmed: no `x-requires` handling in
//!   `validate.rs`) and this task does not add one. The dependency is
//!   instead a natural *consequence* of how `treeRef` resolves: since
//!   [`crate::tree_event::parse_and_validate_trees`] self-gates on
//!   `etdl.tree-event` also being declared under `supplements:`, a document
//!   declaring `etdl.security` without `etdl.tree-event` sees zero trees,
//!   so every `treeRef` correctly fails to resolve (`E-140`) — the
//!   practical effect the dependency declaration asks for, without a
//!   second enforcement mechanism.
//! - A `threatModels`/`controls` manifest that fails to deserialize, or
//!   declares a duplicate `id` within its own collection, is folded into
//!   **E-140** for Threat Models and **E-141** for Controls (the closest
//!   per-object-type bucket; no dedicated codes exist for either, matching
//!   Performance's/Safety's own precedent for this class of gap).
//! - A Control's `mitigates` entry is checked against the **union** of
//!   every successfully-resolved Threat Model's tree's leaves, not one
//!   specific tree — the spec's own field description ("Leaf node ids from
//!   *some* Threat Model's `treeRef` tree") does not name a specific one
//!   when more than one Threat Model is declared. Likewise, W-411 checks a
//!   `mitigates` entry against the union of every Threat Model's
//!   `leafCategories` *keys* (whether or not that entry's STRIDE value
//!   itself was valid — a key existing is what "assigns a category" means
//!   for this rule, independent of the value's own validity, which E-140
//!   already flags separately).

use std::collections::{BTreeMap, BTreeSet};

use etdl_parser::ast::{EtlDocument, Node};
use etdl_tree_core::Tree;

use crate::validate::Diagnostic;

const SECURITY_SUPPLEMENT: &str = "etdl.security";
pub const SECURITY_SCHEMA: &str = "etdl.security/1.0";

const STRIDE_CATEGORIES: [&str; 6] = [
    "spoofing",
    "tampering",
    "repudiation",
    "information-disclosure",
    "denial-of-service",
    "elevation-of-privilege",
];

/// One Threat Model Object under `x-security.threatModels` (Section 4.1).
/// `leafCategories` values are raw strings, not a strictly-typed serde
/// enum — an invalid value must not fail deserialization of every sibling
/// entry (the same "structure via serde, rules via explicit checks" split
/// every other supplement in this compiler uses).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ThreatModel {
    pub id: String,
    #[serde(rename = "treeRef")]
    pub tree_ref: String,
    #[serde(default, rename = "leafCategories")]
    pub leaf_categories: BTreeMap<String, String>,
}

/// One Control Object under `x-security.controls` (Section 4.2).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Control {
    pub id: String,
    #[serde(rename = "nodeRef")]
    pub node_ref: String,
    #[serde(default)]
    pub framework: Option<String>,
    #[serde(rename = "controlId")]
    pub control_id: String,
    #[serde(default)]
    pub mitigates: Vec<String>,
}

/// Every Threat Model/Control that parsed and validated successfully.
#[derive(Debug, Clone, Default)]
pub struct SecurityData {
    pub threat_models: Vec<ThreatModel>,
    pub controls: Vec<Control>,
}

/// Read and validate every Threat Model/Control declared under
/// `x-security` in the document. An object that failed any check is
/// omitted from the returned [`SecurityData`] but always produces a
/// diagnostic.
pub fn parse_and_validate_security(doc: &EtlDocument) -> (SecurityData, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut data = SecurityData::default();

    if !crate::validate::declares_supplement(doc, SECURITY_SUPPLEMENT) {
        return (data, diagnostics);
    }

    let Some(ext) = doc.extensions.get("x-security") else {
        return (data, diagnostics);
    };

    // See the module doc comment: empty (rather than an error) when
    // `etdl.tree-event` isn't also declared — every `treeRef` below then
    // naturally fails to resolve.
    let (trees, _tree_event_diagnostics) = crate::tree_event::parse_and_validate_trees(doc);
    let trees_by_id: BTreeMap<&str, &Tree> = trees.iter().map(|t| (t.id.as_str(), t)).collect();

    let mut all_leaf_ids: BTreeSet<String> = BTreeSet::new();
    let mut categorized_leaf_ids: BTreeSet<String> = BTreeSet::new();

    if let Some(raw) = ext.get("threatModels") {
        match serde_yaml::from_value::<Vec<ThreatModel>>(raw.clone()) {
            Ok(candidates) => {
                let mut seen_ids = BTreeSet::new();
                for threat_model in candidates {
                    let mut has_error = false;

                    if !seen_ids.insert(threat_model.id.clone()) {
                        diagnostics.push(Diagnostic::error(
                            "E-140",
                            format!("x-security: duplicate threat model id '{}'", threat_model.id),
                        ));
                        has_error = true;
                    }

                    let tree = trees_by_id.get(threat_model.tree_ref.as_str()).copied();
                    if let Some(tree) = tree {
                        let leaves: BTreeSet<&str> = tree.leaves().into_iter().collect();
                        for (leaf_id, category) in &threat_model.leaf_categories {
                            all_leaf_ids.insert(leaf_id.clone());
                            categorized_leaf_ids.insert(leaf_id.clone());

                            if !STRIDE_CATEGORIES.contains(&category.as_str()) {
                                diagnostics.push(Diagnostic::error(
                                    "E-140",
                                    format!(
                                        "x-security: threat model '{}': leafCategories['{}'] value '{}' is not a STRIDE category",
                                        threat_model.id, leaf_id, category
                                    ),
                                ));
                                has_error = true;
                            }
                            if !leaves.contains(leaf_id.as_str()) {
                                diagnostics.push(Diagnostic::error(
                                    "E-141",
                                    format!(
                                        "x-security: threat model '{}': leafCategories key '{}' is not a leaf of tree '{}'",
                                        threat_model.id, leaf_id, threat_model.tree_ref
                                    ),
                                ));
                                has_error = true;
                            }
                        }
                        // Every leaf of a resolved tree is a candidate
                        // `mitigates` target, whether or not it has an
                        // entry in `leafCategories` (an uncategorized leaf
                        // is a structural node with no assigned threat
                        // category — not itself an error, spec Section 4.1).
                        all_leaf_ids.extend(leaves.iter().map(|s| s.to_string()));
                    } else {
                        diagnostics.push(Diagnostic::error(
                            "E-140",
                            format!(
                                "x-security: threat model '{}': treeRef '{}' does not name a tree declared in x-tree-event.trees",
                                threat_model.id, threat_model.tree_ref
                            ),
                        ));
                        has_error = true;
                    }

                    if !has_error {
                        data.threat_models.push(threat_model);
                    }
                }
            }
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E-140",
                    format!("x-security: invalid threat model manifest: {e}"),
                ));
            }
        }
    }

    if let Some(raw) = ext.get("controls") {
        match serde_yaml::from_value::<Vec<Control>>(raw.clone()) {
            Ok(candidates) => {
                let mut seen_ids = BTreeSet::new();
                for control in candidates {
                    let mut has_error = false;

                    if !seen_ids.insert(control.id.clone()) {
                        diagnostics.push(Diagnostic::error(
                            "E-141",
                            format!("x-security: duplicate control id '{}'", control.id),
                        ));
                        has_error = true;
                    }

                    if control.mitigates.is_empty() {
                        diagnostics.push(Diagnostic::error(
                            "E-141",
                            format!("x-security: control '{}': mitigates must be non-empty", control.id),
                        ));
                        has_error = true;
                    }

                    if !resolve_barrier_ref(doc, &control.node_ref) {
                        diagnostics.push(Diagnostic::error(
                            "E-141",
                            format!(
                                "x-security: control '{}': nodeRef '{}' does not resolve to a Barrier node",
                                control.id, control.node_ref
                            ),
                        ));
                        has_error = true;
                    }

                    for leaf_id in &control.mitigates {
                        if !all_leaf_ids.contains(leaf_id) {
                            diagnostics.push(Diagnostic::error(
                                "E-141",
                                format!(
                                    "x-security: control '{}': mitigates entry '{}' is not a leaf node id of any declared threat model's tree",
                                    control.id, leaf_id
                                ),
                            ));
                            has_error = true;
                        } else if !categorized_leaf_ids.contains(leaf_id) {
                            diagnostics.push(Diagnostic::warning(
                                "W-411",
                                format!(
                                    "x-security: control '{}': mitigates entry '{}' is not categorized by any declared threat model's leafCategories",
                                    control.id, leaf_id
                                ),
                            ));
                        }
                    }

                    if !has_error {
                        data.controls.push(control);
                    }
                }
            }
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E-141",
                    format!("x-security: invalid control manifest: {e}"),
                ));
            }
        }
    }

    (data, diagnostics)
}

/// Resolve a Control's `nodeRef` against the document's own `eventTrees` —
/// the node-level shape only, Barrier kind only (the same manual-parse
/// style `performance`/`safety`/`diagnostics` use; no generic JSON-Pointer
/// resolver exists in this codebase for same-document references).
fn resolve_barrier_ref(doc: &EtlDocument, node_ref: &str) -> bool {
    let rest = node_ref.trim_start_matches('#');
    let Some(after) = rest.strip_prefix("/eventTrees/") else {
        return false;
    };
    match after.split('/').collect::<Vec<_>>().as_slice() {
        [tree_id, "nodes", node_id] if !tree_id.is_empty() && !node_id.is_empty() => doc
            .event_trees
            .get(*tree_id)
            .and_then(|t| t.nodes.get(*node_id))
            .is_some_and(|n| matches!(n, Node::Barrier(_))),
        _ => false,
    }
}

/// The built-in Security Supplement extension.
#[derive(Debug, Default)]
pub struct SecurityExtension;

impl SecurityExtension {
    pub fn new() -> Self {
        SecurityExtension
    }
}

/// The typed result of the security extension's processing step. Uses
/// [`crate::extension::ExtensionResult`]'s default (empty)
/// `basic_event_overrides()` — a Threat Model/Control never resolves into
/// a fault-tree probability; this supplement performs no automated threat
/// analysis of its own (module docs).
pub struct SecurityResult {
    pub threat_models: Vec<ThreatModel>,
    pub controls: Vec<Control>,
}

impl crate::extension::ExtensionResult for SecurityResult {
    fn extension_id(&self) -> &str {
        SECURITY_SUPPLEMENT
    }
}

impl crate::extension::EtdlExtension for SecurityExtension {
    fn id(&self) -> &str {
        SECURITY_SUPPLEMENT
    }

    fn version(&self) -> &str {
        "1.0"
    }

    fn descriptor(&self) -> crate::extension::SupplementDescriptor {
        crate::extension::SupplementDescriptor {
            summary: "STRIDE-classified attack trees (reusing etdl.tree-event's Tree structure) \
                      and Controls mapped onto core Barrier nodes.",
            schema: Some(SECURITY_SCHEMA),
            diagnostic_codes: &["E-140", "E-141", "W-411"],
            requires: &["etdl.tree-event"],
        }
    }

    fn validate(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let (_data, extra) = parse_and_validate_security(doc);
        diagnostics.extend(extra);
    }

    /// Deliberately does **not** extend `diagnostics` again — same reason
    /// as `performance`/`safety`/`diagnostics`: `run_extensions` only
    /// skips `process()` after an *error*, so W-411 (a warning) would
    /// otherwise be reported twice every time this extension actually runs
    /// through the real pipeline.
    fn process(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn crate::extension::ExtensionResult + '_> {
        let (data, _extra) = parse_and_validate_security(doc);
        Box::new(SecurityResult {
            threat_models: data.threat_models,
            controls: data.controls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::{builtin_registry, EtdlExtension, ExtensionContext};

    /// `eventTrees` includes one Barrier (`RateLimitBarrier`, a
    /// deliberately wrong-kind target is exercised via an Operation node).
    /// `x-tree-event` declares one attack tree (`gateway-compromise`) with
    /// two leaves (`CredentialStuffing`, `ApiKeyLeak`) under an OR gate
    /// (`GatewayCompromised`) — `GatewayCompromised` itself is *not* a
    /// leaf, used to exercise the "leafCategories key must be a leaf"
    /// rejection.
    fn doc_with_security(x_security_yaml: &str) -> EtlDocument {
        let yaml = format!(
            r##"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
supplements:
  - id: etdl.security
    version: "1.0"
  - id: etdl.tree-event
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: {{ id: I, message: "a#/m", next: RateLimitBarrier }}
    nodes:
      RateLimitBarrier:
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
x-tree-event:
  trees:
    - id: "gateway-compromise"
      version: "1"
      root: "GatewayCompromised"
      nodes:
        CredentialStuffing:
          kind: leaf
        ApiKeyLeak:
          kind: leaf
        GatewayCompromised:
          kind: gate
          gate: OR
          children: ["CredentialStuffing", "ApiKeyLeak"]
x-security:
{x_security_yaml}
"##
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn security_extension_is_registered_and_built_in() {
        let registry = builtin_registry();
        assert!(registry.contains(SECURITY_SUPPLEMENT));
        assert!(registry.list().contains(&SECURITY_SUPPLEMENT));
    }

    #[test]
    fn document_without_x_security_has_no_diagnostics() {
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
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.threat_models.is_empty());
        assert!(data.controls.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn valid_threat_model_and_control_have_no_diagnostics() {
        let doc = doc_with_security(
            r##"  threatModels:
    - id: payment-gateway-attack-tree
      treeRef: "gateway-compromise"
      leafCategories:
        CredentialStuffing: spoofing
        ApiKeyLeak: information-disclosure
  controls:
    - id: gateway-rate-limiter
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      framework: "NIST-800-53"
      controlId: "SC-5"
      mitigates: ["CredentialStuffing"]
"##,
        );
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(data.threat_models.len(), 1);
        assert_eq!(data.controls.len(), 1);
    }

    #[test]
    fn missing_threat_models_and_controls_keys_are_not_an_error() {
        let doc = doc_with_security("  {}");
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.threat_models.is_empty());
        assert!(data.controls.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn malformed_threat_models_produces_e140() {
        let doc = doc_with_security("  threatModels: \"oops\"");
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.threat_models.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-140"));
    }

    #[test]
    fn unresolvable_tree_ref_produces_e140() {
        let doc = doc_with_security(
            r##"  threatModels:
    - id: tm1
      treeRef: "does-not-exist"
      leafCategories: {}
"##,
        );
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.threat_models.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-140"));
    }

    #[test]
    fn invalid_stride_category_produces_e140() {
        let doc = doc_with_security(
            r##"  threatModels:
    - id: tm1
      treeRef: "gateway-compromise"
      leafCategories:
        CredentialStuffing: not-a-stride-category
"##,
        );
        let (_data, diagnostics) = parse_and_validate_security(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-140"));
    }

    #[test]
    fn leaf_categories_key_at_non_leaf_produces_e141() {
        let doc = doc_with_security(
            r##"  threatModels:
    - id: tm1
      treeRef: "gateway-compromise"
      leafCategories:
        GatewayCompromised: spoofing
"##,
        );
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.threat_models.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-141"));
    }

    #[test]
    fn uncategorized_leaf_is_not_an_error() {
        // Section 4.1: "not every leaf needs an entry".
        let doc = doc_with_security(
            r##"  threatModels:
    - id: tm1
      treeRef: "gateway-compromise"
      leafCategories:
        CredentialStuffing: spoofing
"##,
        );
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(data.threat_models.len(), 1);
    }

    #[test]
    fn control_node_ref_at_wrong_node_kind_produces_e141() {
        let doc = doc_with_security(
            r##"  controls:
    - id: c1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
      controlId: "SC-5"
      mitigates: ["x"]
"##,
        );
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.controls.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-141"));
    }

    #[test]
    fn empty_mitigates_produces_e141() {
        let doc = doc_with_security(
            r##"  controls:
    - id: c1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      controlId: "SC-5"
      mitigates: []
"##,
        );
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.controls.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-141"));
    }

    #[test]
    fn mitigates_entry_not_a_leaf_produces_e141() {
        let doc = doc_with_security(
            r##"  controls:
    - id: c1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      controlId: "SC-5"
      mitigates: ["NotALeaf"]
"##,
        );
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.controls.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-141"));
    }

    #[test]
    fn mitigates_entry_uncategorized_leaf_produces_w411() {
        let doc = doc_with_security(
            r##"  threatModels:
    - id: tm1
      treeRef: "gateway-compromise"
      leafCategories:
        CredentialStuffing: spoofing
  controls:
    - id: c1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      controlId: "SC-5"
      mitigates: ["ApiKeyLeak"]
"##,
        );
        // ApiKeyLeak is a genuine leaf (E-141 does not fire) but no threat
        // model's leafCategories assigns it a category (W-411, warning
        // only — the control is still accepted).
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert_eq!(data.controls.len(), 1);
        assert!(diagnostics.iter().any(|d| d.code == "W-411"));
        assert!(!diagnostics.iter().any(|d| d.is_error()));
    }

    #[test]
    fn duplicate_threat_model_id_produces_e140() {
        let doc = doc_with_security(
            r##"  threatModels:
    - id: dup
      treeRef: "gateway-compromise"
      leafCategories: {}
    - id: dup
      treeRef: "gateway-compromise"
      leafCategories: {}
"##,
        );
        let (_data, diagnostics) = parse_and_validate_security(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-140" && d.message.contains("duplicate threat model id")));
    }

    #[test]
    fn duplicate_control_id_produces_e141() {
        let doc = doc_with_security(
            r##"  controls:
    - id: dup
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      controlId: "SC-5"
      mitigates: ["x"]
    - id: dup
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      controlId: "SC-6"
      mitigates: ["y"]
"##,
        );
        let (_data, diagnostics) = parse_and_validate_security(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-141" && d.message.contains("duplicate control id")));
    }

    #[test]
    fn security_without_tree_event_declared_has_unresolvable_tree_refs() {
        // etdl.tree-event is not declared under `supplements:` here (only
        // etdl.security is) -> parse_and_validate_trees returns zero trees
        // -> treeRef can never resolve -> E-140, the natural consequence
        // documented in the module doc comment.
        let yaml = r#"
etdl: "1.0.0"
info: { title: "T", version: "1.0.0", domain: "D" }
supplements:
  - id: etdl.security
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
x-security:
  threatModels:
    - id: tm1
      treeRef: "anything"
      leafCategories: {}
"#;
        let doc: EtlDocument = serde_yaml::from_str(yaml).unwrap();
        let (data, diagnostics) = parse_and_validate_security(&doc);
        assert!(data.threat_models.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-140"));
    }

    #[test]
    fn process_returns_typed_result_with_correct_extension_id() {
        let doc = doc_with_security(
            r##"  threatModels:
    - id: tm1
      treeRef: "gateway-compromise"
      leafCategories:
        CredentialStuffing: spoofing
"##,
        );
        let ext = SecurityExtension::new();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        let result = ext.process(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(result.extension_id(), SECURITY_SUPPLEMENT);
        assert!(result.basic_event_overrides().is_empty());
    }
}
