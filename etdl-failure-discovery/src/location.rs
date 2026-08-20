//! Source locations and function/module context for discovery candidates.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A precise source location. Line and column are 1-based for humans;
/// `byte_start`/`byte_end` are 0-based exclusive-end byte offsets into the
/// file, enabling navigation from a candidate back to its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    /// 0-based byte offset of the first byte of the matched span.
    pub byte_start: usize,
    /// 0-based byte offset one past the last byte of the matched span.
    pub byte_end: usize,
}

impl SourceLocation {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        SourceLocation {
            file: file.into(),
            line: 0,
            column: 0,
            end_line: 0,
            end_column: 0,
            byte_start: 0,
            byte_end: 0,
        }
    }
}

/// Where a candidate sits in the codebase: crate, module path, function,
/// and the impl/type it belongs to (when inside one).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FunctionContext {
    /// The crate name (top-level module or `Cargo.toml` package name).
    pub crate_name: Option<String>,
    /// Module path, e.g. `["gateway", "client"]`.
    pub module: Vec<String>,
    /// Function / method name.
    pub function: Option<String>,
    /// The impl block's self type, e.g. `PaymentGateway`.
    pub impl_type: Option<String>,
}

impl FunctionContext {
    /// Full dotted path: `crate::module::function`.
    pub fn dotted_path(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = &self.crate_name {
            parts.push(c.clone());
        }
        parts.extend(self.module.iter().cloned());
        if let Some(t) = &self.impl_type {
            parts.push(t.clone());
        }
        if let Some(f) = &self.function {
            parts.push(f.clone());
        }
        parts.join("::")
    }
}
