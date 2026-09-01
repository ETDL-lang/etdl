//! Proves the Diagnostics Supplement's diagnostics actually surface through
//! the real public `Compiler::validate`/`compile` entry points, not just
//! through `diagnostics::parse_and_validate_diagnostics` called directly
//! (which `etdl-compiler/src/diagnostics.rs`'s own unit tests already
//! cover). Mirrors `performance_wiring_test.rs`/`safety_wiring_test.rs`.

use etdl_compiler::Compiler;
use etdl_parser::asyncapi::AsyncApiRegistry;
use etdl_parser::ast::EtlDocument;

const DOC_WITH_UNRESOLVABLE_CAUSE_REF: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.diagnostics
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: Op }
    nodes:
      Op:
        type: operation
        action: execute
        handler: "h"
        next: C
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: A }
    basicEvents:
      A: { description: "d", probability: 0.01 }
x-diagnostics:
  correlations:
    - id: c1
      spanAttribute: "etdl.node.id"
      spanValue: "x"
      causeRef: "#/faultTrees/FT/basicEvents/DoesNotExist"
"##;

// A no-correlation Operation document: W-412 is a *warning*, not an error,
// so `run_extensions`'s "skip process() after an error" guard does not
// apply — both `validate()` and `process()` run for real. This is exactly
// the case that, for the Performance Supplement, previously produced a
// duplicated diagnostic; Diagnostics' `process()` was written with that fix
// already applied, and this test proves it.
const DOC_WITH_UNCORRELATED_OPERATION: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.diagnostics
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: Op }
    nodes:
      Op:
        type: operation
        action: execute
        handler: "h"
        next: C
        onFailureProbabilitySource: "#/faultTrees/FT/topEvent"
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: A }
    basicEvents:
      A: { description: "d", probability: 0.01 }
x-diagnostics:
  anomalyRules:
    - id: r1
      monitors: "#/eventTrees/OrderFulfillment/nodes/Op"
"##;

const DOC_WITH_UNRESOLVABLE_SPAN_VALUE: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.diagnostics
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: Op }
    nodes:
      Op:
        type: operation
        action: execute
        handler: "h"
        next: C
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: A }
    basicEvents:
      A: { description: "d", probability: 0.01 }
x-diagnostics:
  correlations:
    - id: c1
      spanAttribute: "etdl.node.id"
      spanValue: "DoesNotExist"
      causeRef: "#/faultTrees/FT/basicEvents/A"
"##;

const DOC_WITHOUT_DIAGNOSTICS: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
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
fn diagnostics_supplement_diagnostics_surface_through_compiler_validate() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_UNRESOLVABLE_CAUSE_REF).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        diagnostics.iter().any(|d| d.code == "E-150"),
        "expected E-150 from the Diagnostics Supplement to surface through \
         Compiler::validate (proving Compiler::new()'s extensions seeding \
         actually runs), got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn warning_only_diagnostic_is_not_duplicated_by_process() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_UNCORRELATED_OPERATION).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);
    let w412_count = diagnostics.iter().filter(|d| d.code == "W-412").count();

    assert_eq!(
        w412_count, 1,
        "expected exactly one W-412 (validate() and process() both run for a \
         warning-only document, since only an error skips process()), got {} in {:?}",
        w412_count,
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn unresolvable_span_value_surfaces_through_compiler_validate() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_UNRESOLVABLE_SPAN_VALUE).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        diagnostics.iter().any(|d| d.code == "E-152"),
        "expected E-152 (spanAttribute 'etdl.node.id' with an unresolvable spanValue) to \
         surface through Compiler::validate, got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn document_not_declaring_diagnostics_is_unaffected() {
    // Compatibility guarantee (spec Section 7): silently ignoring
    // `x-diagnostics` (never declared under `supplements:`) leaves the
    // document fully valid under core alone.
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITHOUT_DIAGNOSTICS).expect("doc parses");
    let registry = AsyncApiRegistry::new();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == "E-150" || d.code == "E-151" || d.code == "E-152" || d.code == "W-412"),
        "expected zero diagnostics-supplement-related diagnostics, got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}
