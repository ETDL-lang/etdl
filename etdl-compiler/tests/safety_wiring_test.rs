//! Proves the Safety Supplement's diagnostics actually surface through the
//! real public `Compiler::validate`/`compile` entry points, not just
//! through `safety::parse_and_validate_safety` called directly (which
//! `etdl-compiler/src/safety.rs`'s own unit tests already cover). Mirrors
//! `performance_wiring_test.rs`: Safety is registered generically via
//! `Compiler::new()` seeding `Compiler::extensions` rather than a bespoke
//! direct call, so this test also doubles as proof that path actually
//! executes for a second, independently-registered supplement.

use etdl_compiler::Compiler;
use etdl_parser::asyncapi::AsyncApiRegistry;
use etdl_parser::ast::EtlDocument;

const DOC_WITH_BAD_SIL: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.safety
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: Barrier }
    nodes:
      Barrier:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.ok == true"
            probability: 0.95
            next: C
          - outcome: FAILURE
            condition: default
            probability: 0.05
            next: C
      C: { type: consequence, operation: terminate }
x-safety:
  barriers:
    - id: b1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/Barrier"
      sil: 9
      failureOutcome: SUCCESS
"##;

// A mismatched-riskIndex document: W-410 is a *warning*, not an error, so
// `run_extensions`'s "skip process() after an error" guard does not apply —
// both `validate()` and `process()` run for real. This is exactly the case
// that, for the Performance Supplement, previously produced a duplicated
// diagnostic (process() re-ran validation and re-pushed it); Safety's
// `process()` was written with that fix already applied, and this test
// proves it.
const DOC_WITH_MISMATCHED_RISK_INDEX: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.safety
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: Barrier }
    nodes:
      Barrier:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.ok == true"
            probability: 0.95
            next: C
          - outcome: FAILURE
            condition: default
            probability: 0.05
            next: C
      C: { type: consequence, operation: terminate }
x-safety:
  hazards:
    - id: h1
      description: "d"
      severity: catastrophic
      likelihood: remote
      riskIndex: 4
      consequenceRef: "#/eventTrees/OrderFulfillment/nodes/C"
"##;

// A fault-tree-backed FAILURE branch whose resolved probability (0.05)
// sits well outside the SIL 3 band [1e-4, 1e-3) the barrier declares —
// `validate_sil_constraints` needs *resolved* fault-tree probabilities,
// which only exist after `Compiler::validate_with_base`'s own pipeline
// runs `fault_tree::resolve_fault_trees_with_overrides` — this is the
// proof that actually happens, not something a unit test calling
// `parse_and_validate_safety`/`validate_sil_constraints` directly (as
// `safety.rs`'s own tests do) could ever catch.
const DOC_WITH_SIL_PFD_MISMATCH: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.safety
    version: "1.0"
faultTrees:
  GatewayFailure:
    topEvent: { id: Top, description: "d", rootCause: BE }
    basicEvents:
      BE: { description: "d", probability: 0.05 }
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: Barrier }
    nodes:
      Barrier:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.ok == true"
            probability: 0.95
            next: C
          - outcome: FAILURE
            condition: default
            probabilitySource: "#/faultTrees/GatewayFailure/topEvent"
            next: C
      C: { type: consequence, operation: terminate }
x-safety:
  barriers:
    - id: b1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/Barrier"
      sil: 3
      failureOutcome: FAILURE
"##;

const DOC_WITHOUT_SAFETY: &str = r##"
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
fn safety_diagnostics_surface_through_compiler_validate() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_BAD_SIL).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        diagnostics.iter().any(|d| d.code == "E-130"),
        "expected E-130 from the Safety Supplement to surface through \
         Compiler::validate (proving Compiler::new()'s extensions seeding \
         actually runs), got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn warning_only_diagnostic_is_not_duplicated_by_process() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_MISMATCHED_RISK_INDEX).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);
    let w410_count = diagnostics.iter().filter(|d| d.code == "W-410").count();

    assert_eq!(
        w410_count, 1,
        "expected exactly one W-410 (validate() and process() both run for a \
         warning-only document, since only an error skips process()), got {} in {:?}",
        w410_count,
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn sil_pfd_mismatch_surfaces_through_compiler_validate_not_just_compile() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_SIL_PFD_MISMATCH).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        diagnostics.iter().any(|d| d.code == "E-133"),
        "expected E-133 (SIL 3 declared, resolved probability 0.05 far outside \
         its [1e-4, 1e-3) band) to surface through Compiler::validate itself \
         — validate_sil_constraints is called from validate_with_base \
         specifically so etdl validate (not only etdl compile) catches this \
         before any code exists, got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn document_not_declaring_safety_is_unaffected() {
    // Compatibility guarantee (spec Section 7): silently ignoring
    // `x-safety` (never declared under `supplements:`) leaves the document
    // fully valid under core alone.
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITHOUT_SAFETY).expect("doc parses");
    let registry = AsyncApiRegistry::new();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        !diagnostics.iter().any(|d| d.code.starts_with("E-13") || d.code == "E-132" || d.code == "W-410"),
        "expected zero safety-related diagnostics, got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn both_performance_and_safety_extensions_run_together_without_interference() {
    let doc: EtlDocument = serde_yaml::from_str(
        r##"
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
  - id: etdl.safety
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: Barrier }
    nodes:
      Barrier:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.ok == true"
            probability: 0.95
            next: C
          - outcome: FAILURE
            condition: default
            probability: 0.05
            next: C
      C: { type: consequence, operation: terminate }
x-performance:
  budgets:
    - id: bad-order
      nodeRef: "#/eventTrees/OrderFulfillment"
      p50Ms: 900
      p95Ms: 800
      p99Ms: 2000
x-safety:
  barriers:
    - id: b1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/Barrier"
      sil: 9
      failureOutcome: SUCCESS
"##,
    )
    .expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);
    let codes: Vec<&str> = diagnostics.iter().map(|d| d.code.as_str()).collect();

    assert!(codes.contains(&"E-161"), "expected performance's E-161, got {codes:?}");
    assert!(codes.contains(&"E-130"), "expected safety's E-130, got {codes:?}");
    assert_eq!(
        codes.iter().filter(|c| **c == "E-161").count(),
        1,
        "E-161 must not be duplicated by a second extension's run: {codes:?}"
    );
    assert_eq!(
        codes.iter().filter(|c| **c == "E-130").count(),
        1,
        "E-130 must not be duplicated: {codes:?}"
    );
}
