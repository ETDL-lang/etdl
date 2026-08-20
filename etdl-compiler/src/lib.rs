//! Compiler pipeline for the Event Tree Definition Language (ETDL).
//!
//! Validates `.etdl` documents, resolves fault tree top-event probabilities
//! ([IEC 61025:2006](https://github.com/usamassem/etdl-specification)), and
//! generates service-local code. Reliability — event trees (IEC 62502),
//! fault trees (IEC 61025), retry policies, SLAs — becomes a build-time,
//! machine-checked artifact instead of scattered runtime guesses.
//!
//! # Pipeline
//!
//! 1. [`validate::validate_document`] — structural and semantic diagnostics (E-1xx,
//!    V-1xx..V-5xx, W-4xx)
//! 2. [`fault_tree::resolve_fault_trees`] — exact top-event probability evaluation
//!    (AND/OR/NOT/XOR/VOTING gates, exponential failure model)
//! 3. [`typeck`] — ECEL condition type-checking against AsyncAPI schemas
//! 4. [`codegen::CodeGenerator`] — backend trait; [`codegen::RustCodeGenerator`]
//!    emits async handlers with embedded probabilities, retry policies, and
//!    `etdl-core` instrumentation
//!
//! # Example
//!
//! ```no_run
//! use etdl_compiler::Compiler;
//! use etdl_parser::{parse_document_from_file, load_asyncapi_imports};
//! use std::path::Path;
//!
//! let base = Path::new(".");
//! let doc = parse_document_from_file(&base.join("order-fulfillment.etdl"))?;
//! let registry = load_asyncapi_imports(&doc, base)?;
//! let result = Compiler::new().compile(&doc, &registry);
//! assert!(result.diagnostics.iter().all(|d| !d.is_error()));
//! assert!(result.rust_output.is_some());
//! # Ok::<(), String>(())
//! ```

use etdl_parser::ast::EtlDocument;
use etdl_parser::asyncapi::AsyncApiRegistry;

pub mod codegen;
pub mod extension;
pub mod fault_tree;
#[cfg(feature = "reliability")]
pub mod reliability;
pub mod stdlib;
pub mod tree_event;
mod typeck;
pub mod validate;

pub use codegen::{CodeGenerator, RustCodeGenerator};
pub use extension::{EtdlExtension, ExtensionContext, ExtensionRegistry};
pub use validate::Diagnostic;

pub struct Compiler {
    pub rust_codegen: RustCodeGenerator,
    /// Resolves declared `libraries:` (standard/domain/optional/user). The
    /// built-in standard library and base_dir-relative user libraries
    /// resolve automatically; add optional-library search paths with
    /// [`Compiler::with_library_search_path`].
    pub library_resolver: stdlib::LibraryResolver,
    /// Extensions registered in addition to the built-in ones (the
    /// Reliability and Tree Event supplements, handled internally by
    /// `run_extensions` exactly as before — unaffected by this field).
    /// This is the entry point a non-core supplement (core spec Section
    /// 11.4/11.5 — e.g. a third-party `etdl.chain` implementation)
    /// registers itself through via [`Compiler::with_extension`], so its
    /// `validate`/`process` (core spec Section 11.3) actually run during
    /// [`Compiler::compile`]/[`Compiler::validate`], not just something a
    /// third party could theoretically implement.
    extensions: Vec<Box<dyn EtdlExtension>>,
}

/// A complete compilation result including optional reliability provenance.
#[derive(Debug, Clone)]
pub struct CompilationResult {
    pub diagnostics: Vec<Diagnostic>,
    pub rust_output: Option<String>,
    /// Reliability build manifest, present when the document declares the
    /// reliability supplement and external sources were resolved.
    pub build_manifest: Option<serde_json::Value>,
    /// Identity of every library actually resolved for this build (name,
    /// version, built-in/optional/user), independent of the `reliability`
    /// feature — a document using `libraries:` gets this without needing
    /// any optional feature enabled.
    pub resolved_libraries: Vec<stdlib::LibraryProvenance>,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            rust_codegen: RustCodeGenerator::new(),
            library_resolver: stdlib::LibraryResolver::new(),
            extensions: Vec::new(),
        }
    }

    /// Add a search directory for optional (non-`std.*`) libraries. Checked
    /// in the order added; never consulted for names under the reserved
    /// `std.` namespace.
    pub fn with_library_search_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.library_resolver = self.library_resolver.with_search_path(path);
        self
    }

    /// Register an additional supplement extension (core spec Section 11.5:
    /// "Defining a New Supplement"). Its `validate`/`process` (core spec
    /// Section 11.3) run during [`Compiler::validate`]/[`Compiler::compile`]
    /// exactly like the built-in Reliability/Tree Event extensions do,
    /// gated the same way: only for a document that actually declares the
    /// extension's `id()` under `supplements:` (core spec Section 5.1.1).
    /// This does not replace or reorder the built-in extensions, which
    /// [`Compiler::run_extensions`] continues to handle exactly as before —
    /// it adds a place for anything else to plug in.
    pub fn with_extension(mut self, extension: Box<dyn EtdlExtension>) -> Self {
        self.extensions.push(extension);
        self
    }

    /// Run the full validation pipeline (semantic checks, fault-tree
    /// resolution, probability validation, ECEL type checking) without
    /// generating any code.
    pub fn validate(
        &self,
        doc: &EtlDocument,
        asyncapi_registry: &AsyncApiRegistry,
    ) -> Vec<Diagnostic> {
        self.validate_with_base(doc, asyncapi_registry, std::path::Path::new("."))
    }

    /// Validate, resolving external reliability sources relative to `base_dir`.
    pub fn validate_with_base(
        &self,
        doc: &EtlDocument,
        asyncapi_registry: &AsyncApiRegistry,
        base_dir: &std::path::Path,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Resolve `libraries:` and splice referenced definitions into the
        // fault trees that use them BEFORE structural validation runs, so a
        // qualified library reference (e.g. `std.events.NetworkTimeout`)
        // validates exactly like any other basic-event id. The original
        // `doc` is never mutated.
        let (expanded_doc, resolved_libs, lib_errors) =
            stdlib::expand_libraries(doc, base_dir, &self.library_resolver);
        validate::validate_libraries(doc, &lib_errors, &mut diagnostics);
        let doc = &expanded_doc;
        let _ = &resolved_libs;

        let registered_extension_ids: Vec<&str> =
            self.extensions.iter().map(|e| e.id()).collect();
        validate::validate_document_with_extensions(
            doc,
            asyncapi_registry,
            &registered_extension_ids,
            &mut diagnostics,
        );

        // The Generic Tree Event Supplement is structural-only (no fault-tree
        // overrides to feed forward), so it is validated directly here
        // rather than through `run_extensions`'s override-collecting path —
        // purely additive; nothing about the reliability extension's own
        // call below changed.
        let (_trees, tree_diagnostics) = tree_event::parse_and_validate_trees(doc);
        diagnostics.extend(tree_diagnostics);

        if diagnostics.iter().any(|d| d.is_error()) {
            return diagnostics;
        }

        // Run registered extensions' semantic processing (e.g. external
        // probability resolution) so that values they supply feed evaluation.
        let (overrides, _manifest) = self.run_extensions(doc, base_dir, &mut diagnostics);

        let fault_tree_probs =
            fault_tree::resolve_fault_trees_with_overrides(doc, &overrides, &mut diagnostics);
        let resolved_probabilities =
            validate::resolve_probability_links(doc, &fault_tree_probs, &mut diagnostics);

        validate::validate_probability_sums(doc, &resolved_probabilities, &mut diagnostics);

        if diagnostics.iter().any(|d| d.is_error()) {
            return diagnostics;
        }

        typeck::type_check_conditions(doc, asyncapi_registry, &mut diagnostics);

        diagnostics
    }

    pub fn compile(
        &self,
        doc: &EtlDocument,
        asyncapi_registry: &AsyncApiRegistry,
    ) -> CompilationResult {
        self.compile_with_base(doc, asyncapi_registry, std::path::Path::new("."))
    }

    /// Compile with a base directory for resolving relative reliability
    /// artifact paths.
    pub fn compile_with_base(
        &self,
        doc: &EtlDocument,
        asyncapi_registry: &AsyncApiRegistry,
        base_dir: &std::path::Path,
    ) -> CompilationResult {
        // Resolve `libraries:` once here; `validate_with_base` also expands
        // (idempotently, from the already-expanded document) since it is a
        // public entry point in its own right and must not require callers
        // to pre-expand.
        let (expanded_doc, resolved_libs, _lib_errors) =
            stdlib::expand_libraries(doc, base_dir, &self.library_resolver);
        let doc = &expanded_doc;
        let resolved_libraries: Vec<stdlib::LibraryProvenance> =
            resolved_libs.iter().map(|l| l.provenance()).collect();

        let mut diagnostics = self.validate_with_base(doc, asyncapi_registry, base_dir);

        let has_errors = diagnostics.iter().any(|d| d.is_error());
        if has_errors {
            return CompilationResult {
                diagnostics,
                rust_output: None,
                build_manifest: None,
                resolved_libraries,
            };
        }

        // Extensions: resolve external probability sources to deterministic
        // scalars BEFORE fault-tree evaluation. Preserves the build-time
        // resolution model; nothing is resolved at runtime.
        let (overrides, manifest) = self.run_extensions(doc, base_dir, &mut diagnostics);

        let build_manifest = manifest.as_ref().and_then(|m| serde_json::to_value(m).ok());

        let fault_tree_probs =
            fault_tree::resolve_fault_trees_with_overrides(doc, &overrides, &mut diagnostics);

        let mut rust_output = String::new();
        let gen_result = self.rust_codegen.generate_all(
            doc,
            &fault_tree_probs,
            asyncapi_registry,
            &mut diagnostics,
        );

        if let Ok(gen_code) = gen_result {
            rust_output = gen_code;
        }

        CompilationResult {
            diagnostics,
            rust_output: if rust_output.is_empty() {
                None
            } else {
                Some(rust_output)
            },
            build_manifest,
            resolved_libraries,
        }
    }

    /// Run registered extensions' semantic processing.
    ///
    /// Each extension's `process` step may resolve external values (e.g.
    /// probabilities). The returned map is the aggregated basic-event
    /// probability overrides feeding the existing fault-tree evaluator. With
    /// the `reliability` feature disabled, this returns an empty override map.
    fn run_extensions(
        &self,
        doc: &EtlDocument,
        base_dir: &std::path::Path,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> (fault_tree::BasicEventOverrides, Option<serde_json::Value>) {
        // Always `mut`: with the `reliability` feature disabled there is no
        // built-in resolver to populate it below, but a generically
        // registered extension (`Compiler::with_extension`) still can,
        // regardless of that feature.
        let mut overrides = fault_tree::BasicEventOverrides::new();
        #[cfg(feature = "reliability")]
        let manifest: Option<serde_json::Value> = {
            let (resolved_events, m) = reliability::resolve_reliability(doc, base_dir, diagnostics);
            overrides.extend(
                resolved_events
                    .iter()
                    .map(|r| (r.override_key(), r.resolved.value)),
            );
            m.as_ref().and_then(|m| serde_json::to_value(m).ok())
        };
        // The built-in reliability resolver is compiled out without the
        // `reliability` feature; `doc`/`base_dir`/`diagnostics` remain real
        // parameters regardless, used below by any generically registered
        // extension (`Compiler::with_extension`).
        #[cfg(not(feature = "reliability"))]
        let manifest: Option<serde_json::Value> = None;

        // Additionally registered extensions (`Compiler::with_extension`):
        // run validate() then process() for each one the document actually
        // declares under `supplements:` (the same declare-to-opt-in gate
        // the built-in extensions already use), merging any basic-event
        // overrides they resolve. Their manifests, if any, are not folded
        // into the single `manifest` value above — a caller wanting a
        // registered extension's own output reads it from that extension's
        // `ExtensionResult` directly in a caller-side integration, since
        // this method's `Option<serde_json::Value>` return shape predates
        // there being more than one extension.
        for extension in &self.extensions {
            if !crate::validate::declares_supplement(doc, extension.id()) {
                continue;
            }
            let context = crate::extension::ExtensionContext::new(doc, base_dir);
            extension.validate(doc, &context, diagnostics);
            if diagnostics.iter().any(|d| d.is_error()) {
                continue;
            }
            let result = extension.process(doc, &context, diagnostics);
            overrides.extend(result.basic_event_overrides());
        }

        (overrides, manifest)
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}
