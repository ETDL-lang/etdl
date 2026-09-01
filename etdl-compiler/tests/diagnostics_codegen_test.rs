//! Compile-check proof for the Diagnostics Supplement's codegen wiring:
//! generated code calls `record_failure_with_cause`/
//! `record_success_with_cause`/`record_branch_with_cause` (not the plain
//! `record_failure`/`record_success`/`record_branch`) for a node id some
//! Correlation's `spanValue` names, and the plain calls unchanged for
//! every other node. Unlike `security_codegen_test.rs`, there is no ECEL
//! path or runtime branch-selection behavior to prove live here — this is
//! a compile-only proof, mirroring `codegen_test.rs`'s own style rather
//! than the `gencheck`-harness `cargo run` proofs.

use etdl_parser::ast::EtlDocument;

const DOC_WITH_OPERATION_CORRELATION: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
supplements:
  - id: etdl.diagnostics
    version: "1.0"
components:
  messages:
    M:
      payload: { type: object }
faultTrees:
  PaymentGatewayFailure:
    topEvent: { id: Top, description: "d", rootCause: GatewayUnreachable }
    basicEvents:
      GatewayUnreachable: { description: "d", probability: 0.01 }
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "#/components/messages/M", next: ProcessPaymentOperation }
    nodes:
      ProcessPaymentOperation:
        type: operation
        action: execute
        handler: "h"
        next: FulfillmentConsequence
        onFailure: PaymentFailedConsequence
        onFailureProbabilitySource: "#/faultTrees/PaymentGatewayFailure/topEvent"
      FulfillmentConsequence: { type: consequence, operation: terminate }
      PaymentFailedConsequence: { type: consequence, operation: terminate }
x-diagnostics:
  correlations:
    - id: gateway-timeout-correlation
      spanAttribute: "etdl.node.id"
      spanValue: "ProcessPaymentOperation"
      causeRef: "#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable"
      description: "an anomaly on this span most often traces back to gateway unreachability"
"##;

const DOC_WITHOUT_DIAGNOSTICS: &str = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
components:
  messages:
    M:
      payload: { type: object }
faultTrees:
  PaymentGatewayFailure:
    topEvent: { id: Top, description: "d", rootCause: GatewayUnreachable }
    basicEvents:
      GatewayUnreachable: { description: "d", probability: 0.01 }
eventTrees:
  OrderFulfillment:
    initiatingEvent: { id: I, message: "#/components/messages/M", next: ProcessPaymentOperation }
    nodes:
      ProcessPaymentOperation:
        type: operation
        action: execute
        handler: "h"
        next: FulfillmentConsequence
        onFailure: PaymentFailedConsequence
        onFailureProbabilitySource: "#/faultTrees/PaymentGatewayFailure/topEvent"
      FulfillmentConsequence: { type: consequence, operation: terminate }
      PaymentFailedConsequence: { type: consequence, operation: terminate }
"##;

fn compile(yaml: &str) -> String {
    let doc: EtlDocument = serde_yaml::from_str(yaml).expect("doc parses");
    let registry = etdl_parser::asyncapi::AsyncApiRegistry::new();
    let result = etdl_compiler::Compiler::new().compile(&doc, &registry);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    result.rust_output.expect("compile produced output")
}

#[test]
fn correlated_operation_uses_with_cause_calls() {
    let generated = compile(DOC_WITH_OPERATION_CORRELATION);

    assert!(
        generated.contains(
            "record_success_with_cause(\"ProcessPaymentOperation\""
        ),
        "expected record_success_with_cause for the correlated node, got:\n{generated}"
    );
    assert!(
        generated.contains(
            "record_failure_with_cause(\"ProcessPaymentOperation\""
        ),
        "expected record_failure_with_cause for the correlated node, got:\n{generated}"
    );
    assert!(
        generated.contains("\"#/faultTrees/PaymentGatewayFailure/basicEvents/GatewayUnreachable\""),
        "expected the literal cause_ref baked into the generated call, got:\n{generated}"
    );
    assert!(
        generated.contains("Some(\"an anomaly on this span most often traces back to gateway unreachability\")"),
        "expected the literal description baked into the generated call, got:\n{generated}"
    );
}

#[test]
fn document_without_diagnostics_generates_plain_calls_only() {
    let generated = compile(DOC_WITHOUT_DIAGNOSTICS);

    assert!(generated.contains("record_success(\"ProcessPaymentOperation\""));
    assert!(generated.contains("record_failure(\"ProcessPaymentOperation\""));
    assert!(!generated.contains("record_success_with_cause"));
    assert!(!generated.contains("record_failure_with_cause"));
    assert!(!generated.contains("record_branch_with_cause"));
}
