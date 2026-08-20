//! Property-style checks across every distribution: CDF bounds,
//! monotonicity, and limiting behavior. Hand-written over a fixed grid of
//! points (this workspace does not use `proptest` — one of its existing
//! usages, in `etdl-parser`, is a known source of CI hangs, and adding a
//! second dependency on it for this crate is not justified when explicit,
//! deterministic grid checks already give the same correctness guarantee
//! for these smooth, well-behaved functions).

use etdl_probability_core::distribution::{Beta, Binomial, Exponential, Normal};
use etdl_probability_core::Probability;

const GRID: [f64; 9] = [0.0, 0.05, 0.1, 0.25, 0.4, 0.5, 0.75, 0.9, 1.0];

#[test]
fn beta_cdf_is_bounded_and_non_decreasing() {
    let b = Beta::new(2.5, 6.0).unwrap();
    let mut prev = 0.0;
    for &x in &GRID {
        let cdf = b.cdf(x);
        assert!((0.0..=1.0).contains(&cdf), "cdf({x})={cdf} out of [0,1]");
        assert!(cdf >= prev - 1e-12, "cdf not non-decreasing at x={x}");
        prev = cdf;
    }
    assert!((b.cdf(0.0) - 0.0).abs() < 1e-9);
    assert!((b.cdf(1.0) - 1.0).abs() < 1e-9);
}

#[test]
fn beta_pdf_integrates_to_approximately_one() {
    // Coarse numerical integration (trapezoid rule) as a sanity check that
    // the density is normalized -- not a substitute for the exact-formula
    // tests in distribution/beta.rs, just an independent cross-check.
    let b = Beta::new(3.0, 5.0).unwrap();
    let n = 2000;
    let dx = 1.0 / n as f64;
    let mut total = 0.0;
    for i in 0..n {
        let x0 = i as f64 * dx;
        let x1 = (i + 1) as f64 * dx;
        total += 0.5 * (b.pdf(x0.max(1e-9)) + b.pdf(x1.min(1.0 - 1e-9))) * dx;
    }
    assert!((total - 1.0).abs() < 1e-3, "integral={total}");
}

#[test]
fn binomial_cdf_is_bounded_and_non_decreasing() {
    let binom = Binomial::new(50, Probability::new(0.4).unwrap()).unwrap();
    let mut prev = 0.0;
    for k in 0..=50u64 {
        let cdf = binom.cdf(k).value();
        assert!((0.0..=1.0).contains(&cdf), "cdf({k})={cdf} out of [0,1]");
        assert!(cdf >= prev - 1e-12, "cdf not non-decreasing at k={k}");
        prev = cdf;
    }
    assert!((binom.cdf(50).value() - 1.0).abs() < 1e-9);
}

#[test]
fn binomial_pmf_is_bounded_for_every_k() {
    let binom = Binomial::new(30, Probability::new(0.15).unwrap()).unwrap();
    for k in 0..=30u64 {
        let pmf = binom.pmf(k).value();
        assert!((0.0..=1.0).contains(&pmf), "pmf({k})={pmf} out of [0,1]");
    }
}

#[test]
fn exponential_cdf_is_bounded_non_decreasing_and_approaches_one() {
    let e = Exponential::new(0.05).unwrap();
    let mut prev = 0.0;
    for &x in &[0.0, 1.0, 5.0, 10.0, 50.0, 100.0, 1000.0] {
        let cdf = e.cdf(x);
        assert!((0.0..=1.0).contains(&cdf), "cdf({x})={cdf} out of [0,1]");
        assert!(cdf >= prev - 1e-12, "cdf not non-decreasing at x={x}");
        prev = cdf;
    }
    // CDF(x) -> 1 as x -> infinity (approximately, for large finite x).
    assert!(e.cdf(1000.0) > 0.999);
}

#[test]
fn normal_cdf_is_bounded_non_decreasing_and_approaches_bounds() {
    let n = Normal::new(0.0, 1.0).unwrap();
    let mut prev = 0.0;
    for z in [-5.0, -3.0, -1.0, 0.0, 1.0, 3.0, 5.0] {
        let cdf = n.cdf(z);
        assert!((0.0..=1.0).contains(&cdf), "cdf({z})={cdf} out of [0,1]");
        assert!(cdf >= prev - 1e-12, "cdf not non-decreasing at z={z}");
        prev = cdf;
    }
    // CDF(-large) -> 0, CDF(+large) -> 1.
    assert!(n.cdf(-5.0) < 1e-5, "cdf(-5) = {}", n.cdf(-5.0));
    assert!(n.cdf(5.0) > 1.0 - 1e-5, "cdf(5) = {}", n.cdf(5.0));
}

#[test]
fn complement_of_every_valid_probability_remains_in_bounds() {
    for &v in &GRID {
        let p = Probability::new(v).unwrap();
        let c = etdl_probability_core::complement(p);
        assert!((0.0..=1.0).contains(&c.value()));
        // Complement is its own inverse.
        let back = etdl_probability_core::complement(c);
        assert!((back.value() - v).abs() < 1e-12);
    }
}

#[test]
fn independent_and_or_remain_in_bounds_across_the_grid() {
    for &a in &GRID {
        for &b in &GRID {
            let pa = Probability::new(a).unwrap();
            let pb = Probability::new(b).unwrap();
            let and = etdl_probability_core::independent_and(pa, pb);
            let or = etdl_probability_core::independent_or(pa, pb);
            assert!((0.0..=1.0).contains(&and.value()), "AND({a},{b})={}", and.value());
            assert!((0.0..=1.0).contains(&or.value()), "OR({a},{b})={}", or.value());
            // AND <= min(a,b) <= max(a,b) <= OR, always.
            assert!(and.value() <= a.min(b) + 1e-12);
            assert!(or.value() >= a.max(b) - 1e-12);
        }
    }
}
