//! Compiler pipeline for the Event Tree Definition Language (ETDL).
//!
//! Validates `.etdl` documents, resolves fault tree top-event probabilities
//! ([IEC 61025:2006](https://github.com/ETDL-lang/etdl-specification)), and
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

pub use codegen::{CodeGenerator, GeneratedFile, RustCodeGenerator};
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

/// Result of compiling for one arbitrary target generator
/// ([`Compiler::compile_target`]/[`Compiler::compile_target_with_base`]) —
/// the target-neutral counterpart to [`CompilationResult`], which stays
/// Rust-specific (`rust_output: Option<String>`) for backward compatibility.
#[derive(Debug, Clone)]
pub struct TargetCompilationResult {
    pub diagnostics: Vec<Diagnostic>,
    /// The generated files, present iff generation succeeded with no
    /// upstream errors. Empty output (a generator returning zero files) is
    /// treated the same as `None`, matching `CompilationResult::rust_output`.
    pub files: Option<Vec<codegen::GeneratedFile>>,
    pub build_manifest: Option<serde_json::Value>,
    pub resolved_libraries: Vec<stdlib::LibraryProvenance>,
}

/// The target-neutral part of the pipeline (library expansion, validation,
/// extension processing, fault-tree probability resolution) — computed once
/// and shared by every target's compilation, so no target implementation
/// (Rust or otherwise) re-runs parsing, semantic validation, or fault-tree
/// evaluation. See `docs/architecture/targets.md`.
struct Prepared {
    expanded_doc: EtlDocument,
    fault_tree_probs: std::collections::BTreeMap<String, f64>,
    diagnostics: Vec<Diagnostic>,
    build_manifest: Option<serde_json::Value>,
    resolved_libraries: Vec<stdlib::LibraryProvenance>,
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
        let prepared = self.prepare(doc, asyncapi_registry, base_dir);
        if prepared.diagnostics.iter().any(|d| d.is_error()) {
            return CompilationResult {
                diagnostics: prepared.diagnostics,
                rust_output: None,
                build_manifest: prepared.build_manifest,
                resolved_libraries: prepared.resolved_libraries,
            };
        }

        let mut diagnostics = prepared.diagnostics;
        // "generated" is a placeholder stem: compile_with_base predates
        // per-file naming (callers only ever read `rust_output`'s string
        // content, never a filename derived from it), so nothing observes
        // this value — it exists only to satisfy generate_all's signature,
        // which every target (not just this one) now takes a stem through.
        let gen_result = self.rust_codegen.generate_all(
            &prepared.expanded_doc,
            &prepared.fault_tree_probs,
            asyncapi_registry,
            "generated",
            &mut diagnostics,
        );
        let rust_output = gen_result
            .ok()
            .and_then(|files| files.into_iter().next())
            .map(|f| f.contents)
            .filter(|s| !s.is_empty());

        CompilationResult {
            diagnostics,
            rust_output,
            build_manifest: prepared.build_manifest,
            resolved_libraries: prepared.resolved_libraries,
        }
    }

    /// Compile for an arbitrary registered target (see `etdl-cli`'s target
    /// registry) rather than the built-in Rust target specifically. `stem`
    /// is the input document's filename without extension, used for output
    /// naming exactly like `compile_with_base`'s Rust path already does.
    pub fn compile_target(
        &self,
        doc: &EtlDocument,
        asyncapi_registry: &AsyncApiRegistry,
        generator: &dyn CodeGenerator,
        stem: &str,
    ) -> TargetCompilationResult {
        self.compile_target_with_base(doc, asyncapi_registry, std::path::Path::new("."), generator, stem)
    }

    /// [`Compiler::compile_target`] with a base directory for resolving
    /// relative reliability artifact paths.
    pub fn compile_target_with_base(
        &self,
        doc: &EtlDocument,
        asyncapi_registry: &AsyncApiRegistry,
        base_dir: &std::path::Path,
        generator: &dyn CodeGenerator,
        stem: &str,
    ) -> TargetCompilationResult {
        let prepared = self.prepare(doc, asyncapi_registry, base_dir);
        if prepared.diagnostics.iter().any(|d| d.is_error()) {
            return TargetCompilationResult {
                diagnostics: prepared.diagnostics,
                files: None,
                build_manifest: prepared.build_manifest,
                resolved_libraries: prepared.resolved_libraries,
            };
        }

        let mut diagnostics = prepared.diagnostics;
        let gen_result = generator.generate_all(
            &prepared.expanded_doc,
            &prepared.fault_tree_probs,
            asyncapi_registry,
            stem,
            &mut diagnostics,
        );
        let files = gen_result.ok().filter(|files| !files.is_empty());

        TargetCompilationResult {
            diagnostics,
            files,
            build_manifest: prepared.build_manifest,
            resolved_libraries: prepared.resolved_libraries,
        }
    }

    /// Shared pipeline for every target: library expansion, validation,
    /// extension processing, fault-tree probability resolution. Faithfully
    /// mirrors what `compile_with_base` always did inline (including
    /// `validate_with_base`'s own idempotent re-expansion of `libraries:` —
    /// see its doc comment) — extracted here, unchanged, so a second target
    /// can share it instead of duplicating it.
    fn prepare(
        &self,
        doc: &EtlDocument,
        asyncapi_registry: &AsyncApiRegistry,
        base_dir: &std::path::Path,
    ) -> Prepared {
        let (expanded_doc, resolved_libs, _lib_errors) =
            stdlib::expand_libraries(doc, base_dir, &self.library_resolver);
        let resolved_libraries: Vec<stdlib::LibraryProvenance> =
            resolved_libs.iter().map(|l| l.provenance()).collect();

        let mut diagnostics = self.validate_with_base(&expanded_doc, asyncapi_registry, base_dir);
        let has_errors = diagnostics.iter().any(|d| d.is_error());
        if has_errors {
            return Prepared {
                expanded_doc,
                fault_tree_probs: std::collections::BTreeMap::new(),
                diagnostics,
                build_manifest: None,
                resolved_libraries,
            };
        }

        // Extensions: resolve external probability sources to deterministic
        // scalars BEFORE fault-tree evaluation. Preserves the build-time
        // resolution model; nothing is resolved at runtime.
        let (overrides, manifest) = self.run_extensions(&expanded_doc, base_dir, &mut diagnostics);
        let build_manifest = manifest.as_ref().and_then(|m| serde_json::to_value(m).ok());
        let fault_tree_probs = fault_tree::resolve_fault_trees_with_overrides(
            &expanded_doc,
            &overrides,
            &mut diagnostics,
        );

        Prepared {
            expanded_doc,
            fault_tree_probs,
            diagnostics,
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
