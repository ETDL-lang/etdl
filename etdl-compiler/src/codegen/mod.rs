use etdl_parser::ast::EtlDocument;
use etdl_parser::asyncapi::AsyncApiRegistry;
use std::collections::BTreeMap;

use crate::validate::Diagnostic;

mod rust;
pub use rust::RustCodeGenerator;

/// One file a target generator produces. `relative_path` is relative to the
/// `--out-dir` the CLI was given — a single-file target (Rust) returns one
/// entry named from `stem` (e.g. `"order-fulfillment.rs"`); a target whose
/// ecosystem expects a package/directory layout (Java, Go, ...) returns
/// several, with `relative_path` encoding that structure (e.g.
/// `"com/example/OrderFulfillment.java"`). The registry-facing side (the
/// CLI) never special-cases *how many* files a target produces — it just
/// writes whatever list comes back, creating parent directories as needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: String,
    pub contents: String,
}

impl GeneratedFile {
    pub fn new(relative_path: impl Into<String>, contents: impl Into<String>) -> Self {
        GeneratedFile {
            relative_path: relative_path.into(),
            contents: contents.into(),
        }
    }
}

/// A pluggable code-generation backend (spec-neutral term: "target"). Every
/// target consumes the *same* validated `EtlDocument` + resolved fault-tree
/// probabilities + AsyncAPI registry — parsing, semantic validation, and
/// fault-tree evaluation happen exactly once, upstream of this trait, in
/// [`crate::Compiler`]; a target implementation only turns that already-
/// resolved representation into target-language source text. Nothing here
/// re-parses `.etdl`, re-validates ECEL conditions, or re-evaluates fault
/// trees — see `docs/architecture/targets.md`.
pub trait CodeGenerator {
    /// Short, stable identifier used on the CLI (`--target <name>`) and in
    /// the target registry (`etdl-cli`'s `TargetRegistry`). Lowercase,
    /// matches the `--target` value exactly (e.g. `"rust"`, `"java"`).
    fn target_name(&self) -> &'static str;

    /// Generate this target's output for `doc`. `stem` is the input
    /// document's filename without extension (what today's Rust target
    /// already names its single output file after); other targets may use
    /// it as a package/module root name instead of a literal filename.
    fn generate_all(
        &self,
        doc: &EtlDocument,
        fault_tree_probs: &BTreeMap<String, f64>,
        registry: &AsyncApiRegistry,
        stem: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Vec<GeneratedFile>, String>;
}
