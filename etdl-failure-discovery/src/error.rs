//! Structured errors for failure discovery.

use std::path::PathBuf;

/// Errors that can occur during failure discovery. Each variant is structured;
/// discovery never collapses everything into a bare string.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("invalid discovery configuration: {0}")]
    InvalidConfig(String),

    #[error("unsupported language '{0}'; supported: {1}")]
    UnsupportedLanguage(String, String),

    #[error("failed to read source '{path}': {source}")]
    SourceRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse source '{path}': {message}")]
    Parse { path: PathBuf, message: String },

    #[error("analysis of '{path}' failed: {message}")]
    Analysis { path: PathBuf, message: String },

    #[error("ontology lookup failed: {0}")]
    OntologyLookup(String),

    #[error("report serialization failed: {0}")]
    ReportSerialization(String),

    #[error("path '{0}' does not exist")]
    NoSuchPath(PathBuf),

    #[error("path '{0}' is not a file or directory")]
    InvalidPath(PathBuf),
}
