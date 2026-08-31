//! A codegen-level `Result::Err(String)` (e.g. a Barrier using
//! `reliability.in_range`/`performance.in_budget` whose link doesn't
//! resolve — see `codegen/rust.rs`'s `try_render_reliability_condition`/
//! `try_render_performance_condition`) used to fail safely (no broken code
//! ever emitted) but *silently*: `Compiler::compile_with_base`/
//! `compile_target_with_base` discarded the error string via
//! `gen_result.ok()`, so the CLI printed "compilation failed... 0 errors"
//! with no indication why. Fixed by pushing the message as an `E-109`
//! diagnostic through the same channel every other failure already
//! reports through. These tests prove it for both supplements that have a
//! codegen-level "not properly linked" check.

use etdl_compiler::Compiler;
use etdl_parser::asyncapi::AsyncApiRegistry;
use etdl_parser::ast::EtlDocument;

const PERFORMANCE_BARRIER_NOT_LINKED: &str = r##"
etdl: "1.0.0"
info: { title: "T", version: "1.0.0", domain: "D" }
components:
  messages:
    Trigger: { name: Trigger, payload: { type: object } }
supplements:
  - id: etdl.performance
    version: "1.0"
x-performance:
  budgets:
    - id: op-budget
      nodeRef: "#/eventTrees/T/nodes/DoThing"
      p50Ms: 10
      p95Ms: 50
      p99Ms: 100
eventTrees:
  T:
    initiatingEvent: { id: Trig, message: "#/components/messages/Trigger", next: PerfBarrier }
    nodes:
      PerfBarrier:
        type: barrier
        branches:
          - outcome: OK
            condition: "performance.in_budget == true"
            probability: 0.99
            next: DoThing
          - outcome: DEGRADED
            condition: default
            probability: 0.01
            next: C
      DoThing: { type: operation, action: execute, handler: "doThing", next: C }
      C: { type: consequence, operation: terminate }
"##;

// Deliberately declares `probabilitySource` (satisfying core's V-203,
// which requires every branch to have one) pointing at a fault tree that
// is *not* listed under `x-live-reliability.faultTrees` — typeck's E-173
// only checks the supplement is declared and the path shape is exactly
// `reliability.in_range`, not whether this specific link resolves, so
// this reaches codegen's own defensive check.
const RELIABILITY_BRANCH_NOT_LINKED: &str = r##"
etdl: "1.0.0"
info: { title: "T", version: "1.0.0", domain: "D" }
components:
  messages:
    Trigger: { name: Trigger, payload: { type: object } }
faultTrees:
  GatewayFailure:
    topEvent: { id: GatewayFailureTop, description: "top", rootCause: GatewayUnreachable }
    basicEvents:
      GatewayUnreachable: { description: "gw", probability: 0.1 }
x-live-reliability:
  faultTrees: []
supplements:
  - id: etdl.live-reliability
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: { id: Trig, message: "#/components/messages/Trigger", next: RiskBarrier }
    nodes:
      RiskBarrier:
        type: barrier
        branches:
          - outcome: OK
            condition: "reliability.in_range == true"
            probabilitySource: "#/faultTrees/GatewayFailure/topEvent"
            next: C
          - outcome: BAD
            condition: default
            next: C
      C: { type: consequence, operation: terminate }
"##;

#[test]
fn performance_barrier_not_linked_surfaces_as_e109() {
    let doc: EtlDocument = serde_yaml::from_str(PERFORMANCE_BARRIER_NOT_LINKED).expect("doc parses");
    let registry = AsyncApiRegistry::new();

    let result = Compiler::new().compile(&doc, &registry);

    assert!(result.rust_output.is_none(), "codegen should not emit anything");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "E-109" && d.message.contains("barrierChecks")),
        "expected E-109 explaining the missing barrierChecks link, got {:?}",
        result.diagnostics.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
    );
}

#[test]
fn reliability_branch_not_linked_surfaces_as_e109() {
    let doc: EtlDocument = serde_yaml::from_str(RELIABILITY_BRANCH_NOT_LINKED).expect("doc parses");
    let registry = AsyncApiRegistry::new();

    let result = Compiler::new().compile(&doc, &registry);

    assert!(result.rust_output.is_none(), "codegen should not emit anything");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "E-109" && d.message.contains("etdl.live-reliability")),
        "expected E-109 explaining the unresolved live-reliability link, got {:?}",
        result.diagnostics.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
    );
}
