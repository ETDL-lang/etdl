//! Proves a non-built-in `EtdlExtension` — the shape a third-party,
//! non-core supplement (core spec Section 11.4/11.5; e.g. a future
//! `etdl.chain` implementation) would be — can actually be registered with
//! `Compiler::with_extension` and have its `validate`/`process` run during
//! real compilation, contributing both a diagnostic and a fault-tree
//! probability override. Before this test existed, `Compiler` had no way
//! to accept an extension beyond the two hard-coded built-in ones
//! (`etdl.reliability`, `etdl.tree-event`), even though `EtdlExtension`/
//! `ExtensionRegistry` were already public API — the mechanism's shape was
//! public but nothing let a caller actually wire an instance of it into
//! `compile()`/`validate()`.

use etdl_compiler::extension::{EtdlExtension, ExtensionContext, ExtensionResult};
use etdl_compiler::validate::Diagnostic;
use etdl_compiler::Compiler;
use etdl_parser::asyncapi::AsyncApiRegistry;
use etdl_parser::ast::EtlDocument;

const EXTENSION_ID: &str = "etdl.example-third-party";

/// A minimal third-party extension: on `validate`, emits an advisory
/// diagnostic; on `process`, resolves a fixed probability for one basic
/// event, exactly the shape a real external-value-resolving supplement
/// (like the reliability extension, or a future chain-attestation one)
/// would use.
struct ThirdPartyExtension;

impl EtdlExtension for ThirdPartyExtension {
    fn id(&self) -> &str {
        EXTENSION_ID
    }

    fn version(&self) -> &str {
        "1.0"
    }

    fn validate(
        &self,
        _doc: &EtlDocument,
        _context: &ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        diagnostics.push(Diagnostic::warning(
            "W-THIRD-PARTY-001",
            "third-party extension validate() ran".to_string(),
        ));
    }

    fn process(
        &self,
        _doc: &EtlDocument,
        _context: &ExtensionContext<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn ExtensionResult + '_> {
        Box::new(ThirdPartyResult)
    }
}

struct ThirdPartyResult;

impl ExtensionResult for ThirdPartyResult {
    fn extension_id(&self) -> &str {
        EXTENSION_ID
    }

    fn basic_event_overrides(&self) -> Vec<(String, f64)> {
        vec![(
            etdl_compiler::fault_tree::override_key("FT", "A"),
            0.5,
        )]
    }
}

const DOC: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.example-third-party
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: Op }
    nodes:
      Op:
        type: operation
        action: execute
        handler: "some_handler"
        next: C
        onFailure: C
        onFailureProbabilitySource: "#/faultTrees/FT/topEvent"
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: A }
    basicEvents:
      A: { description: "overridden by the third-party extension", probability: 0.001 }
"##;

fn stub_registry() -> AsyncApiRegistry {
    let mut registry = AsyncApiRegistry::new();
    let stub = r#"
asyncapi: '3.0.0'
info:
  title: stub
  version: '1.0.0'
channels: {}
components:
  messages:
    m:
      name: m
      payload:
        type: object
        properties:
          ok:
            type: boolean
"#;
    let _ = registry.load_from_content("a", stub, false);
    registry
}

#[test]
fn third_party_extension_validate_and_process_both_run() {
    let doc: EtlDocument = serde_yaml::from_str(DOC).expect("doc parses");
    let registry = stub_registry();

    let compiler = Compiler::new().with_extension(Box::new(ThirdPartyExtension));
    let diagnostics = compiler.validate(&doc, &registry);

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "W-THIRD-PARTY-001"),
        "expected the third-party extension's validate() diagnostic, got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );

    // The extension's process() override (A -> 0.5) must have fed fault-tree
    // evaluation, which `onFailureProbabilitySource` embeds into the
    // generated Rust as a constant: it must reflect 0.5, not the document's
    // own declared 0.001 for A.
    let compiled = compiler.compile(&doc, &registry);
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.code == "W-THIRD-PARTY-001"),
        "expected the same diagnostic from compile() as from validate()"
    );
    let rust = compiled.rust_output.expect("rust output present");
    assert!(
        rust.contains("0.5"),
        "expected the third-party extension's override (0.5) embedded in generated code, got:\n{rust}"
    );
    assert!(
        !rust.contains("0.001"),
        "the document's own declared probability (0.001) must have been overridden, got:\n{rust}"
    );
}
