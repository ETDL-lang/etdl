//! End-to-end integration: generated Java calling the *actual compiled
//! Rust runtime* (`libetdl_runtime_ffi`), not a Java reimplementation of
//! it. Requires both `javac`/`java` (JDK 21, `--enable-preview`) and the
//! `etdl-runtime-ffi` cdylib; each is built/located on demand and every
//! test skips (with a clear message, not a failure) if unavailable, per
//! the project's policy that no target's tests should require every
//! toolchain to run `cargo test --workspace`.
//!
//! This is deliberately a *separate* file from `java_compile_check.rs`
//! (which only proves the generated Java is syntactically/semantically
//! valid): these tests link and run hand-authored developer code
//! (a `Handlers`/`Publisher` implementation and a `main`) against the
//! generated orchestration, proving the whole chain — developer code →
//! generated facade → native binding → Rust runtime — actually works.

use etdl_compiler::Compiler;
use etdl_parser::{load_asyncapi_imports, parse_document_from_file};
use etdl_target_java::JavaCodeGenerator;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Crate-local copy — see `java_compile_check.rs`'s `fixtures_dir` doc
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

fn javac_available() -> bool {
    Command::new("javac")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn write_files(dir: &Path, files: &[(&str, String)]) {
    for (relative_path, contents) in files {
        let path = dir.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
    }
}

/// Compiles every `.java` file under `src_dir` with `--release 21
/// --enable-preview` (java.lang.foreign is a preview API in JDK 21,
/// finalized flag-free in JDK 22+ — see `EtdlNative.java`'s doc comment).
fn compile_all(src_dir: &Path, out_dir: &Path) {
    std::fs::create_dir_all(out_dir).unwrap();
    let java_files = collect_java_files(src_dir);
    let output = Command::new("javac")
        .arg("--release")
        .arg("21")
        .arg("--enable-preview")
        .arg("-d")
        .arg(out_dir)
        .args(&java_files)
        .output()
        .expect("invoke javac");
    assert!(
        output.status.success(),
        "javac failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn collect_java_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_java_files(&path));
        } else if path.extension().is_some_and(|e| e == "java") {
            files.push(path);
        }
    }
    files
}

/// Runs `mainClass` with the flags `EtdlNative` requires
/// (`--enable-preview --enable-native-access=ALL-UNNAMED`) and
/// `-Detdl.runtime.library=<path>` pointing directly at the just-built
/// cdylib (avoiding any dependence on `java.library.path`/install
/// location — see `EtdlNative.load()`'s resolution order).
fn run_main(out_dir: &Path, main_class: &str, runtime_lib: &Path) -> std::process::Output {
    Command::new("java")
        .arg("--enable-preview")
        .arg("--enable-native-access=ALL-UNNAMED")
        .arg(format!(
            "-Detdl.runtime.library={}",
            runtime_lib.display()
        ))
        .arg("-cp")
        .arg(out_dir)
        .arg(main_class)
        .output()
        .expect("invoke java")
}

#[test]
fn order_fulfillment_workflow_runs_against_the_real_rust_runtime() {
    if !javac_available() {
        eprintln!("skipping: javac not found on PATH");
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
    let generator = JavaCodeGenerator::new();
    let result =
        compiler.compile_target_with_base(&doc, &registry, &base, &generator, "order-fulfillment");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!(
        "etdl-java-runtime-of-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let src_dir = tmp.join("src");
    let out_dir = tmp.join("out");

    for f in &files {
        let path = src_dir.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
    }

    // Developer-owned code: implements the generated interfaces, never
    // touches the generated/runtime files above.
    write_files(
        &src_dir,
        &[
            (
                "fulfillmentcontext/TestHandlers.java",
                r#"package fulfillmentcontext;

import etdl.runtime.WorkflowError;

public class TestHandlers implements OrderFulfillmentHandlers {
    @Override
    public Object stripeChargeHandler(OrderPlaced message) throws WorkflowError {
        return "charged:" + message.payload().orderId();
    }
}
"#
                .to_string(),
            ),
            (
                "fulfillmentcontext/TestPublisher.java",
                r#"package fulfillmentcontext;

import etdl.runtime.Publisher;
import etdl.runtime.WorkflowError;

public class TestPublisher implements Publisher {
    @Override
    public void publish(String channel, Object payload) throws WorkflowError {
        System.out.println("PUBLISHED channel=" + channel + " payload=" + payload);
    }
}
"#
                .to_string(),
            ),
            (
                "fulfillmentcontext/Main.java",
                r#"package fulfillmentcontext;

import etdl.runtime.WorkflowError;
import java.util.List;
import java.util.Map;

public class Main {
    public static void main(String[] args) throws WorkflowError {
        var item = new OrderPlacedPayloadItemsItem(5L, "SKU-1");
        var payload = new OrderPlacedPayload(List.of(item), "order-1");
        var message = new OrderPlaced(payload, Map.of());

        OrderFulfillmentWorkflow.handleOrderPlacedTrigger(message, new TestPublisher(), new TestHandlers());
        System.out.println("MAIN_OK");
    }
}
"#
                .to_string(),
            ),
        ],
    );

    compile_all(&src_dir, &out_dir);
    let output = run_main(&out_dir, "fulfillmentcontext.Main", &runtime_lib);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "java run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("MAIN_OK"), "stdout: {stdout}");
    assert!(
        stdout.contains("PUBLISHED channel=FulfillmentChannel"),
        "the barrier condition (native Condition-free path, plain comparison) and retry \
         (native RetryPolicy.execute callback) must have succeeded to reach this consequence; \
         stdout: {stdout}"
    );
    // Proof this actually ran the real etdl-core BranchMonitor (its
    // telemetry/flush output), not a Java-side stand-in.
    assert!(
        stderr.contains("[etdl]"),
        "expected etdl-core's own telemetry/flush output on stderr, got: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn in_matches_condition_evaluates_through_the_real_rust_regex_engine() {
    if !javac_available() {
        eprintln!("skipping: javac not found on PATH");
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
    let generator = JavaCodeGenerator::new();
    let result =
        compiler.compile_target_with_base(&doc, &registry, &base, &generator, "in-matches-check");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!(
        "etdl-java-runtime-im-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let src_dir = tmp.join("src");
    let out_dir = tmp.join("out");

    for f in &files {
        let path = src_dir.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
    }

    write_files(
        &src_dir,
        &[(
            "fulfillmentcontext/Main.java",
            r#"package fulfillmentcontext;

import etdl.runtime.WorkflowError;
import java.util.List;
import java.util.Map;

public class Main {
    public static void main(String[] args) throws WorkflowError {
        // "SKU-9" is not in ["SKU-1", "SKU-2"] (native `in` -> false) but
        // does match "^SKU-[0-9]+$" (native `matches`, RE2 via etdl-core's
        // own regex engine -> true) -> SKU_PREFIX_MATCH branch.
        var item = new OrderPlacedPayloadItemsItem(1L, "SKU-9");
        var payload = new OrderPlacedPayload(List.of(item), "order-2");
        var message = new OrderPlaced(payload, Map.of());

        InMatchesCheckWorkflow.handleOrderPlacedTrigger(message, null, new InMatchesCheckHandlers() {});
        System.out.println("MAIN_OK");
    }
}
"#
            .to_string(),
        )],
    );

    compile_all(&src_dir, &out_dir);
    let output = run_main(&out_dir, "fulfillmentcontext.Main", &runtime_lib);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "java run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("MAIN_OK"), "stdout: {stdout}");

    let _ = std::fs::remove_dir_all(&tmp);
}
