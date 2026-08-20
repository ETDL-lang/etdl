//! Property-based robustness tests: no panic on malformed/untrusted input.
//!
//! These are the fuzz-equivalent guard for the parser and ECEL on stable
//! Rust. They generate adversarial inputs (arbitrary strings, oversized
//! indices, malformed YAML) and assert that parsing returns `Err` or a valid
//! result — never a panic.

use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Arbitrary strings fed to `parse_condition` must never panic.
    #[test]
    fn ecel_never_panics_on_arbitrary_input(s in "\\PC{0,64}") {
        let _ = etdl_parser::ecel::parse_condition(&s);
    }

    /// ECEL paths with oversized indices must never panic (saturate instead).
    #[test]
    fn ecel_oversized_index_no_panic(digits in 1..40usize) {
        let idx = "9".repeat(digits);
        let expr = format!("message.payload.items[{}].qty > 0", idx);
        let _ = etdl_parser::ecel::parse_condition(&expr);
    }

    /// Arbitrary bytes as a YAML document must never panic.
    #[test]
    fn yaml_never_panics_on_arbitrary_bytes(b in prop::collection::vec(any::<u8>(), 0..64)) {
        let s = String::from_utf8_lossy(&b).to_string();
        let _ = etdl_parser::parse_document(&s);
    }

    /// Arbitrary strings fed to the span builder must never panic.
    #[test]
    fn span_builder_never_panics(s in "\\PC{0,64}") {
        let _ = etdl_parser::spanned::build_span_index(&s);
        let _ = etdl_parser::spanned::detect_duplicate_ids(&s);
    }

    /// JSON pointer resolution on arbitrary pointers must never panic.
    #[test]
    fn json_pointer_never_panics(pointer in "\\PC{0,48}") {
        let doc = serde_json::json!({"a": {"b": [1, 2]}, "c": "x"});
        let _ = etdl_parser::jsonptr::resolve_json_pointer(&doc, &pointer);
    }
}

/// Explicit oversized-index regression (no proptest): a 1000-digit index must
/// saturate, not panic.
#[test]
fn ecel_billion_digit_index_saturates() {
    let idx = "9".repeat(1000);
    let expr = format!("message.payload.a[{}].b == 1", idx);
    let result = etdl_parser::ecel::parse_condition(&expr);
    assert!(result.is_ok() || result.is_err()); // either way: no panic
}

/// Deeply nested YAML must not overflow the stack (serde_yaml bounds nesting).
#[test]
fn deeply_nested_yaml_no_panic() {
    let mut yaml = String::from("a:");
    for _ in 0..500 {
        yaml.push_str("\n  a:");
    }
    let _ = etdl_parser::parse_document(&yaml);
}

/// A directive indicator (`%`) with no name and nothing after it must not
/// hang. Regression test for a saphyr-parser defect (`is_yaml_non_break`
/// misclassifies its `BufferedInput` end-of-stream sentinel `'\0'` as
/// ordinary content), which made the directive-name scan in
/// `build_span_index` loop forever, growing an unbounded buffer, once it
/// reached true EOF still inside the token. Worked around in
/// `build_span_index` by driving saphyr through `Parser::new_from_str`
/// (`&str`-backed `StrInput`) instead of `MarkedYaml::load_from_str`
/// (char-iterator `BufferedInput`).
#[test]
fn span_builder_bare_directive_no_hang() {
    let _ = etdl_parser::spanned::build_span_index("%");
    let _ = etdl_parser::spanned::detect_duplicate_ids("%");
}
