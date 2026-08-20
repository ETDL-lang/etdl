//! Performance benchmarks for the ETDL parser and compiler.
//!
//! Run with: `cargo bench --bench etdl_bench`
//!
//! Baselines are documented in `docs/PERFORMANCE.md`. These benches are
//! informational, not a performance claim.

use criterion::{criterion_group, criterion_main, Criterion};

fn parse_fixture() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../etdl-cli/tests/fixtures/order-fulfillment.etdl"
    ))
    .expect("fixture present")
}

fn bench_parse(c: &mut Criterion) {
    let src = parse_fixture();
    c.bench_function("parse_document", |b| {
        b.iter(|| {
            let _ = etdl_parser::parse_document(&src);
        })
    });
}

fn bench_validate(c: &mut Criterion) {
    let src = parse_fixture();
    let doc = etdl_parser::parse_document(&src).expect("parses");
    let registry = etdl_parser::asyncapi::AsyncApiRegistry::new();
    let compiler = etdl_compiler::Compiler::new();
    c.bench_function("validate_document", |b| {
        b.iter(|| {
            let _ = compiler.validate(&doc, &registry);
        })
    });
}

fn bench_compile(c: &mut Criterion) {
    let src = parse_fixture();
    let doc = etdl_parser::parse_document(&src).expect("parses");
    let registry = etdl_parser::asyncapi::AsyncApiRegistry::new();
    let compiler = etdl_compiler::Compiler::new();
    c.bench_function("compile_rust", |b| {
        b.iter(|| {
            let _ = compiler.compile(&doc, &registry);
        })
    });
}

fn bench_ecel(c: &mut Criterion) {
    c.bench_function("parse_ecel_condition", |b| {
        b.iter(|| {
            let _ = etdl_parser::ecel::parse_condition("message.payload.items[*].qty > 0");
        })
    });
}

criterion_group!(
    benches,
    bench_parse,
    bench_validate,
    bench_compile,
    bench_ecel
);
criterion_main!(benches);
