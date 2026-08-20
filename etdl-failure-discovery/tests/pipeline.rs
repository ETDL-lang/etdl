//! Full-pipeline integration test: discovery -> human review -> externally
//! supplied reliability estimate -> .rprob -> ETDL -> fault tree.
//!
//! This proves the systems are connected WITHOUT conflating discovery with
//! estimation: the deterministic probability used by the fault tree is the
//! externally supplied value, never the discovery confidence.

use etdl_failure_discovery::analyzer::SourceAnalyzer;
use etdl_failure_discovery::bridge::{accepted_candidates_to_artifact, SuppliedEstimate};
use etdl_failure_discovery::candidate::CandidateStatus;
use etdl_failure_discovery::config::DiscoveryConfig;
use etdl_failure_discovery::rust::RustAnalyzer;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/service")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("etdl_disc_e2e_{}_{}", std::process::id(), tag));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn discovery_to_etdl_fault_tree_uses_external_value_not_confidence() {
    // 1. DISCOVER: analyze the fixture service.
    let report = RustAnalyzer::new()
        .analyze_project(&fixture_dir(), &DiscoveryConfig::default())
        .expect("discovery runs");
    assert!(!report.candidates.is_empty());

    // 2. HUMAN REVIEW: an engineer accepts one candidate (e.g. the network
    //    timeout candidate) and maps it to the ontology. Discovery itself never
    //    auto-accepts.
    let timeout_candidate = report
        .candidates
        .iter()
        .find(|c| c.id == "failure.network.network_operation")
        .expect("expected a network candidate");
    assert_eq!(timeout_candidate.status, CandidateStatus::Candidate);

    // 3. ESTIMATE: a separate reliability engineering process supplies an
    //    explicit probability based on evidence/model — NOT the discovery
    //    confidence (0.8) and NOT invented.
    let external_value = 0.0015; // e.g. observed 1 timeout in ~667 requests
    assert!(
        (external_value - timeout_candidate.confidence).abs() > 0.1,
        "the estimate must differ from the discovery confidence to prove they are distinct"
    );

    let mut accepted = timeout_candidate.clone();
    accepted.status = CandidateStatus::Accepted;

    let estimate = SuppliedEstimate {
        candidate_id: accepted.id.clone(),
        value: external_value,
        basis: "observed 1 timeout in 667 requests".to_string(),
        metric: etdl_reliability_core::probability::ProbabilityMetric::Probability,
        time_basis: Some(etdl_reliability_core::probability::TimeBasis::PerRequest),
        source: etdl_reliability_core::probability::ProbabilitySource::Measurement,
    };

    // 4. ARTIFACT: build a .rprob from the accepted candidate + supplied value.
    let artifact = accepted_candidates_to_artifact(&[accepted], &[estimate], "payment-gateway")
        .expect("artifact built");
    let artifact_path = tmp_dir("a").join("gateway.rprob");
    let artifact_yaml = serde_yaml::to_string(&artifact).unwrap();
    std::fs::write(&artifact_path, artifact_yaml).expect("write artifact");

    // 5. ETDL: compile a document that resolves this artifact into a fault tree.
    let api_path = tmp_dir("a").join("api.yaml");
    std::fs::write(
        &api_path,
        r#"asyncapi: "3.0.0"
info:
  title: api
  version: "1.0.0"
channels: {}
components:
  messages:
    Event:
      name: Event
      payload:
        type: object
        properties:
          ok: { type: boolean }
"#,
    )
    .expect("write api.yaml");

    let etdl_path = tmp_dir("a").join("service.etdl");
    let etdl = format!(
        r##"
etdl: "1.0.0"
info:
  title: "Discovery pipeline"
  version: "1.0.0"
  domain: "PaymentsContext"
asyncapi_imports:
  api: "./api.yaml"
supplements:
  - id: etdl.reliability
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: {{ id: I, message: "api#/components/messages/Event", next: Charge }}
    nodes:
      Charge:
        type: operation
        action: execute
        handler: "charge_handler"
        next: Done
        onFailure: Fail
        onFailureProbabilitySource: "#/faultTrees/PaymentGatewayFailure/topEvent"
      Done:
        type: consequence
        operation: terminate
      Fail:
        type: consequence
        operation: terminate
faultTrees:
  PaymentGatewayFailure:
    topEvent:
      id: Top
      description: "charge capture fails"
      rootCause: GatewayFailure
    gates:
      GatewayFailure:
        type: OR
        inputs: [GatewayTimeout, GatewayUnreachable]
    basicEvents:
      GatewayTimeout:
        description: "gateway did not respond"
        probability: 0.01
        x-reliability:
          source: gw
          estimate: {estimate_key}
      GatewayUnreachable:
        description: "gateway unreachable"
        probability: 0.02
x-reliability:
  sources:
    - id: gw
      type: external
      file: "{artifact_rel}"
"##,
        estimate_key = "failure.network.network_operation",
        artifact_rel = artifact_path.display(),
    );
    std::fs::write(&etdl_path, etdl).expect("write etdl");

    // 6. COMPILE: the fault tree must use the externally supplied value.
    let base = tmp_dir("a");
    let content = std::fs::read_to_string(&etdl_path).unwrap();
    let doc = etdl_parser::parse_document(&content).expect("document parses");
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

    // The generated code embeds the fault-tree top-event probability, computed
    // from the externally supplied value, not the discovery confidence.
    let rust = result.rust_output.expect("generated code");
    let top = 1.0 - (1.0 - external_value) * (1.0 - 0.02);
    let top_str = format!("{top:.6}");
    assert!(
        rust.contains(&top_str),
        "generated code must embed the top-event probability {top_str}"
    );
    // The discovery confidence (0.8) must NOT appear as a probability constant.
    assert!(
        !rust.contains("0.800000"),
        "discovery confidence must not leak into generated code"
    );

    // Verify the resolved basic-event value directly via the public API.
    let (resolved, _) =
        etdl_compiler::reliability::resolve_reliability(&doc, &base, &mut Vec::new());
    assert_eq!(resolved.len(), 1);
    assert!((resolved[0].resolved.value - external_value).abs() < 1e-12);

    let _ = std::fs::remove_dir_all(tmp_dir("a"));
}

/// Full pipeline with the REAL estimator: discovery -> review -> observations
/// -> empirical estimation -> artifact -> ETDL -> fault tree.
#[test]
fn discovery_review_observation_estimation_artifact_etdl_fault_tree() {
    // 1. DISCOVER.
    let report = RustAnalyzer::new()
        .analyze_project(&fixture_dir(), &DiscoveryConfig::default())
        .expect("discovery runs");
    let candidate = report
        .candidates
        .iter()
        .find(|c| c.id == "failure.network.network_operation")
        .expect("network candidate");

    // 2. REVIEW (immutable record, advisory boundary).
    let mut review = etdl_reliability::ReviewRecord::new(
        &candidate.id,
        etdl_reliability::ReviewStatus::Accepted,
    );
    review.rationale = Some("matches production HTTP timeout path".into());
    review.selected_ontology_id = Some("failure.dependency.timeout".into());
    let accepted = etdl_reliability::ReviewedFailureMode::from_review(review);
    assert_eq!(
        accepted.status,
        etdl_reliability::failure::FailureStatus::Accepted
    );
    assert_eq!(accepted.failure_mode_id, "failure.dependency.timeout");

    // 3. OBSERVATIONS (explicit exposure).
    let observation = etdl_reliability::observations::AggregateObservation {
        id: None,
        failure_mode: accepted.failure_mode_id.clone(),
        exposure: 1_000_000,
        failures: 2_400,
        exposure_unit: etdl_reliability_core::probability::TimeBasis::PerRequest,
        conditions: vec!["production".into()],
        interval: None,
        source: Some("prod-2026-08".into()),
        version: Some("1".into()),
    };
    observation.validate().expect("observation valid");

    // 4. ESTIMATE with the empirical binomial estimator.
    use etdl_reliability::analysis::ReliabilityEstimator;
    let estimator = etdl_reliability::analysis::EmpiricalBinomialEstimator::new();
    let config = etdl_reliability::analysis::EstimationConfig::default();
    let estimate = estimator.estimate(&observation, &config).expect("estimate");
    let p = estimate.value.expect("estimated value");
    assert!((p - 0.0024).abs() < 1e-12, "expected 0.0024, got {p}");
    assert!(
        (p - candidate.confidence).abs() > 0.5,
        "estimate must differ from discovery confidence"
    );

    // 5. ARTIFACT.
    let mut artifact = etdl_reliability_core::artifact::ReliabilityArtifact::new("payment-gateway");
    artifact.version = Some("1.0.0".into());
    artifact.add(estimate.clone()).unwrap();
    let artifact_path = tmp_dir("b").join("estimated.rprob");
    std::fs::write(&artifact_path, serde_yaml::to_string(&artifact).unwrap())
        .expect("write artifact");

    // 6. ETDL + fault tree (reuse the same doc shape as the first test, but the
    //    estimate id is the failure-mode id).
    let api_path = tmp_dir("b").join("api.yaml");
    std::fs::write(
        &api_path,
        r#"asyncapi: "3.0.0"
info:
  title: api
  version: "1.0.0"
channels: {}
components:
  messages:
    Event:
      name: Event
      payload:
        type: object
        properties:
          ok: { type: boolean }
"#,
    )
    .expect("write api.yaml");

    let etdl_path = tmp_dir("b").join("service2.etdl");
    let etdl = format!(
        r##"
etdl: "1.0.0"
info:
  title: "Discovery pipeline 2"
  version: "1.0.0"
  domain: "PaymentsContext"
asyncapi_imports:
  api: "./api.yaml"
supplements:
  - id: etdl.reliability
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: {{ id: I, message: "api#/components/messages/Event", next: Charge }}
    nodes:
      Charge:
        type: operation
        action: execute
        handler: "charge_handler"
        next: Done
        onFailure: Fail
        onFailureProbabilitySource: "#/faultTrees/PaymentGatewayFailure/topEvent"
      Done:
        type: consequence
        operation: terminate
      Fail:
        type: consequence
        operation: terminate
faultTrees:
  PaymentGatewayFailure:
    topEvent:
      id: Top
      description: "charge capture fails"
      rootCause: GatewayFailure
    gates:
      GatewayFailure:
        type: OR
        inputs: [GatewayTimeout, GatewayUnreachable]
    basicEvents:
      GatewayTimeout:
        description: "gateway did not respond"
        probability: 0.01
        x-reliability:
          source: gw
          estimate: {estimate_key}
      GatewayUnreachable:
        description: "gateway unreachable"
        probability: 0.02
x-reliability:
  sources:
    - id: gw
      type: external
      file: "{artifact_rel}"
"##,
        estimate_key = "failure.dependency.timeout",
        artifact_rel = artifact_path.display(),
    );
    std::fs::write(&etdl_path, etdl).expect("write etdl");

    let base = tmp_dir("b");
    let content = std::fs::read_to_string(&etdl_path).unwrap();
    let doc = etdl_parser::parse_document(&content).expect("document parses");
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

    // The fault tree uses the ESTIMATED value (0.0024), not discovery confidence.
    let rust = result.rust_output.expect("generated code");
    let top = 1.0 - (1.0 - 0.0024) * (1.0 - 0.02);
    let top_str = format!("{top:.6}");
    assert!(
        rust.contains(&top_str),
        "generated code must embed the top-event probability {top_str} from the estimate"
    );

    // The build manifest records the estimation provenance.
    let manifest = result.build_manifest.expect("manifest");
    let entry = manifest["resolved_probabilities"][0].clone();
    assert_eq!(entry["value"].as_f64().unwrap(), 0.0024);
    assert_eq!(
        entry["method"].as_str().unwrap(),
        "binomial/empirical/binomial"
    );
    assert_eq!(entry["state"].as_str().unwrap(), "Estimated");
    assert_eq!(
        entry["provenance"]["dataset"].as_str().unwrap(),
        "prod-2026-08"
    );

    let _ = std::fs::remove_dir_all(tmp_dir("b"));
}
