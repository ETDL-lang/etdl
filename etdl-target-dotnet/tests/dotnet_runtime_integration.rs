//! End-to-end integration: generated C# calling the *actual compiled Rust
//! runtime* (`libetdl_runtime_ffi`) via modern P/Invoke, not a C#
//! reimplementation of it. Requires the `dotnet` SDK and the
//! `etdl-runtime-ffi` cdylib; each is located/built on demand and every
//! test skips (with a clear message, not a failure) if unavailable.

use etdl_compiler::Compiler;
use etdl_parser::{load_asyncapi_imports, parse_document_from_file};
use etdl_target_dotnet::DotnetCodeGenerator;
use std::path::PathBuf;
use std::process::Command;

/// Crate-local copy — see `dotnet_generation.rs`'s `fixtures_dir` doc
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

fn dotnet_available() -> bool {
    Command::new("dotnet")
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

#[test]
fn order_fulfillment_workflow_runs_against_the_real_rust_runtime() {
    if !dotnet_available() {
        eprintln!("skipping: dotnet SDK not found on PATH");
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
    let generator = DotnetCodeGenerator::new();
    let result =
        compiler.compile_target_with_base(&doc, &registry, &base, &generator, "order-fulfillment");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()), "{:?}", result.diagnostics);
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!("etdl-dotnet-runtime-of-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    for f in &files {
        let path = tmp.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
    }

    // Developer-owned code: implements the generated interfaces, never
    // touches the generated/runtime files above.
    write_files(
        &tmp,
        &[(
            "Program.cs",
            r#"using Etdl.Runtime;
using FulfillmentContext;
using System;
using System.Collections.Generic;

class TestHandlers : IOrderFulfillmentHandlers
{
    public object? StripeChargeHandler(OrderPlaced message) => "charged:" + message.Payload.OrderId;
}

class TestPublisher : IPublisher
{
    public void Publish(string channel, object? payload) =>
        Console.WriteLine($"PUBLISHED channel={channel} payload={payload}");
}

class Program
{
    static void Main()
    {
        var item = new OrderPlacedPayloadItemsItem(5, "SKU-1");
        var payload = new OrderPlacedPayload(new[] { item }, "order-1");
        var message = new OrderPlaced(payload, new Dictionary<string, object>());

        OrderFulfillmentWorkflow.HandleOrderPlacedTrigger(message, new TestPublisher(), new TestHandlers());
        Console.WriteLine("MAIN_OK");
    }
}
"#
            .to_string(),
        )],
    );

    let build_output = Command::new("dotnet")
        .arg("build")
        .arg("-v")
        .arg("quiet")
        .current_dir(&tmp)
        .output()
        .expect("invoke dotnet build");
    assert!(
        build_output.status.success(),
        "dotnet build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr)
    );

    let run_output = Command::new("dotnet")
        .arg("run")
        .arg("--no-build")
        .current_dir(&tmp)
        .env("ETDL_RUNTIME_LIBRARY", &runtime_lib)
        .output()
        .expect("invoke dotnet run");

    let stdout = String::from_utf8_lossy(&run_output.stdout);
    let stderr = String::from_utf8_lossy(&run_output.stderr);
    assert!(
        run_output.status.success(),
        "dotnet run failed:\nstdout: {stdout}\nstderr: {stderr}"
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
    if !dotnet_available() {
        eprintln!("skipping: dotnet SDK not found on PATH");
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
    let generator = DotnetCodeGenerator::new();
    let result =
        compiler.compile_target_with_base(&doc, &registry, &base, &generator, "in-matches-check");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()), "{:?}", result.diagnostics);
    let files = result.files.expect("generation succeeded");

    let tmp = std::env::temp_dir().join(format!("etdl-dotnet-runtime-im-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    for f in &files {
        let path = tmp.join(&f.relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.contents).unwrap();
    }

    write_files(
        &tmp,
        &[(
            "Program.cs",
            r#"using FulfillmentContext;
using System;

class NoOpHandlers : IInMatchesCheckHandlers { }

class Program
{
    static void Main()
    {
        var item = new OrderPlacedPayloadItemsItem(1, "SKU-9");
        var payload = new OrderPlacedPayload(new[] { item }, "order-2");
        var message = new OrderPlaced(payload, null);

        InMatchesCheckWorkflow.HandleOrderPlacedTrigger(message, null!, new NoOpHandlers());
        Console.WriteLine("MAIN_OK");
    }
}
"#
            .to_string(),
        )],
    );

    let build_output = Command::new("dotnet")
        .arg("build")
        .arg("-v")
        .arg("quiet")
        .current_dir(&tmp)
        .output()
        .expect("invoke dotnet build");
    assert!(
        build_output.status.success(),
        "dotnet build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr)
    );

    let run_output = Command::new("dotnet")
        .arg("run")
        .arg("--no-build")
        .current_dir(&tmp)
        .env("ETDL_RUNTIME_LIBRARY", &runtime_lib)
        .output()
        .expect("invoke dotnet run");

    let stdout = String::from_utf8_lossy(&run_output.stdout);
    let stderr = String::from_utf8_lossy(&run_output.stderr);
    assert!(
        run_output.status.success(),
        "dotnet run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("MAIN_OK"), "stdout: {stdout}\nstderr: {stderr}");

    let _ = std::fs::remove_dir_all(&tmp);
}
