//! Runtime helpers for generated ECEL conditions.
//!
//! ECEL's `in` and `matches` operators are not native Rust syntax, so generated
//! code lowers them to calls in this module. `in` becomes a linear-time
//! membership check; `matches` becomes a regular-expression match. The regex
//! engine is `regex` (a RE2-style linear-time engine), satisfying ETDL §6.5's
//! mandate that `matches` be a safe, linear-time RE2-compatible match.

use regex::Regex;

/// Returns true when `value` equals any element of `items`.
///
/// Used for ECEL `x in [a, b, c]` and `x in <array-typed path>`. Generic
/// over *two* types (`T: PartialEq<U>`, not `T: PartialEq<T>`) rather than
/// requiring `items` and `value` to share one type: a string-array literal
/// (`vec!["A", "B"]`, elements `&str`) compared against a generated
/// payload field access (typically an owned `String`) are two different
/// Rust types even though they're the same ECEL type — `&str: PartialEq<
/// String>` is a real, symmetric impl in `std` (the same one that makes
/// `"foo" == some_string` ergonomic), so this compiles for that case
/// without codegen having to paper over the type difference itself. Same-
/// type calls (`contains(&[1, 2, 3], &2)`) still work unchanged, since
/// `T: PartialEq<T>` is the trivial case of `T: PartialEq<U>` with `U = T`.
pub fn contains<T, U>(items: &[T], value: &U) -> bool
where
    T: PartialEq<U>,
{
    items.iter().any(|item| item == value)
}

/// Returns true when `value` matches the RE2-compatible pattern `pattern`.
///
/// Used for ECEL `x matches "pattern"`. The pattern is compiled per call;
/// a caller holding many evaluations should pre-compile with [`Regex`].
pub fn matches(value: &str, pattern: &str) -> bool {
    Regex::new(pattern)
        .map(|re| re.is_match(value))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_finds_element() {
        assert!(contains(&[1, 2, 3], &2));
        assert!(!contains(&[1, 2, 3], &4));
        assert!(!contains::<i32, i32>(&[], &1));
    }

    /// Regression test for the exact shape generated code produces for
    /// ECEL `x in [a, b, c]`: a `&str` array literal (`vec!["A", "B"]`)
    /// checked against an owned `String` field access. Before `contains`
    /// took two independent type parameters (`T: PartialEq<U>` instead of
    /// `T: PartialEq<T>`), this was a compile error (`expected &str, found
    /// &String`) for every generated `in` condition over a string field —
    /// the compile-check harness never caught it because its fixture only
    /// exercised a numeric `>` comparison.
    #[test]
    fn contains_compares_str_array_against_owned_string_field() {
        let items: Vec<&str> = vec!["A", "B"];
        let value: String = "A".to_string();
        assert!(contains(&items, &value));
        let other: String = "C".to_string();
        assert!(!contains(&items, &other));
    }

    #[test]
    fn matches_re2() {
        assert!(matches("ORD-12345678", r"^ORD-[0-9]{8}$"));
        assert!(!matches("order-12345678", r"^ORD-[0-9]{8}$"));
    }

    #[test]
    fn invalid_pattern_is_false() {
        assert!(!matches("anything", "[unclosed"));
    }
}
