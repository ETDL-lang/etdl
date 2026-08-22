//! Structural generation tests (no Go toolchain needed) plus `go`-gated
//! build-check tests.
//!
//! Unlike the Java/Python/.NET targets, this crate was implemented without
//! access to a `go` toolchain in the development environment — see this
//! crate's module-level doc comment ("Untested in this environment") for
//! exactly what that means and why the generated code should still be
//! trustworthy. The `go_build_*` tests below exist so that, the moment a
//! `go` toolchain *is* available (in CI, or a developer's machine), they
//! start actually verifying the generated code compiles — they are not
//! meant to silently stay skipped forever.

use etdl_compiler::Compiler;
use etdl_parser::{load_asyncapi_imports, parse_document_from_file};
use etdl_target_go::GoCodeGenerator;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Crate-local copy (duplicated from `etdl-cli/tests/fixtures`) — not a
/// `../etdl-cli` relative path, so this crate builds and tests standalone
/// outside the `etdl` monorepo, e.g. once split into its own repo (see
/// `docs/architecture/targets.md`).
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn workspace_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn generate(fixture: &str, stem: &str) -> etdl_compiler::TargetCompilationResult {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join(fixture)).expect("parse fixture");
    let registry = load_asyncapi_imports(&doc, &base).expect("load asyncapi imports");
    let compiler = Compiler::new();
    let generator = GoCodeGenerator::new();
    compiler.compile_target_with_base(&doc, &registry, &base, &generator, stem)
}

#[test]
fn go_generation_order_fulfillment_produces_expected_files() {
    let result = generate("order-fulfillment.etdl", "order-fulfillment");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()), "{:?}", result.diagnostics);
    let files = result.files.expect("generation produced files");
    let paths: Vec<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();

    assert!(paths.contains(&"etdl/runtime/native.go"));
    assert!(paths.contains(&"etdl/runtime/branch_monitor.go"));
    assert!(paths.contains(&"etdl/runtime/retry_policy.go"));
    assert!(paths.contains(&"etdl/runtime/condition.go"));
    assert!(paths.contains(&"go.mod"));
    assert!(paths.contains(&"fulfillmentcontext/messages.go"));
    assert!(paths.contains(&"fulfillmentcontext/order_fulfillment_handlers.go"));
    assert!(paths.contains(&"fulfillmentcontext/workflow.go"));

    let workflow = files
        .iter()
        .find(|f| f.relative_path == "fulfillmentcontext/workflow.go")
        .unwrap();
    assert!(workflow.contents.contains("func HandleOrderPlacedTrigger("));
    assert!(workflow.contents.contains("etdlruntime.NewRetryPolicy(3, 250, etdlruntime.BackoffExponential)"));
    assert!(workflow.contents.contains("ProcessPaymentOperationFailureProbability"));
    // Every path through the function must return (Go requires it) —
    // spot-check the two terminal branches exist.
    assert!(workflow.contents.contains("return publisher.Publish(\"FulfillmentChannel\""));
    assert!(workflow.contents.contains("return publisher.Publish(\"DeadLetterChannel\""));

    let handlers = files
        .iter()
        .find(|f| f.relative_path == "fulfillmentcontext/order_fulfillment_handlers.go")
        .unwrap();
    assert!(handlers.contents.contains("type OrderFulfillmentHandlers interface"));
    assert!(handlers.contents.contains("StripeChargeHandler(message OrderPlaced) (any, error)"));
}

#[test]
fn go_generation_inline_messages_resolves_internal_refs() {
    let result = generate("inline-messages.etdl", "inline-messages");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
    let files = result.files.expect("generation produced files");
    let messages = files
        .iter()
        .find(|f| f.relative_path == "fulfillmentcontext/messages.go")
        .expect("Internal Message Reference resolved to a generated struct");
    assert!(messages.contents.contains("type OrderPlaced struct"));
}

#[test]
fn go_generation_omits_unused_time_import_when_no_retry_policy() {
    // in-matches-check.etdl has no operations at all (barrier + terminate
    // consequence only) — Go errors on an unused import, so `workflow.go`
    // must not import "time" here even though order-fulfillment's does.
    let result = generate("in-matches-check.etdl", "in-matches-check");
    let files = result.files.expect("generation succeeded");
    let workflow = files.iter().find(|f| f.relative_path.ends_with("workflow.go")).unwrap();
    assert!(!workflow.contents.contains("\"time\""), "got: {}", workflow.contents);
}

#[test]
fn semantic_equivalence_same_probability_reaches_rust_and_go() {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join("order-fulfillment.etdl")).expect("parse");
    let registry = load_asyncapi_imports(&doc, &base).expect("asyncapi");
    let compiler = Compiler::new();

    let rust_result = compiler.compile_with_base(&doc, &registry, &base);
    let rust_output = rust_result.rust_output.expect("rust generation succeeded");

    let go_generator = GoCodeGenerator::new();
    let go_result =
        compiler.compile_target_with_base(&doc, &registry, &base, &go_generator, "order-fulfillment");
    let go_files = go_result.files.expect("go generation succeeded");
    let workflow = go_files.iter().find(|f| f.relative_path.ends_with("workflow.go")).unwrap();

    assert!(rust_output.contains("0.012987"), "rust output: {rust_output}");
    assert!(workflow.contents.contains("0.012987"), "go output: {}", workflow.contents);
}

fn go_available() -> bool {
    Command::new("go")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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

/// Locates the `etdl-runtime-ffi` header directory (for `CGO_CFLAGS -I...`),
/// in order: `ETDL_RUNTIME_FFI_INCLUDE_DIR` env var, this crate's own
/// workspace (`../etdl-runtime-ffi/include`, while still inside the `etdl`
/// monorepo), or the documented sibling-checkout convention
/// (`../etdl/etdl-runtime-ffi/include`) once this crate lives in its own
/// repo. Returns `None` (never panics) if none exist, so callers can skip.
fn runtime_ffi_include_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ETDL_RUNTIME_FFI_INCLUDE_DIR") {
        let dir = PathBuf::from(dir);
        if dir.join("etdl_runtime.h").exists() {
            return Some(dir);
        }
    }
    let in_workspace = workspace_dir().join("etdl-runtime-ffi").join("include");
    if in_workspace.join("etdl_runtime.h").exists() {
        return Some(in_workspace);
    }
    let sibling = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../etdl/etdl-runtime-ffi/include");
    if sibling.join("etdl_runtime.h").exists() {
        return Some(sibling);
    }
    None
}

/// Locates a directory containing a built `etdl-runtime-ffi` cdylib, in
/// order: `ETDL_RUNTIME_FFI_LIB_DIR` env var, this crate's own workspace
/// `target/{debug,release}` (building on demand if needed, while still
/// inside the `etdl` monorepo), or `../etdl/target/{debug,release}` (the
/// documented sibling-checkout convention once this crate lives in its
/// own repo). Returns `None` (never panics) if none produce a usable
/// library, so callers can skip gracefully.
fn native_lib_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ETDL_RUNTIME_FFI_LIB_DIR") {
        let dir = PathBuf::from(dir);
        if dir.join(native_lib_filename()).exists() {
            return Some(dir);
        }
    }

    let target_dir = workspace_dir().join("target");
    for profile in ["debug", "release"] {
        let dir = target_dir.join(profile);
        if dir.join(native_lib_filename()).exists() {
            return Some(dir);
        }
    }
    if let Ok(status) = Command::new("cargo")
        .args(["build", "-p", "etdl-runtime-ffi"])
        .current_dir(workspace_dir())
        .status()
    {
        if status.success() {
            let dir = target_dir.join("debug");
            if dir.join(native_lib_filename()).exists() {
                return Some(dir);
            }
        }
    }

    let sibling_target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../etdl/target");
    for profile in ["release", "debug"] {
        let dir = sibling_target.join(profile);
        if dir.join(native_lib_filename()).exists() {
            return Some(dir);
        }
    }

    None
}

fn write_files(dir: &Path, files: &[etdl_compiler::GeneratedFile]) {
    for f in files {
        let path = dir.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
    }
}

/// `go build ./...` (not `go run` — no `main` package is generated; this
/// is a library) on the generated output, CGO-configured to point at this
/// repository's own `etdl-runtime-ffi` build. Skips with a clear message
/// (never a failure) when `go` isn't on `PATH`, or the native library
/// hasn't been built — see the module doc comment for why this target's
/// generated code hasn't otherwise been verified to compile.
#[test]
fn go_build_order_fulfillment_output() {
    if !go_available() {
        eprintln!("skipping: go toolchain not found on PATH (this target's generated code has not been verified to build — see etdl-target-go's module doc comment)");
        return;
    }
    let Some(lib_dir) = native_lib_dir() else {
        eprintln!("skipping: could not locate a built etdl-runtime-ffi (run `cargo build -p etdl-runtime-ffi` first, or set ETDL_RUNTIME_FFI_LIB_DIR)");
        return;
    };
    let Some(include_dir) = runtime_ffi_include_dir() else {
        eprintln!("skipping: could not locate etdl-runtime-ffi's include/etdl_runtime.h (set ETDL_RUNTIME_FFI_INCLUDE_DIR)");
        return;
    };

    let result = generate("order-fulfillment.etdl", "order-fulfillment");
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!("etdl-go-build-of-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    write_files(&tmp, &files);

    let output = Command::new("go")
        .arg("build")
        .arg("./...")
        .current_dir(&tmp)
        .env("CGO_ENABLED", "1")
        .env("CGO_CFLAGS", format!("-I{}", include_dir.display()))
        .env("CGO_LDFLAGS", format!("-L{}", lib_dir.display()))
        .output()
        .expect("invoke go build");

    assert!(
        output.status.success(),
        "go build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn go_build_in_matches_check_output() {
    if !go_available() {
        eprintln!("skipping: go toolchain not found on PATH (this target's generated code has not been verified to build — see etdl-target-go's module doc comment)");
        return;
    }
    let Some(lib_dir) = native_lib_dir() else {
        eprintln!("skipping: could not locate a built etdl-runtime-ffi (run `cargo build -p etdl-runtime-ffi` first, or set ETDL_RUNTIME_FFI_LIB_DIR)");
        return;
    };
    let Some(include_dir) = runtime_ffi_include_dir() else {
        eprintln!("skipping: could not locate etdl-runtime-ffi's include/etdl_runtime.h (set ETDL_RUNTIME_FFI_INCLUDE_DIR)");
        return;
    };

    let result = generate("in-matches-check.etdl", "in-matches-check");
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!("etdl-go-build-im-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    write_files(&tmp, &files);

    let output = Command::new("go")
        .arg("build")
        .arg("./...")
        .current_dir(&tmp)
        .env("CGO_ENABLED", "1")
        .env("CGO_CFLAGS", format!("-I{}", include_dir.display()))
        .env("CGO_LDFLAGS", format!("-L{}", lib_dir.display()))
        .output()
        .expect("invoke go build");

    assert!(
        output.status.success(),
        "go build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
