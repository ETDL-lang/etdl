//! Parsing for the Event Tree Definition Language (ETDL).
//!
//! `etdl-parser` reads `.etdl` documents — the declarative, design-time DSL for
//! reliability-aware event-driven microservices defined by
//! [IEC 62502:2010 (event tree analysis)](https://github.com/ETDL-lang/etdl-specification)
//! and [IEC 61025:2006 (fault tree analysis)](https://github.com/ETDL-lang/etdl-specification) —
//! and resolves their references into AsyncAPI 3.0 contracts.
//!
//! # Features
//!
//! - [`ast`] — full document AST with manual `Deserialize` (supports legacy
//!   `eventTree` and `x-*` extension fields)
//! - [`ecel`] — the Event-tree Condition Expression Language (ECEL), a typed,
//!   side-effect-free expression language for barrier branch conditions
//! - [`asyncapi`] — AsyncAPI 3.0 document loading and a schema-introspecting registry
//! - [`jsonptr`] — RFC 6901 JSON Pointer resolution for `onFailureProbabilitySource`
//!   and other in-document references
//!
//! # Example
//!
//! ```no_run
//! use etdl_parser::parse_document_from_file;
//! use std::path::Path;
//!
//! let doc = parse_document_from_file(Path::new("order-fulfillment.etdl"))?;
//! # Ok::<(), String>(())
//! ```

pub mod ast;
pub mod asyncapi;
pub mod ecel;
pub mod jsonptr;
pub mod semantic;
pub mod spanned;

use ast::{EtlDocument, LibraryDocument};
use asyncapi::AsyncApiRegistry;
use std::path::Path;

pub fn parse_document(yaml_str: &str) -> Result<EtlDocument, String> {
    let doc: EtlDocument =
        serde_yaml::from_str(yaml_str).map_err(|e| format!("YAML parse error: {}", e))?;
    Ok(doc)
}

/// Parse an ETDL **library document** (a reusable component catalog — see
/// [`ast::LibraryDocument`]), as opposed to an ordinary system document.
pub fn parse_library_document(yaml_str: &str) -> Result<LibraryDocument, String> {
    let doc: LibraryDocument =
        serde_yaml::from_str(yaml_str).map_err(|e| format!("YAML parse error: {}", e))?;
    Ok(doc)
}

/// Like [`parse_document`] but preserves the underlying `serde_yaml` error so
/// callers can recover a source position via [`serde_yaml::Error::location`].
pub fn parse_document_raw(yaml_str: &str) -> Result<EtlDocument, serde_yaml::Error> {
    serde_yaml::from_str(yaml_str)
}

pub fn parse_document_from_file(path: &Path) -> Result<EtlDocument, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("cannot read file: {}", e))?;

    if content.starts_with('\u{feff}') {
        return Err("E-102: document contains a byte-order mark (BOM)".to_string());
    }

    parse_document(&content)
}

pub fn load_asyncapi_imports(
    doc: &EtlDocument,
    base_dir: &Path,
) -> Result<AsyncApiRegistry, String> {
    let mut registry = AsyncApiRegistry::new();
    for (alias, location) in &doc.asyncapi_imports {
        registry.load(alias, location, base_dir)?;
    }
    Ok(registry)
}
