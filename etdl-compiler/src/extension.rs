//! Generic semantic-extension mechanism for the ETDL compiler.
//!
//! This is the reusable extension seam the Reliability Supplement and any
//! future tree-event/domain supplement plug into. The core compiler only knows
//! "a document may declare supplements; a registered extension may add
//! validation and semantic processing." It does not embed reliability-specific
//! logic here.
//!
//! The lifecycle (adapted to the existing pipeline):
//!
//! ```text
//! parse
//!   -> core validation
//!   -> extension discovery (supplement declarations -> registry lookup)
//!   -> extension validation
//!   -> extension semantic processing (e.g. external probability resolution)
//!   -> core compilation
//!   -> code generation
//! ```

use etdl_parser::ast::EtlDocument;
use std::collections::BTreeMap;

use crate::validate::Diagnostic;

/// The context an extension receives while processing a document.
#[derive(Debug, Clone)]
pub struct ExtensionContext<'a> {
    pub doc: &'a EtlDocument,
    /// Base directory for resolving relative artifact paths.
    pub base_dir: &'a std::path::Path,
    /// Free-form configuration for the extension (e.g. from `x-reliability`).
    pub config: BTreeMap<String, serde_yaml::Value>,
}

impl<'a> ExtensionContext<'a> {
    pub fn new(doc: &'a EtlDocument, base_dir: &'a std::path::Path) -> Self {
        ExtensionContext {
            doc,
            base_dir,
            config: BTreeMap::new(),
        }
    }
}

/// Self-reported metadata about what a supplement validates and declares —
/// colocated in the same module as the `parse_and_validate_*`/`validate()`
/// logic that actually produces `diagnostic_codes`, so a change to one is a
/// scroll away from the other, not a separate hand-maintained copy in a
/// different crate. `etdl capabilities`/`etdl supplement list` read this
/// generically (`ExtensionRegistry::list`/`lookup` + `descriptor()`)
/// instead of each caller hard-coding a per-supplement summary — see
/// `crate::performance`'s `descriptor()` for the reference shape every
/// built-in supplement follows.
#[derive(Debug, Clone, Copy, Default)]
pub struct SupplementDescriptor {
    /// One-line human summary of what this supplement validates/declares.
    /// Empty (the default) for an extension that doesn't override
    /// `EtdlExtension::descriptor` — e.g. a dynamically loaded `.wasm`
    /// plugin, whose wire ABI (`docs/reference/supplement-plugins.md`)
    /// carries no description field to report here.
    pub summary: &'static str,
    /// The schema/version string this supplement's `x-*` field is
    /// versioned against (e.g. `"etdl.performance/1.0"`), if it has one
    /// distinct from `EtdlExtension::version()`.
    pub schema: Option<&'static str>,
    /// Every diagnostic code this supplement's own validation can produce.
    pub diagnostic_codes: &'static [&'static str],
    /// Other supplement ids this one has a real dependency on, if any (e.g.
    /// `etdl.security` on `etdl.tree-event` — see `crate::security`'s
    /// module docs for what "dependency" means in practice here).
    pub requires: &'static [&'static str],
}

/// A semantic extension that plugs into the ETDL compiler.
///
/// Implementations SHOULD be lightweight and deterministic. An extension must
/// not silently change core ETDL semantics; it adds validation and semantic
/// processing only.
pub trait EtdlExtension: Send + Sync {
    /// The namespaced extension id, e.g. `etdl.reliability`.
    fn id(&self) -> &str;

    /// The extension version.
    fn version(&self) -> &str;

    /// Self-reported description of this supplement, for `etdl
    /// capabilities`/`etdl supplement list` to surface generically.
    /// Default: empty (an extension with nothing distinctive to report,
    /// e.g. a third-party `Compiler::with_extension` caller in a test, or a
    /// dynamically loaded `.wasm` plugin — see [`SupplementDescriptor`]).
    fn descriptor(&self) -> SupplementDescriptor {
        SupplementDescriptor::default()
    }

    /// Validate the document's use of this extension. Called after core
    /// validation; diagnostics are appended to `diagnostics`.
    fn validate(
        &self,
        doc: &EtlDocument,
        context: &ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    );

    /// Optional semantic processing step, run before fault-tree evaluation.
    /// Returns diagnostics. Implementations that resolve external values (e.g.
    /// probabilities) surface them here.
    fn process(
        &self,
        _doc: &EtlDocument,
        _context: &ExtensionContext<'_>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn ExtensionResult + '_> {
        Box::new(NoopExtensionResult)
    }
}

/// Result of an extension's semantic processing step. The reliability extension
/// returns resolved external probabilities through this; future extensions may
/// return their own typed results.
pub trait ExtensionResult {
    /// The extension id that produced this result.
    fn extension_id(&self) -> &str;

    /// Basic-event probability overrides this extension's processing step
    /// resolved, as `(override_key, value)` pairs — the same shape
    /// `fault_tree::BasicEventOverrides` already consumes. Default: none.
    /// An extension that resolves external values into fault-tree
    /// probabilities (as the reliability extension does today, via its own
    /// dedicated, hard-coded path in `Compiler::run_extensions`) overrides
    /// this so a *generically registered* extension (`Compiler::
    /// with_extension`) can contribute overrides the same way, without
    /// `run_extensions` needing to know the extension's concrete result
    /// type.
    fn basic_event_overrides(&self) -> Vec<(String, f64)> {
        Vec::new()
    }
}

/// A no-op result (extensions that do no semantic processing).
pub struct NoopExtensionResult;

impl ExtensionResult for NoopExtensionResult {
    fn extension_id(&self) -> &str {
        ""
    }
}

/// A deterministic registry of registered extensions.
#[derive(Default)]
pub struct ExtensionRegistry {
    extensions: BTreeMap<String, Box<dyn EtdlExtension>>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        ExtensionRegistry::default()
    }

    /// Register an extension. Registering a duplicate id replaces the previous
    /// entry (deterministic last-write-wins).
    pub fn register<E: EtdlExtension + 'static>(&mut self, extension: E) {
        self.extensions
            .insert(extension.id().to_string(), Box::new(extension));
    }

    pub fn lookup(&self, id: &str) -> Option<&dyn EtdlExtension> {
        self.extensions.get(id).map(|b| b.as_ref())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.extensions.contains_key(id)
    }

    /// Registered extension ids, sorted (deterministic).
    pub fn list(&self) -> Vec<&str> {
        self.extensions.keys().map(|s| s.as_str()).collect()
    }
}

/// Built-in extensions shipped with the compiler. This registry is
/// discoverability/support-checking only (`etdl capabilities`, `etdl
/// supplement list`, the E-108/W-407 "is this supplement supported" check
/// in `validate::supplement_is_supported`) — it does **not** by itself make
/// an extension's `validate`/`process` run during `Compiler::validate`/
/// `compile`. Tree Event and Reliability each additionally have their own
/// special-cased call elsewhere in the pipeline; Performance instead relies
/// on `Compiler::new()` also seeding `Compiler::extensions` with it, so it
/// executes through the same generic path a third-party `with_extension`
/// supplement uses — see `crate::performance`'s module docs for why that's
/// the preferred shape for a new supplement going forward.
pub fn builtin_registry() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::new();
    // Domain-neutral, always compiled in — not gated behind the
    // `reliability` feature.
    registry.register(crate::tree_event::TreeEventExtension::new());
    registry.register(crate::performance::PerformanceExtension::new());
    registry.register(crate::safety::SafetyExtension::new());
    registry.register(crate::diagnostics::DiagnosticsExtension::new());
    registry.register(crate::security::SecurityExtension::new());
    #[cfg(feature = "reliability")]
    {
        registry.register(crate::reliability::ReliabilityExtension::new());
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestExt;

    impl EtdlExtension for TestExt {
        fn id(&self) -> &str {
            "etdl.test"
        }
        fn version(&self) -> &str {
            "1.0"
        }
        fn validate(
            &self,
            _doc: &EtlDocument,
            _context: &ExtensionContext<'_>,
            _diagnostics: &mut Vec<Diagnostic>,
        ) {
        }
    }

    #[test]
    fn registry_is_deterministic() {
        let mut r = ExtensionRegistry::new();
        r.register(TestExt);
        r.register(TestExt);
        assert!(r.contains("etdl.test"));
        assert!(r.lookup("etdl.test").is_some());
        assert_eq!(r.list(), vec!["etdl.test"]);
    }

    #[test]
    fn lookup_missing_is_none() {
        let r = ExtensionRegistry::new();
        assert!(r.lookup("etdl.nope").is_none());
    }

    /// `etdl capabilities`/`etdl supplement list` read every built-in
    /// extension's `descriptor()` generically (see `docs/CLI.md`'s `etdl
    /// capabilities` section) — an extension registered here with the
    /// trait's silent default (empty `summary`, empty `diagnostic_codes`)
    /// would print as a blank line with no way for a caller to notice.
    /// This guards that every built-in actually overrides `descriptor()`,
    /// so a future supplement added to `builtin_registry()` without one
    /// fails a test instead of shipping silently undocumented.
    #[test]
    fn every_built_in_extension_has_a_non_empty_descriptor() {
        let registry = builtin_registry();
        for id in registry.list() {
            let ext = registry.lookup(id).expect("listed");
            let d = ext.descriptor();
            assert!(
                !d.summary.is_empty(),
                "{id}: EtdlExtension::descriptor() left `summary` at the trait default (empty) — implement it"
            );
            assert!(
                !d.diagnostic_codes.is_empty(),
                "{id}: EtdlExtension::descriptor() left `diagnostic_codes` at the trait default (empty) — implement it"
            );
        }
    }
}
