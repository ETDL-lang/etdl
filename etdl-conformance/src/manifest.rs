//! The machine-readable conformance manifest (task §47): what this
//! implementation build claims to support, and against which versions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConformanceManifest {
    pub etdl_language_version: String,
    pub implementation_version: String,
    pub conformance_suite_version: String,
    pub supported_supplements: Vec<SupplementInfo>,
    pub supported_libraries: Vec<String>,
    pub supported_targets: Vec<String>,
    pub supported_artifact_schemas: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupplementInfo {
    pub id: String,
    pub version: String,
    /// Whether this build was compiled with the capability present at all
    /// (distinct from whether every conformance vector for it passes —
    /// see [`crate::report`]).
    pub available: bool,
}

impl ConformanceManifest {
    /// Builds the manifest for *this* compiled binary — reads compile-time
    /// feature flags only, never probes the filesystem or network. `caller`
    /// supplies the pieces that vary by feature flag (schema constants
    /// behind optional crates) so this function has no direct dependency
    /// on any optional crate, matching the same "lean build must still
    /// compile" discipline `etdl-cli`'s `capabilities` command already
    /// follows.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        implementation_version: &str,
        reliability_available: bool,
        tree_event_schema: &str,
        performance_schema: &str,
        safety_schema: &str,
        diagnostics_schema: &str,
        security_schema: &str,
        std_probability_schema: &str,
        std_library_schema: &str,
        predictive_reliability_schema: &str,
        reliability_artifact_schema: Option<&str>,
    ) -> Self {
        let mut supplements = vec![
            SupplementInfo {
                id: "etdl.tree-event".to_string(),
                version: tree_event_schema.to_string(),
                available: true,
            },
            SupplementInfo {
                id: "etdl.performance".to_string(),
                version: performance_schema.to_string(),
                available: true,
            },
            SupplementInfo {
                id: "etdl.safety".to_string(),
                version: safety_schema.to_string(),
                available: true,
            },
            SupplementInfo {
                id: "etdl.diagnostics".to_string(),
                version: diagnostics_schema.to_string(),
                available: true,
            },
            SupplementInfo {
                id: "etdl.security".to_string(),
                version: security_schema.to_string(),
                available: true,
            },
        ];
        supplements.push(SupplementInfo {
            id: "etdl.reliability".to_string(),
            version: "1.0".to_string(),
            available: reliability_available,
        });
        supplements.push(SupplementInfo {
            id: "etdl.predictive-reliability".to_string(),
            version: predictive_reliability_schema.to_string(),
            available: reliability_available,
        });
        supplements.push(SupplementInfo {
            id: "etdl.runtime-feedback-calibration".to_string(),
            version: "1.0".to_string(),
            available: reliability_available,
        });

        let mut artifact_schemas = vec![];
        if let Some(schema) = reliability_artifact_schema {
            artifact_schemas.push(schema.to_string());
        }

        ConformanceManifest {
            etdl_language_version: crate::ETDL_LANGUAGE_VERSION.to_string(),
            implementation_version: implementation_version.to_string(),
            conformance_suite_version: crate::CONFORMANCE_SUITE_VERSION.to_string(),
            supported_supplements: supplements,
            supported_libraries: vec![
                std_library_schema.to_string(),
                std_probability_schema.to_string(),
            ],
            supported_targets: vec!["native".to_string(), "wasm32-unknown-unknown".to_string()],
            supported_artifact_schemas: artifact_schemas,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_serializes_to_json() {
        let manifest = ConformanceManifest::build(
            "0.2.2",
            true,
            "etdl.tree-event/1.0",
            "etdl.performance/1.0",
            "etdl.safety/1.0",
            "etdl.diagnostics/1.0",
            "etdl.security/1.0",
            "std.probability/1.0",
            "std.library/1.0",
            "etdl.predictive-reliability/1.0",
            Some("etdl.reliability-artifact/1.0"),
        );
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("etdl.predictive-reliability"));
    }
}
