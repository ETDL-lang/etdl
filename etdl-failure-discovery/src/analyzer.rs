//! The analyzer abstraction and registry.

use std::path::Path;

use crate::config::DiscoveryConfig;
use crate::error::DiscoveryError;
use crate::report::DiscoveryReport;

/// A source analyzer for a specific language.
///
/// Analyzers are deterministic: given the same source, version, and
/// configuration they always produce the same report. They never execute
/// analyzed code, never call the network, and never modify the ontology.
pub trait SourceAnalyzer: Send + Sync {
    /// The language this analyzer understands, e.g. `rust`.
    fn language(&self) -> &str;

    /// The analyzer implementation version (distinct from the crate version).
    fn version(&self) -> &str;

    /// Analyze a single source file.
    fn analyze_file(
        &self,
        path: &Path,
        config: &DiscoveryConfig,
    ) -> Result<DiscoveryReport, DiscoveryError>;

    /// Analyze an entire project (file, directory, or workspace root).
    fn analyze_project(
        &self,
        root: &Path,
        config: &DiscoveryConfig,
    ) -> Result<DiscoveryReport, DiscoveryError>;
}

/// The built-in analyzer registry. Analyzers are compiled in — there is no
/// runtime download or dynamic loading.
pub struct AnalyzerRegistry {
    analyzers: Vec<Box<dyn SourceAnalyzer>>,
}

impl Default for AnalyzerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyzerRegistry {
    pub fn new() -> Self {
        AnalyzerRegistry {
            analyzers: vec![Box::new(crate::rust::RustAnalyzer::new())],
        }
    }

    pub fn language(&self, language: &str) -> Option<&dyn SourceAnalyzer> {
        self.analyzers
            .iter()
            .find(|a| a.language() == language)
            .map(|a| a.as_ref())
    }

    pub fn supported_languages(&self) -> Vec<&str> {
        self.analyzers.iter().map(|a| a.language()).collect()
    }

    pub fn all(&self) -> Vec<&dyn SourceAnalyzer> {
        self.analyzers.iter().map(|a| a.as_ref()).collect()
    }
}
