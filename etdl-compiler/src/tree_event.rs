//! Compiler integration for the ETDL Generic Tree Event Supplement
//! (`etdl.tree-event`).
//!
//! Mirrors the Reliability Supplement's own integration
//! (`crate::reliability`) exactly: reads a document's `x-tree-event`
//! extension field (the same generic `x-*` mechanism every extension
//! already uses — **zero parser/AST changes were needed for this
//! supplement**), deserializes it into `etdl-tree-core::Tree` values, and
//! validates them. Unlike the reliability supplement, tree-event has
//! nothing to resolve into fault-tree overrides — it is purely structural
//! validation, so `process()` just returns the parsed trees for a caller
//! that wants them (e.g. `etdl tree inspect`).
//!
//! Registered **unconditionally** in [`crate::extension::builtin_registry`]
//! (not behind the `reliability` Cargo feature) — the tree-event supplement
//! is domain-neutral, built-in infrastructure, not an optional reliability
//! feature. See `docs/reference/generic-tree-event-supplement.md`.

use etdl_parser::ast::EtlDocument;
use etdl_tree_core::{Tree, TreeError};

use crate::validate::Diagnostic;

const TREE_EVENT_SUPPLEMENT: &str = "etdl.tree-event";

/// Read every `Tree` declared under `x-tree-event.trees` in the document.
/// Returns `(trees, diagnostics)`: trees that failed to parse or validate
/// are omitted from `trees` but always produce a diagnostic — never a
/// silent drop.
pub fn parse_and_validate_trees(doc: &EtlDocument) -> (Vec<Tree>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut trees = Vec::new();

    // Mirrors the reliability supplement exactly: `x-tree-event` is only
    // processed when the document explicitly opts in via `supplements:`,
    // never merely because the extension field happens to be present.
    if !crate::validate::declares_supplement(doc, TREE_EVENT_SUPPLEMENT) {
        return (trees, diagnostics);
    }

    let Some(ext) = doc.extensions.get("x-tree-event") else {
        return (trees, diagnostics);
    };
    let raw_trees = match ext.get("trees") {
        Some(v) => v,
        None => {
            diagnostics.push(Diagnostic::error(
                "E-120",
                "x-tree-event: missing required 'trees' field".to_string(),
            ));
            return (trees, diagnostics);
        }
    };
    let parsed: Result<Vec<Tree>, _> = serde_yaml::from_value(raw_trees.clone());
    let candidates = match parsed {
        Ok(t) => t,
        Err(e) => {
            diagnostics.push(Diagnostic::error(
                "E-120",
                format!("x-tree-event: invalid tree manifest: {e}"),
            ));
            return (trees, diagnostics);
        }
    };

    let mut seen_ids = std::collections::BTreeSet::new();
    for tree in candidates {
        if !seen_ids.insert(tree.id.clone()) {
            diagnostics.push(Diagnostic::error(
                "E-122",
                format!("x-tree-event: duplicate tree id '{}'", tree.id),
            ));
            continue;
        }
        match tree.validate() {
            Ok(()) => trees.push(tree),
            Err(errors) => {
                for e in errors {
                    diagnostics.push(Diagnostic::error("E-121", format_tree_error(&e)));
                }
            }
        }
    }

    (trees, diagnostics)
}

fn format_tree_error(e: &TreeError) -> String {
    format!("x-tree-event: {e}")
}

/// The built-in Generic Tree Event Supplement extension.
#[derive(Debug, Default)]
pub struct TreeEventExtension;

impl TreeEventExtension {
    pub fn new() -> Self {
        TreeEventExtension
    }
}

/// The typed result of the tree-event extension's processing step: every
/// tree that parsed and validated successfully.
pub struct TreeEventResult {
    pub trees: Vec<Tree>,
}

impl crate::extension::ExtensionResult for TreeEventResult {
    fn extension_id(&self) -> &str {
        TREE_EVENT_SUPPLEMENT
    }
}

impl crate::extension::EtdlExtension for TreeEventExtension {
    fn id(&self) -> &str {
        TREE_EVENT_SUPPLEMENT
    }

    fn version(&self) -> &str {
        "1.0"
    }

    fn descriptor(&self) -> crate::extension::SupplementDescriptor {
        crate::extension::SupplementDescriptor {
            summary: "Domain-neutral tree-of-events structure — nodes, logical gates \
                      (AND/OR/NOT/XOR/K_OF_N), validation, traversal — for reliability, safety, \
                      security, and future domains to each interpret independently.",
            schema: Some(etdl_tree_core::TREE_SCHEMA),
            diagnostic_codes: &["E-120", "E-121", "E-122"],
            requires: &[],
        }
    }

    fn validate(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let (_trees, tree_diagnostics) = parse_and_validate_trees(doc);
        diagnostics.extend(tree_diagnostics);
    }

    fn process(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn crate::extension::ExtensionResult + '_> {
        let (trees, tree_diagnostics) = parse_and_validate_trees(doc);
        diagnostics.extend(tree_diagnostics);
        Box::new(TreeEventResult { trees })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::{builtin_registry, EtdlExtension, ExtensionContext};

    fn doc_with_trees(x_tree_event_yaml: &str) -> EtlDocument {
        let yaml = format!(
            r#"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
supplements:
  - id: etdl.tree-event
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: {{ id: I, message: "a#/m", next: C }}
    nodes:
      C: {{ type: consequence, operation: terminate }}
x-tree-event:
{x_tree_event_yaml}
"#
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn tree_event_extension_is_registered_and_built_in() {
        let registry = builtin_registry();
        assert!(registry.contains(TREE_EVENT_SUPPLEMENT));
        assert!(registry.list().contains(&TREE_EVENT_SUPPLEMENT));
    }

    #[test]
    fn valid_tree_parses_and_processes() {
        let doc = doc_with_trees(
            r#"  trees:
    - id: "demo"
      version: "1"
      root: "A"
      nodes:
        A:
          kind: leaf
"#,
        );
        let ext = TreeEventExtension::new();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        let result = ext.process(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
        assert_eq!(result.extension_id(), TREE_EVENT_SUPPLEMENT);
    }

    #[test]
    fn cyclic_tree_produces_e121() {
        let doc = doc_with_trees(
            r#"  trees:
    - id: "demo"
      version: "1"
      root: "A"
      nodes:
        A:
          kind: gate
          gate: NOT
          children: ["B"]
        B:
          kind: gate
          gate: NOT
          children: ["A"]
"#,
        );
        let ext = TreeEventExtension::new();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        ext.validate(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.iter().any(|d| d.code == "E-121" && d.message.contains("cycle")));
    }

    #[test]
    fn duplicate_tree_id_produces_e122() {
        let doc = doc_with_trees(
            r#"  trees:
    - id: "demo"
      version: "1"
      root: "A"
      nodes:
        A: { kind: leaf }
    - id: "demo"
      version: "2"
      root: "B"
      nodes:
        B: { kind: leaf }
"#,
        );
        let ext = TreeEventExtension::new();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        ext.validate(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.iter().any(|d| d.code == "E-122"));
    }

    #[test]
    fn document_without_x_tree_event_has_no_trees_and_no_diagnostics() {
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
        let (trees, diagnostics) = parse_and_validate_trees(&doc);
        assert!(trees.is_empty());
        assert!(diagnostics.is_empty());
    }
}
