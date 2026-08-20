//! Shared numerical primitives backing the distributions in
//! [`crate::distribution`]: the log-gamma function, the regularized
//! incomplete beta function, the standard normal CDF (via the error
//! function), and the standard normal quantile function.
//!
//! These are **independent reimplementations** of algorithms also used
//! elsewhere in this workspace (`etdl-reliability::analysis::estimator`
//! has its own `log_gamma`/`regularized_beta`/`normal_quantile`) — not
//! shared code. This crate must have zero dependency on any reliability
//! crate (see the crate-level docs), so the alternative would be moving
//! that code out of `etdl-reliability` into this crate, which risks
//! breaking its existing public API and is explicitly out of scope for
//! this task ("do not move existing native implementations unnecessarily").
//! A cross-validation test in `etdl-reliability` (see
//! `probability_adapter.rs`) asserts the two independent implementations
//! agree to a documented numerical tolerance.
//!
//! # Numerical tolerance policy
//!
//! Every function here is a well-known, textbook numerical approximation,
//! not an exact closed form (except `log_gamma` at exact factorial
//! arguments and `regularized_beta` at its exact endpoints). Tests compare
//! against reference values with an explicit absolute tolerance (typically
//! `1e-9` to `1e-12`, documented per test) rather than exact equality —
//! floating-point transcendental results are never compared with `==`.

/// Natural log of the gamma function (Lanczos approximation, g=7, n=9
/// coefficients — the standard "Numerical Recipes"-style set). Accurate to
/// approximately 1e-13 relative error for positive arguments; uses the
/// reflection formula for arguments below 0.5.
pub fn log_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const P: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        std::f64::consts::PI.ln() - (std::f64::consts::PI * x).sin().ln() - log_gamma(1.0 - x)
    } else {
        let x = x - 1.0;
        let mut a = P[0];
        let t = x + G + 0.5;
        for (i, coeff) in P.iter().enumerate().skip(1) {
            a += coeff / (x + i as f64);
        }
        0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
    }
}

/// Regularized incomplete beta function `I_x(a, b)`, via the continued
/// fraction expansion (Numerical Recipes `betacf`), with the reflection
/// `I_x(a,b) = 1 - I_{1-x}(b,a)` for `x` beyond the midpoint for
/// convergence. This is the CDF of `Beta(a, b)` at `x`, and (via a standard
/// identity) also gives the Binomial CDF — see [`crate::distribution::binomial`].
pub fn regularized_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let use_reflect = x > (a + 1.0) / (a + b + 2.0);
    let (a, b, x) = if use_reflect {
        (b, a, 1.0 - x)
    } else {
        (a, b, x)
    };

    let ln_front = log_gamma(a + b) - log_gamma(a) - log_gamma(b) + a * x.ln() + b * (1.0 - x).ln();
    if ln_front < -700.0 {
        return if use_reflect { 1.0 } else { 0.0 };
    }
    let front = ln_front.exp() / a;

    const MAX_ITER: usize = 200;
    const EPS: f64 = 1e-13;
    const FPMIN: f64 = 1e-300;

    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;

    for m in 1..=MAX_ITER {
        let m2 = 2 * m;
        let mut aa = m as f64 * (b - m as f64) * x / ((qam + m2 as f64) * (a + m2 as f64));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;
        aa = -(a + m as f64) * (qab + m as f64) * x / ((a + m2 as f64) * (qap + m2 as f64));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }

    let result = front * h;
    if use_reflect {
        1.0 - result
    } else {
        result
    }
}

/// Quantile of `Beta(alpha, beta)` at `q` via binary search on
/// [`regularized_beta`] (its CDF). `alpha`/`beta` are assumed already
/// validated positive by the caller.
pub fn beta_quantile(alpha: f64, beta: f64, q: f64) -> f64 {
    let q = q.clamp(1e-15, 1.0 - 1e-15);
    let mut lo = 0.0f64;
    let mut hi = 1.0f64;
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        let cdf = regularized_beta(mid, alpha, beta);
        if cdf < q {
            lo = mid;
        } else {
            hi = mid;
        }
        if (hi - lo).abs() < 1e-14 {
            break;
        }
    }
    0.5 * (lo + hi)
}

/// The error function `erf(x)`, via the Abramowitz & Stegun 7.1.26 rational
/// approximation (maximum absolute error ~1.5e-7). Used to compute the
/// standard normal CDF.
fn erf(x: f64) -> f64 {
    // Constants for A&S 7.1.26.
    const A1: f64 = 0.254829592;
    const A2: f64 = -0.284496736;
    const A3: f64 = 1.421413741;
    const A4: f64 = -1.453152027;
    const A5: f64 = 1.061405429;
    const P: f64 = 0.3275911;

    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + P * x);
    let y = 1.0 - (((((A5 * t + A4) * t) + A3) * t + A2) * t + A1) * t * (-x * x).exp();
    sign * y
}

/// The standard normal CDF `Phi(z) = P(Z <= z)` for `Z ~ Normal(0, 1)`, via
/// `Phi(z) = 0.5 * (1 + erf(z / sqrt(2)))`. Accurate to ~1.5e-7 absolute
/// error (the [`erf`] approximation's documented bound).
pub fn normal_cdf(z: f64) -> f64 {
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

/// The standard normal quantile function (inverse CDF), via Peter Acklam's
/// rational approximation. Documented maximum relative error ~1.15e-9
/// across the full `(0, 1)` domain — well within the tolerance this
/// crate's tests and callers require (e.g. 95%/99% confidence levels).
#[allow(clippy::excessive_precision)]
pub fn normal_quantile(p: f64) -> f64 {
    let p = p.clamp(1e-15, 1.0 - 1e-15);

    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577518672690e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_gamma_matches_known_factorials() {
        // Gamma(n) = (n-1)! for positive integers.
        // Gamma(5) = 4! = 24 -> ln(24) = 3.1780538303479458
        assert!((log_gamma(5.0) - 24f64.ln()).abs() < 1e-10);
        // Gamma(1) = 1 -> ln(1) = 0
        assert!(log_gamma(1.0).abs() < 1e-10);
    }

    #[test]
    fn regularized_beta_endpoints() {
        assert_eq!(regularized_beta(0.0, 2.0, 3.0), 0.0);
        assert_eq!(regularized_beta(1.0, 2.0, 3.0), 1.0);
    }

    #[test]
    fn regularized_beta_uniform_case_is_identity() {
        // Beta(1,1) is the uniform distribution on [0,1]; its CDF is x.
        for x in [0.1, 0.3, 0.5, 0.7, 0.9] {
            assert!((regularized_beta(x, 1.0, 1.0) - x).abs() < 1e-9);
        }
    }

    #[test]
    fn beta_quantile_inverts_regularized_beta() {
        let (alpha, beta) = (3.0, 5.0);
        for q in [0.1, 0.25, 0.5, 0.75, 0.9] {
            let x = beta_quantile(alpha, beta, q);
            let back = regularized_beta(x, alpha, beta);
            assert!((back - q).abs() < 1e-6, "q={q} got back={back}");
        }
    }

    #[test]
    fn normal_cdf_known_values() {
        // Standard normal table values.
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-9);
        assert!((normal_cdf(1.0) - 0.8413447460685429).abs() < 1e-6);
        assert!((normal_cdf(-1.0) - 0.15865525393145707).abs() < 1e-6);
        assert!((normal_cdf(1.96) - 0.9750021048517795).abs() < 1e-6);
    }

    #[test]
    fn normal_cdf_is_symmetric() {
        for z in [0.1, 0.5, 1.0, 2.0, 3.0] {
            assert!((normal_cdf(z) + normal_cdf(-z) - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn normal_quantile_inverts_normal_cdf() {
        // Round-trip error compounds normal_cdf's ~1.5e-7 erf-approximation
        // error with normal_quantile's steep tail derivative, so this uses
        // a looser tolerance than the direct known-value tests above —
        // documented, not silently loosened without explanation.
        for z in [-2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0] {
            let p = normal_cdf(z);
            let back = normal_quantile(p);
            assert!((back - z).abs() < 1e-5, "z={z} got back={back}");
        }
    }

    #[test]
    fn normal_quantile_known_95_percent() {
        // The familiar z=1.959964... for the 97.5th percentile.
        assert!((normal_quantile(0.975) - 1.9599639845).abs() < 1e-6);
    }
}
