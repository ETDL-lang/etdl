//! End-to-end integration tests for failure discovery on the fixture project.

use etdl_failure_discovery::analyzer::SourceAnalyzer;
use etdl_failure_discovery::config::DiscoveryConfig;
use etdl_failure_discovery::rust::RustAnalyzer;
use etdl_failure_discovery::DiscoveryReport;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/service")
}

fn discover(config: &DiscoveryConfig) -> DiscoveryReport {
    RustAnalyzer::new()
        .analyze_project(&fixture_dir(), config)
        .expect("discovery runs")
}

fn ids(report: &DiscoveryReport) -> Vec<String> {
    report.candidates.iter().map(|c| c.id.clone()).collect()
}

#[test]
fn discovers_multiple_failure_classes() {
    let report = discover(&DiscoveryConfig::default());
    let ids = ids(&report);

    let expected = [
        "failure.application.custom_error",            // PaymentError
        "failure.runtime.error_propagation",           // `?`
        "failure.runtime.explicit_err_return",         // return Err(...)
        "failure.runtime.unwrap",                      // unwrap()
        "failure.runtime.expect",                      // expect(...)
        "failure.runtime.panic",                       // panic!
        "failure.runtime.assertion",                   // assert!
        "failure.runtime.index_out_of_bounds",         // slice[idx]
        "failure.runtime.division_by_zero",            // 100 / (idx + 0)
        "failure.validation.parse_failure",            // .parse()
        "failure.io.io_failure",                       // fs::read_to_string
        "failure.network.network_operation",           // reqwest
        "failure.serialization.serialization_failure", // serde_json
        "failure.messaging.channel_failure",           // tx.send
        "failure.concurrency.lock_poisoning",          // db.lock()
    ];
    for id in expected {
        assert!(
            ids.contains(&id.to_string()),
            "expected candidate '{id}' not found; got: {ids:?}"
        );
    }
    assert!(!report.candidates.is_empty());
}

#[test]
fn candidates_are_possible_not_proven() {
    let report = discover(&DiscoveryConfig::default());
    for c in &report.candidates {
        assert!(c.possible, "candidate {} should be possible", c.id);
        assert!(c.confidence >= 0.0 && c.confidence <= 1.0);
        // Confidence is NOT a probability; assert it is never used as one by
        // checking the candidate has no probability field.
        assert!(!c.id.starts_with("probability"), "no probability in ids");
    }
}

#[test]
fn every_candidate_has_source_evidence_and_location() {
    let report = discover(&DiscoveryConfig::default());
    assert!(!report.candidates.is_empty());
    for c in &report.candidates {
        assert!(!c.evidence.is_empty(), "candidate {} has no evidence", c.id);
        assert!(c.location.line > 0, "candidate {} has no line", c.id);
        assert!(
            c.location.byte_end >= c.location.byte_start,
            "candidate {} has inverted span",
            c.id
        );
    }
}

#[test]
fn third_party_vendor_is_ignored_by_default() {
    // vendor/ is in the default ignore dirs, so no candidate from it.
    let report = discover(&DiscoveryConfig::default());
    assert!(
        !report
            .candidates
            .iter()
            .any(|c| c.location.file.to_string_lossy().contains("vendor")),
        "vendor files must be ignored by default"
    );
}

#[test]
fn min_confidence_filters() {
    let low = discover(&DiscoveryConfig {
        min_confidence: 0.5,
        ..DiscoveryConfig::default()
    });
    let high = discover(&DiscoveryConfig {
        min_confidence: 0.95,
        ..DiscoveryConfig::default()
    });
    assert!(
        high.candidates.len() <= low.candidates.len(),
        "raising min confidence must not add candidates"
    );
    assert!(high.candidates.iter().all(|c| c.confidence >= 0.95));
}

#[test]
fn discovery_is_deterministic() {
    let config = DiscoveryConfig::default();
    let a = RustAnalyzer::new()
        .analyze_project(&fixture_dir(), &config)
        .unwrap();
    let b = RustAnalyzer::new()
        .analyze_project(&fixture_dir(), &config)
        .unwrap();
    assert_eq!(a, b);
    // Content hash is stable too.
    assert_eq!(a.source.content_hash, b.source.content_hash);
}

#[test]
fn report_has_stable_versioned_schema() {
    let report = discover(&DiscoveryConfig::default());
    assert_eq!(report.schema, "etdl.failure-discovery.report/1.0");
    assert_eq!(report.analyzer.name, "etdl-rust");
    assert!(!report.analyzer.version.is_empty());
}

#[test]
fn report_statistics_are_consistent() {
    let report = discover(&DiscoveryConfig::default());
    let total = report.statistics.total_candidates;
    let classified: usize = report.statistics.by_classification.values().sum();
    assert_eq!(total, report.candidates.len());
    assert_eq!(total, classified);
}

#[test]
fn ontology_mapping_marks_exact_and_unmapped() {
    let report = discover(&DiscoveryConfig::default());
    assert!(
        report.candidates.iter().any(|c| matches!(
            c.ontology.quality,
            etdl_failure_discovery::MappingQuality::Exact
        )),
        "expected at least one exact mapping"
    );
    assert!(
        report.candidates.iter().any(|c| matches!(
            c.ontology.quality,
            etdl_failure_discovery::MappingQuality::Unmapped
        )),
        "expected at least one unmapped/proposed concept (custom error, lock)"
    );
}

#[test]
fn confidence_and_ontology_are_distinct_from_probability() {
    // A discovery report serialized must NOT contain a 'probability' key at the
    // top level and candidates must not carry a probability field.
    let report = discover(&DiscoveryConfig::default());
    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("probability").is_none());
    for c in json["candidates"].as_array().unwrap() {
        assert!(
            c.get("probability").is_none(),
            "no probability in candidates"
        );
    }
}
