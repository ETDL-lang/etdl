//! End-to-end tests for ETDL Standard Library 1.0: a document declares
//! `libraries:`, references a qualified id as an ordinary fault-tree gate
//! input, and the *unmodified* validate/compile/analyze pipeline resolves
//! it. Covers every scenario listed in the feature's acceptance criteria:
//! import + use, source-only library, optional-library resolution, missing
//! library, cyclic dependency, incompatible version, deterministic
//! resolution, and stdlib identity in build metadata.

use etdl_compiler::Compiler;
use etdl_parser::asyncapi::AsyncApiRegistry;

/// A minimal in-memory AsyncAPI registry so `initiatingEvent.message` can
/// resolve without touching the filesystem (mirrors how
/// `load_from_content` is the WASM-safe alternative to `load`).
fn registry_with_message() -> AsyncApiRegistry {
    let mut r = AsyncApiRegistry::new();
    r.load_from_content(
        "a",
        r#"{"components":{"messages":{"M":{"payload":{"type":"object"}}}}}"#,
        true,
    )
    .expect("registers");
    r
}

/// `inputs` is one extra gate input (often a qualified library reference);
/// `Filler` is always the second, since OR gates require >= 2 inputs
/// (V-501) — a structural rule this feature does not change.
fn doc_with_libraries(libraries_yaml: &str, inputs: &str) -> etdl_parser::ast::EtlDocument {
    let yaml = format!(
        r#"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
asyncapi_imports:
  a: "unused.yaml"
{libraries_yaml}
eventTrees:
  T:
    initiatingEvent: {{ id: I, message: "a#/components/messages/M", next: C }}
    nodes:
      C: {{ type: consequence, operation: terminate }}
faultTrees:
  FT:
    topEvent: {{ id: Top, description: "t", rootCause: G }}
    gates:
      G: {{ type: OR, inputs: [{inputs}, "Filler"] }}
    basicEvents:
      Filler:
        description: "always-present second input"
        probability: 0.01
"#
    );
    etdl_parser::parse_document(&yaml).expect("document parses")
}

#[test]
fn importing_a_standard_library_module_and_using_its_content() {
    let doc = doc_with_libraries(
        "libraries:\n  - name: std.events\n    version: \"1.0\"\n",
        "\"std.events.NetworkTimeout\"",
    );
    let compiler = Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));

    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    assert!(result.rust_output.is_some(), "compilation should succeed");
    assert_eq!(result.resolved_libraries.len(), 1);
    assert_eq!(result.resolved_libraries[0].name, "std.events");
    assert_eq!(result.resolved_libraries[0].version, "1.0");
    assert_eq!(result.resolved_libraries[0].kind, "built-in");
}

#[test]
fn documents_without_libraries_are_unaffected() {
    // Backward compatibility: no `libraries:` field at all.
    let doc = doc_with_libraries("", "\"Local\"");
    let compiler = Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));
    // "Local" is undeclared, so this document is intentionally invalid —
    // the point is only that `resolved_libraries` is empty and nothing
    // library-related fires when no library is declared.
    assert!(result.resolved_libraries.is_empty());
    assert!(!result
        .diagnostics
        .iter()
        .any(|d| d.code.starts_with("E-11") || d.code == "W-409"));
}

#[test]
fn source_only_library_is_valid() {
    // std.events has no native (Rust) component: it is exactly one .etdl
    // file. Demonstrated by successfully resolving and using it with zero
    // additional compiled-in logic beyond the generic stdlib resolver.
    let libs = etdl_compiler::stdlib::list_builtin();
    assert_eq!(libs.len(), 3);
    let events = libs
        .iter()
        .find_map(|r| r.as_ref().ok().filter(|l| l.name == "std.events"))
        .expect("std.events parses");
    assert!(events.basic_events.contains_key("NetworkTimeout"));
    let logic = libs
        .iter()
        .find_map(|r| r.as_ref().ok().filter(|l| l.name == "std.logic"))
        .expect("std.logic parses");
    assert!(logic.gates.contains_key("AnyNetworkFailure"));
    let probability = libs
        .iter()
        .find_map(|r| r.as_ref().ok().filter(|l| l.name == "std.probability"))
        .expect("std.probability parses");
    assert!(probability.basic_events.contains_key("Certain"));
}

#[test]
fn optional_library_resolves_from_a_search_path() {
    let dir = std::env::temp_dir().join(format!(
        "etdl-stdlib-optional-test-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("acme.shipping")).unwrap();
    std::fs::write(
        dir.join("acme.shipping").join("lib.etdl"),
        r#"
etdl: "1.0.0"
library:
  name: acme.shipping
  version: "1.0"
components:
  basic_events:
    CarrierApiDown:
      description: "the shipping carrier's API is unreachable"
      probability: 0.002
"#,
    )
    .unwrap();

    let doc = doc_with_libraries(
        "libraries:\n  - name: acme.shipping\n    version: \"1.0\"\n    required: true\n",
        "\"acme.shipping.CarrierApiDown\"",
    );
    let compiler = Compiler::new().with_library_search_path(&dir);
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));

    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    assert!(result.rust_output.is_some());
    assert_eq!(result.resolved_libraries[0].kind, "optional");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_required_library_fails_compilation_with_e116() {
    let doc = doc_with_libraries(
        "libraries:\n  - name: does.not.exist\n    version: \"1.0\"\n    required: true\n",
        "\"x\"",
    );
    let compiler = Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));

    assert!(result.rust_output.is_none(), "must not compile");
    assert!(result.diagnostics.iter().any(|d| d.code == "E-116"));
}

#[test]
fn missing_optional_library_warns_but_does_not_block_unrelated_compilation() {
    // The library reference itself, and *only* the library reference,
    // becomes undefined; a fault tree not using it compiles normally.
    let doc = doc_with_libraries(
        "libraries:\n  - name: does.not.exist\n    version: \"1.0\"\n    required: false\n",
        "\"Local\"",
    );
    let mut doc = doc;
    doc.fault_trees
        .as_mut()
        .unwrap()
        .get_mut("FT")
        .unwrap()
        .basic_events
        .insert(
            "Local".to_string(),
            etdl_parser::ast::BasicEvent {
                description: "local".to_string(),
                probability: Some(0.1),
                failure_rate: None,
                mission_time: None,
                undeveloped: None,
                event_type: None,
                message: None,
                extensions: Default::default(),
            },
        );
    let compiler = Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));

    assert!(result.diagnostics.iter().any(|d| d.code == "W-409"));
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "an unresolvable OPTIONAL, unreferenced library must not fail the build: {:?}",
        result.diagnostics
    );
    assert!(result.rust_output.is_some());
}

#[test]
fn non_cyclic_dependency_chain_resolves_both_libraries() {
    // stdlib A depends on stdlib B (no cycle): resolving A must also
    // resolve B, and A's own gate referencing B's basic event must splice
    // correctly end to end.
    let dir = std::env::temp_dir().join(format!(
        "etdl-stdlib-chain-test-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("chain.a")).unwrap();
    std::fs::create_dir_all(dir.join("chain.b")).unwrap();
    std::fs::write(
        dir.join("chain.a").join("lib.etdl"),
        r#"
etdl: "1.0.0"
library:
  name: chain.a
  version: "1.0"
  dependsOn:
    - name: chain.b
      version: "1.0"
components:
  gates:
    UsesB:
      type: OR
      inputs: ["chain.b.BaseSignal", "chain.a.OwnSignal"]
  basic_events:
    OwnSignal:
      description: "a's own signal"
      probability: 0.01
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("chain.b").join("lib.etdl"),
        r#"
etdl: "1.0.0"
library:
  name: chain.b
  version: "1.0"
components:
  basic_events:
    BaseSignal:
      description: "b's signal"
      probability: 0.02
"#,
    )
    .unwrap();

    let doc = doc_with_libraries(
        "libraries:\n  - name: chain.a\n    version: \"1.0\"\n    required: true\n",
        "\"chain.a.UsesB\"",
    );
    let compiler = Compiler::new().with_library_search_path(&dir);
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));

    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    assert!(result.rust_output.is_some());

    let resolved_names: Vec<&str> = result
        .resolved_libraries
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert!(resolved_names.contains(&"chain.a"));
    assert!(resolved_names.contains(&"chain.b"), "transitive dependency must also resolve");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn std_logic_composition_compiles_end_to_end() {
    // std.logic.AnyNetworkFailure composes real std.events basic events
    // (each with its own illustrative probability) -- unlike the old
    // placeholder-signal shape (SignalA/B/C with no probability, requiring
    // every input overridden just to compile), this needs zero overriding
    // to be usable: importing std.logic alone should resolve and compile
    // clean.
    let doc = doc_with_libraries(
        "libraries:\n  - name: std.logic\n    version: \"1.0\"\n",
        "\"std.logic.AnyNetworkFailure\"",
    );
    let compiler = Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));

    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    assert!(result.rust_output.is_some());

    let resolved_names: Vec<&str> = result
        .resolved_libraries
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert!(resolved_names.contains(&"std.logic"));
    assert!(
        resolved_names.contains(&"std.events"),
        "std.logic depends on std.events; the transitive dependency must also resolve"
    );
}

#[test]
fn cyclic_library_dependency_is_rejected_with_e117() {
    let dir = std::env::temp_dir().join(format!(
        "etdl-stdlib-e2e-cycle-test-{:x}",
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

    let doc = doc_with_libraries("libraries:\n  - name: a\n    version: \"1.0\"\n", "\"x\"");
    let compiler = Compiler::new().with_library_search_path(&dir);
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));

    assert!(result.diagnostics.iter().any(|d| d.code == "E-117"));
    assert!(result.rust_output.is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn incompatible_major_version_is_rejected_with_e114() {
    let doc = doc_with_libraries(
        "libraries:\n  - name: std.events\n    version: \"2.0\"\n",
        "\"x\"",
    );
    let compiler = Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));

    assert!(result.diagnostics.iter().any(|d| d.code == "E-114"));
}

#[test]
fn resolution_is_deterministic_across_compile_runs() {
    let doc = doc_with_libraries(
        "libraries:\n  - name: std.events\n    version: \"1.0\"\n",
        "\"std.events.NetworkTimeout\"",
    );
    let compiler = Compiler::new();
    let a = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));
    let b = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));
    assert_eq!(a.resolved_libraries, b.resolved_libraries);
    assert_eq!(a.rust_output, b.rust_output);
}

#[test]
fn invalid_library_name_is_rejected_with_e113() {
    let doc = doc_with_libraries(
        "libraries:\n  - name: \"Not A Valid Name!\"\n    version: \"1.0\"\n",
        "\"x\"",
    );
    let compiler = Compiler::new();
    let result = compiler.compile_with_base(&doc, &registry_with_message(), std::path::Path::new("."));
    assert!(result.diagnostics.iter().any(|d| d.code == "E-113"));
}
