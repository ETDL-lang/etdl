//! Compiler integration for the ETDL Reliability Supplement (`etdl.reliability`).
//!
//! This module wires the `etdl-reliability` domain crate into the compiler:
//! it reads the document's `x-reliability` extension, loads reliability
//! artifacts, and resolves external probability sources to deterministic
//! scalars **before** fault-tree evaluation. Nothing is resolved at runtime.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use etdl_parser::ast::EtlDocument;
use etdl_reliability_core::artifact::{
    ArtifactResolver, ReliabilityArtifact, ResolveOutcome, UnknownProbabilityPolicy,
};
use etdl_reliability_core::ResolvedProbability;

use crate::validate::Diagnostic;

/// A resolved probability for a basic event, plus provenance.
#[derive(Debug, Clone)]
pub struct ResolvedBasicEvent {
    pub fault_tree: String,
    pub basic_event: String,
    pub resolved: ResolvedProbability,
}

impl ResolvedBasicEvent {
    /// The compound override key, unambiguous across fault trees.
    pub fn override_key(&self) -> String {
        crate::fault_tree::override_key(&self.fault_tree, &self.basic_event)
    }
}

/// The build manifest: reproducible provenance for a reliability-aware build.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BuildManifest {
    pub etdl_version: String,
    pub compiler_version: String,
    /// Compiler features enabled in this build (e.g. `["reliability"]`).
    pub enabled_features: Vec<String>,
    /// Reliability implementation versions that produced this build.
    pub implementations: Vec<ImplementationVersion>,
    /// Artifact schema version accepted by this build.
    pub artifact_schema_version: String,
    pub supplements: Vec<SupplementUsed>,
    pub reliability_artifacts: Vec<String>,
    pub resolved_probabilities: Vec<ResolvedEntry>,
}

/// The version of the built-in reliability implementation crate.
fn etdl_reliability_core_crate_version() -> String {
    etdl_reliability_core::VERSION.to_string()
}

/// A compiled-in implementation and its version (reproducibility: which
/// reliability implementation produced this binary).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImplementationVersion {
    pub name: String,
    pub version: String,
}

/// The enabled compiler features, for build-manifest reproducibility.
pub fn enabled_features() -> Vec<&'static str> {
    let mut features = Vec::new();
    if cfg!(feature = "reliability") {
        features.push("reliability");
    }
    features
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SupplementUsed {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedEntry {
    pub fault_tree: String,
    pub basic_event: String,
    pub value: f64,
    pub estimate_id: String,
    pub artifact_id: String,
    pub artifact_version: Option<String>,
    /// The estimation method recorded on the source estimate, if any.
    pub method: Option<String>,
    /// Conditions on the source estimate, if any.
    pub conditions: Vec<String>,
    /// The estimate's state.
    pub state: Option<String>,
    /// The estimate's version.
    pub estimate_version: Option<String>,
    /// Dataset/model provenance from the source estimate, if any.
    pub provenance: Option<serde_json::Value>,
}

const RELIABILITY_SUPPLEMENT: &str = "etdl.reliability";

/// Configuration read from the document's `x-reliability` extension.
#[derive(Debug, Default)]
struct ReliabilityConfig {
    sources: Vec<SourceConfig>,
    unknown_policy: UnknownProbabilityPolicy,
}

#[derive(Debug)]
struct SourceConfig {
    id: String,
    file: String,
}

/// Load and resolve all external probability sources declared in the document.
///
/// Returns the resolved basic-event probabilities and a build manifest.
/// If any required external source cannot be resolved, diagnostics are added
/// and an empty result is returned.
pub fn resolve_reliability(
    doc: &EtlDocument,
    base_dir: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<ResolvedBasicEvent>, Option<BuildManifest>) {
    if !crate::validate::declares_supplement(doc, RELIABILITY_SUPPLEMENT) {
        return (Vec::new(), None);
    }

    let config = parse_config(doc);

    // Load artifacts.
    let mut artifacts: BTreeMap<String, ReliabilityArtifact> = BTreeMap::new();
    let mut artifact_files: Vec<String> = Vec::new();
    for src in &config.sources {
        let path = match resolve_path(base_dir, &src.file) {
            Ok(p) => p,
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E-110",
                    format!("reliability source '{}': {}", src.file, e),
                ));
                continue;
            }
        };
        let artifact = match load_artifact(&path) {
            Ok(a) => a,
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E-110",
                    format!("reliability source '{}': {}", src.file, e),
                ));
                continue;
            }
        };
        artifact_files.push(src.file.clone());
        artifacts.insert(src.id.clone(), artifact);
    }

    let resolver = ArtifactResolver::new(config.unknown_policy);

    // For each basic event with an `x-reliability: { source, estimate }`
    // annotation, resolve the estimate from the named artifact.
    let mut resolved: Vec<ResolvedBasicEvent> = Vec::new();
    if let Some(fts) = &doc.fault_trees {
        for (ft_id, ft) in fts {
            for (be_id, be) in &ft.basic_events {
                if let Some((src_id, est_id)) = basic_event_reliability_source(be) {
                    let artifact = match artifacts.get(&src_id) {
                        Some(a) => a,
                        None => {
                            diagnostics.push(Diagnostic::error(
                                "E-111",
                                format!(
                                    "basic event '{}' (fault tree '{}') references reliability source '{}' which is not declared in x-reliability.sources",
                                    be_id, ft_id, src_id
                                ),
                            ));
                            continue;
                        }
                    };
                    match resolver.resolve(artifact, &est_id) {
                        Ok(ResolveOutcome::Resolved(r)) => {
                            resolved.push(ResolvedBasicEvent {
                                fault_tree: ft_id.clone(),
                                basic_event: be_id.clone(),
                                resolved: r,
                            });
                        }
                        Ok(ResolveOutcome::Unknown { .. }) => {
                            // Policy governs unknown-valued estimates only.
                            match config.unknown_policy {
                                UnknownProbabilityPolicy::Error => {
                                    diagnostics.push(Diagnostic::error(
                                        "E-112",
                                        format!(
                                            "basic event '{}' (fault tree '{}'): reliability estimate '{}' from source '{}' has no deterministic value (unknown)",
                                            be_id, ft_id, est_id, src_id
                                        ),
                                    ));
                                }
                                UnknownProbabilityPolicy::Warning => {
                                    diagnostics.push(Diagnostic::warning(
                                        "W-408",
                                        format!(
                                            "basic event '{}' (fault tree '{}'): reliability estimate '{}' from source '{}' is unknown; falling back to the declared probability",
                                            be_id, ft_id, est_id, src_id
                                        ),
                                    ));
                                }
                                UnknownProbabilityPolicy::Allow => {}
                            }
                        }
                        Ok(ResolveOutcome::Missing { .. }) => {
                            // A missing estimate id is always an error: the
                            // document references something the artifact lacks.
                            diagnostics.push(Diagnostic::error(
                                "E-112",
                                format!(
                                    "basic event '{}' (fault tree '{}'): reliability estimate '{}' from source '{}' does not exist in the artifact",
                                    be_id, ft_id, est_id, src_id
                                ),
                            ));
                        }
                        Err(e) => {
                            diagnostics.push(Diagnostic::error(
                                "E-112",
                                format!(
                                    "basic event '{}' (fault tree '{}'): cannot resolve reliability estimate '{}' from source '{}': {}",
                                    be_id, ft_id, est_id, src_id, e
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    let manifest = if resolved.is_empty() && artifacts.is_empty() {
        None
    } else {
        Some(BuildManifest {
            etdl_version: doc.etdl.clone(),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            enabled_features: enabled_features().into_iter().map(String::from).collect(),
            implementations: vec![ImplementationVersion {
                name: "etdl-reliability-core".to_string(),
                version: etdl_reliability_core_crate_version(),
            }],
            artifact_schema_version: etdl_reliability_core::artifact::ARTIFACT_SCHEMA.to_string(),
            supplements: doc
                .supplements
                .iter()
                .map(|s| SupplementUsed {
                    id: s.id.clone(),
                    version: s.version.clone(),
                })
                .collect(),
            reliability_artifacts: artifact_files,
            resolved_probabilities: resolved
                .iter()
                .map(|r| {
                    let estimate = artifacts
                        .values()
                        .find(|a| a.id == r.resolved.artifact_id)
                        .and_then(|a| a.get(&r.resolved.estimate_id))
                        .or_else(|| {
                            artifacts
                                .values()
                                .find_map(|a| a.get(&r.resolved.estimate_id))
                        });
                    ResolvedEntry {
                        fault_tree: r.fault_tree.clone(),
                        basic_event: r.basic_event.clone(),
                        value: r.resolved.value,
                        estimate_id: r.resolved.estimate_id.clone(),
                        artifact_id: r.resolved.artifact_id.clone(),
                        artifact_version: r.resolved.artifact_version.clone(),
                        method: estimate.and_then(|e| e.method.clone()),
                        conditions: estimate.map(|e| e.conditions.clone()).unwrap_or_default(),
                        state: estimate.map(|e| format!("{:?}", e.state)),
                        estimate_version: estimate.and_then(|e| e.version.clone()),
                        provenance: estimate
                            .and_then(|e| e.provenance.clone())
                            .and_then(|p| serde_json::to_value(&p).ok()),
                    }
                })
                .collect(),
        })
    };

    (resolved, manifest)
}

fn parse_config(doc: &EtlDocument) -> ReliabilityConfig {
    let mut config = ReliabilityConfig::default();
    let ext = doc.extensions.get("x-reliability");
    let Some(ext) = ext else {
        return config;
    };
    let obj = match ext.as_mapping() {
        Some(m) => m,
        None => return config,
    };

    if let Some(sources) = obj.get(serde_yaml::Value::String("sources".into())) {
        if let Some(arr) = sources.as_sequence() {
            for src in arr {
                if let Some(map) = src.as_mapping() {
                    let id = map
                        .get(serde_yaml::Value::String("id".into()))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let file = map
                        .get(serde_yaml::Value::String("file".into()))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    if !id.is_empty() && !file.is_empty() {
                        config.sources.push(SourceConfig { id, file });
                    }
                }
            }
        }
    }

    if let Some(policy) = obj.get(serde_yaml::Value::String("unknownPolicy".into())) {
        match policy.as_str() {
            Some("error") => config.unknown_policy = UnknownProbabilityPolicy::Error,
            Some("allow") => config.unknown_policy = UnknownProbabilityPolicy::Allow,
            _ => config.unknown_policy = UnknownProbabilityPolicy::Warning,
        }
    }

    config
}

/// Read `x-reliability: { source, estimate }` from a basic event.
fn basic_event_reliability_source(be: &etdl_parser::ast::BasicEvent) -> Option<(String, String)> {
    let ext = be.extensions.get("x-reliability")?;
    let obj = ext.as_mapping()?;
    let source = obj
        .get(serde_yaml::Value::String("source".into()))?
        .as_str()?;
    let estimate = obj
        .get(serde_yaml::Value::String("estimate".into()))?
        .as_str()?;
    Some((source.to_string(), estimate.to_string()))
}

fn resolve_path(base_dir: &Path, file: &str) -> Result<PathBuf, String> {
    // Path traversal guard (mirrors the AsyncAPI import guard): local sources
    // MUST NOT escape the project root (spec §12).
    if file.split('/').any(|seg| seg == "..") {
        return Err(format!(
            "reliability source '{}' must not contain '..' (path traversal outside the project root is forbidden)",
            file
        ));
    }
    let p = Path::new(file);
    if p.is_absolute() {
        // Absolute paths are allowed as-is (caller-provided and trusted).
        Ok(p.to_path_buf())
    } else {
        Ok(base_dir.join(p))
    }
}

fn load_artifact(path: &Path) -> Result<ReliabilityArtifact, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read artifact: {}", e))?;
    let artifact = match path.extension().and_then(|e| e.to_str()) {
        Some("json") => ReliabilityArtifact::from_json(&content).map_err(|e| e.to_string()),
        Some("etdl") | Some("yaml") | Some("yml") | Some("rprob") => {
            ReliabilityArtifact::from_yaml(&content).map_err(|e| e.to_string())
        }
        _ => ReliabilityArtifact::from_yaml(&content).map_err(|e| e.to_string()),
    }?;

    // Semantic validation: a malformed artifact must fail the build rather than
    // silently missing estimates. The deterministic path only needs the
    // probability-like estimates, so validate the artifact structurally and let
    // per-estimate resolution errors surface as E-112 with provenance.
    let issues = etdl_reliability_core::validation::validate_artifact_issues(&artifact);
    let structural: Vec<_> = issues
        .iter()
        .filter(|i| {
            !matches!(
                i,
                etdl_reliability_core::validation::ArtifactIssue::MetricRequiresValue(..)
            )
        })
        .collect();
    if let Some(first) = structural.first() {
        return Err(format!("artifact '{}' is invalid: {}", artifact.id, first));
    }

    Ok(artifact)
}

/// Load a reliability artifact for CLI inspection/validation. Unlike the
/// compiler build path, this performs full semantic validation so the CLI can
/// surface every issue (structural and per-estimate).
pub fn load_artifact_for_cli(path: &Path) -> Result<ReliabilityArtifact, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read artifact: {}", e))?;
    let artifact = match path.extension().and_then(|e| e.to_str()) {
        Some("json") => ReliabilityArtifact::from_json(&content).map_err(|e| e.to_string()),
        Some("etdl") | Some("yaml") | Some("yml") | Some("rprob") => {
            ReliabilityArtifact::from_yaml(&content).map_err(|e| e.to_string())
        }
        _ => ReliabilityArtifact::from_yaml(&content).map_err(|e| e.to_string()),
    }?;
    Ok(artifact)
}

/// Re-export for tests/consumers.
pub use etdl_reliability_core::estimate::ProbabilityState;

/// The built-in Reliability Supplement extension. Registered in the generic
/// extension registry (`crate::extension::builtin_registry`) and driven through
/// the same lifecycle as any future supplement.
#[derive(Debug, Default)]
pub struct ReliabilityExtension;

impl ReliabilityExtension {
    pub fn new() -> Self {
        ReliabilityExtension
    }
}

/// The typed result of the reliability extension's semantic processing step.
pub struct ReliabilityResult {
    pub resolved: Vec<ResolvedBasicEvent>,
    pub manifest: Option<BuildManifest>,
}

impl crate::extension::ExtensionResult for ReliabilityResult {
    fn extension_id(&self) -> &str {
        RELIABILITY_SUPPLEMENT
    }
}

impl crate::extension::EtdlExtension for ReliabilityExtension {
    fn id(&self) -> &str {
        RELIABILITY_SUPPLEMENT
    }

    fn version(&self) -> &str {
        "1.0"
    }

    fn validate(
        &self,
        doc: &EtlDocument,
        _context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Core supplement validation (id/version/required) already ran in
        // validate::validate_supplements. Here we can add extension-specific
        // structural validation; none beyond that is required for 1.0.
        let _ = (doc, diagnostics);
    }

    fn process(
        &self,
        doc: &EtlDocument,
        context: &crate::extension::ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn crate::extension::ExtensionResult + '_> {
        let (resolved, manifest) = resolve_reliability(doc, context.base_dir, diagnostics);
        Box::new(ReliabilityResult { resolved, manifest })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::{builtin_registry, ExtensionContext};

    /// A minimal reliability document with one external-sourced basic event.
    fn doc_with_source() -> EtlDocument {
        let yaml = r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
supplements:
  - id: etdl.reliability
    version: "1.0"
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
faultTrees:
  FT:
    topEvent: { id: Top, description: "t", rootCause: G }
    gates:
      G: { type: OR, inputs: [A, B] }
    basicEvents:
      A:
        description: "a"
        x-reliability:
          source: gw
          estimate: est.timeout
      B:
        description: "b"
        probability: 0.5
x-reliability:
  sources:
    - id: gw
      type: external
      file: "./missing.rprob"
"#;
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn reliability_extension_is_registered_and_processable() {
        let registry = builtin_registry();
        assert!(registry.contains("etdl.reliability"));
        assert!(registry.list().contains(&"etdl.reliability"));

        let ext = registry.lookup("etdl.reliability").expect("registered");
        assert_eq!(ext.id(), "etdl.reliability");

        let doc = doc_with_source();
        let base = std::path::Path::new(".");
        let ctx = ExtensionContext::new(&doc, base);
        let mut diagnostics = Vec::new();
        let result = ext.process(&doc, &ctx, &mut diagnostics);
        assert_eq!(result.extension_id(), "etdl.reliability");
        // The artifact is missing, so no values resolve; diagnostics record it.
        assert!(
            diagnostics.iter().any(|d| d.is_error()),
            "missing artifact should produce an error diagnostic"
        );
    }
}
