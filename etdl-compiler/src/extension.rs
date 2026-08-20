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

/// Built-in extensions shipped with the compiler.
pub fn builtin_registry() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::new();
    // Domain-neutral, always compiled in — not gated behind the
    // `reliability` feature.
    registry.register(crate::tree_event::TreeEventExtension::new());
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
}
