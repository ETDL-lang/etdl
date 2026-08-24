//! Proves the Performance Supplement's diagnostics actually surface
//! through the real public `Compiler::validate`/`compile` entry points, not
//! just through `performance::parse_and_validate_budgets` called directly
//! (which `etdl-compiler/src/performance.rs`'s own unit tests already
//! cover). Unlike the Tree Event Supplement — which has no equivalent
//! wiring-level test today — Performance is registered generically via
//! `Compiler::new()` seeding `Compiler::extensions` (see `lib.rs` and
//! `performance` module docs) rather than a bespoke direct call, so this
//! test also doubles as proof that path actually executes.

use etdl_compiler::Compiler;
use etdl_parser::asyncapi::AsyncApiRegistry;
use etdl_parser::ast::EtlDocument;

const DOC_WITH_BAD_ORDERING: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.performance
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
x-performance:
  budgets:
    - id: bad-order
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/Op"
      p50Ms: 900
      p95Ms: 800
      p99Ms: 2000
"##;

// A duplicate-`nodeRef` document: W-413 is a *warning*, not an error, so
// `run_extensions`'s "skip process() after an error" guard does not apply —
// both `validate()` and `process()` run for real. This is exactly the case
// that previously produced two identical W-413s (process() re-ran
// `parse_and_validate_budgets` and re-pushed the same diagnostics
// `validate()` had already reported); an E-160/E-161 (error) case would not
// have caught that bug, since process() is skipped entirely once an error
// is present.
const DOC_WITH_DUPLICATE_NODE_REF: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.performance
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
x-performance:
  budgets:
    - id: first
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/Op"
      p50Ms: 100
      p95Ms: 200
      p99Ms: 300
    - id: second
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/Op"
      p50Ms: 100
      p95Ms: 200
      p99Ms: 300
"##;

const DOC_WITHOUT_PERFORMANCE: &str = r##"
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
fn performance_diagnostics_surface_through_compiler_validate() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_BAD_ORDERING).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        diagnostics.iter().any(|d| d.code == "E-161"),
        "expected E-161 from the Performance Supplement to surface through \
         Compiler::validate (proving Compiler::new()'s extensions seeding \
         actually runs), got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn warning_only_diagnostic_is_not_duplicated_by_process() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_DUPLICATE_NODE_REF).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);
    let w413_count = diagnostics.iter().filter(|d| d.code == "W-413").count();

    assert_eq!(
        w413_count, 1,
        "expected exactly one W-413 (validate() and process() both run for a \
         warning-only document, since only an error skips process() — a \
         process() that re-validates and re-pushes diagnostics would double \
         this), got {} in {:?}",
        w413_count,
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn document_not_declaring_performance_is_unaffected() {
    // Compatibility guarantee (spec Section 7): silently ignoring
    // `x-performance` (never declared under `supplements:`) leaves the
    // document fully valid under core alone — proves the generic
    // `Compiler::extensions` seeding does not run unconditionally, only
    // when a document opts in.
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITHOUT_PERFORMANCE).expect("doc parses");
    let registry = AsyncApiRegistry::new();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        !diagnostics.iter().any(|d| d.code.starts_with("E-16") || d.code == "W-413"),
        "expected zero performance-related diagnostics, got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}
