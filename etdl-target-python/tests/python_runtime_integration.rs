//! End-to-end integration: generated Python calling the *actual compiled
//! Rust runtime* (`libetdl_runtime_ffi`) via `ctypes`, not a Python
//! reimplementation of it. Requires `python3` and the `etdl-runtime-ffi`
//! cdylib; each is located/built on demand and every test skips (with a
//! clear message, not a failure) if unavailable.

use etdl_compiler::Compiler;
use etdl_parser::{load_asyncapi_imports, parse_document_from_file};
use etdl_target_python::PythonCodeGenerator;
use std::path::PathBuf;
use std::process::Command;

/// Crate-local copy — see `python_generation.rs`'s `fixtures_dir` doc
/// comment for why this isn't a `../etdl-cli` relative path.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn workspace_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn native_lib_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libetdl_runtime_ffi.dylib"
    } else if cfg!(target_os = "windows") {
        "etdl_runtime_ffi.dll"
    } else {
        "libetdl_runtime_ffi.so"
    }
}

/// Locates a built `etdl-runtime-ffi` cdylib, in order:
/// 1. `ETDL_RUNTIME_FFI_LIB_DIR` env var (explicit — set this in CI, or
///    when this crate has been split out of the `etdl` monorepo into its
///    own repo and `etdl-runtime-ffi` isn't a workspace sibling anymore).
/// 2. This crate's own workspace `target/{debug,release}` — works while
///    still inside the `etdl` monorepo; builds `etdl-runtime-ffi` on
///    demand there if it isn't already built.
/// 3. `../etdl/target/{debug,release}` — the documented sibling-checkout
///    convention for local development once this crate lives in its own
///    repo: clone `etdl` next to it and this finds that build output.
/// Returns `None` (never panics) if none of these produce a usable
/// library, so callers can skip gracefully.
fn ensure_runtime_ffi_built() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ETDL_RUNTIME_FFI_LIB_DIR") {
        let candidate = PathBuf::from(dir).join(native_lib_filename());
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let target_dir = workspace_dir().join("target");
    for profile in ["debug", "release"] {
        let candidate = target_dir.join(profile).join(native_lib_filename());
        if candidate.exists() {
            return Some(candidate);
        }
    }
    if let Ok(status) = Command::new("cargo")
        .args(["build", "-p", "etdl-runtime-ffi"])
        .current_dir(workspace_dir())
        .status()
    {
        if status.success() {
            let candidate = target_dir.join("debug").join(native_lib_filename());
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let sibling_target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../etdl/target");
    for profile in ["release", "debug"] {
        let candidate = sibling_target.join(profile).join(native_lib_filename());
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn python_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn write_files(dir: &std::path::Path, files: &[(&str, String)]) {
    for (relative_path, contents) in files {
        let path = dir.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
    }
}

fn run_python(src_dir: &std::path::Path, module: &str, runtime_lib: &std::path::Path) -> std::process::Output {
    Command::new("python3")
        .arg("-c")
        .arg(format!("import {module}; {module}.main()"))
        .current_dir(src_dir)
        .env("ETDL_RUNTIME_LIBRARY", runtime_lib)
        .output()
        .expect("invoke python3")
}

#[test]
fn order_fulfillment_workflow_runs_against_the_real_rust_runtime() {
    if !python_available() {
        eprintln!("skipping: python3 not found on PATH");
        return;
    }
    let Some(runtime_lib) = ensure_runtime_ffi_built() else {
        eprintln!("skipping: could not build/locate etdl-runtime-ffi");
        return;
    };

    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join("order-fulfillment.etdl")).expect("parse");
    let registry = load_asyncapi_imports(&doc, &base).expect("asyncapi");
    let compiler = Compiler::new();
    let generator = PythonCodeGenerator::new();
    let result =
        compiler.compile_target_with_base(&doc, &registry, &base, &generator, "order-fulfillment");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()), "{:?}", result.diagnostics);
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!("etdl-py-runtime-of-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    for f in &files {
        let path = tmp.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
    }

    write_files(
        &tmp,
        &[
            (
                "fulfillment_context/test_handlers.py",
                r#"from etdl.runtime.errors import WorkflowError
from .order_fulfillment_handlers import OrderFulfillmentHandlers


class TestHandlers(OrderFulfillmentHandlers):
    def stripe_charge_handler(self, message):
        return "charged:" + message.payload.order_id
"#
                .to_string(),
            ),
            (
                "fulfillment_context/test_publisher.py",
                r#"from etdl.runtime.publisher import Publisher


class TestPublisher(Publisher):
    def publish(self, channel, payload):
        print(f"PUBLISHED channel={channel} payload={payload}")
"#
                .to_string(),
            ),
            (
                "fulfillment_context/main.py",
                r#"from .messages import OrderPlaced, OrderPlacedPayload, OrderPlacedPayloadItemsItem
from .test_handlers import TestHandlers
from .test_publisher import TestPublisher
from .workflow import handle_order_placed_trigger


def main():
    item = OrderPlacedPayloadItemsItem(qty=5, sku="SKU-1")
    payload = OrderPlacedPayload(items=[item], order_id="order-1")
    message = OrderPlaced(payload=payload, headers={})

    handle_order_placed_trigger(message, TestPublisher(), TestHandlers())
    print("MAIN_OK")
"#
                .to_string(),
            ),
        ],
    );

    let output = run_python(&tmp, "fulfillment_context.main", &runtime_lib);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "python run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("MAIN_OK"), "stdout: {stdout}\nstderr: {stderr}");
    assert!(
        stdout.contains("PUBLISHED channel=FulfillmentChannel"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("[etdl]"),
        "expected etdl-core's own telemetry/flush output on stderr, got: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn in_matches_condition_evaluates_through_the_real_rust_regex_engine() {
    if !python_available() {
        eprintln!("skipping: python3 not found on PATH");
        return;
    }
    let Some(runtime_lib) = ensure_runtime_ffi_built() else {
        eprintln!("skipping: could not build/locate etdl-runtime-ffi");
        return;
    };

    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join("in-matches-check.etdl")).expect("parse");
    let registry = load_asyncapi_imports(&doc, &base).expect("asyncapi");
    let compiler = Compiler::new();
    let generator = PythonCodeGenerator::new();
    let result =
        compiler.compile_target_with_base(&doc, &registry, &base, &generator, "in-matches-check");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()), "{:?}", result.diagnostics);
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!("etdl-py-runtime-im-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    for f in &files {
        let path = tmp.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
    }

    write_files(
        &tmp,
        &[(
            "fulfillment_context/main.py",
            r#"from .in_matches_check_handlers import InMatchesCheckHandlers
from .messages import OrderPlaced, OrderPlacedPayload, OrderPlacedPayloadItemsItem
from .workflow import handle_order_placed_trigger


class NoOpHandlers(InMatchesCheckHandlers):
    pass


def main():
    item = OrderPlacedPayloadItemsItem(qty=1, sku="SKU-9")
    payload = OrderPlacedPayload(items=[item], order_id="order-2")
    message = OrderPlaced(payload=payload, headers={})

    handle_order_placed_trigger(message, None, NoOpHandlers())
    print("MAIN_OK")
"#
            .to_string(),
        )],
    );

    let output = run_python(&tmp, "fulfillment_context.main", &runtime_lib);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "python run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("MAIN_OK"), "stdout: {stdout}\nstderr: {stderr}");

    let _ = std::fs::remove_dir_all(&tmp);
}
