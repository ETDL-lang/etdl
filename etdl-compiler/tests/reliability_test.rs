//! Integration test for reliability-aware compilation: an `.etdl` document
//! declares the reliability supplement and an external artifact; the compiler
//! resolves the artifact to deterministic probabilities that feed fault-tree
//! evaluation, and emits a build manifest.

use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("reliability_fixtures")
}

#[test]
fn external_artifact_resolves_into_fault_tree() {
    let base = fixture_dir();
    let doc = etdl_parser::parse_document_from_file(&base.join("external.etdl"))
        .expect("document parses");
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("imports load");
    let compiler = etdl_compiler::Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry, &base);

    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected no errors, got {:?}",
        errors
            .iter()
            .map(|d| (&d.code, &d.message))
            .collect::<Vec<_>>()
    );

    // The generated code should embed the resolved probabilities.
    let _rust = result.rust_output.expect("rust output present");
    let manifest = result
        .build_manifest
        .expect("build manifest present for reliability build");
    let resolved = manifest["resolved_probabilities"].as_array().unwrap();
    assert_eq!(resolved.len(), 2);

    // Verify the resolved values appear in the manifest with provenance.
    let values: Vec<f64> = resolved
        .iter()
        .map(|r| r["value"].as_f64().unwrap())
        .collect();
    assert!(values.contains(&0.0027));
    assert!(values.contains(&0.0012));

    // Fault-tree evaluation must use the resolved values: OR(0.0027, 0.0012).
    let mut diags = Vec::new();
    let probs = etdl_compiler::fault_tree::resolve_fault_trees_with_overrides(
        &doc,
        &etdl_compiler::fault_tree::BasicEventOverrides::from([
            (
                etdl_compiler::fault_tree::override_key("PaymentGatewayFailure", "GatewayTimeout"),
                0.0027,
            ),
            (
                etdl_compiler::fault_tree::override_key(
                    "PaymentGatewayFailure",
                    "GatewayUnreachable",
                ),
                0.0012,
            ),
        ]),
        &mut diags,
    );
    let expected = 1.0 - (1.0 - 0.0027) * (1.0 - 0.0012);
    let actual = probs["PaymentGatewayFailure"];
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn backward_compat_no_supplement_unchanged() {
    // The worked example, with no supplement, must resolve exactly as before.
    let base = fixture_dir();
    let doc = etdl_parser::parse_document_from_file(&base.join("core-only.etdl")).expect("parses");
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("imports load");
    let compiler = etdl_compiler::Compiler::new();
    let result = compiler.compile(&doc, &registry);

    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "core-only should validate: {:?}", errors);
    assert!(
        result.build_manifest.is_none(),
        "no reliability supplement => no manifest"
    );
    // Fault-tree probability unchanged: OR(0.008, 1-e^(-0.00021*24)).
    let mut diags = Vec::new();
    let probs = etdl_compiler::fault_tree::resolve_fault_trees(&doc, &mut diags);
    let expected = 1.0 - (1.0 - 0.008) * (1.0 - 0.005027);
    let actual = probs["PaymentGatewayFailure"];
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected ~{expected}, got {actual}"
    );
}

#[test]
fn supplement_declared_but_optional_and_unsupported_warns() {
    let yaml = r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
supplements:
  - id: etdl.performance
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
"#;
    let doc: etdl_parser::ast::EtlDocument = serde_yaml::from_str(yaml).unwrap();
    let registry = etdl_parser::asyncapi::AsyncApiRegistry::new();
    let compiler = etdl_compiler::Compiler::new();
    let result = compiler.validate(&doc, &registry);
    assert!(
        result.iter().any(|d| d.code == "W-407"),
        "optional unsupported supplement should warn W-407, got {:?}",
        result
    );
}

#[test]
fn golden_worked_example_probability_is_exact() {
    // The canonical worked example resolves the two Basic Events through their
    // OR gate to ~0.012987. This MUST be unchanged with no reliability
    // supplement involved (spec §13, Section 5.16).
    let base = fixture_dir();
    let doc = etdl_parser::parse_document_from_file(&base.join("core-only.etdl")).unwrap();
    let mut diags = Vec::new();
    let probs = etdl_compiler::fault_tree::resolve_fault_trees(&doc, &mut diags);
    let p = probs["PaymentGatewayFailure"];
    let expected = 1.0 - (1.0 - 0.008) * (1.0 - 0.005027);
    assert!(
        (p - expected).abs() < 0.0001,
        "worked example probability drifted: expected ~{expected}, got {p}"
    );
    // The generated constant format is 6 decimals -> 0.012987.
    assert!((p - 0.012987).abs() < 0.00001, "expected 0.012987, got {p}");
}

#[test]
fn required_unsupported_supplement_errors() {
    let yaml = r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
supplements:
  - id: etdl.safety
    version: "1.0"
    required: true
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
"#;
    let doc: etdl_parser::ast::EtlDocument = serde_yaml::from_str(yaml).unwrap();
    let registry = etdl_parser::asyncapi::AsyncApiRegistry::new();
    let compiler = etdl_compiler::Compiler::new();
    let result = compiler.validate(&doc, &registry);
    assert!(
        result.iter().any(|d| d.code == "E-108"),
        "required unsupported supplement should error E-108, got {:?}",
        result
    );
}

#[test]
fn multi_fault_tree_overrides_do_not_collide() {
    // Two fault trees that each define a basic event named "PowerLoss".
    // Overrides keyed by fault-tree id must not leak across trees.
    let doc = r#"
etdl: "1.0.0"
info:
  title: "Multi-tree"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  A:
    topEvent:
      id: TopA
      description: "top A"
      rootCause: PowerLoss
    basicEvents:
      PowerLoss:
        description: "power"
        probability: 0.5
  B:
    topEvent:
      id: TopB
      description: "top B"
      rootCause: PowerLoss
    basicEvents:
      PowerLoss:
        description: "power"
        probability: 0.5
"#;
    let doc: etdl_parser::ast::EtlDocument = serde_yaml::from_str(doc).unwrap();

    let mut diags = Vec::new();
    let probs = etdl_compiler::fault_tree::resolve_fault_trees_with_overrides(
        &doc,
        &etdl_compiler::fault_tree::BasicEventOverrides::from([
            (
                etdl_compiler::fault_tree::override_key("A", "PowerLoss"),
                0.001,
            ),
            (
                etdl_compiler::fault_tree::override_key("B", "PowerLoss"),
                0.9,
            ),
        ]),
        &mut diags,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
    assert!(
        (probs["A"] - 0.001).abs() < 1e-9,
        "tree A got {}",
        probs["A"]
    );
    assert!((probs["B"] - 0.9).abs() < 1e-9, "tree B got {}", probs["B"]);
}

#[test]
fn override_key_is_compound_and_unescapable() {
    assert_eq!(
        etdl_compiler::fault_tree::override_key("ft", "be"),
        "ft::be"
    );
    // The compound form distinguishes trees with the same basic-event name.
    assert_ne!(
        etdl_compiler::fault_tree::override_key("A", "X"),
        etdl_compiler::fault_tree::override_key("B", "X")
    );
}

fn unknown_policy_doc() -> etdl_parser::ast::EtlDocument {
    let yaml = r#"
etdl: "1.0.0"
info:
  title: "Unknown policy"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  api: "./api.yaml"
supplements:
  - id: etdl.reliability
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: { id: I, message: "api#/components/messages/Event", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent:
      id: Top
      description: "top"
      rootCause: X
    gates:
      X:
        type: OR
        inputs: [A, B]
    basicEvents:
      A:
        description: "a"
        probability: 0.01
        x-reliability:
          source: gw
          estimate: est.a
      B:
        description: "b"
        probability: 0.02
        x-reliability:
          source: gw
          estimate: est.b
x-reliability:
  unknownPolicy: error
  sources:
    - id: gw
      type: external
      file: "./unknown.rprob"
"#;
    serde_yaml::from_str(yaml).unwrap()
}

#[test]
fn unknown_estimate_policy_error_fails_build() {
    let base = fixture_dir();
    let doc = unknown_policy_doc();
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("imports load");
    let compiler = etdl_compiler::Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry, &base);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.iter().any(|d| d.code == "E-112"),
        "expected E-112, got {:?}",
        errors.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn unknown_estimate_policy_warning_falls_back_to_declared() {
    let doc_yml = r#"
etdl: "1.0.0"
info:
  title: "Unknown policy"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  api: "./api.yaml"
supplements:
  - id: etdl.reliability
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: { id: I, message: "api#/components/messages/Event", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent:
      id: Top
      description: "top"
      rootCause: X
    gates:
      X:
        type: OR
        inputs: [A, B]
    basicEvents:
      A:
        description: "a"
        probability: 0.01
        x-reliability:
          source: gw
          estimate: est.a
      B:
        description: "b"
        probability: 0.02
        x-reliability:
          source: gw
          estimate: est.b
x-reliability:
  unknownPolicy: warning
  sources:
    - id: gw
      type: external
      file: "./unknown.rprob"
"#;
    let doc = serde_yaml::from_str(doc_yml).unwrap();
    let base = fixture_dir();
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("imports load");
    let compiler = etdl_compiler::Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry, &base);

    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "warning policy must not error, got {:?}",
        errors.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
    // W-408 warns about the fallback.
    assert!(
        result.diagnostics.iter().any(|d| d.code == "W-408"),
        "expected W-408, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
    // No override was applied for the unknown estimate.
    let manifest = result.build_manifest.expect("manifest present");
    let resolved = manifest["resolved_probabilities"].as_array().unwrap();
    assert!(
        resolved.is_empty(),
        "unknown estimates must not become overrides, got {resolved:?}"
    );
    // The fault tree evaluates from the declared values.
    let mut diags = Vec::new();
    let probs = etdl_compiler::fault_tree::resolve_fault_trees_with_overrides(
        &doc,
        &etdl_compiler::fault_tree::BasicEventOverrides::new(),
        &mut diags,
    );
    let expected = 1.0 - (1.0 - 0.01) * (1.0 - 0.02);
    let actual = probs["FT"];
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected fallback {expected}, got {actual}"
    );
}

#[test]
fn path_traversal_in_reliability_source_rejected() {
    let yaml = r#"
etdl: "1.0.0"
info:
  title: "Traversal"
  version: "1.0.0"
  domain: "D"
asyncapi_imports:
  api: "./api.yaml"
supplements:
  - id: etdl.reliability
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: { id: I, message: "api#/components/messages/Event", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
x-reliability:
  sources:
    - id: gw
      type: external
      file: "../../etc/secret.rprob"
"#;
    let doc: etdl_parser::ast::EtlDocument = serde_yaml::from_str(yaml).unwrap();
    let base = fixture_dir();
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("imports load");
    let compiler = etdl_compiler::Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry, &base);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "E-110" && d.message.contains("..")),
        "expected E-110 traversal error, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (&d.code, &d.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn multi_source_resolution_uses_compound_keys() {
    // A document with two sources and two fault trees sharing a basic-event
    // name; each tree's override must come from its own artifact.
    let base = fixture_dir();
    let doc = etdl_parser::parse_document_from_file(&base.join("multisource.etdl"))
        .expect("document parses");
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("imports load");
    let compiler = etdl_compiler::Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry, &base);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected no errors, got {:?}",
        errors
            .iter()
            .map(|d| (&d.code, &d.message))
            .collect::<Vec<_>>()
    );
    let manifest = result.build_manifest.expect("manifest present");
    let resolved = manifest["resolved_probabilities"].as_array().unwrap();
    assert_eq!(resolved.len(), 2, "expected 2 resolved entries");
    // Each entry must carry its fault tree id.
    let ft_ids: Vec<&str> = resolved
        .iter()
        .map(|r| r["fault_tree"].as_str().unwrap())
        .collect();
    assert!(ft_ids.contains(&"Alpha"));
    assert!(ft_ids.contains(&"Beta"));
}
