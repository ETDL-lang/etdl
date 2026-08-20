//! The discovery report: a stable, versioned, machine-readable result.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::candidate::DiscoveryCandidate;
use crate::config::DiscoveryConfig;

/// The discovery report schema version. Versioned independently of the tool.
pub const REPORT_SCHEMA: &str = "etdl.failure-discovery.report/1.0";

/// Identifies the analyzer that produced a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerMetadata {
    pub name: String,
    pub version: String,
    pub language: String,
}

/// Identifies the analyzed source for provenance and reproducibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub path: PathBuf,
    /// Deterministic content hash over all analyzed source.
    pub content_hash: String,
    /// Number of source files analyzed.
    pub file_count: usize,
    /// Crate/package name, when known.
    pub package_name: Option<String>,
}

/// Per-candidate statistics. Counts are over candidates that survived the
/// configured minimum confidence.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReportStatistics {
    pub total_candidates: usize,
    pub by_classification: BTreeMap<String, usize>,
    pub by_severity: BTreeMap<String, usize>,
    /// Candidates with confidence >= 0.8.
    pub high_confidence: usize,
    /// Candidates mapped to an ontology concept (Exact/Probable/Deprecated).
    pub mapped: usize,
    /// Candidates that propose a new ontology concept.
    pub unmapped: usize,
    /// Potential-panic candidates (unwrap/expect/panic/index/assert/div).
    pub potential_panic: usize,
}

impl ReportStatistics {
    pub fn compute(candidates: &[DiscoveryCandidate]) -> Self {
        let mut by_classification = BTreeMap::new();
        let mut by_severity = BTreeMap::new();
        let mut high = 0;
        let mut mapped = 0;
        let mut unmapped = 0;
        let mut panic = 0;
        for c in candidates {
            *by_classification
                .entry(c.classification.label().to_string())
                .or_insert(0) += 1;
            *by_severity
                .entry(format!("{:?}", c.severity).to_lowercase())
                .or_insert(0) += 1;
            if c.confidence >= 0.8 {
                high += 1;
            }
            match c.ontology.quality {
                crate::mapping::MappingQuality::Exact
                | crate::mapping::MappingQuality::Probable
                | crate::mapping::MappingQuality::Deprecated => mapped += 1,
                _ => unmapped += 1,
            }
            if c.id.contains("panic")
                || c.id.contains("unwrap")
                || c.id.contains("expect")
                || c.id.contains("index")
                || c.id.contains("assert")
                || c.id.contains("division")
            {
                panic += 1;
            }
        }
        ReportStatistics {
            total_candidates: candidates.len(),
            by_classification,
            by_severity,
            high_confidence: high,
            mapped,
            unmapped,
            potential_panic: panic,
        }
    }
}

/// The machine-readable discovery report.
///
/// The report is **discovery output** — candidates with evidence, confidence,
/// and ontology mapping. It is NOT a reliability probability artifact and must
/// never be consumed as if its `confidence` values were probabilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryReport {
    pub schema: String,
    pub analyzer: AnalyzerMetadata,
    pub source: SourceIdentity,
    /// The configuration snapshot that produced this report (for
    /// reproducibility). Absolute machine-specific paths are excluded.
    pub config: ReportConfig,
    pub candidates: Vec<DiscoveryCandidate>,
    /// Non-fatal issues encountered during analysis.
    pub diagnostics: Vec<String>,
    pub statistics: ReportStatistics,
}

/// Configuration snapshot embedded in a report (paths relative to the root).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportConfig {
    pub language: Option<String>,
    pub min_confidence: f64,
    pub ontology_policy: String,
    pub ignore_dirs: Vec<String>,
}

impl DiscoveryReport {
    pub fn new(
        analyzer: AnalyzerMetadata,
        source: SourceIdentity,
        config: &DiscoveryConfig,
    ) -> Self {
        DiscoveryReport {
            schema: REPORT_SCHEMA.to_string(),
            analyzer,
            source,
            config: ReportConfig {
                language: config.language.clone(),
                min_confidence: config.min_confidence,
                ontology_policy: format!("{:?}", config.ontology_policy).to_lowercase(),
                ignore_dirs: config.ignore_dirs.clone(),
            },
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            statistics: ReportStatistics::default(),
        }
    }

    /// Sort candidates deterministically by (file, line, column, id).
    pub fn sort(&mut self) {
        self.candidates.sort_by(|a, b| {
            (&a.location.file, a.location.line, a.location.column, &a.id).cmp(&(
                &b.location.file,
                b.location.line,
                b.location.column,
                &b.id,
            ))
        });
        self.statistics = ReportStatistics::compute(&self.candidates);
    }
}
