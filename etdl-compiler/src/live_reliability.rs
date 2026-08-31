//! Compiler integration for the Live Reliability Supplement
//! (`etdl.live-reliability`).
//!
//! Reads a document's `x-live-reliability` extension field and validates
//! it against the document's own `faultTrees`. Unlike every other
//! built-in supplement, this one's validated data isn't consumed only for
//! diagnostics/compile-time resolution — `codegen/rust.rs` reads it to
//! decide whether to emit `etdl_core::live` registration code and whether
//! a barrier's branch condition may use `reliability.in_range`. See
//! `docs/reference/live-reliability.md` and
//! `docs/reliability/runtime-feedback-calibration.md` (this supplement is
//! a deliberately separate, opt-in exception to that doc's "runtime never
//! changes compiled probabilities" invariant — that invariant still governs
//! every fault tree that doesn't declare this supplement).
//!
//! Registered like [`crate::safety::SafetyExtension`]: no bespoke pipeline
//! call, seeded into [`crate::Compiler::new`]'s `extensions` list and
//! [`crate::extension::builtin_registry`].

use std::collections::BTreeSet;

use etdl_parser::ast::EtlDocument;

use crate::validate::Diagnostic;

const LIVE_RELIABILITY_SUPPLEMENT: &str = "etdl.live-reliability";
pub const LIVE_RELIABILITY_SCHEMA: &str = "etdl.live-reliability/1.0";

/// Matches `SlaTracker`'s own `DEFAULT_DEVIATION_THRESHOLD`
/// (`etdl-core/src/sla.rs`) — `reliability.in_range` is the same
/// "observed vs. baseline, more than this much apart is abnormal" concept,
/// just against a live baseline instead of a single declared constant.
fn default_threshold() -> f64 {
    0.10
}

/// How many pseudo-observations a `local` basic event's declared
/// probability is worth before real observations start moving its live
/// estimate. 20 mirrors the order of magnitude `SlaTracker::
/// MIN_OBSERVATIONS` already uses as "enough data to mean something".
pub(crate) fn default_prior_strength() -> f64 {
    20.0
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LiveBasicEventDecl {
    pub id: String,
    /// `"local"` (this service observes it) or `"inbound"` (owned by an
    /// upstream service, arrives only via a received message's headers).
    pub source: String,
    #[serde(default = "default_prior_strength", rename = "priorStrength")]
    pub prior_strength: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LiveFaultTreeDecl {
    pub id: String,
    #[serde(rename = "basicEvents")]
    pub basic_events: Vec<LiveBasicEventDecl>,
    #[serde(default = "default_threshold")]
    pub threshold: f64,
}

#[derive(Debug, Clone, Default)]
pub struct LiveReliabilityData {
    pub fault_trees: Vec<LiveFaultTreeDecl>,
}

/// Read and validate every fault tree declared under `x-live-reliability`.
/// A fault tree or basic event that failed any check is omitted from the
/// returned [`LiveReliabilityData`] but always produces a diagnostic — the
/// same "structure via serde, rules via explicit checks, one bad entry
/// doesn't drop its valid siblings" split every other supplement here
/// uses.
pub fn parse_and_validate_live_reliability(doc: &EtlDocument) -> (LiveReliabilityData, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut data = LiveReliabilityData::default();

    if !crate::validate::declares_supplement(doc, LIVE_RELIABILITY_SUPPLEMENT) {
        return (data, diagnostics);
    }

    let Some(ext) = doc.extensions.get("x-live-reliability") else {
        return (data, diagnostics);
    };

    let Some(raw_trees) = ext.get("faultTrees") else {
        return (data, diagnostics);
    };

    let candidates: Vec<LiveFaultTreeDecl> = match serde_yaml::from_value(raw_trees.clone()) {
        Ok(c) => c,
        Err(e) => {
            diagnostics.push(Diagnostic::error(
                "E-170",
                format!("x-live-reliability: invalid faultTrees manifest: {e}"),
            ));
            return (data, diagnostics);
        }
    };

    let mut seen_tree_ids = BTreeSet::new();
    for decl in candidates {
        let mut has_error = false;

        if !seen_tree_ids.insert(decl.id.clone()) {
            diagnostics.push(Diagnostic::error(
                "E-172",
                format!("x-live-reliability: duplicate faultTrees entry for '{}'", decl.id),
            ));
            has_error = true;
        }

        let Some(fault_tree) = doc.fault_trees.as_ref().and_then(|fts| fts.get(&decl.id)) else {
            diagnostics.push(Diagnostic::error(
                "E-171",
                format!(
                    "x-live-reliability: faultTrees entry '{}' does not resolve to a declared fault tree",
                    decl.id
                ),
            ));
            continue;
        };

        if !decl.threshold.is_finite() || decl.threshold < 0.0 {
            diagnostics.push(Diagnostic::error(
                "E-170",
                format!(
                    "x-live-reliability: faultTrees entry '{}': threshold must be a non-negative finite number (got {})",
                    decl.id, decl.threshold
                ),
            ));
            has_error = true;
        }

        let mut seen_event_ids = BTreeSet::new();
        let mut valid_events = Vec::new();
        for event in decl.basic_events {
            let mut event_has_error = false;

            if !seen_event_ids.insert(event.id.clone()) {
                diagnostics.push(Diagnostic::error(
                    "E-172",
                    format!(
                        "x-live-reliability: faultTrees entry '{}': duplicate basicEvents entry for '{}'",
                        decl.id, event.id
                    ),
                ));
                event_has_error = true;
            }

            if !fault_tree.basic_events.contains_key(&event.id) {
                diagnostics.push(Diagnostic::error(
                    "E-171",
                    format!(
                        "x-live-reliability: faultTrees entry '{}': basicEvents entry '{}' is not a basic event of that fault tree",
                        decl.id, event.id
                    ),
                ));
                event_has_error = true;
            }

            match event.source.as_str() {
                "local" => {
                    if !event.prior_strength.is_finite() || event.prior_strength <= 0.0 {
                        diagnostics.push(Diagnostic::error(
                            "E-170",
                            format!(
                                "x-live-reliability: faultTrees entry '{}': basic event '{}': priorStrength must be a positive finite number (got {})",
                                decl.id, event.id, event.prior_strength
                            ),
                        ));
                        event_has_error = true;
                    } else if event.prior_strength < 1.0 {
                        diagnostics.push(Diagnostic::warning(
                            "W-414",
                            format!(
                                "x-live-reliability: faultTrees entry '{}': basic event '{}': priorStrength {} is below 1.0 — a single observation will dominate the declared probability almost immediately",
                                decl.id, event.id, event.prior_strength
                            ),
                        ));
                    }
                }
                "inbound" => {}
                other => {
                    diagnostics.push(Diagnostic::error(
                        "E-170",
                        format!(
                            "x-live-reliability: faultTrees entry '{}': basic event '{}': source '{}' is not one of local, inbound",
                            decl.id, event.id, other
                        ),
                    ));
                    event_has_error = true;
                }
            }

            if !event_has_error {
                valid_events.push(event);
            }
        }

        if !has_error {
            data.fault_trees.push(LiveFaultTreeDecl {
                id: decl.id,
                basic_events: valid_events,
                threshold: decl.threshold,
            });
        }
    }

    (data, diagnostics)
}

/// The built-in Live Reliability Supplement extension.
#[derive(Debug, Default)]
pub struct LiveReliabilityExtension;

impl LiveReliabilityExtension {
    pub fn new() -> Self {
        LiveReliabilityExtension
    }
}

/// Uses [`crate::extension::ExtensionResult`]'s default (empty)
/// `basic_event_overrides()` — this supplement never overrides a
/// *compile-time* fault-tree probability; its whole purpose is a separate
/// runtime layer codegen wires up independently (Part 5), not a value fed
/// back into `fault_tree::resolve_fault_trees`.
pub struct LiveReliabilityResult;

impl crate::extension::ExtensionResult for LiveReliabilityResult {
    fn extension_id(&self) -> &str {
        LIVE_RELIABILITY_SUPPLEMENT
    }
}

impl crate::extension::EtdlExtension for LiveReliabilityExtension {
    fn id(&self) -> &str {
        LIVE_RELIABILITY_SUPPLEMENT
    }

    fn version(&self) -> &str {
        "1.0"
    }

    fn descriptor(&self) -> crate::extension::SupplementDescriptor {
        crate::extension::SupplementDescriptor {
            summary: "Live, decentralized, per-node fault-tree probability recomputation at \
                      runtime — an explicitly opt-in, authoritative exception to the rule that \
                      runtime observations never change compiled probabilities.",
            schema: Some(LIVE_RELIABILITY_SCHEMA),
            // E-173 is reported by `typeck` (the `reliability.in_range`
            // ECEL path root), not by this module's own
            // `parse_and_validate_live_reliability` — listed here anyway
            // since it's only ever relevant when this supplement is in
            // play.
            diagnostic_codes: &["E-170", "E-171", "E-172", "E-173", "W-414"],
            requires: &[],
        }
    }

    fn validate(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let (_data, live_diagnostics) = parse_and_validate_live_reliability(doc);
        diagnostics.extend(live_diagnostics);
    }

    /// Deliberately does **not** extend `diagnostics` again — same reason
    /// as `safety::SafetyExtension::process`: `run_extensions` only skips
    /// `process()` after an *error*, so W-414 would otherwise be reported
    /// twice every time this extension actually runs.
    fn process(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn crate::extension::ExtensionResult + '_> {
        let _ = doc;
        Box::new(LiveReliabilityResult)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::{builtin_registry, EtdlExtension, ExtensionContext};

    fn doc_with_live_reliability(x_live_reliability_yaml: &str) -> EtlDocument {
        let yaml = format!(
            r##"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
supplements:
  - id: etdl.live-reliability
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: {{ id: I, message: "a#/m", next: C }}
    nodes:
      C: {{ type: consequence, operation: terminate }}
faultTrees:
  PaymentGatewayFailure:
    topEvent:
      id: PaymentCaptureFailed
      description: "d"
      rootCause: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
    basicEvents:
      GatewayUnreachable:
        description: "d"
        probability: 0.01
      ChargeRejected:
        description: "d"
        probability: 0.02
x-live-reliability:
{x_live_reliability_yaml}
"##
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn live_reliability_extension_is_registered_and_built_in() {
        let registry = builtin_registry();
        assert!(registry.contains(LIVE_RELIABILITY_SUPPLEMENT));
        assert!(registry.list().contains(&LIVE_RELIABILITY_SUPPLEMENT));
    }

    #[test]
    fn document_without_x_live_reliability_has_no_diagnostics() {
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
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(data.fault_trees.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn valid_declaration_has_no_diagnostics() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      threshold: 0.1
      basicEvents:
        - id: GatewayUnreachable
          source: local
          priorStrength: 20
        - id: ChargeRejected
          source: inbound
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(data.fault_trees.len(), 1);
        assert_eq!(data.fault_trees[0].basic_events.len(), 2);
    }

    #[test]
    fn defaults_apply_when_threshold_and_prior_strength_are_omitted() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      basicEvents:
        - id: GatewayUnreachable
          source: local
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(data.fault_trees[0].threshold, 0.10);
        assert_eq!(data.fault_trees[0].basic_events[0].prior_strength, 20.0);
    }

    #[test]
    fn malformed_manifest_produces_e170() {
        let doc = doc_with_live_reliability("  faultTrees: \"oops\"");
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(data.fault_trees.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-170"));
    }

    #[test]
    fn unresolvable_fault_tree_id_produces_e171() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: NoSuchFaultTree
      basicEvents: []
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(data.fault_trees.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-171"));
    }

    #[test]
    fn unresolvable_basic_event_id_produces_e171() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      basicEvents:
        - id: DoesNotExist
          source: local
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(data.fault_trees[0].basic_events.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-171"));
    }

    #[test]
    fn invalid_source_produces_e170() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      basicEvents:
        - id: GatewayUnreachable
          source: sometimes
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(data.fault_trees[0].basic_events.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-170"));
    }

    #[test]
    fn non_positive_prior_strength_produces_e170() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      basicEvents:
        - id: GatewayUnreachable
          source: local
          priorStrength: 0
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(data.fault_trees[0].basic_events.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-170"));
    }

    #[test]
    fn small_prior_strength_produces_w414_but_is_still_accepted() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      basicEvents:
        - id: GatewayUnreachable
          source: local
          priorStrength: 0.5
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert_eq!(data.fault_trees[0].basic_events.len(), 1);
        assert!(diagnostics.iter().any(|d| d.code == "W-414"));
        assert!(!diagnostics.iter().any(|d| d.is_error()));
    }

    #[test]
    fn negative_threshold_produces_e170() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      threshold: -0.1
      basicEvents: []
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(data.fault_trees.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "E-170"));
    }

    #[test]
    fn duplicate_fault_tree_entry_produces_e172() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      basicEvents: []
    - id: PaymentGatewayFailure
      basicEvents: []
"##,
        );
        let (_data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-172" && d.message.contains("duplicate")));
    }

    #[test]
    fn duplicate_basic_event_entry_produces_e172() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      basicEvents:
        - id: GatewayUnreachable
          source: local
        - id: GatewayUnreachable
          source: inbound
"##,
        );
        let (data, diagnostics) = parse_and_validate_live_reliability(&doc);
        assert_eq!(data.fault_trees[0].basic_events.len(), 1);
        assert!(diagnostics.iter().any(|d| d.code == "E-172" && d.message.contains("duplicate")));
    }

    #[test]
    fn process_returns_typed_result_with_correct_extension_id() {
        let doc = doc_with_live_reliability(
            r##"  faultTrees:
    - id: PaymentGatewayFailure
      basicEvents:
        - id: GatewayUnreachable
          source: local
"##,
        );
        let ext = LiveReliabilityExtension::new();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        let result = ext.process(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(result.extension_id(), LIVE_RELIABILITY_SUPPLEMENT);
        assert!(result.basic_event_overrides().is_empty());
    }
}
