//! Compile-check + run-check for the Performance Supplement's codegen
//! (Part 4): registration, structural `maxConcurrency` enforcement, and
//! `performance.in_budget` Barrier branch selection.
//!
//! Kept as a single test function, not split across several `#[test]`s:
//! every step writes into the shared `gencheck` crate's `src/generated.rs`
//! (same constraint `codegen_test.rs`'s own doc comment already notes —
//! two test *functions* doing that would race under the default parallel
//! test runner).

use std::path::PathBuf;
use std::process::Command;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../etdl-cli/tests/fixtures/performance-check.etdl")
}

fn gencheck_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("gencheck")
        .join("src")
}

#[test]
fn performance_fixture_compiles_and_runs_with_real_enforcement() {
    let doc = etdl_parser::parse_document_from_file(&fixture_path()).expect("fixture parses");
    let base = fixture_path().parent().unwrap().to_path_buf();
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("asyncapi imports load");
    let result = etdl_compiler::Compiler::new().compile(&doc, &registry);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    let generated = result.rust_output.expect("compile produced output");

    // Sanity checks proving the new codegen paths actually fired, before
    // spending time on a real `cargo run`.
    assert!(generated.contains("etdl_core::perf::register_budget(\"concurrency-budget\""));
    assert!(generated.contains("etdl_core::perf::register_budget(\"latency-budget\""));
    assert!(generated.contains("etdl_core::perf::enter(\"concurrency-budget\")"));
    assert!(generated.contains("etdl_core::perf::in_budget(\"latency-budget\")"));
    assert!(generated.contains("Duration::from_millis(500)"), "explicit timeoutMs should win over p99Ms");

    let out_path = gencheck_src().join("generated.rs");
    std::fs::write(&out_path, &generated).expect("write generated code");

    // The real proof this feature is authoritative, not just that it
    // compiles: `cargo run` drives concurrent execution and real elapsed
    // time through the generated handlers — `gencheck/src/main.rs`'s
    // `gen-check-performance` block asserts the concurrency limit is
    // never exceeded and that `performance.in_budget` branch selection
    // actually flips once observed latency drifts past the declared
    // budget. A failed assertion there panics, so a non-zero exit here
    // means the enforcement didn't actually happen.
    let status = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--features",
            "gen-check-inline,gen-check-performance",
        ])
        .current_dir(gencheck_src().parent().unwrap())
        .status()
        .expect("cargo run runs");

    let _ = std::fs::remove_file(&out_path);

    assert!(
        status.success(),
        "performance fixture's generated code failed at runtime (see gencheck's own \
         assertions). Generated source:\n{generated}"
    );
}
