//! Reliability traceability: link every estimate back to its origin.
//!
//! The goal of the Reliability Supplement is that an engineer can trace a
//! number backwards:
//!
//! ```text
//! TOP EVENT PROBABILITY
//!   -> fault tree
//!     -> basic event probability
//!       -> reliability estimate
//!         -> estimation method
//!           -> observations
//!             -> evidence
//!               -> failure mode
//!                 -> ontology
//!                   -> discovery candidate
//!                     -> source location
//! ```
//!
//! This module provides a serializable [`EstimateTrace`] that records every
//! link that is available (never discarding information that exists).

use serde::{Deserialize, Serialize};

use etdl_reliability_core::estimate::ProbabilityEstimate;
use etdl_reliability_core::provenance::Provenance;

/// The trace for one failure mode's estimate. Every field is optional; links
/// are recorded only when present.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EstimateTrace {
    /// Stable failure-mode identity.
    pub failure_mode: Option<String>,
    /// Ontology concept id and the ontology version that defined it.
    pub ontology_id: Option<String>,
    pub ontology_version: Option<String>,
    /// Source location evidence (human-readable).
    pub source_location: Option<String>,
    /// Discovery evidence (what static analysis found).
    #[serde(default)]
    pub discovery_evidence: Vec<String>,
    /// Discovery report id.
    pub discovery_report: Option<String>,
    /// Observation source / dataset.
    pub observation_source: Option<String>,
    /// (exposure, failures) when known.
    pub exposure: Option<u64>,
    pub failures: Option<u64>,
    /// Estimation method.
    pub method: Option<String>,
    /// Provenance recorded on the estimate.
    pub provenance: Option<Provenance>,
    /// Artifact id and version that carried this estimate.
    pub artifact_id: Option<String>,
    pub artifact_version: Option<String>,
    /// Estimate version.
    pub estimate_version: Option<String>,
}

impl EstimateTrace {
    pub fn new() -> Self {
        EstimateTrace::default()
    }

    /// The full backward trace, one line per step, for human display.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let push = |out: &mut String, label: &str, value: &str| {
            out.push_str(&format!("  {label}: {value}\n"));
        };
        if let Some(fm) = &self.failure_mode {
            push(&mut out, "failure", fm);
        }
        if let Some(o) = &self.ontology_id {
            let v = self
                .ontology_version
                .as_deref()
                .unwrap_or("(unknown version)");
            push(&mut out, "ontology", &format!("{o} @ {v}"));
        }
        if let Some(loc) = &self.source_location {
            push(&mut out, "discovered at", loc);
        }
        for e in &self.discovery_evidence {
            push(&mut out, "discovery evidence", e);
        }
        if let Some(r) = &self.discovery_report {
            push(&mut out, "discovery report", r);
        }
        if let (Some(e), Some(f)) = (self.exposure, self.failures) {
            push(&mut out, "observations", &format!("{f} / {e} failures"));
        }
        if let Some(m) = &self.method {
            push(&mut out, "estimation", m);
        }
        if let Some(p) = &self.provenance {
            push(&mut out, "provenance", &format!("{:?}", p));
        }
        if let Some(a) = &self.artifact_id {
            let v = self.artifact_version.as_deref().unwrap_or("?");
            push(&mut out, "artifact", &format!("{a} v{v}"));
        }
        if let Some(v) = &self.estimate_version {
            push(&mut out, "estimate version", v);
        }
        out
    }
}

/// Build a trace from an estimate and whatever link metadata is available.
pub fn trace_from_estimate(
    estimate: &ProbabilityEstimate,
    artifact_id: Option<&str>,
    artifact_version: Option<&str>,
) -> EstimateTrace {
    EstimateTrace {
        failure_mode: Some(estimate.event.clone()),
        ontology_id: None,
        ontology_version: None,
        source_location: None,
        discovery_evidence: Vec::new(),
        discovery_report: None,
        observation_source: estimate.provenance.as_ref().and_then(|p| p.dataset.clone()),
        exposure: None,
        failures: None,
        method: estimate.method.clone(),
        provenance: estimate.provenance.clone(),
        artifact_id: artifact_id.map(|s| s.to_string()),
        artifact_version: artifact_version.map(|s| s.to_string()),
        estimate_version: estimate.version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_backward_trace() {
        let mut t = EstimateTrace::new();
        t.failure_mode = Some("failure.gateway.timeout".into());
        t.ontology_id = Some("failure.dependency.timeout".into());
        t.ontology_version = Some("1.0".into());
        t.source_location = Some("src/payment.rs:143".into());
        t.discovery_evidence = vec!["timeout API".into()];
        t.observation_source = Some("prod-obs".into());
        t.exposure = Some(100_000);
        t.failures = Some(37);
        t.method = Some("binomial/empirical".into());
        t.artifact_id = Some("payment-reliability".into());
        t.artifact_version = Some("1.2".into());
        let rendered = t.render();
        assert!(rendered.contains("failure: failure.gateway.timeout"));
        assert!(rendered.contains("discovered at: src/payment.rs:143"));
        assert!(rendered.contains("observations: 37 / 100000 failures"));
        assert!(rendered.contains("artifact: payment-reliability v1.2"));
    }
}
