//! Compile-check + run-check for the Safety Supplement's codegen (Part 4):
//! registration, `safety.sil_maintained` branch selection, and the
//! live-aware `record_branch` probability argument. Mirrors
//! `live_reliability_codegen_test.rs` exactly — the same harness, a
//! different ECEL path and a different band (the barrier's declared SIL,
//! not a threshold).
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
        .join("../etdl-cli/tests/fixtures/safety-check.etdl")
}

fn gencheck_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("gencheck")
        .join("src")
}

#[test]
fn safety_fixture_compiles_and_runs_with_live_branch_selection() {
    let doc = etdl_parser::parse_document_from_file(&fixture_path()).expect("fixture parses");
    let base = fixture_path().parent().unwrap().to_path_buf();
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("asyncapi imports load");
    let result = etdl_compiler::Compiler::new().compile(&doc, &registry);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    let generated = result.rust_output.expect("compile produced output");

    // Sanity checks proving the new codegen path actually fired, before
    // spending time on a real `cargo run`.
    assert!(generated.contains("LiveFaultTreeBuilder::new"));
    assert!(generated.contains("etdl_core::live::current_probability("));
    assert!(generated.contains(">= 0.001000 && p < 0.010000"));
    assert!(generated.contains("etdl_ensure_live_gateway_failure_registered"));

    let out_path = gencheck_src().join("generated.rs");
    std::fs::write(&out_path, &generated).expect("write generated code");

    // The real proof this feature is "authoritative," not just that it
    // compiles: `cargo run` (this fixture has no hardcoded smoke test of
    // its own, unlike `gen-check`'s order-fulfillment fixture, so an
    // execution check is the only way to observe behavior).
    // `gencheck/src/main.rs`'s `gen-check-safety` block drives
    // `etdl_core::live::record_observation` directly (as an embedding
    // application would) and asserts branch selection flips from the
    // SUCCESS (normal) branch to the FAILURE (fail-safe) branch as the
    // live estimate drifts outside the barrier's declared SIL band — a
    // failed assertion there panics, so a non-zero exit here means the
    // live behavior didn't actually happen.
    let status = Command::new("cargo")
        .args(["run", "--quiet", "--features", "gen-check-inline,gen-check-safety"])
        .current_dir(gencheck_src().parent().unwrap())
        .status()
        .expect("cargo run runs");

    let _ = std::fs::remove_file(&out_path);

    assert!(
        status.success(),
        "safety-check fixture's generated code failed at runtime (see gencheck's own \
         assertions). Generated source:\n{generated}"
    );
}
