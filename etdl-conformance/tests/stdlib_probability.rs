//! LIB-PROB-* vectors: `std.probability` (native layer `etdl-probability-
//! core`) conformance. Covers task §13 (probability invariants) and the
//! `std.probability` API surface documented in
//! `docs/reference/standard-probability-library.md`.
//!
//! Every numerical assertion here compares the implementation's output to
//! either [`etdl_conformance::reference`] (independently coded) or a
//! hand-derived textbook constant — never to a second call into the same
//! formula (see the crate's "no self-certification loop" doc).

use etdl_conformance::reference;
use etdl_conformance::vector::{ConformanceVector, Level};
use etdl_probability_core::distribution::{Beta, Binomial, Normal};
use etdl_probability_core::{
    bayes, complement, conditional, independent_and, independent_or, mutually_exclusive_or,
    Probability,
};

const NUMERIC_TOLERANCE: f64 = 1e-9;

// ---------------------------------------------------------------------
// §13 Probability invariants
// ---------------------------------------------------------------------

#[test]
fn lib_prob_001_probability_is_bounded_zero_to_one_by_construction() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-001",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#types",
        "0 <= P(E) <= 1; Probability::new rejects (never clamps) out-of-range or non-finite values",
    );
    assert!(Probability::new(0.0).is_ok(), "{}", VECTOR.id);
    assert!(Probability::new(1.0).is_ok(), "{}", VECTOR.id);
    assert!(Probability::new(0.5).is_ok(), "{}", VECTOR.id);
    assert!(
        Probability::new(-0.0001).is_err(),
        "{}: must reject < 0",
        VECTOR.id
    );
    assert!(
        Probability::new(1.0001).is_err(),
        "{}: must reject > 1",
        VECTOR.id
    );
    assert!(
        Probability::new(f64::NAN).is_err(),
        "{}: must reject NaN",
        VECTOR.id
    );
    assert!(
        Probability::new(f64::INFINITY).is_err(),
        "{}: must reject infinity",
        VECTOR.id
    );
}

#[test]
fn lib_prob_002_binomial_cdf_is_non_decreasing() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-002",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#distributions",
        "CDF is non-decreasing",
    );
    let binom = Binomial::new(25, Probability::new(0.33).unwrap()).unwrap();
    let mut previous = 0.0;
    for k in 0..=25 {
        let current = binom.cdf(k).value();
        assert!(
            current + 1e-12 >= previous,
            "{}: cdf must be non-decreasing at k={k} ({previous} -> {current})",
            VECTOR.id
        );
        previous = current;
    }
    assert!(
        (binom.cdf(25).value() - 1.0).abs() < NUMERIC_TOLERANCE,
        "{}: cdf must reach 1 at k=n",
        VECTOR.id
    );
}

#[test]
fn lib_prob_003_binomial_cdf_matches_independent_reference() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-003",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#distributions",
        "Binomial::cdf (regularized-incomplete-beta-based) must agree with an \
         independently-coded direct-summation binomial CDF",
    );
    for (n, p) in [(10u64, 0.1), (25, 0.5), (40, 0.85), (12, 0.02)] {
        let binom = Binomial::new(n, Probability::new(p).unwrap()).unwrap();
        for k in 0..=n {
            let implementation = binom.cdf(k).value();
            let oracle = reference::binomial_cdf(n, k, p);
            assert!(
                (implementation - oracle).abs() < 1e-6,
                "{}: n={n} p={p} k={k}: implementation={implementation} oracle={oracle}",
                VECTOR.id
            );
        }
    }
}

#[test]
fn lib_prob_004_beta_cdf_is_non_decreasing_and_quantile_inverts_it() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-004",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#distributions",
        "CDF is non-decreasing; quantile is its inverse",
    );
    let beta = Beta::new(3.0, 7.0).unwrap();
    let mut previous = 0.0;
    for i in 0..=20 {
        let x = i as f64 / 20.0;
        let current = beta.cdf(x);
        assert!(current + 1e-12 >= previous, "{}: x={x}", VECTOR.id);
        previous = current;
    }
    for q in [0.1, 0.25, 0.5, 0.75, 0.9] {
        let x = beta.quantile(q);
        assert!(
            (beta.cdf(x) - q).abs() < 1e-6,
            "{}: quantile({q})={x} but cdf(x)={}",
            VECTOR.id,
            beta.cdf(x)
        );
    }
}

#[test]
fn lib_prob_005_normal_cdf_matches_the_textbook_95_percent_constant() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-005",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#distributions",
        "the standard normal CDF at the well-known z=1.959964 two-sided 95% \
         quantile must equal 0.975 (a textbook constant, not implementation-derived)",
    );
    let standard_normal = Normal::new(0.0, 1.0).unwrap();
    // 1.9599639845400545 is the textbook two-sided-95% critical value,
    // reproduced here from statistical tables independent of this
    // implementation's own `normal_quantile`.
    let z = 1.959_963_984_540_054_5;
    assert!(
        (standard_normal.cdf(z) - 0.975).abs() < 1e-6,
        "{}: cdf({z}) = {}, expected 0.975",
        VECTOR.id,
        standard_normal.cdf(z)
    );
}

// ---------------------------------------------------------------------
// Composition operations vs. the independent reference oracle
// ---------------------------------------------------------------------

#[test]
fn lib_prob_006_composition_ops_match_the_independent_reference() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-006",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#composition",
        "complement/independent_and/independent_or must equal the closed-form reference formulas",
    );
    for (a, b) in [(0.1, 0.2), (0.5, 0.5), (0.999, 0.001), (0.0, 1.0)] {
        let pa = Probability::new(a).unwrap();
        let pb = Probability::new(b).unwrap();

        assert!(
            (complement(pa).value() - reference::complement(a)).abs() < NUMERIC_TOLERANCE,
            "{}: complement({a})",
            VECTOR.id
        );
        assert!(
            (independent_and(pa, pb).value() - reference::independent_and(a, b)).abs()
                < NUMERIC_TOLERANCE,
            "{}: independent_and({a},{b})",
            VECTOR.id
        );
        assert!(
            (independent_or(pa, pb).value() - reference::independent_or(a, b)).abs()
                < NUMERIC_TOLERANCE,
            "{}: independent_or({a},{b})",
            VECTOR.id
        );
    }
}

#[test]
fn lib_prob_007_de_morgan_identity_holds_for_independent_composition() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-007",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#composition",
        "P(A or B) = complement(P(not A and not B)) under independence (De Morgan), \
         a boolean identity the composition operators must jointly satisfy",
    );
    // Property test over a small deterministic grid rather than random
    // sampling — the identity is exact and a grid is exhaustive enough to
    // catch a broken operator without introducing nondeterminism.
    for i in 0..=10 {
        for j in 0..=10 {
            let a = i as f64 / 10.0;
            let b = j as f64 / 10.0;
            let pa = Probability::new(a).unwrap();
            let pb = Probability::new(b).unwrap();

            let direct = independent_or(pa, pb).value();
            let via_de_morgan = complement(independent_and(complement(pa), complement(pb))).value();
            assert!(
                (direct - via_de_morgan).abs() < 1e-12,
                "{}: a={a} b={b}: {direct} vs {via_de_morgan}",
                VECTOR.id
            );
        }
    }
}

#[test]
fn lib_prob_008_conditional_and_bayes_are_mutually_consistent() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-008",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#composition",
        "bayes(likelihood, prior, marginal) inverts conditional(joint, marginal) \
         for a self-consistent joint/marginal/prior triple",
    );
    // Construct a genuinely consistent scenario: P(A)=0.3, P(B|A)=0.4,
    // P(B)=0.5 => P(A and B)=0.12, and Bayes must recover P(A|B)=0.24.
    let prior_a = Probability::new(0.3).unwrap();
    let likelihood_b_given_a = Probability::new(0.4).unwrap();
    let marginal_b = Probability::new(0.5).unwrap();
    let joint = Probability::new(0.12).unwrap();

    let posterior = bayes(likelihood_b_given_a, prior_a, marginal_b).unwrap();
    assert!(
        (posterior.value() - 0.24).abs() < 1e-9,
        "{}: bayes result = {}",
        VECTOR.id,
        posterior.value()
    );

    let recovered_conditional = conditional(joint, marginal_b).unwrap();
    assert!(
        (recovered_conditional.value() - posterior.value()).abs() < 1e-9,
        "{}: conditional(joint, marginal_b)={} disagrees with bayes()={}",
        VECTOR.id,
        recovered_conditional.value(),
        posterior.value()
    );
}

#[test]
fn lib_prob_009_mutually_exclusive_or_rejects_impossible_inputs() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "LIB-PROB-009",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#composition",
        "mutually_exclusive_or must reject inputs whose sum exceeds 1 (not silently clamp)",
    );
    let a = Probability::new(0.7).unwrap();
    let b = Probability::new(0.6).unwrap();
    assert!(
        mutually_exclusive_or(a, b).is_err(),
        "{}: 0.7 + 0.6 > 1 must be rejected, not clamped",
        VECTOR.id
    );
}
