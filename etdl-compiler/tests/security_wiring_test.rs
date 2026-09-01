//! Proves the Security Supplement's diagnostics actually surface through
//! the real public `Compiler::validate`/`compile` entry points, not just
//! through `security::parse_and_validate_security` called directly (which
//! `etdl-compiler/src/security.rs`'s own unit tests already cover) —
//! including its cross-supplement dependency on the *already-registered*
//! Tree Event Supplement, which nothing before this test proved actually
//! resolves through `Compiler::new()`'s real extension list rather than
//! only through `tree_event::parse_and_validate_trees` called directly by
//! `security`'s own unit-test fixtures.

use etdl_compiler::Compiler;
use etdl_parser::asyncapi::AsyncApiRegistry;
use etdl_parser::ast::EtlDocument;

const DOC_WITH_UNRESOLVABLE_TREE_REF: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.security
    version: "1.0"
  - id: etdl.tree-event
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
x-security:
  threatModels:
    - id: tm1
      treeRef: "does-not-exist"
      leafCategories: {}
"##;

// A control mitigating an uncategorized-but-real leaf: W-411 is a
// *warning*, not an error, so `run_extensions`'s "skip process() after an
// error" guard does not apply — both `validate()` and `process()` run for
// real. This is exactly the case that, for the Performance Supplement,
// previously produced a duplicated diagnostic; Security's `process()` was
// written with that fix already applied, and this test proves it.
const DOC_WITH_UNCATEGORIZED_MITIGATED_LEAF: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.security
    version: "1.0"
  - id: etdl.tree-event
    version: "1.0"
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: RateLimitBarrier }
    nodes:
      RateLimitBarrier:
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
x-tree-event:
  trees:
    - id: "gateway-compromise"
      version: "1"
      root: "GatewayCompromised"
      nodes:
        ApiKeyLeak:
          kind: leaf
        CredentialStuffing:
          kind: leaf
        GatewayCompromised:
          kind: gate
          gate: OR
          children: ["ApiKeyLeak", "CredentialStuffing"]
x-security:
  threatModels:
    - id: tm1
      treeRef: "gateway-compromise"
      leafCategories:
        CredentialStuffing: spoofing
  controls:
    - id: c1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      controlId: "SC-5"
      mitigates: ["ApiKeyLeak"]
"##;

// A fault-tree-backed FAILURE branch whose resolved probability (0.05)
// exceeds the declared maxBypassProbability (0.001) —
// `validate_control_thresholds` needs *resolved* fault-tree probabilities,
// which only exist after `Compiler::validate_with_base`'s own pipeline
// runs `fault_tree::resolve_fault_trees_with_overrides` — this is the
// proof that actually happens, not something a unit test calling
// `parse_and_validate_security`/`validate_control_thresholds` directly (as
// `security.rs`'s own tests do) could ever catch.
const DOC_WITH_BYPASS_THRESHOLD_MISMATCH: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  a: "./stub.yaml"
supplements:
  - id: etdl.security
    version: "1.0"
  - id: etdl.tree-event
    version: "1.0"
faultTrees:
  RateLimitBypass:
    topEvent: { id: Top, description: "d", rootCause: BE }
    basicEvents:
      BE: { description: "d", probability: 0.05 }
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: RateLimitBarrier }
    nodes:
      RateLimitBarrier:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.ok == true"
            probability: 0.95
            next: C
          - outcome: FAILURE
            condition: default
            probabilitySource: "#/faultTrees/RateLimitBypass/topEvent"
            next: C
      C: { type: consequence, operation: terminate }
x-tree-event:
  trees:
    - id: "rate-limit-attack-tree"
      version: "1"
      root: "Bypassed"
      nodes:
        RateLimitBypassLeaf:
          kind: leaf
        OtherLeaf:
          kind: leaf
        Bypassed:
          kind: gate
          gate: OR
          children: ["RateLimitBypassLeaf", "OtherLeaf"]
x-security:
  threatModels:
    - id: tm1
      treeRef: "rate-limit-attack-tree"
      leafCategories: {}
  controls:
    - id: c1
      nodeRef: "#/eventTrees/OrderFulfillment/nodes/RateLimitBarrier"
      controlId: "SC-5"
      mitigates: ["RateLimitBypassLeaf"]
      bypassOutcome: FAILURE
      maxBypassProbability: 0.001
"##;

const DOC_WITHOUT_SECURITY: &str = r##"
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
fn security_diagnostics_surface_through_compiler_validate() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_UNRESOLVABLE_TREE_REF).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        diagnostics.iter().any(|d| d.code == "E-140"),
        "expected E-140 from the Security Supplement to surface through \
         Compiler::validate (proving Compiler::new()'s extensions seeding \
         actually runs), got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn tree_event_cross_dependency_resolves_through_the_real_pipeline() {
    // The inverse of the above: a treeRef that DOES resolve, proving
    // security::parse_and_validate_security's internal call to
    // tree_event::parse_and_validate_trees sees the same x-tree-event data
    // Tree Event's own (separately, directly-called) pipeline wiring does.
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_UNCATEGORIZED_MITIGATED_LEAF).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        !diagnostics.iter().any(|d| d.code == "E-140" || d.code == "E-141"),
        "expected the treeRef/leaf resolution to succeed (no E-140/E-141), got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn warning_only_diagnostic_is_not_duplicated_by_process() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_UNCATEGORIZED_MITIGATED_LEAF).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);
    let w411_count = diagnostics.iter().filter(|d| d.code == "W-411").count();

    assert_eq!(
        w411_count, 1,
        "expected exactly one W-411 (validate() and process() both run for a \
         warning-only document, since only an error skips process()), got {} in {:?}",
        w411_count,
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn bypass_threshold_mismatch_surfaces_through_compiler_validate_not_just_compile() {
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITH_BYPASS_THRESHOLD_MISMATCH).expect("doc parses");
    let registry = stub_registry();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        diagnostics.iter().any(|d| d.code == "E-142"),
        "expected E-142 (maxBypassProbability 0.001 declared, resolved probability 0.05 far \
         exceeds it) to surface through Compiler::validate itself — \
         validate_control_thresholds is called from validate_with_base specifically so etdl \
         validate (not only etdl compile) catches this before any code exists, got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn document_not_declaring_security_is_unaffected() {
    // Compatibility guarantee (spec Section 7): silently ignoring
    // `x-security` (never declared under `supplements:`) leaves the
    // document fully valid under core alone.
    let doc: EtlDocument = serde_yaml::from_str(DOC_WITHOUT_SECURITY).expect("doc parses");
    let registry = AsyncApiRegistry::new();

    let diagnostics = Compiler::new().validate(&doc, &registry);

    assert!(
        !diagnostics.iter().any(|d| {
            matches!(d.code.as_str(), "E-140" | "E-141" | "E-142" | "E-143" | "W-411" | "W-416")
        }),
        "expected zero security-related diagnostics, got {:?}",
        diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}
