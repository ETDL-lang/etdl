//! Two-service cross-process proof for the Live Reliability Supplement.
//!
//! `live_reliability_codegen_test.rs` already proves the codegen
//! integration (registration, `reliability.in_range` branch selection, the
//! live-aware `record_branch` argument) end-to-end within a single process.
//! What it can't prove is decentralization itself: `etdl_core::live`'s
//! registry is a `static` — two logical "services" sharing one process
//! would also share one registry entry per fault-tree id, which isn't what
//! happens in a real deployment (each service is its own process with its
//! own registry).
//!
//! This test proves the real thing: two independently-compiled fixtures
//! (`live-reliability-producer.etdl`, `live-reliability-consumer.etdl`),
//! each run as its own `cargo run` **subprocess** (so each genuinely gets
//! its own process-local `etdl_core::live::REGISTRY`), handing the
//! producer's `outbound_snapshot` headers to the consumer through a file —
//! standing in for "the next message crossing a real broker," per this
//! feature's documented decentralization model (no shared memory, no
//! central coordinator). The consumer's basic event is declared `inbound`
//! (never locally observed), so if its branch selection still flips to
//! ABNORMAL, that value can only have arrived via the handoff file, i.e.
//! via `apply_inbound` reading the producer's headers — proof the value
//! genuinely crossed the process boundary rather than being computed
//! locally.
//!
//! Kept as a single test function for the same reason
//! `live_reliability_codegen_test.rs` is: every step writes into the
//! shared `gencheck` crate's `src/generated.rs`, and two test *functions*
//! doing that would race under the default parallel test runner.

use std::path::PathBuf;
use std::process::Command;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../etdl-cli/tests/fixtures")
        .join(name)
}

fn gencheck_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("gencheck")
}

fn gencheck_src() -> PathBuf {
    gencheck_dir().join("src")
}

/// Compiles `fixture_name`, writes the generated Rust into the gencheck
/// crate, and returns the generated source (for the caller's own sanity
/// assertions before spending time on `cargo run`).
fn compile_into_gencheck(fixture_name: &str) -> String {
    let path = fixture_path(fixture_name);
    let doc = etdl_parser::parse_document_from_file(&path).expect("fixture parses");
    let base = path.parent().unwrap().to_path_buf();
    let registry = etdl_parser::load_asyncapi_imports(&doc, &base).expect("asyncapi imports load");
    let result = etdl_compiler::Compiler::new().compile(&doc, &registry);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "{fixture_name}: unexpected errors: {errors:?}");
    let generated = result.rust_output.expect("compile produced output");

    std::fs::write(gencheck_src().join("generated.rs"), &generated).expect("write generated code");
    generated
}

#[test]
fn live_reliability_propagates_across_two_independently_compiled_services() {
    let handoff = std::env::temp_dir().join(format!(
        "etdl-live-reliability-handoff-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&handoff);

    // --- Producer service: compile, run, drive observations, hand off. ---
    let producer_src = compile_into_gencheck("live-reliability-producer.etdl");
    assert!(producer_src.contains("LiveFaultTreeBuilder::new"));
    assert!(producer_src.contains(".local_leaf(\"GatewayUnreachable\""));
    assert!(producer_src.contains("publish_with_headers"));

    let producer_status = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--features",
            "gen-check-inline,gen-check-live-reliability-producer",
        ])
        .current_dir(gencheck_dir())
        .env("ETDL_LIVE_RELIABILITY_HANDOFF", &handoff)
        .status()
        .expect("cargo run (producer) runs");
    assert!(
        producer_status.success(),
        "producer service's generated code failed at runtime. Generated source:\n{producer_src}"
    );
    assert!(
        handoff.exists(),
        "producer should have written the handoff file at {handoff:?}"
    );

    // --- Consumer service: compile (separate `cargo run`, separate process
    // and thus separate `etdl_core::live::REGISTRY` from the producer
    // above), read the handoff file, assert branch selection flipped. ---
    let consumer_src = compile_into_gencheck("live-reliability-consumer.etdl");
    assert!(consumer_src.contains(".inbound_leaf(\"GatewayUnreachable\""));
    assert!(consumer_src.contains("etdl_core::live::in_range("));
    assert!(consumer_src.contains("etdl_core::live::apply_inbound("));

    let consumer_status = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--features",
            "gen-check-inline,gen-check-live-reliability-consumer",
        ])
        .current_dir(gencheck_dir())
        .env("ETDL_LIVE_RELIABILITY_HANDOFF", &handoff)
        .status()
        .expect("cargo run (consumer) runs");

    let _ = std::fs::remove_file(&handoff);
    let _ = std::fs::remove_file(gencheck_src().join("generated.rs"));

    assert!(
        consumer_status.success(),
        "consumer service's generated code failed at runtime — expected its branch \
         selection to flip to ABNORMAL purely from the producer's handed-off values \
         (see gencheck's own assertions). Generated source:\n{consumer_src}"
    );
}
