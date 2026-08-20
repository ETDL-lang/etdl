//! ETDL conformance suite.
//!
//! Each case is a self-contained ETDL document plus expected behavior:
//! - `valid` cases must validate with zero errors.
//! - `invalid` cases must produce the listed diagnostic codes.
//! - `probability` cases must resolve to the expected top-event probability.
//!
//! The suite is designed so a third-party ETDL implementation could run the
//! same corpus: every case is expressed declaratively here (document YAML +
//! expectations), and the runner is trivially portable.

use etdl_compiler::validate::Diagnostic;
use etdl_parser::ast::EtlDocument;
use etdl_parser::asyncapi::AsyncApiRegistry;

struct Case {
    name: &'static str,
    yaml: &'static str,
    expected_valid: bool,
    expected_codes: &'static [&'static str],
    expected_probability: Option<(&'static str, f64)>,
}

fn run_case(case: &Case) -> (Vec<Diagnostic>, std::collections::BTreeMap<String, f64>) {
    let yaml_with_import = case.yaml.replace(
        "asyncapi_imports: {}",
        "asyncapi_imports:\n  a: \"./stub.yaml\"",
    );
    let doc: EtlDocument = serde_yaml::from_str(&yaml_with_import).expect("case parses");

    // Build a stub registry that resolves `a#/components/messages/m` and
    // provides a payload with `ok: boolean`, so ECEL type-checking and
    // downstream stages run.
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

    let compiler = etdl_compiler::Compiler::new();
    let diagnostics = compiler.validate(&doc, &registry);
    let probs = etdl_compiler::fault_tree::resolve_fault_trees(&doc, &mut Vec::new());
    (diagnostics, probs)
}

fn assert_case(case: &Case) {
    let (diagnostics, probs) = run_case(case);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.clone()).collect();

    if case.expected_valid {
        assert!(
            diagnostics.iter().all(|d| !d.is_error()),
            "case '{}' should be valid but got: {:?}",
            case.name,
            codes
        );
    } else {
        for expected in case.expected_codes {
            assert!(
                codes.iter().any(|c| c == expected),
                "case '{}' expected code {} but got {:?}",
                case.name,
                expected,
                codes
            );
        }
    }

    if let Some((ft, expected_p)) = case.expected_probability {
        let actual = probs.get(ft).copied().unwrap_or(f64::NAN);
        assert!(
            (actual - expected_p).abs() < 1e-6,
            "case '{}' expected P({})={} but got {}",
            case.name,
            ft,
            expected_p,
            actual
        );
    }
}

// Shared minimal valid skeleton (asyncapi reference not resolved -> type checks
// skipped, so conditions do not raise V-204).
const VALID_TREE: &str = r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent:
      id: I
      message: "a#/components/messages/m"
      next: B
    nodes:
      B:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.ok == true"
            probability: 0.6
            next: C
          - outcome: FAILURE
            condition: default
            probability: 0.4
            next: C
      C:
        type: consequence
        operation: terminate
"#;

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "valid-minimal",
            yaml: VALID_TREE,
            expected_valid: true,
            expected_codes: &[],
            expected_probability: None,
        },
        Case {
            name: "invalid-future-major",
            yaml: r#"
etdl: "2.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
"#,
            expected_valid: false,
            expected_codes: &["E-100"],
            expected_probability: None,
        },
        Case {
            name: "invalid-branch-sum",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: B }
    nodes:
      B:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.ok == true"
            probability: 0.9
            next: C
          - outcome: FAILURE
            condition: default
            probability: 0.2
            next: C
      C: { type: consequence, operation: terminate }
"#,
            expected_valid: false,
            expected_codes: &["V-203"],
            expected_probability: None,
        },
        Case {
            name: "invalid-payload-type-mismatch",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: B }
    nodes:
      B:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.ok > 0"
            probability: 0.5
            next: C
          - outcome: FAILURE
            condition: default
            probability: 0.5
            next: C
      C: { type: consequence, operation: terminate }
"#,
            // Regression case for the "V-204 is dead for `message.payload.*`
            // paths" bug: `ok` is a boolean field but the barrier compares it
            // with an ordering operator, which requires a number. Before the
            // fix, `resolve_schema_path` never stripped the `payload` root
            // segment (only `message`), so this path always resolved to
            // `Unknown` and V-204 silently never fired for ANY payload path.
            expected_valid: false,
            expected_codes: &["V-204"],
            expected_probability: None,
        },
        Case {
            name: "probability-and",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: G }
    gates:
      G:
        type: AND
        inputs: [A, B]
    basicEvents:
      A: { description: "a", probability: 0.5 }
      B: { description: "b", probability: 0.5 }
"#,
            expected_valid: true,
            expected_codes: &[],
            expected_probability: Some(("FT", 0.25)),
        },
        Case {
            name: "probability-or",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: G }
    gates:
      G:
        type: OR
        inputs: [A, B]
    basicEvents:
      A: { description: "a", probability: 0.1 }
      B: { description: "b", probability: 0.2 }
"#,
            expected_valid: true,
            expected_codes: &[],
            expected_probability: Some(("FT", 1.0 - 0.9 * 0.8)),
        },
        Case {
            name: "probability-exponential",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: A }
    basicEvents:
      A: { description: "a", failureRate: 0.1, missionTime: 10 }
"#,
            expected_valid: true,
            expected_codes: &[],
            expected_probability: Some(("FT", 1.0 - (-0.1f64 * 10.0).exp())),
        },
        Case {
            name: "probability-voting",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: G }
    gates:
      G:
        type: VOTING
        k: 2
        inputs: [A, B, C]
    basicEvents:
      A: { description: "a", probability: 0.5 }
      B: { description: "b", probability: 0.5 }
      C: { description: "c", probability: 0.5 }
"#,
            expected_valid: true,
            expected_codes: &[],
            // 2-of-3, Bin(3, 0.5): P(X>=2) = 0.5
            expected_probability: Some(("FT", 0.5)),
        },
        Case {
            name: "invalid-voting-k",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: G }
    gates:
      G:
        type: VOTING
        k: 5
        inputs: [A, B]
    basicEvents:
      A: { description: "a", probability: 0.5 }
      B: { description: "b", probability: 0.5 }
"#,
            expected_valid: false,
            expected_codes: &["V-502"],
            expected_probability: None,
        },
        Case {
            name: "invalid-negative-failure-rate",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: A }
    basicEvents:
      A: { description: "a", failureRate: -0.1, missionTime: 10 }
"#,
            expected_valid: false,
            expected_codes: &["V-507"],
            expected_probability: None,
        },
        Case {
            name: "invalid-out-of-range-probability",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: A }
    basicEvents:
      A: { description: "a", probability: 1.5 }
"#,
            expected_valid: false,
            expected_codes: &["V-507"],
            expected_probability: None,
        },
        Case {
            name: "invalid-nonterminating",
            yaml: r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: O }
    nodes:
      O:
        type: operation
        action: execute
        handler: "h"
        next: O
"#,
            expected_valid: false,
            expected_codes: &["V-104", "V-102"],
            expected_probability: None,
        },
        Case {
            name: "invalid-transfer-missing-tree",
            yaml: r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/components/messages/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: E }
    basicEvents:
      E: { description: "e", probability: 0.01 }
    transfers:
      Gone:
        target: "#/faultTrees/Nope/topEvent"
"##,
            expected_valid: false,
            expected_codes: &["V-506"],
            expected_probability: None,
        },
    ]
}

/// Run every case and panic on any failure. Exposed so both the integration
/// test and a future CLI conformance command can invoke it.
pub fn run_conformance_main() -> usize {
    let cases = cases();
    let mut failures = 0;
    for case in &cases {
        let result = std::panic::catch_unwind(|| assert_case(case));
        match result {
            Ok(()) => {}
            Err(_) => {
                failures += 1;
                eprintln!("CONFORMANCE FAIL: {}", case.name);
            }
        }
    }
    assert_eq!(
        failures,
        0,
        "{} of {} conformance cases failed",
        failures,
        cases.len()
    );
    eprintln!("conformance: {} cases passed", cases.len());
    failures
}

#[cfg(test)]
mod tests {
    #[test]
    fn conformance_suite() {
        super::run_conformance_main();
    }
}
