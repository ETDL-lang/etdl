//! Compiler integration for the ETDL Safety Supplement (`etdl.safety`).
//!
//! Reads a document's `x-safety` extension field (the same generic `x-*`
//! mechanism every extension already uses — zero parser/AST changes were
//! needed), deserializes it into [`Hazard`]/[`SafetyBarrier`] values, and
//! validates them. Gives safety meaning — Safety Integrity Level,
//! independence, common-cause grouping — to structures ETDL Core already
//! defines (Consequence, Barrier); defines no new probability mathematics
//! and never recomputes a fault-tree-derived branch probability.
//!
//! Registered like [`crate::performance::PerformanceExtension`], **not**
//! like [`crate::tree_event::TreeEventExtension`]: `SafetyExtension` has no
//! bespoke direct call anywhere in `Compiler`'s pipeline (`lib.rs`). It is
//! registered in [`crate::extension::builtin_registry`] (discoverability:
//! `etdl capabilities`/`etdl supplement list`/E-108) and separately seeded
//! into `Compiler::new()`'s `extensions` list, so it runs through the same
//! generic `EtdlExtension::validate`/`process` path a third-party
//! `Compiler::with_extension` supplement uses. See
//! `docs/reference/safety-supplement.md`.
//!
//! ## Interpretations beyond the literal diagnostic table
//!
//! The normative spec's Section 5 table has no dedicated code for a few
//! MUST-level rules Section 4 states; each is folded into **E-130** (the
//! spec's own multi-condition catch-all for this object), documented here
//! and in the reference doc rather than silently invented:
//! - A duplicate Hazard or Safety Barrier `id` (Section 4.1/4.2 both say
//!   "unique within ...").
//! - A `hazards`/`barriers` manifest that fails to deserialize at all (no
//!   analog to the Tree Event Supplement's dedicated "manifest invalid"
//!   code exists here either).
//! - A Hazard's `riskIndex` outside `[1,4]` (Section 4.1's field table
//!   states the bound; E-130's prose only names `sil`'s bound explicitly).
//!
//! `E-132` ("list each other ... or are transitively connected through
//! further *mutual* `independentOf` declarations") is implemented as: an
//! edge between two Safety Barriers exists only when **both** list each
//! other in `independentOf` (a one-sided claim forms no edge); a
//! `commonCauseGroup` shared by two barriers connected — directly or
//! transitively — through such mutual edges is the contradiction.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use etdl_parser::ast::{EtlDocument, Node};

use crate::validate::Diagnostic;

const SAFETY_SUPPLEMENT: &str = "etdl.safety";
pub const SAFETY_SCHEMA: &str = "etdl.safety/1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Catastrophic,
    Critical,
    Marginal,
    Negligible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Likelihood {
    Frequent,
    Probable,
    Occasional,
    Remote,
    Improbable,
}

fn parse_severity(s: &str) -> Option<Severity> {
    match s {
        "catastrophic" => Some(Severity::Catastrophic),
        "critical" => Some(Severity::Critical),
        "marginal" => Some(Severity::Marginal),
        "negligible" => Some(Severity::Negligible),
        _ => None,
    }
}

fn parse_likelihood(s: &str) -> Option<Likelihood> {
    match s {
        "frequent" => Some(Likelihood::Frequent),
        "probable" => Some(Likelihood::Probable),
        "occasional" => Some(Likelihood::Occasional),
        "remote" => Some(Likelihood::Remote),
        "improbable" => Some(Likelihood::Improbable),
        _ => None,
    }
}

/// The Section 4.1 risk matrix: severity x likelihood -> Risk Index.
fn risk_matrix_value(severity: Severity, likelihood: Likelihood) -> i64 {
    use Likelihood::*;
    use Severity::*;
    match (severity, likelihood) {
        (Catastrophic, Frequent) => 1,
        (Catastrophic, Probable) => 1,
        (Catastrophic, Occasional) => 1,
        (Catastrophic, Remote) => 2,
        (Catastrophic, Improbable) => 2,
        (Critical, Frequent) => 1,
        (Critical, Probable) => 1,
        (Critical, Occasional) => 2,
        (Critical, Remote) => 2,
        (Critical, Improbable) => 3,
        (Marginal, Frequent) => 1,
        (Marginal, Probable) => 2,
        (Marginal, Occasional) => 3,
        (Marginal, Remote) => 3,
        (Marginal, Improbable) => 4,
        (Negligible, Frequent) => 2,
        (Negligible, Probable) => 3,
        (Negligible, Occasional) => 4,
        (Negligible, Remote) => 4,
        (Negligible, Improbable) => 4,
    }
}

/// One Hazard Object under `x-safety.hazards` (Section 4.1). `severity`/
/// `likelihood` are raw strings, not a strictly-typed serde enum — a single
/// hazard with an invalid value must not fail deserialization of every
/// sibling hazard in the array (the same "structure via serde, rules via
/// explicit checks" split `performance::Budget` uses).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Hazard {
    pub id: String,
    pub description: String,
    pub severity: String,
    pub likelihood: String,
    #[serde(rename = "riskIndex")]
    pub risk_index: i64,
    #[serde(rename = "consequenceRef")]
    pub consequence_ref: String,
}

/// One Safety Barrier Object under `x-safety.barriers` (Section 4.2).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SafetyBarrier {
    pub id: String,
    #[serde(rename = "nodeRef")]
    pub node_ref: String,
    pub sil: i64,
    #[serde(default, rename = "independentOf")]
    pub independent_of: Vec<String>,
    #[serde(default, rename = "commonCauseGroup")]
    pub common_cause_group: Option<String>,
}

/// Every Hazard/Safety Barrier that parsed and validated successfully.
#[derive(Debug, Clone, Default)]
pub struct SafetyData {
    pub hazards: Vec<Hazard>,
    pub barriers: Vec<SafetyBarrier>,
}

/// Read and validate every Hazard/Safety Barrier declared under `x-safety`
/// in the document. A hazard or barrier that failed any check is omitted
/// from the returned [`SafetyData`] but always produces a diagnostic.
pub fn parse_and_validate_safety(doc: &EtlDocument) -> (SafetyData, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut data = SafetyData::default();

    // `x-safety` is only processed when the document explicitly opts in via
    // `supplements:`, never merely because the extension field happens to
    // be present — the same gate every other supplement uses.
    if !crate::validate::declares_supplement(doc, SAFETY_SUPPLEMENT) {
        return (data, diagnostics);
    }

    let Some(ext) = doc.extensions.get("x-safety") else {
        return (data, diagnostics);
    };

    if let Some(raw_hazards) = ext.get("hazards") {
        match serde_yaml::from_value::<Vec<Hazard>>(raw_hazards.clone()) {
            Ok(candidates) => {
                let mut seen_ids = BTreeSet::new();
                for hazard in candidates {
                    let mut has_error = false;

                    if !seen_ids.insert(hazard.id.clone()) {
                        diagnostics.push(Diagnostic::error(
                            "E-130",
                            format!("x-safety: duplicate hazard id '{}'", hazard.id),
                        ));
                        has_error = true;
                    }

                    let severity = parse_severity(&hazard.severity);
                    if severity.is_none() {
                        diagnostics.push(Diagnostic::error(
                            "E-130",
                            format!(
                                "x-safety: hazard '{}': severity '{}' is not one of catastrophic, critical, marginal, negligible",
                                hazard.id, hazard.severity
                            ),
                        ));
                        has_error = true;
                    }

                    let likelihood = parse_likelihood(&hazard.likelihood);
                    if likelihood.is_none() {
                        diagnostics.push(Diagnostic::error(
                            "E-130",
                            format!(
                                "x-safety: hazard '{}': likelihood '{}' is not one of frequent, probable, occasional, remote, improbable",
                                hazard.id, hazard.likelihood
                            ),
                        ));
                        has_error = true;
                    }

                    let risk_index_valid = (1..=4).contains(&hazard.risk_index);
                    if !risk_index_valid {
                        diagnostics.push(Diagnostic::error(
                            "E-130",
                            format!(
                                "x-safety: hazard '{}': riskIndex must be an integer in [1,4] (got {})",
                                hazard.id, hazard.risk_index
                            ),
                        ));
                        has_error = true;
                    }

                    if !resolve_node_of_kind(doc, &hazard.consequence_ref, |n| {
                        matches!(n, Node::Consequence(_))
                    }) {
                        diagnostics.push(Diagnostic::error(
                            "E-131",
                            format!(
                                "x-safety: hazard '{}': consequenceRef '{}' does not resolve to a Consequence node",
                                hazard.id, hazard.consequence_ref
                            ),
                        ));
                        has_error = true;
                    }

                    // Only meaningful once severity/likelihood/riskIndex are
                    // themselves valid — otherwise this would double-report
                    // the same underlying problem E-130 already covers.
                    if let (Some(sev), Some(like), true) = (severity, likelihood, risk_index_valid) {
                        let expected = risk_matrix_value(sev, like);
                        if hazard.risk_index != expected {
                            diagnostics.push(Diagnostic::warning(
                                "W-410",
                                format!(
                                    "x-safety: hazard '{}': declared riskIndex {} does not match the risk matrix value {} for severity='{}'/likelihood='{}'",
                                    hazard.id, hazard.risk_index, expected, hazard.severity, hazard.likelihood
                                ),
                            ));
                        }
                    }

                    if !has_error {
                        data.hazards.push(hazard);
                    }
                }
            }
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E-130",
                    format!("x-safety: invalid hazard manifest: {e}"),
                ));
            }
        }
    }

    if let Some(raw_barriers) = ext.get("barriers") {
        match serde_yaml::from_value::<Vec<SafetyBarrier>>(raw_barriers.clone()) {
            Ok(candidates) => {
                let mut seen_ids = BTreeSet::new();
                let mut valid_barriers = Vec::new();
                for barrier in candidates {
                    let mut has_error = false;

                    if !seen_ids.insert(barrier.id.clone()) {
                        diagnostics.push(Diagnostic::error(
                            "E-130",
                            format!("x-safety: duplicate safety barrier id '{}'", barrier.id),
                        ));
                        has_error = true;
                    }

                    if !(1..=4).contains(&barrier.sil) {
                        diagnostics.push(Diagnostic::error(
                            "E-130",
                            format!(
                                "x-safety: safety barrier '{}': sil must be an integer in [1,4] (got {})",
                                barrier.id, barrier.sil
                            ),
                        ));
                        has_error = true;
                    }

                    if !resolve_node_of_kind(doc, &barrier.node_ref, |n| matches!(n, Node::Barrier(_))) {
                        diagnostics.push(Diagnostic::error(
                            "E-131",
                            format!(
                                "x-safety: safety barrier '{}': nodeRef '{}' does not resolve to a Barrier node",
                                barrier.id, barrier.node_ref
                            ),
                        ));
                        has_error = true;
                    }

                    if !has_error {
                        valid_barriers.push(barrier);
                    }
                }
                check_common_cause_contradictions(&valid_barriers, &mut diagnostics);
                data.barriers = valid_barriers;
            }
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E-130",
                    format!("x-safety: invalid safety barrier manifest: {e}"),
                ));
            }
        }
    }

    (data, diagnostics)
}

/// Resolve `node_ref` against the document's own `eventTrees` (the same
/// manual-parse style `performance::resolve_node_ref`/`validate::
/// check_transfers` use for internal cross-references — no generic
/// JSON-Pointer resolver exists in this codebase for same-document refs).
/// Unlike `performance`'s `nodeRef`, both `consequenceRef` and `nodeRef`
/// here require the node-level shape only (`^#/eventTrees/[^/]+/nodes/[^/]+$`
/// — no whole-tree alternative), per the JSON Schema.
fn resolve_node_of_kind(doc: &EtlDocument, node_ref: &str, is_kind: impl Fn(&Node) -> bool) -> bool {
    let rest = node_ref.trim_start_matches('#');
    let Some(after) = rest.strip_prefix("/eventTrees/") else {
        return false;
    };
    match after.split('/').collect::<Vec<_>>().as_slice() {
        [tree_id, "nodes", node_id] if !tree_id.is_empty() && !node_id.is_empty() => doc
            .event_trees
            .get(*tree_id)
            .and_then(|t| t.nodes.get(*node_id))
            .is_some_and(is_kind),
        _ => false,
    }
}

/// E-132: two Safety Barriers connected — directly or transitively —
/// through *mutual* `independentOf` edges while sharing a non-empty
/// `commonCauseGroup`. Only ever called with already-individually-valid
/// barriers (§ module docs).
fn check_common_cause_contradictions(barriers: &[SafetyBarrier], diagnostics: &mut Vec<Diagnostic>) {
    let by_id: BTreeMap<&str, &SafetyBarrier> = barriers.iter().map(|b| (b.id.as_str(), b)).collect();

    let mut adjacency: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for barrier in barriers {
        for other_id in &barrier.independent_of {
            if let Some(other) = by_id.get(other_id.as_str()) {
                let mutual = other.independent_of.iter().any(|id| id == &barrier.id);
                if mutual {
                    adjacency.entry(barrier.id.as_str()).or_default().insert(other.id.as_str());
                    adjacency.entry(other.id.as_str()).or_default().insert(barrier.id.as_str());
                }
            }
        }
    }

    let mut visited: BTreeSet<&str> = BTreeSet::new();
    for barrier in barriers {
        if visited.contains(barrier.id.as_str()) {
            continue;
        }

        // BFS over mutual edges to find this barrier's connected component.
        let mut component = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(barrier.id.as_str());
        visited.insert(barrier.id.as_str());
        while let Some(id) = queue.pop_front() {
            component.push(id);
            if let Some(neighbors) = adjacency.get(id) {
                for &n in neighbors {
                    if visited.insert(n) {
                        queue.push_back(n);
                    }
                }
            }
        }

        let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for id in &component {
            if let Some(group) = by_id[id].common_cause_group.as_deref() {
                if !group.is_empty() {
                    groups.entry(group).or_default().push(id);
                }
            }
        }

        for (group, mut members) in groups {
            if members.len() >= 2 {
                members.sort_unstable();
                diagnostics.push(Diagnostic::error(
                    "E-132",
                    format!(
                        "x-safety: barriers {} mutually claim independentOf each other but share commonCauseGroup '{}' — self-contradictory",
                        members.join(", "),
                        group
                    ),
                ));
            }
        }
    }
}

/// The built-in Safety Supplement extension.
#[derive(Debug, Default)]
pub struct SafetyExtension;

impl SafetyExtension {
    pub fn new() -> Self {
        SafetyExtension
    }
}

/// The typed result of the safety extension's processing step. Uses
/// [`crate::extension::ExtensionResult`]'s default (empty)
/// `basic_event_overrides()` — a Hazard/Safety Barrier never resolves into
/// a fault-tree probability (residual risk is read from core's own
/// fault-tree evaluation, never recomputed here — spec Section 6).
pub struct SafetyResult {
    pub hazards: Vec<Hazard>,
    pub barriers: Vec<SafetyBarrier>,
}

impl crate::extension::ExtensionResult for SafetyResult {
    fn extension_id(&self) -> &str {
        SAFETY_SUPPLEMENT
    }
}

impl crate::extension::EtdlExtension for SafetyExtension {
    fn id(&self) -> &str {
        SAFETY_SUPPLEMENT
    }

    fn version(&self) -> &str {
        "1.0"
    }

    fn descriptor(&self) -> crate::extension::SupplementDescriptor {
        crate::extension::SupplementDescriptor {
            summary: "Hazard classification against a severity/likelihood risk matrix, and \
                      Safety Integrity Level/independence declarations on core Barrier nodes; \
                      no new probability mathematics.",
            schema: Some(SAFETY_SCHEMA),
            diagnostic_codes: &["E-130", "E-131", "E-132", "W-410"],
            requires: &[],
        }
    }

    fn validate(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let (_data, safety_diagnostics) = parse_and_validate_safety(doc);
        diagnostics.extend(safety_diagnostics);
    }

    /// Deliberately does **not** extend `diagnostics` again — same reason as
    /// `performance::PerformanceExtension::process`: `run_extensions` only
    /// skips `process()` after an *error*, so W-410 (a warning) would
    /// otherwise be reported twice every time this extension actually runs
    /// through the real pipeline.
    fn process(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn crate::extension::ExtensionResult + '_> {
        let (data, _safety_diagnostics) = parse_and_validate_safety(doc);
        Box::new(SafetyResult {
            hazards: data.hazards,
            barriers: data.barriers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::{builtin_registry, EtdlExtension, ExtensionContext};

    /// `eventTrees` includes one Barrier (`RetryBarrier`), one Operation
    /// (`ProcessPaymentOperation` — a deliberately wrong-kind target for
    /// `nodeRef`), and one Consequence (`FulfillmentConsequence`).
    fn doc_with_safety(x_safety_yaml: &str) -> EtlDocument {
        let yaml = format!(
            r##"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
supplements:
  - id: etdl.safety
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
        next: FulfillmentConsequence
      FulfillmentConsequence:
        type: consequence
        operation: terminate
x-safety:
{x_safety_yaml}
"##
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn safety_extension_is_registered_and_built_in() {
        let registry = builtin_registry();
        assert!(registry.contains(SAFETY_SUPPLEMENT));
        assert!(registry.list().contains(&SAFETY_SUPPLEMENT));
    }

    #[test]
    fn document_without_x_safety_has_no_diagnostics() {
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
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(data.hazards.is_empty());
        assert!(data.barriers.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn valid_hazard_and_barrier_have_no_diagnostics() {
        let doc = doc_with_safety(
            r##"  hazards:
    - id: gateway-unavailable
      description: "payment cannot be captured while the gateway is down"
      severity: critical
      likelihood: remote
      riskIndex: 2
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/FulfillmentConsequence"
  barriers:
    - id: retry-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 2
"##,
        );
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(data.hazards.len(), 1);
        assert_eq!(data.barriers.len(), 1);
    }

    #[test]
    fn missing_hazards_and_barriers_keys_are_not_an_error() {
        let doc = doc_with_safety("  {}");
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(data.hazards.is_empty());
        assert!(data.barriers.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn malformed_hazards_produces_e130() {
        let doc = doc_with_safety("  hazards: \"oops\"");
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(data.hazards.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-130"));
    }

    #[test]
    fn invalid_severity_produces_e130() {
        let doc = doc_with_safety(
            r##"  hazards:
    - id: h1
      description: "d"
      severity: extremely-bad
      likelihood: remote
      riskIndex: 2
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/FulfillmentConsequence"
"##,
        );
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(data.hazards.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-130"));
    }

    #[test]
    fn invalid_likelihood_produces_e130() {
        let doc = doc_with_safety(
            r##"  hazards:
    - id: h1
      description: "d"
      severity: critical
      likelihood: all-the-time
      riskIndex: 2
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/FulfillmentConsequence"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-130"));
    }

    #[test]
    fn out_of_range_risk_index_produces_e130() {
        let doc = doc_with_safety(
            r##"  hazards:
    - id: h1
      description: "d"
      severity: critical
      likelihood: remote
      riskIndex: 9
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/FulfillmentConsequence"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-130"));
        assert!(!diagnostics.iter().any(|d| d.code == "W-410"));
    }

    #[test]
    fn consequence_ref_at_wrong_node_kind_produces_e131() {
        let doc = doc_with_safety(
            r##"  hazards:
    - id: h1
      description: "d"
      severity: critical
      likelihood: remote
      riskIndex: 2
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
"##,
        );
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(data.hazards.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-131"));
    }

    #[test]
    fn barrier_node_ref_at_wrong_node_kind_produces_e131() {
        let doc = doc_with_safety(
            r##"  barriers:
    - id: b1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/ProcessPaymentOperation"
      sil: 2
"##,
        );
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(data.barriers.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-131"));
    }

    #[test]
    fn unresolvable_node_ref_produces_e131() {
        let doc = doc_with_safety(
            r##"  barriers:
    - id: b1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/DoesNotExist"
      sil: 2
"##,
        );
        let (_data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-131"));
    }

    #[test]
    fn out_of_range_sil_produces_e130() {
        let doc = doc_with_safety(
            r##"  barriers:
    - id: b1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 7
"##,
        );
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(data.barriers.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-130"));
    }

    #[test]
    fn duplicate_hazard_id_produces_e130() {
        let doc = doc_with_safety(
            r##"  hazards:
    - id: dup
      description: "d1"
      severity: critical
      likelihood: remote
      riskIndex: 2
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/FulfillmentConsequence"
    - id: dup
      description: "d2"
      severity: marginal
      likelihood: probable
      riskIndex: 2
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/FulfillmentConsequence"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-130" && d.message.contains("duplicate hazard id")));
    }

    #[test]
    fn duplicate_barrier_id_produces_e130() {
        let doc = doc_with_safety(
            r##"  barriers:
    - id: dup
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 1
    - id: dup
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 2
"##,
        );
        let (_data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-130" && d.message.contains("duplicate safety barrier id")));
    }

    #[test]
    fn mismatched_risk_index_produces_w410() {
        // catastrophic/remote -> matrix value 2, declared 4.
        let doc = doc_with_safety(
            r##"  hazards:
    - id: h1
      description: "d"
      severity: catastrophic
      likelihood: remote
      riskIndex: 4
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/FulfillmentConsequence"
"##,
        );
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        // W-410 is a warning, not a rejection: the hazard is still accepted.
        assert_eq!(data.hazards.len(), 1);
        assert!(diagnostics.iter().any(|d| d.code == "W-410"));
        assert!(!diagnostics.iter().any(|d| d.is_error()));
    }

    #[test]
    fn matching_risk_index_has_no_w410() {
        // catastrophic/remote -> matrix value 2.
        let doc = doc_with_safety(
            r##"  hazards:
    - id: h1
      description: "d"
      severity: catastrophic
      likelihood: remote
      riskIndex: 2
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/FulfillmentConsequence"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    }

    #[test]
    fn mutual_independent_of_with_shared_common_cause_group_produces_e132() {
        let doc = doc_with_safety(
            r##"  barriers:
    - id: retry-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 2
      independentOf: ["fallback-barrier"]
      commonCauseGroup: "primary-network-path"
    - id: fallback-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 1
      independentOf: ["retry-barrier"]
      commonCauseGroup: "primary-network-path"
"##,
        );
        let (data, diagnostics) = parse_and_validate_safety(&doc);
        // The contradiction is a fault in the *claim*, not the barriers'
        // own individual field validity — both remain accepted.
        assert_eq!(data.barriers.len(), 2);
        assert!(diagnostics.iter().any(|d| d.code == "E-132"));
    }

    #[test]
    fn one_sided_independent_of_does_not_produce_e132() {
        // retry-barrier claims independence from fallback-barrier, but
        // fallback-barrier never reciprocates — not a "mutual" claim.
        let doc = doc_with_safety(
            r##"  barriers:
    - id: retry-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 2
      independentOf: ["fallback-barrier"]
      commonCauseGroup: "primary-network-path"
    - id: fallback-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 1
      commonCauseGroup: "primary-network-path"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(!diagnostics.iter().any(|d| d.code == "E-132"));
    }

    #[test]
    fn mutual_independent_of_with_different_common_cause_groups_does_not_produce_e132() {
        let doc = doc_with_safety(
            r##"  barriers:
    - id: retry-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 2
      independentOf: ["fallback-barrier"]
      commonCauseGroup: "primary-network-path"
    - id: fallback-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 1
      independentOf: ["retry-barrier"]
      commonCauseGroup: "secondary-network-path"
"##,
        );
        let (_data, diagnostics) = parse_and_validate_safety(&doc);
        assert!(!diagnostics.iter().any(|d| d.code == "E-132"));
    }

    #[test]
    fn process_returns_typed_result_with_correct_extension_id() {
        let doc = doc_with_safety(
            r##"  barriers:
    - id: retry-barrier
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RetryBarrier"
      sil: 2
"##,
        );
        let ext = SafetyExtension::new();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        let result = ext.process(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(result.extension_id(), SAFETY_SUPPLEMENT);
        assert!(result.basic_event_overrides().is_empty());
    }
}
