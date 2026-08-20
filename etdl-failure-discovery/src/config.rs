//! Discovery configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How discovered candidates map into the ontology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OntologyPolicy {
    /// Map candidates to existing ontology concepts; propose new concepts for
    /// unmapped candidates. Never alters the ontology.
    #[default]
    Auto,
    /// Only report mappings that are `Exact`; everything else is `Unmapped`.
    Conservative,
    /// Do not map at all; every candidate is `Unmapped`.
    Off,
}

/// Discovery configuration. All defaults are chosen for deterministic,
/// conservative local analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Language of the analyzer, e.g. `rust`. If `None`, inferred from file
    /// extensions.
    pub language: Option<String>,
    /// Minimum discovery confidence to keep a candidate in [0, 1].
    pub min_confidence: f64,
    /// Paths to include (files or directories). Empty = everything under the
    /// root (minus exclusions).
    pub include: Vec<PathBuf>,
    /// Paths to exclude (files or directories).
    pub exclude: Vec<PathBuf>,
    /// Directory names always ignored (e.g. `target`, `.git`, `node_modules`).
    pub ignore_dirs: Vec<String>,
    /// Glob-like suffix patterns of files to ignore (e.g. `*_test.rs`).
    pub ignore_patterns: Vec<String>,
    /// Ontology mapping policy.
    pub ontology_policy: OntologyPolicy,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            language: None,
            min_confidence: 0.5,
            include: Vec::new(),
            exclude: Vec::new(),
            ignore_dirs: vec![
                "target".to_string(),
                ".git".to_string(),
                "node_modules".to_string(),
                "vendor".to_string(),
                "generated".to_string(),
                "third_party".to_string(),
                "fixtures".to_string(),
                "build".to_string(),
            ],
            ignore_patterns: vec![
                "*.test.rs".to_string(),
                "*.tests.rs".to_string(),
                "*_generated.rs".to_string(),
            ],
            ontology_policy: OntologyPolicy::Auto,
        }
    }
}

impl DiscoveryConfig {
    /// Whether a path (file or directory) should be skipped.
    pub fn is_excluded(&self, path: &std::path::Path) -> bool {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if self.ignore_dirs.iter().any(|d| d == &file_name) {
            return true;
        }

        let rel = path.display().to_string();
        let normalized = rel.replace('\\', "/");
        let fname = normalized.rsplit('/').next().unwrap_or(&normalized);
        if self
            .ignore_patterns
            .iter()
            .any(|pat| glob_suffix_match(pat, fname))
        {
            return true;
        }

        if self.include.iter().any(|i| path.starts_with(i))
            && !self.exclude.iter().any(|e| path.starts_with(e))
        {
            // In-include but not excluded: keep.
            return false;
        }

        self.exclude
            .iter()
            .any(|e| path == *e || path.starts_with(e))
    }
}

/// Match a simple suffix pattern like `*.test.rs` against a file name.
fn glob_suffix_match(pattern: &str, name: &str) -> bool {
    if let Some(stripped) = pattern.strip_prefix('*') {
        name.ends_with(stripped)
    } else {
        name == pattern
    }
}
