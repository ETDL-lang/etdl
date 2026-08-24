//! Generated-code compile check.
//!
//! Generates Rust for the order-fulfillment fixture, writes it into the
//! `etdl-gencheck` crate's `src/generated.rs`, and runs
//! `cargo check --features gen-check` to prove the generated code compiles
//! against the etdl-core runtime and the message/handler stubs.
//!
//! This is the primary guard against generated-code regressions (P0-1).

use std::path::PathBuf;
use std::process::Command;

const FIXTURE: &str = "order-fulfillment.etdl";
// `in`/`matches` lower to `etdl_core::condition` helper calls rather than
// native Rust operators — the only construct in this crate that generates
// code depending on argument-type coercion working out (see
// `condition_to_rust_code`'s `C::In`/`C::Matches` arms and
// `etdl_core::condition::{contains, matches}`'s doc comments for the
// mismatched-type bug this used to hide). `FIXTURE` alone never exercises
// either operator (only `qty > 0`), so it would not have caught a
// regression here — this fixture is a deliberate, separate compile check
// for exactly that gap.
const IN_MATCHES_FIXTURE: &str = "in-matches-check.etdl";
// Exercises inline `components.messages` Message References and a bare
// Channel Reference (spec Sections 5.3.4/5.3.5) — a document with no
// `asyncapi_imports` at all, so `generate_inline_message_types` must emit
// real Rust struct definitions rather than relying on an external
// AsyncAPI-toolchain-generated `{alias}::messages::*` module.
const INLINE_MESSAGES_FIXTURE: &str = "inline-messages.etdl";
// Exercises the extended ECEL grammar (spec §6.2 boolean combinators,
// arithmetic, built-in functions, `defined()`) and, specifically,
// `message.headers.*` access — previously broken codegen (direct field
// access on `Option<serde_json::Value>`, which cannot compile) that this
// fixture's `HeadersBarrier` node exists to catch a regression on.
const ECEL_EXTENDED_FIXTURE: &str = "ecel-extended.etdl";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_path(name: &str) -> PathBuf {
    let cli_dir = manifest_dir().join("../etdl-cli");
    cli_dir.join("tests").join("fixtures").join(name)
}

fn fixture_base() -> PathBuf {
    let cli_dir = manifest_dir().join("../etdl-cli");
    cli_dir.join("tests").join("fixtures")
}

fn gencheck_src() -> PathBuf {
    manifest_dir().join("tests").join("gencheck").join("src")
}

/// Compiles `fixture_name`, writes the generated Rust into the gencheck
/// crate, and runs `cargo check --features <features>` on it. Panics with
/// the generated source dumped on failure. Cleans up the generated file
/// before returning either way, so sequential calls (see
/// `generated_code_compiles` below, which checks fixtures one after
/// another in the same test) never see a stale file from a previous call.
///
/// `features` selects which of `main.rs`'s `#[cfg(feature = ...)]` bodies
/// get compiled alongside the generated code: `gen-check` also compiles a
/// hardcoded runtime smoke test specific to the default fixture's message
/// types (`orders_api::messages::OrderPlaced`); `gen-check-inline` compiles
/// only the generated code itself, for a fixture (like
/// `INLINE_MESSAGES_FIXTURE`) whose message types are generated inline and
/// so aren't compatible with that hardcoded call.
fn compile_and_check(fixture_name: &str, features: &str) {
    let doc =
        etdl_parser::parse_document_from_file(&fixture_path(fixture_name)).expect("fixture parses");
    let registry =
        etdl_parser::load_asyncapi_imports(&doc, &fixture_base()).expect("asyncapi imports load");
    let result = etdl_compiler::Compiler::new().compile(&doc, &registry);

    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "fixture '{fixture_name}' should have no errors, got {:?}",
        errors
    );

    let generated = result.rust_output.expect("compile produced output");

    // Write the generated file into the gencheck crate's src/ directory.
    let out_path = gencheck_src().join("generated.rs");
    std::fs::write(&out_path, &generated).expect("write generated code");

    // Run `cargo check --features <features>` on the gencheck crate.
    let status = Command::new("cargo")
        .args(["check", "--quiet", "--features", features])
        .current_dir(gencheck_src().parent().unwrap())
        .status()
        .expect("cargo check runs");

    // Clean up the generated artifact so the workspace stays pristine.
    let _ = std::fs::remove_file(&out_path);

    assert!(
        status.success(),
        "fixture '{fixture_name}' generated code failed `cargo check`. Dump:\n{}",
        generated
    );
}

#[test]
fn generated_code_compiles() {
    compile_and_check(FIXTURE, "gen-check");
    compile_and_check(IN_MATCHES_FIXTURE, "gen-check");
    compile_and_check(INLINE_MESSAGES_FIXTURE, "gen-check-inline");
    compile_and_check(ECEL_EXTENDED_FIXTURE, "gen-check-inline");
}
