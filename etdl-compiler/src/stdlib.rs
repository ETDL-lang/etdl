//! ETDL Standard Library resolution and library expansion.
//!
//! A **library** is a reusable component catalog written in ordinary ETDL
//! (see [`etdl_parser::ast::LibraryDocument`]). A document declares which
//! libraries it uses in `libraries:` (see [`LibraryImport`]) and references
//! their contents by **qualified id**: `<library-name>.<short-name>`, e.g.
//! `std.events.NetworkTimeout` used as a gate input exactly like any other
//! basic-event id.
//!
//! [`expand_libraries`] is the *only* new compiler primitive this requires:
//! given a parsed document, it resolves every declared library
//! (transitively, with cycle detection) and returns a **new** document with
//! each referenced qualified id spliced into the fault tree that references
//! it, as an ordinary `BasicEvent`. Everything downstream — type checking,
//! fault-tree evaluation, code generation — is completely unaware libraries
//! exist; a qualified id is just another map key to them. This is
//! deliberate: the standard library is a source-expansion concern, not a
//! collection of compiler special cases.
//!
//! ## Layers
//!
//! ```text
//! ETDL Core               language, compiler, runtime primitives
//!    |
//! ETDL Standard Library   built-in, embedded, `std.*` (this module's BuiltIn kind)
//!    |
//! ETDL Domain Libraries   e.g. the reliability supplement (a *separate*,
//!    |                    already-existing mechanism: compiled-in Rust via
//!    |                    `supplements:` + `extension::EtdlExtension` — not
//!    |                    reimplemented or replaced by this module)
//! ETDL Optional Libraries installed separately, resolved from a search path
//!    |                    (this module's Optional kind)
//! User Libraries          project-local, resolved relative to the document
//!                         (this module's User kind)
//! ```
//!
//! ## What resolves where
//!
//! - Names starting with `std.` **only** resolve from the embedded built-in
//!   registry. This is the whole of the anti-shadowing rule: it is not a
//!   precedence order that optional/user libraries could win by being
//!   listed first, it is a hard partition. See [`LibraryError::Shadowing`].
//! - Everything else is searched, in order: [`LibraryResolver::search_paths`]
//!   (optional libraries), then `<base_dir>/lib/<name>/lib.etdl` (a user
//!   library local to the importing document, mirroring how
//!   `asyncapi_imports` resolves relative paths against `base_dir`).
//!
//! ## Reliability compatibility
//!
//! This module does not depend on `etdl-reliability-core`, and nothing in
//! the existing reliability pipeline (evidence, estimation, artifacts,
//! calibration, observation, dependency/CCF analysis) depends on this
//! module either. `expand_libraries` runs as an independent step alongside
//! (not instead of) `reliability::resolve_reliability`; a future domain
//! library MAY depend on `std.*` without requiring any reliability change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use etdl_parser::ast::{BasicEvent, EtlDocument, FaultTree, LibraryDocument, LibraryImport};

/// Schema identity for the built-in standard library package (distinct from
/// `doc.etdl`, from crate versions, and from `ARTIFACT_SCHEMA` — see
/// `docs/reference/standard-library.md` for the full versioning-axis table).
pub const STDLIB_SCHEMA: &str = "etdl.stdlib/1.0";

/// The reserved namespace prefix for the built-in standard library.
pub const STD_NAMESPACE: &str = "std.";

/// Where a resolved library's definitions came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryKind {
    /// Embedded in the compiler binary; available without installing
    /// anything (`std.*`).
    BuiltIn,
    /// Found on a configured library search path.
    Optional,
    /// Found relative to the importing document (`<base_dir>/lib/<name>/`).
    User,
}

impl LibraryKind {
    pub fn label(self) -> &'static str {
        match self {
            LibraryKind::BuiltIn => "built-in",
            LibraryKind::Optional => "optional",
            LibraryKind::User => "user",
        }
    }
}

/// One resolved library: its identity plus the basic-event definitions it
/// provides, keyed by short (unqualified) name.
#[derive(Debug, Clone)]
pub struct ResolvedLibrary {
    pub name: String,
    pub version: String,
    pub kind: LibraryKind,
    pub description: Option<String>,
    pub basic_events: BTreeMap<String, BasicEvent>,
    /// Named, reusable boolean combinators built from the same core
    /// `GateType` a document's own `gates:` already uses (see
    /// `splice_referenced_definitions`). A gate's own `inputs` may
    /// reference further qualified ids, resolved transitively.
    pub gates: BTreeMap<String, etdl_parser::ast::Gate>,
    pub depends_on: Vec<LibraryImport>,
}

impl ResolvedLibrary {
    /// A lightweight, serializable identity summary for build provenance —
    /// deliberately without the resolved component definitions themselves.
    pub fn provenance(&self) -> LibraryProvenance {
        LibraryProvenance {
            name: self.name.clone(),
            version: self.version.clone(),
            kind: self.kind.label().to_string(),
        }
    }
}

/// Identity of one resolved library, for build-manifest provenance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LibraryProvenance {
    pub name: String,
    pub version: String,
    pub kind: String,
}

/// A problem resolving a declared library. Never silently substitutes
/// absence or a different library; every failure is reported.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LibraryError {
    #[error("library '{name}' was not found ({searched})")]
    NotFound { name: String, searched: String },
    #[error(
        "library '{name}': requested version '{requested}' is incompatible with the resolved \
         version '{found}' (major version must match)"
    )]
    IncompatibleVersion {
        name: String,
        requested: String,
        found: String,
    },
    #[error("cyclic library dependency: {}", chain.join(" -> "))]
    Cyclic { chain: Vec<String> },
    #[error("library '{name}': {reason}")]
    InvalidManifest { name: String, reason: String },
    #[error(
        "library '{name}' is reserved for the built-in standard library ('{prefix}' prefix) \
         and cannot be resolved from an optional or user source"
    )]
    Shadowing { name: String, prefix: String },
}

/// The built-in standard library's embedded source, `(name, raw ETDL)`.
/// Growing the standard library means adding an entry here and a file under
/// `etdl-compiler/stdlib/`; nothing else in the compiler needs to change.
///
/// These live under `etdl-compiler/stdlib/` (a sibling of `src/`), not
/// `etdl-compiler/src/`, so a reader can find `std.events` etc. as ordinary
/// ETDL source without digging into compiler internals. They can't live at
/// the repository root: `cargo package`/`cargo publish` only includes files
/// within the crate's own directory, and `include_str!` of a path outside it
/// silently works in a workspace checkout but fails when the crate is
/// packaged standalone for crates.io.
fn builtin_sources() -> &'static [(&'static str, &'static str)] {
    &[
        ("std.events", include_str!("../stdlib/events/lib.etdl")),
        ("std.logic", include_str!("../stdlib/logic/lib.etdl")),
        (
            "std.probability",
            include_str!("../stdlib/probability/lib.etdl"),
        ),
    ]
}

/// Resolves declared libraries to parsed [`LibraryDocument`]s.
#[derive(Debug, Clone, Default)]
pub struct LibraryResolver {
    /// Optional-library search directories, checked in this order. Each may
    /// contain `<library-name>/lib.etdl`. Never consulted for `std.*`.
    pub search_paths: Vec<PathBuf>,
}

impl LibraryResolver {
    pub fn new() -> Self {
        LibraryResolver::default()
    }

    pub fn with_search_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.search_paths.push(path.into());
        self
    }

    /// Names of every built-in standard library module.
    pub fn builtin_names() -> Vec<&'static str> {
        builtin_sources().iter().map(|(n, _)| *n).collect()
    }

    /// Load and parse one library by name, without resolving its
    /// dependencies. `base_dir` is the importing document's directory, used
    /// only for user-library resolution (mirrors `asyncapi_imports`).
    fn load(&self, name: &str, base_dir: &Path) -> Result<(LibraryKind, LibraryDocument), LibraryError> {
        if let Some((_, src)) = builtin_sources().iter().find(|(n, _)| *n == name) {
            return Ok((LibraryKind::BuiltIn, parse_library(name, src)?));
        }

        let user_path = base_dir.join("lib").join(name).join("lib.etdl");
        let is_reserved = name.starts_with(STD_NAMESPACE);

        if is_reserved {
            // `std.*` never resolves from a search path or user directory,
            // even if one happens to exist under this name — it is shadowed,
            // not used, so the reserved namespace stays protected.
            let shadow_found = self
                .search_paths
                .iter()
                .any(|d| d.join(name).join("lib.etdl").exists())
                || user_path.exists();
            if shadow_found {
                return Err(LibraryError::Shadowing {
                    name: name.to_string(),
                    prefix: STD_NAMESPACE.to_string(),
                });
            }
            return Err(LibraryError::NotFound {
                name: name.to_string(),
                searched: "the built-in standard library registry (reserved namespace: never \
                           searched elsewhere)"
                    .to_string(),
            });
        }

        for search_dir in &self.search_paths {
            let path = search_dir.join(name).join("lib.etdl");
            if path.exists() {
                let content = read_library_file(&path, name)?;
                return Ok((LibraryKind::Optional, parse_library(name, &content)?));
            }
        }

        if user_path.exists() {
            let content = read_library_file(&user_path, name)?;
            return Ok((LibraryKind::User, parse_library(name, &content)?));
        }

        Err(LibraryError::NotFound {
            name: name.to_string(),
            searched: format!(
                "built-in registry, {} search path(s), and '{}'",
                self.search_paths.len(),
                user_path.display()
            ),
        })
    }
}

fn read_library_file(path: &Path, name: &str) -> Result<String, LibraryError> {
    std::fs::read_to_string(path).map_err(|e| LibraryError::InvalidManifest {
        name: name.to_string(),
        reason: format!("cannot read library file: {e}"),
    })
}

fn parse_library(expected_name: &str, content: &str) -> Result<LibraryDocument, LibraryError> {
    let doc = etdl_parser::parse_library_document(content).map_err(|e| LibraryError::InvalidManifest {
        name: expected_name.to_string(),
        reason: e,
    })?;
    if doc.library.name != expected_name {
        return Err(LibraryError::InvalidManifest {
            name: expected_name.to_string(),
            reason: format!(
                "declares name '{}' but was resolved as '{}'",
                doc.library.name, expected_name
            ),
        });
    }
    Ok(doc)
}

/// The major component of a dotted version string (`"1.2"` -> `Some(1)`),
/// the same rule already used for `doc.etdl` and `Supplement::version`.
fn major_version(version: &str) -> Option<u64> {
    let trimmed = version.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.split(['.', '+']).next()?.trim().parse().ok()
}

fn check_version_compatible(name: &str, requested: &str, found: &str) -> Result<(), LibraryError> {
    match (major_version(requested), major_version(found)) {
        (Some(r), Some(f)) if r == f => Ok(()),
        _ => Err(LibraryError::IncompatibleVersion {
            name: name.to_string(),
            requested: requested.to_string(),
            found: found.to_string(),
        }),
    }
}

/// Every built-in standard library module, resolved. For introspection
/// (`etdl library list`) — the compile path resolves lazily and only
/// resolves what a document actually declares.
pub fn list_builtin() -> Vec<Result<ResolvedLibrary, LibraryError>> {
    builtin_sources()
        .iter()
        .map(|(name, src)| {
            parse_library(name, src).map(|doc| ResolvedLibrary {
                name: (*name).to_string(),
                version: doc.library.version.clone(),
                kind: LibraryKind::BuiltIn,
                description: doc.library.description.clone(),
                basic_events: doc.components.basic_events.clone().unwrap_or_default(),
                gates: doc.components.gates.clone().unwrap_or_default(),
                depends_on: doc.library.depends_on.clone(),
            })
        })
        .collect()
}

/// Resolve `import` and everything it transitively depends on into
/// `resolved`, detecting cycles via `stack` (the names currently being
/// resolved, in resolution order). Errors are collected, not short-circuited,
/// so a caller sees every problem in one pass.
fn resolve_transitively(
    name: &str,
    requested_version: &str,
    base_dir: &Path,
    resolver: &LibraryResolver,
    resolved: &mut BTreeMap<String, ResolvedLibrary>,
    stack: &mut Vec<String>,
    errors: &mut Vec<LibraryError>,
) {
    // Check the in-progress stack BEFORE the resolved cache: a library is
    // inserted into `resolved` before its own dependencies are walked (see
    // below), so if it is re-encountered while still on the stack, that is
    // exactly a cycle, not "already resolved".
    if stack.iter().any(|n| n == name) {
        let mut chain = stack.clone();
        chain.push(name.to_string());
        errors.push(LibraryError::Cyclic { chain });
        return;
    }
    if let Some(existing) = resolved.get(name) {
        if let Err(e) = check_version_compatible(name, requested_version, &existing.version) {
            errors.push(e);
        }
        return;
    }

    stack.push(name.to_string());
    match resolver.load(name, base_dir) {
        Ok((kind, lib_doc)) => {
            if let Err(e) = check_version_compatible(name, requested_version, &lib_doc.library.version) {
                errors.push(e);
            }
            let depends_on = lib_doc.library.depends_on.clone();
            let basic_events = lib_doc.components.basic_events.clone().unwrap_or_default();
            let gates = lib_doc.components.gates.clone().unwrap_or_default();
            resolved.insert(
                name.to_string(),
                ResolvedLibrary {
                    name: name.to_string(),
                    version: lib_doc.library.version.clone(),
                    kind,
                    description: lib_doc.library.description.clone(),
                    basic_events,
                    gates,
                    depends_on: depends_on.clone(),
                },
            );
            for dep in &depends_on {
                resolve_transitively(&dep.name, &dep.version, base_dir, resolver, resolved, stack, errors);
            }
        }
        Err(e) => errors.push(e),
    }
    stack.pop();
}

/// Every id referenced directly in `ft` that could plausibly be a qualified
/// library reference (gate inputs and the top event's root cause). Does not
/// look inside already-resolved library gates — see
/// [`splice_referenced_definitions`], which walks those separately.
fn referenced_ids(ft: &FaultTree) -> Vec<String> {
    let mut ids = Vec::new();
    ids.push(ft.top_event.root_cause.clone());
    if let Some(gates) = &ft.gates {
        for gate in gates.values() {
            ids.extend(gate.inputs.iter().cloned());
        }
    }
    ids
}

/// What a qualified id resolves to in a library: a reusable named boolean
/// combinator (built from the same core `GateType` values a document's own
/// `gates:` already uses) or a reusable named basic-event definition.
fn lookup_qualified<'a, 'b>(
    qualified_id: &'b str,
    resolved: &'a BTreeMap<String, ResolvedLibrary>,
) -> Option<(&'a ResolvedLibrary, &'b str)> {
    for lib in resolved.values() {
        let prefix = format!("{}.", lib.name);
        if let Some(short_name) = qualified_id.strip_prefix(&prefix) {
            return Some((lib, short_name));
        }
    }
    None
}

/// Splice every library-provided gate or basic event `ft` transitively
/// references into `ft`, under its qualified id — unless `ft` already
/// declares that id itself (locally, or from an earlier splice pass), in
/// which case the local declaration wins: a document can always override a
/// library default by declaring the same qualified id itself, and a
/// library-provided gate that references a qualified id the importer has
/// overridden picks up the override, not the library's own placeholder.
///
/// This is a fixpoint over a worklist, not a single scan, because a
/// library-provided *gate* may itself reference further qualified ids
/// (its own placeholder inputs, or another library) that also need
/// resolving before the fault tree is structurally complete.
fn splice_referenced_definitions(ft: &mut FaultTree, resolved: &BTreeMap<String, ResolvedLibrary>) {
    let mut worklist: Vec<String> = referenced_ids(ft);
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    while let Some(qualified_id) = worklist.pop() {
        if !seen.insert(qualified_id.clone()) {
            continue; // already processed this run (also guards a malformed
                       // library gate that references itself).
        }
        if ft.basic_events.contains_key(&qualified_id) {
            continue;
        }
        if ft.gates.as_ref().is_some_and(|g| g.contains_key(&qualified_id)) {
            continue;
        }
        let Some((lib, short_name)) = lookup_qualified(&qualified_id, resolved) else {
            continue; // not a library reference; left for ordinary
                       // undefined-reference validation to report.
        };
        if let Some(gate) = lib.gates.get(short_name) {
            worklist.extend(gate.inputs.iter().cloned());
            ft.gates
                .get_or_insert_with(BTreeMap::new)
                .insert(qualified_id, gate.clone());
        } else if let Some(be) = lib.basic_events.get(short_name) {
            ft.basic_events.insert(qualified_id, be.clone());
        }
    }
}

/// Resolve every library `doc` declares (transitively, with cycle
/// detection) and return a **new** document with library-provided basic
/// events spliced into whichever fault trees reference them by qualified
/// id. `doc` itself is never mutated. `required: true` (the default) means
/// resolution failure is reported here as an error the caller should treat
/// as fatal; `required: false` failures are also returned (the caller
/// decides whether to downgrade them to a warning — see
/// `validate::validate_libraries`, which applies exactly that policy).
pub fn expand_libraries(
    doc: &EtlDocument,
    base_dir: &Path,
    resolver: &LibraryResolver,
) -> (EtlDocument, Vec<ResolvedLibrary>, Vec<LibraryError>) {
    let mut errors = Vec::new();
    let mut resolved: BTreeMap<String, ResolvedLibrary> = BTreeMap::new();
    let mut stack: Vec<String> = Vec::new();

    for import in &doc.libraries {
        resolve_transitively(
            &import.name,
            &import.version,
            base_dir,
            resolver,
            &mut resolved,
            &mut stack,
            &mut errors,
        );
    }

    let mut expanded = doc.clone();
    if let Some(fault_trees) = &mut expanded.fault_trees {
        for ft in fault_trees.values_mut() {
            splice_referenced_definitions(ft, &resolved);
        }
    }

    (expanded, resolved.into_values().collect(), errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn doc_importing(libraries: Vec<LibraryImport>, inputs: Vec<&str>) -> EtlDocument {
        let yaml = format!(
            r#"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
eventTrees:
  T:
    initiatingEvent: {{ id: I, message: "a#/m", next: C }}
    nodes:
      C: {{ type: consequence, operation: terminate }}
faultTrees:
  FT:
    topEvent: {{ id: Top, description: "t", rootCause: G }}
    gates:
      G: {{ type: OR, inputs: [{}] }}
    basicEvents: {{}}
"#,
            inputs.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(", ")
        );
        let mut doc = etdl_parser::parse_document(&yaml).expect("valid doc");
        doc.libraries = libraries;
        doc
    }

    #[test]
    fn resolves_builtin_and_splices_referenced_basic_event() {
        let doc = doc_importing(
            vec![LibraryImport {
                name: "std.events".to_string(),
                version: "1.0".to_string(),
                required: true,
            }],
            vec!["std.events.NetworkTimeout", "LocalThing"],
        );
        let resolver = LibraryResolver::new();
        let (expanded, resolved, errors) = expand_libraries(&doc, Path::new("."), &resolver);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].kind, LibraryKind::BuiltIn);

        let ft = &expanded.fault_trees.as_ref().unwrap()["FT"];
        let be = ft
            .basic_events
            .get("std.events.NetworkTimeout")
            .expect("spliced in");
        assert!((be.probability.unwrap() - 0.001).abs() < 1e-12);
        // Unreferenced library entries are not spliced in.
        assert!(!ft.basic_events.contains_key("std.events.ProcessCrash"));
        // The original document is untouched.
        assert!(!doc.fault_trees.as_ref().unwrap()["FT"]
            .basic_events
            .contains_key("std.events.NetworkTimeout"));
    }

    #[test]
    fn splices_a_library_gate_and_transitively_its_own_inputs() {
        let dir = std::env::temp_dir().join(format!(
            "etdl-stdlib-gate-splice-test-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("test.logic")).unwrap();
        std::fs::write(
            dir.join("test.logic").join("lib.etdl"),
            r#"
etdl: "1.0.0"
library:
  name: test.logic
  version: "1.0"
components:
  basic_events:
    InputA:
      description: "placeholder input A"
    InputB:
      description: "placeholder input B"
  gates:
    AnyOf:
      type: OR
      inputs: ["test.logic.InputA", "test.logic.InputB"]
"#,
        )
        .unwrap();

        let doc = doc_importing(
            vec![LibraryImport {
                name: "test.logic".to_string(),
                version: "1.0".to_string(),
                required: true,
            }],
            vec!["test.logic.AnyOf", "LocalThing"],
        );
        let resolver = LibraryResolver::new().with_search_path(&dir);
        let (expanded, _resolved, errors) = expand_libraries(&doc, Path::new("."), &resolver);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");

        let ft = &expanded.fault_trees.as_ref().unwrap()["FT"];
        // The gate itself was spliced in under its qualified id.
        let gate = ft
            .gates
            .as_ref()
            .and_then(|g| g.get("test.logic.AnyOf"))
            .expect("gate spliced in");
        assert_eq!(gate.inputs, vec!["test.logic.InputA", "test.logic.InputB"]);
        // ...and its own (also-qualified) inputs were transitively resolved
        // into basic events, not left dangling.
        assert!(ft.basic_events.contains_key("test.logic.InputA"));
        assert!(ft.basic_events.contains_key("test.logic.InputB"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn overriding_a_library_gates_placeholder_input_flows_through() {
        // A document can override just ONE of a library gate's placeholder
        // inputs by declaring that qualified id itself; the spliced gate
        // then evaluates against the override, not the library default —
        // the documented substitute for true template parameterization.
        let dir = std::env::temp_dir().join(format!(
            "etdl-stdlib-gate-override-test-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("test.logic")).unwrap();
        std::fs::write(
            dir.join("test.logic").join("lib.etdl"),
            r#"
etdl: "1.0.0"
library:
  name: test.logic
  version: "1.0"
components:
  basic_events:
    InputA:
      description: "placeholder input A"
    InputB:
      description: "placeholder input B"
  gates:
    AnyOf:
      type: OR
      inputs: ["test.logic.InputA", "test.logic.InputB"]
"#,
        )
        .unwrap();

        let mut doc = doc_importing(
            vec![LibraryImport {
                name: "test.logic".to_string(),
                version: "1.0".to_string(),
                required: true,
            }],
            vec!["test.logic.AnyOf", "LocalThing"],
        );
        doc.fault_trees.as_mut().unwrap().get_mut("FT").unwrap().basic_events.insert(
            "test.logic.InputA".to_string(),
            etdl_parser::ast::BasicEvent {
                description: "overridden".to_string(),
                probability: Some(0.42),
                failure_rate: None,
                mission_time: None,
                undeveloped: None,
                event_type: None,
                message: None,
                extensions: BTreeMap::new(),
            },
        );
        let resolver = LibraryResolver::new().with_search_path(&dir);
        let (expanded, _resolved, errors) = expand_libraries(&doc, Path::new("."), &resolver);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");

        let ft = &expanded.fault_trees.as_ref().unwrap()["FT"];
        assert_eq!(
            ft.basic_events["test.logic.InputA"].probability,
            Some(0.42)
        );
        // The library's own InputB default is still used.
        assert!(ft.basic_events["test.logic.InputB"].probability.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn local_declaration_overrides_library_default() {
        let mut doc = doc_importing(
            vec![LibraryImport {
                name: "std.events".to_string(),
                version: "1.0".to_string(),
                required: true,
            }],
            vec!["std.events.NetworkTimeout"],
        );
        doc.fault_trees.as_mut().unwrap().get_mut("FT").unwrap().basic_events.insert(
            "std.events.NetworkTimeout".to_string(),
            etdl_parser::ast::BasicEvent {
                description: "overridden".to_string(),
                probability: Some(0.5),
                failure_rate: None,
                mission_time: None,
                undeveloped: None,
                event_type: None,
                message: None,
                extensions: BTreeMap::new(),
            },
        );
        let resolver = LibraryResolver::new();
        let (expanded, _resolved, errors) = expand_libraries(&doc, Path::new("."), &resolver);
        assert!(errors.is_empty());
        let ft = &expanded.fault_trees.as_ref().unwrap()["FT"];
        assert_eq!(ft.basic_events["std.events.NetworkTimeout"].probability, Some(0.5));
    }

    #[test]
    fn missing_library_is_reported_not_silently_skipped() {
        let doc = doc_importing(
            vec![LibraryImport {
                name: "std.nonexistent".to_string(),
                version: "1.0".to_string(),
                required: true,
            }],
            vec![],
        );
        let resolver = LibraryResolver::new();
        let (_expanded, _resolved, errors) = expand_libraries(&doc, Path::new("."), &resolver);
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LibraryError::NotFound { .. }));
    }

    #[test]
    fn optional_library_cannot_shadow_std_namespace() {
        let dir = std::env::temp_dir().join(format!(
            "etdl-stdlib-shadow-test-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("std.events")).unwrap();
        std::fs::write(
            dir.join("std.events").join("lib.etdl"),
            "etdl: \"1.0.0\"\nlibrary: { name: std.events, version: \"99.0\" }\ncomponents: {}\n",
        )
        .unwrap();

        let doc = doc_importing(
            vec![LibraryImport {
                name: "std.events".to_string(),
                version: "1.0".to_string(),
                required: true,
            }],
            vec![],
        );
        let resolver = LibraryResolver::new().with_search_path(&dir);
        let (_expanded, resolved, errors) = expand_libraries(&doc, Path::new("."), &resolver);
        // Must resolve to the built-in (version 1.0), never the planted
        // "shadow" copy (version 99.0) on the search path.
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(resolved[0].version, "1.0");
        assert_eq!(resolved[0].kind, LibraryKind::BuiltIn);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cyclic_dependency_is_detected_not_infinitely_recursed() {
        let dir = std::env::temp_dir().join(format!(
            "etdl-stdlib-cycle-test-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::write(
            dir.join("a").join("lib.etdl"),
            "etdl: \"1.0.0\"\nlibrary: { name: a, version: \"1.0\", dependsOn: [{ name: b, version: \"1.0\" }] }\ncomponents: {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b").join("lib.etdl"),
            "etdl: \"1.0.0\"\nlibrary: { name: b, version: \"1.0\", dependsOn: [{ name: a, version: \"1.0\" }] }\ncomponents: {}\n",
        )
        .unwrap();

        let doc = doc_importing(
            vec![LibraryImport {
                name: "a".to_string(),
                version: "1.0".to_string(),
                required: true,
            }],
            vec![],
        );
        let resolver = LibraryResolver::new().with_search_path(&dir);
        let (_expanded, _resolved, errors) = expand_libraries(&doc, Path::new("."), &resolver);
        assert!(errors.iter().any(|e| matches!(e, LibraryError::Cyclic { .. })));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn incompatible_major_version_is_rejected() {
        let doc = doc_importing(
            vec![LibraryImport {
                name: "std.events".to_string(),
                version: "2.0".to_string(),
                required: true,
            }],
            vec![],
        );
        let resolver = LibraryResolver::new();
        let (_expanded, _resolved, errors) = expand_libraries(&doc, Path::new("."), &resolver);
        assert!(errors
            .iter()
            .any(|e| matches!(e, LibraryError::IncompatibleVersion { .. })));
    }

    #[test]
    fn resolution_is_deterministic_across_repeated_runs() {
        let doc = doc_importing(
            vec![LibraryImport {
                name: "std.events".to_string(),
                version: "1.0".to_string(),
                required: true,
            }],
            vec!["std.events.NetworkTimeout", "std.events.ProcessCrash"],
        );
        let resolver = LibraryResolver::new();
        let (expanded_a, _, errors_a) = expand_libraries(&doc, Path::new("."), &resolver);
        let (expanded_b, _, errors_b) = expand_libraries(&doc, Path::new("."), &resolver);
        assert!(errors_a.is_empty() && errors_b.is_empty());
        let ft_a = &expanded_a.fault_trees.as_ref().unwrap()["FT"];
        let ft_b = &expanded_b.fault_trees.as_ref().unwrap()["FT"];
        assert_eq!(ft_a.basic_events.len(), ft_b.basic_events.len());
        for (k, v) in &ft_a.basic_events {
            assert_eq!(ft_b.basic_events.get(k).map(|b| b.probability), Some(v.probability));
        }
    }

    #[test]
    fn builtin_events_library_parses_and_is_source_only() {
        // "A library containing only *.etdl must be valid" — demonstrated
        // directly: std.events has no native component, just this file.
        let (_, src) = builtin_sources()[0];
        let doc = etdl_parser::parse_library_document(src).expect("std.events parses");
        assert_eq!(doc.library.name, "std.events");
        assert!(!doc.components.basic_events.unwrap_or_default().is_empty());
    }
}
