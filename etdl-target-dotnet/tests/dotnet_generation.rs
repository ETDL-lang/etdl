//! Structural generation tests (no .NET SDK needed).

use etdl_compiler::Compiler;
use etdl_parser::{load_asyncapi_imports, parse_document_from_file};
use etdl_target_dotnet::DotnetCodeGenerator;
use std::path::PathBuf;

/// Crate-local copy (duplicated from `etdl-cli/tests/fixtures`) — not a
/// `../etdl-cli` relative path, so this crate builds and tests standalone
/// outside the `etdl` monorepo, e.g. once split into its own repo (see
/// `docs/architecture/targets.md`).
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn generate(fixture: &str, stem: &str) -> etdl_compiler::TargetCompilationResult {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join(fixture)).expect("parse fixture");
    let registry = load_asyncapi_imports(&doc, &base).expect("load asyncapi imports");
    let compiler = Compiler::new();
    let generator = DotnetCodeGenerator::new();
    compiler.compile_target_with_base(&doc, &registry, &base, &generator, stem)
}

#[test]
fn dotnet_generation_order_fulfillment_produces_expected_files() {
    let result = generate("order-fulfillment.etdl", "order-fulfillment");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()), "{:?}", result.diagnostics);
    let files = result.files.expect("generation produced files");
    let paths: Vec<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();

    assert!(paths.contains(&"Etdl/Runtime/NativeMethods.cs"));
    assert!(paths.contains(&"Etdl/Runtime/BranchMonitor.cs"));
    assert!(paths.contains(&"Etdl/Runtime/RetryPolicy.cs"));
    assert!(paths.contains(&"Etdl/Runtime/Condition.cs"));
    assert!(paths.contains(&"OrderFulfillment.csproj"));
    assert!(paths.contains(&"FulfillmentContext/Messages.cs"));
    assert!(paths.contains(&"FulfillmentContext/IOrderFulfillmentHandlers.cs"));
    assert!(paths.contains(&"FulfillmentContext/OrderFulfillmentWorkflow.cs"));

    let workflow = files
        .iter()
        .find(|f| f.relative_path == "FulfillmentContext/OrderFulfillmentWorkflow.cs")
        .unwrap();
    assert!(workflow.contents.contains("public static void HandleOrderPlacedTrigger("));
    assert!(workflow.contents.contains("using var retry = new RetryPolicy(3, 250, BackoffStrategy.Exponential);"));
    assert!(workflow.contents.contains("ProcessPaymentOperationFailureProbability"));

    let handlers = files
        .iter()
        .find(|f| f.relative_path == "FulfillmentContext/IOrderFulfillmentHandlers.cs")
        .unwrap();
    assert!(handlers.contents.contains("public interface IOrderFulfillmentHandlers"));
    assert!(handlers.contents.contains("StripeChargeHandler(OrderPlaced message)"));

    // net9.0, not net8.0: targeting a framework version other than the
    // installed SDK's major version forces a slow cross-version runtime
    // pack download on first build (discovered the hard way — see the
    // .csproj template's own comment in etdl-target-dotnet/src/lib.rs).
    let csproj = files.iter().find(|f| f.relative_path == "OrderFulfillment.csproj").unwrap();
    assert!(csproj.contents.contains("<TargetFramework>net9.0</TargetFramework>"));
}

#[test]
fn dotnet_generation_inline_messages_resolves_internal_refs() {
    let result = generate("inline-messages.etdl", "inline-messages");
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
    let files = result.files.expect("generation produced files");
    let messages = files
        .iter()
        .find(|f| f.relative_path == "FulfillmentContext/Messages.cs")
        .expect("Internal Message Reference resolved to a generated record");
    assert!(messages.contents.contains("public sealed record OrderPlaced("));
}

#[test]
fn semantic_equivalence_same_probability_reaches_rust_and_dotnet() {
    let base = fixtures_dir();
    let doc = parse_document_from_file(&base.join("order-fulfillment.etdl")).expect("parse");
    let registry = load_asyncapi_imports(&doc, &base).expect("asyncapi");
    let compiler = Compiler::new();

    let rust_result = compiler.compile_with_base(&doc, &registry, &base);
    let rust_output = rust_result.rust_output.expect("rust generation succeeded");

    let dotnet_generator = DotnetCodeGenerator::new();
    let dotnet_result =
        compiler.compile_target_with_base(&doc, &registry, &base, &dotnet_generator, "order-fulfillment");
    let dotnet_files = dotnet_result.files.expect("dotnet generation succeeded");
    let workflow = dotnet_files
        .iter()
        .find(|f| f.relative_path.ends_with("Workflow.cs"))
        .unwrap();

    assert!(rust_output.contains("0.012987"), "rust output: {rust_output}");
    assert!(workflow.contents.contains("0.012987"), "dotnet output: {}", workflow.contents);
}
