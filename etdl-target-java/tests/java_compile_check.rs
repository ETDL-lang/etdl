//! Java-target tests.
//!
//! - Structural generation tests (`java_generation_*`) run unconditionally —
//!   no JDK needed, just like every other test in this workspace.
//! - `javac`-gated compile-check tests (`javac_compiles_*`) actually invoke
//!   `javac` against the generated output, proving it is real, compilable
//!   Java — not merely well-formed-looking text. They skip gracefully (a
//!   passing no-op) when `javac` is not on `PATH`, so this crate's test
//!   suite — and by extension `cargo test --workspace` — never requires a
//!   JDK to pass, mirroring how the Rust target's own compile-check
//!   (`etdl-compiler/tests/codegen_test.rs`) never requires anything beyond
//!   `cargo`.
//! - `semantic_equivalence` proves the Rust and Java targets are fed the
//!   exact same resolved fault-tree probability by the shared
//!   `Compiler::prepare` pipeline, without re-testing fault-tree semantics
//!   themselves (already covered by `etdl-compiler`'s own test suite).

use etdl_compiler::Compiler;
use etdl_parser::{load_asyncapi_imports, parse_document_from_file};
use etdl_target_java::JavaCodeGenerator;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Local copy of the 3 `.etdl` fixtures (+ their `asyncapi/` stubs) this
/// crate's tests need, duplicated from `etdl-cli/tests/fixtures` — kept
/// crate-local (not a `../etdl-cli` relative path) so this crate builds
/// and tests standalone outside the `etdl` monorepo, e.g. once split into
/// its own repo (see `docs/architecture/targets.md`).
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn generate_java(fixture: &str, stem: &str) -> etdl_compiler::TargetCompilationResult {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join(fixture)).expect("parse fixture");
    let registry = load_asyncapi_imports(&doc, &base).expect("load asyncapi imports");
    let compiler = Compiler::new();
    let generator = JavaCodeGenerator::new();
    compiler.compile_target_with_base(&doc, &registry, &base, &generator, stem)
}

#[test]
fn java_generation_order_fulfillment_produces_expected_files() {
    let result = generate_java("order-fulfillment.etdl", "order-fulfillment");
    assert!(
        result.diagnostics.iter().all(|d| !d.is_error()),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let files = result.files.expect("generation produced files");
    let paths: Vec<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();

    assert!(paths.contains(&"etdl/runtime/WorkflowError.java"));
    assert!(paths.contains(&"etdl/runtime/Publisher.java"));
    assert!(paths.contains(&"etdl/runtime/RetryPolicy.java"));
    assert!(paths.contains(&"fulfillmentcontext/OrderPlaced.java"));
    assert!(paths.contains(&"fulfillmentcontext/OrderFulfillmentHandlers.java"));
    assert!(paths.contains(&"fulfillmentcontext/OrderFulfillmentWorkflow.java"));

    let workflow = files
        .iter()
        .find(|f| f.relative_path == "fulfillmentcontext/OrderFulfillmentWorkflow.java")
        .unwrap();
    assert!(workflow.contents.contains("public final class OrderFulfillmentWorkflow"));
    assert!(workflow.contents.contains("RetryPolicy retry = new RetryPolicy(3, 250, BackoffStrategy.EXPONENTIAL)"));
    assert!(workflow.contents.contains("PROCESS_PAYMENT_OPERATION_FAILURE_PROBABILITY"));

    let handlers = files
        .iter()
        .find(|f| f.relative_path == "fulfillmentcontext/OrderFulfillmentHandlers.java")
        .unwrap();
    assert!(handlers.contents.contains("public interface OrderFulfillmentHandlers"));
    assert!(handlers.contents.contains("Object stripeChargeHandler(OrderPlaced message) throws WorkflowError;"));
}

#[test]
fn java_generation_inline_messages_resolves_internal_refs() {
    let result = generate_java("inline-messages.etdl", "inline-messages");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
    let files = result.files.expect("generation produced files");
    let order_placed = files
        .iter()
        .find(|f| f.relative_path == "fulfillmentcontext/OrderPlaced.java")
        .expect("Internal Message Reference resolved to a generated record");
    assert!(order_placed.contents.contains("public record OrderPlaced"));
    assert!(order_placed.contents.contains("String orderId"));
}

#[test]
fn document_domain_lowercases_into_the_java_package() {
    // `info.domain` (spec-validated as `^[A-Za-z][A-Za-z0-9]*$`) becomes the
    // Java package for document-specific output, lowercased.
    let doc_yaml = r##"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "AcmeOrders123"
eventTrees:
  T:
    initiatingEvent:
      id: I
      message: "#/components/messages/M"
      next: C
    nodes:
      C:
        type: consequence
        operation: terminate
components:
  messages:
    M:
      payload:
        type: object
"##;
    let doc: etdl_parser::ast::EtlDocument = serde_yaml::from_str(doc_yaml).expect("valid yaml");
    let registry = etdl_parser::asyncapi::AsyncApiRegistry::new();
    let compiler = Compiler::new();
    let generator = JavaCodeGenerator::new();
    let result = compiler.compile_target(&doc, &registry, &generator, "t");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
    let files = result.files.expect("generation produced files");
    assert!(
        files.iter().any(|f| f.relative_path.starts_with("acmeorders123/")),
        "expected the lowercased domain as package dir, got: {:?}",
        files.iter().map(|f| &f.relative_path).collect::<Vec<_>>()
    );
}

#[test]
fn generator_package_override_takes_precedence_over_domain() {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join("inline-messages.etdl")).expect("parse");
    let registry = load_asyncapi_imports(&doc, &base).expect("asyncapi");
    let compiler = Compiler::new();
    let generator = JavaCodeGenerator {
        version: "test".to_string(),
        package: Some("com.acme.custom".to_string()),
    };
    let result = compiler.compile_target_with_base(&doc, &registry, &base, &generator, "inline-messages");
    let files = result.files.expect("generation succeeded");
    assert!(
        files.iter().any(|f| f.relative_path.starts_with("com/acme/custom/")),
        "got: {:?}",
        files.iter().map(|f| &f.relative_path).collect::<Vec<_>>()
    );
}

/// Proves the Rust and Java targets consume identical resolved fault-tree
/// probabilities from the shared `Compiler::prepare` pipeline — without
/// re-testing fault-tree evaluation itself (`etdl-compiler`'s own tests
/// already cover that exhaustively).
#[test]
fn semantic_equivalence_same_probability_reaches_both_targets() {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join("order-fulfillment.etdl")).expect("parse");
    let registry = load_asyncapi_imports(&doc, &base).expect("asyncapi");
    let compiler = Compiler::new();

    let rust_result = compiler.compile_with_base(&doc, &registry, &base);
    let rust_output = rust_result.rust_output.expect("rust generation succeeded");

    let java_generator = JavaCodeGenerator::new();
    let java_result =
        compiler.compile_target_with_base(&doc, &registry, &base, &java_generator, "order-fulfillment");
    let java_files = java_result.files.expect("java generation succeeded");
    let java_workflow = java_files
        .iter()
        .find(|f| f.relative_path.ends_with("Workflow.java"))
        .unwrap();

    // Same fault-tree top-event probability (IEC 61025 OR-gate evaluation),
    // formatted identically (`{:.6}` in both generators), landing in both
    // outputs — proof it was resolved once, upstream, and handed unchanged
    // to each target rather than recomputed per target.
    assert!(rust_output.contains("0.012987"), "rust output: {rust_output}");
    assert!(java_workflow.contents.contains("0.012987"), "java output: {}", java_workflow.contents);
}

fn javac_available() -> bool {
    Command::new("javac")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn compile_with_javac(files: &[etdl_compiler::GeneratedFile], src_dir: &Path, out_dir: &Path) {
    for f in files {
        let path = src_dir.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
    }
    std::fs::create_dir_all(out_dir).unwrap();

    let java_files: Vec<PathBuf> = files.iter().map(|f| src_dir.join(&f.relative_path)).collect();
    // `--release 21 --enable-preview`: `etdl/runtime/EtdlNative.java` uses
    // `java.lang.foreign` (the Foreign Function & Memory API), a preview
    // feature in JDK 21 (finalized, flag-free, in JDK 22+).
    let output = Command::new("javac")
        .arg("--release")
        .arg("21")
        .arg("--enable-preview")
        .arg("-d")
        .arg(out_dir)
        .args(&java_files)
        .output()
        .expect("failed to invoke javac");

    assert!(
        output.status.success(),
        "javac failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn javac_compiles_order_fulfillment_output() {
    if !javac_available() {
        eprintln!("skipping: javac not found on PATH");
        return;
    }
    let result = generate_java("order-fulfillment.etdl", "order-fulfillment");
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!(
        "etdl-java-gencheck-of-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    compile_with_javac(&files, &tmp.join("src"), &tmp.join("out"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn javac_compiles_inline_messages_output() {
    if !javac_available() {
        eprintln!("skipping: javac not found on PATH");
        return;
    }
    let result = generate_java("inline-messages.etdl", "inline-messages");
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!(
        "etdl-java-gencheck-im-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    compile_with_javac(&files, &tmp.join("src"), &tmp.join("out"));
    let _ = std::fs::remove_dir_all(&tmp);
}
