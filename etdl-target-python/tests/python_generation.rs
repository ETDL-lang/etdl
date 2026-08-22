//! Structural generation tests (no Python/toolchain needed) plus a
//! `python3`-gated syntax-only compile-check (`py_compile`, cheaper than
//! `java_compile_check.rs`'s `javac` equivalent — Python has no separate
//! "compile" step in the AOT sense, but `py_compile` still catches syntax
//! errors without executing anything).

use etdl_compiler::Compiler;
use etdl_parser::{load_asyncapi_imports, parse_document_from_file};
use etdl_target_python::PythonCodeGenerator;
use std::path::PathBuf;
use std::process::Command;

/// Crate-local copy, kept in sync with the other target crates'
/// `tests/fixtures` — not a `../etdl-cli` relative path, so this crate
/// builds and tests standalone outside the `etdl` monorepo.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn generate(fixture: &str, stem: &str) -> etdl_compiler::TargetCompilationResult {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join(fixture)).expect("parse fixture");
    let registry = load_asyncapi_imports(&doc, &base).expect("load asyncapi imports");
    let compiler = Compiler::new();
    let generator = PythonCodeGenerator::new();
    compiler.compile_target_with_base(&doc, &registry, &base, &generator, stem)
}

#[test]
fn python_generation_order_fulfillment_produces_expected_files() {
    let result = generate("order-fulfillment.etdl", "order-fulfillment");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()), "{:?}", result.diagnostics);
    let files = result.files.expect("generation produced files");
    let paths: Vec<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();

    assert!(paths.contains(&"etdl/runtime/native.py"));
    assert!(paths.contains(&"etdl/runtime/branch_monitor.py"));
    assert!(paths.contains(&"etdl/runtime/retry_policy.py"));
    assert!(paths.contains(&"etdl/runtime/condition.py"));
    assert!(paths.contains(&"fulfillment_context/messages.py"));
    assert!(paths.contains(&"fulfillment_context/order_fulfillment_handlers.py"));
    assert!(paths.contains(&"fulfillment_context/workflow.py"));

    let workflow = files
        .iter()
        .find(|f| f.relative_path == "fulfillment_context/workflow.py")
        .unwrap();
    assert!(workflow.contents.contains("def handle_order_placed_trigger("));
    assert!(workflow.contents.contains("with RetryPolicy(3, 250, BackoffStrategy.EXPONENTIAL) as retry:"));
    assert!(workflow.contents.contains("PROCESS_PAYMENT_OPERATION_FAILURE_PROBABILITY"));

    let handlers = files
        .iter()
        .find(|f| f.relative_path == "fulfillment_context/order_fulfillment_handlers.py")
        .unwrap();
    assert!(handlers.contents.contains("class OrderFulfillmentHandlers(ABC)"));
    assert!(handlers.contents.contains("def stripe_charge_handler(self, message"));
}

#[test]
fn python_generation_inline_messages_resolves_internal_refs() {
    let result = generate("inline-messages.etdl", "inline-messages");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
    let files = result.files.expect("generation produced files");
    let messages = files
        .iter()
        .find(|f| f.relative_path == "fulfillment_context/messages.py")
        .expect("Internal Message Reference resolved to a generated dataclass");
    assert!(messages.contents.contains("class OrderPlaced:"));
    assert!(messages.contents.contains("order_id: str"));
}

#[test]
fn semantic_equivalence_same_probability_reaches_rust_and_python() {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join("order-fulfillment.etdl")).expect("parse");
    let registry = load_asyncapi_imports(&doc, &base).expect("asyncapi");
    let compiler = Compiler::new();

    let rust_result = compiler.compile_with_base(&doc, &registry, &base);
    let rust_output = rust_result.rust_output.expect("rust generation succeeded");

    let python_generator = PythonCodeGenerator::new();
    let python_result =
        compiler.compile_target_with_base(&doc, &registry, &base, &python_generator, "order-fulfillment");
    let python_files = python_result.files.expect("python generation succeeded");
    let workflow = python_files
        .iter()
        .find(|f| f.relative_path.ends_with("workflow.py"))
        .unwrap();

    assert!(rust_output.contains("0.012987"), "rust output: {rust_output}");
    assert!(workflow.contents.contains("0.012987"), "python output: {}", workflow.contents);
}

fn python_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Syntax-only check via `python3 -m py_compile` — no `etdl-runtime-ffi`
/// needed (unlike `python_runtime_integration.rs`'s tests): `native.py`'s
/// `ctypes.CDLL(...)` call only executes when the module is *imported*
/// (`native.py` calls `_load()` at module scope), so a syntax-only check
/// intentionally does not import it, just parses/compiles every generated
/// file to bytecode.
#[test]
fn py_compile_order_fulfillment_output() {
    if !python_available() {
        eprintln!("skipping: python3 not found on PATH");
        return;
    }
    let result = generate("order-fulfillment.etdl", "order-fulfillment");
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!("etdl-py-compile-of-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let mut py_files = Vec::new();
    for f in &files {
        let path = tmp.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
        py_files.push(path);
    }

    let output = Command::new("python3")
        .arg("-m")
        .arg("py_compile")
        .args(&py_files)
        .output()
        .expect("invoke python3 -m py_compile");
    assert!(
        output.status.success(),
        "py_compile failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
