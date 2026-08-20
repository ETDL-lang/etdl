//! An independent mathematical reference oracle.
//!
//! **This module must never call into `etdl-probability-core`,
//! `etdl-reliability-core`, or `etdl-reliability`'s own formulas.** Every
//! function here is coded directly from the mathematical definition, using
//! only `std`'s floating-point primitives (`exp`, `ln`, `powf`, ...) — the
//! platform's floating-point primitives are not the thing under test; the
//! crates' domain formulas (survival/hazard functions, regularized
//! incomplete beta, log-gamma) are. Where an implementation crate uses one
//! algorithm (e.g. the regularized incomplete beta function for a binomial
//! tail probability), this module deliberately uses a *different* one
//! (direct PMF summation) computing the *same* normative quantity — so a
//! bug in either algorithm's translation of the math is likely to produce
//! a disagreement, which is the entire point of an independent oracle (see
//! `docs/reference/conformance-framework.md`, "No self-certification
//! loop").
//!
//! This is intentionally small: a reference layer, not a second compiler
//! (per this task's own instruction not to duplicate the implementation).

/// `S(t) = exp(-lambda * t)` for the constant-hazard (exponential) model.
pub fn exponential_survival(lambda: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return 1.0;
    }
    (-lambda * t).exp()
}

pub fn exponential_failure_probability(lambda: f64, t: f64) -> f64 {
    1.0 - exponential_survival(lambda, t)
}

/// `h(t) = lambda`, constant.
pub fn exponential_hazard(lambda: f64, _t: f64) -> f64 {
    lambda
}

/// `H(t) = lambda * t`.
pub fn exponential_cumulative_hazard(lambda: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    lambda * t
}

/// `S(t) = exp(-(t/scale)^shape)` for the two-parameter Weibull model.
pub fn weibull_survival(shape: f64, scale: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return 1.0;
    }
    (-(t / scale).powf(shape)).exp()
}

pub fn weibull_failure_probability(shape: f64, scale: f64, t: f64) -> f64 {
    1.0 - weibull_survival(shape, scale, t)
}

/// `h(t) = (shape/scale) * (t/scale)^(shape-1)`.
pub fn weibull_hazard(shape: f64, scale: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return if shape < 1.0 {
            f64::INFINITY
        } else if shape > 1.0 {
            0.0
        } else {
            1.0 / scale
        };
    }
    (shape / scale) * (t / scale).powf(shape - 1.0)
}

/// `H(t) = (t/scale)^shape`.
pub fn weibull_cumulative_hazard(shape: f64, scale: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    (t / scale).powf(shape)
}

/// `C(n, k)`, the binomial coefficient, via a multiplicative recurrence
/// (no factorials, no gamma function — deliberately a different code path
/// from any log-gamma-based implementation). Exact for the small `n` this
/// oracle is used with (documented per-vector; see
/// `docs/reference/conformance-framework.md`'s tolerance policy).
pub fn binomial_coefficient(n: u64, k: u64) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut result = 1.0f64;
    for i in 0..k {
        result *= (n - i) as f64;
        result /= (i + 1) as f64;
    }
    result
}

/// `P(X = k)` for `X ~ Binomial(n, p)`, by direct evaluation of the PMF
/// formula — independent of any regularized-incomplete-beta-based CDF
/// implementation.
pub fn binomial_pmf(n: u64, k: u64, p: f64) -> f64 {
    if !(0.0..=1.0).contains(&p) || k > n {
        return 0.0;
    }
    binomial_coefficient(n, k) * p.powi(k as i32) * (1.0 - p).powi((n - k) as i32)
}

/// `P(X <= k)` for `X ~ Binomial(n, p)`, by direct PMF summation.
pub fn binomial_cdf(n: u64, k: u64, p: f64) -> f64 {
    (0..=k.min(n)).map(|i| binomial_pmf(n, i, p)).sum()
}

/// `P(X >= k)` for `X ~ Binomial(n, p)`, by direct PMF summation
/// (`1 - P(X <= k-1)`, computed by summing the complementary range rather
/// than subtracting, to avoid the same cancellation the exponential
/// distribution's own `cdf` avoids via `expm1` elsewhere in this
/// workspace).
pub fn binomial_survival_ge(n: u64, k: u64, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    (k..=n).map(|i| binomial_pmf(n, i, p)).sum()
}

/// The exact two-sided binomial test p-value, standard "doubling" method:
/// `min(2 * min(P(X<=k), P(X>=k)), 1)`. This is the same *definition*
/// `etdl-reliability::calibration::binomial_test_two_sided` documents
/// itself as computing (via the regularized incomplete beta function
/// instead of direct summation) — this oracle exists to catch an
/// implementation bug in that translation, not to propose a different
/// statistical test.
pub fn binomial_test_two_sided(k: u64, n: u64, p0: f64) -> f64 {
    if n == 0 {
        return f64::NAN;
    }
    let p0 = p0.clamp(0.0, 1.0);
    if p0 <= 0.0 {
        return if k == 0 { 1.0 } else { 0.0 };
    }
    if p0 >= 1.0 {
        return if k == n { 1.0 } else { 0.0 };
    }
    let p_le = binomial_cdf(n, k, p0);
    let p_ge = binomial_survival_ge(n, k, p0);
    (2.0 * p_le.min(p_ge)).min(1.0)
}

/// `complement(p) = 1 - p`.
pub fn complement(p: f64) -> f64 {
    1.0 - p
}

/// `P(A and B) = P(A) * P(B)` under independence.
pub fn independent_and(a: f64, b: f64) -> f64 {
    a * b
}

/// `P(A or B) = 1 - (1-P(A))(1-P(B))` under independence.
pub fn independent_or(a: f64, b: f64) -> f64 {
    1.0 - (1.0 - a) * (1.0 - b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_reference_reproduces_the_textbook_identity() {
        // lambda=0.001/hr, t=100h: R(t) = exp(-0.1).
        let expected = (-0.1f64).exp();
        assert!((exponential_survival(0.001, 100.0) - expected).abs() < 1e-15);
    }

    #[test]
    fn binomial_coefficient_known_values() {
        assert_eq!(binomial_coefficient(5, 0), 1.0);
        assert_eq!(binomial_coefficient(5, 5), 1.0);
        assert_eq!(binomial_coefficient(5, 2), 10.0);
        assert_eq!(binomial_coefficient(10, 3), 120.0);
    }

    #[test]
    fn binomial_pmf_sums_to_one() {
        let n = 20;
        let p = 0.37;
        let total: f64 = (0..=n).map(|k| binomial_pmf(n, k, p)).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn binomial_cdf_plus_survival_ge_of_next_is_one() {
        let n = 15;
        let p = 0.2;
        for k in 0..=n {
            let cdf = binomial_cdf(n, k, p);
            let sf_ge = if k < n {
                binomial_survival_ge(n, k + 1, p)
            } else {
                0.0
            };
            assert!((cdf + sf_ge - 1.0).abs() < 1e-9, "k={k}");
        }
    }

    #[test]
    fn two_sided_test_at_the_null_is_close_to_one() {
        // k/n exactly equal to p0 should never be flagged as significant.
        let p = binomial_test_two_sided(10, 20, 0.5);
        assert!(p > 0.5, "expected a high p-value at the null, got {p}");
    }

    #[test]
    fn two_sided_test_far_from_null_is_small() {
        let p = binomial_test_two_sided(18, 20, 0.1);
        assert!(
            p < 0.01,
            "expected a small p-value far from the null, got {p}"
        );
    }
}
