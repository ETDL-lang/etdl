//! Stable candidate identity.
//!
//! Candidate identities are **concept-based**, not line-based, so they survive
//! source movement. The identity scheme follows the ontology convention:
//! `failure.<domain>.<concept>`. A source location is attached separately.

/// Build a stable identity for a discovery candidate.
///
/// `domain` is the classification domain (e.g. `network`, `database`,
/// `runtime`, `concurrency`), `concept` is the specific mechanism (e.g.
/// `unwrap`, `panic`, `timeout`). The result reads like
/// `failure.runtime.unwrap` or `failure.dependency.timeout`.
pub fn candidate_id(domain: &str, concept: &str) -> String {
    format!("failure.{}.{}", sanitize(domain), sanitize(concept))
}

/// Build a stable identity for a candidate attached to a named symbol, e.g.
/// a custom error type `PaymentError` -> `failure.application.payment_error`.
pub fn symbol_candidate_id(domain: &str, symbol: &str) -> String {
    format!("failure.{}.{}", sanitize(domain), sanitize(symbol))
}

/// Keep an identity token filesystem/ontology-safe: lowercase, `_`-join,
/// splitting camelCase boundaries (`PaymentError` -> `payment_error`).
fn sanitize(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut prev_lower = false;
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            let is_upper = c.is_uppercase();
            let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if is_upper && (prev_lower || next_is_lower) && !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = !is_upper;
        } else {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("unknown");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_and_clean() {
        assert_eq!(
            candidate_id("network", "Timeout"),
            "failure.network.timeout"
        );
        assert_eq!(
            candidate_id("runtime", "unwrap()"),
            "failure.runtime.unwrap"
        );
        assert_eq!(
            symbol_candidate_id("application", "PaymentError"),
            "failure.application.payment_error"
        );
    }

    #[test]
    fn sanitize_handles_edges() {
        assert_eq!(sanitize(""), "unknown");
        assert_eq!(sanitize("!!!"), "unknown");
        assert_eq!(sanitize("a..b"), "a_b");
    }
}
