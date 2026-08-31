//! Gate combinators beyond simple independent AND/OR
//! ([`crate::probability::independent_and_n`]/[`crate::probability::independent_or_n`]):
//! NOT, XOR, generalized k-of-n voting (heterogeneous per-input
//! probabilities), INHIBIT, and PRIORITY_AND.
//!
//! Extracted from `etdl-compiler::fault_tree`'s compile-time gate
//! resolution so there is exactly one implementation of "how each gate
//! type combines probabilities" — shared by compile-time fault-tree
//! resolution and any runtime live-recombination that needs the same math,
//! never two copies that could drift. `etdl-compiler` still owns the
//! `GateType` → function dispatch (this crate has no `etdl-parser`
//! dependency, by the same "zero dependency on any reliability/language
//! crate" rule documented in this crate's module doc).

use crate::probability::{Probability, ProbabilityError};

/// A problem combining probabilities through a gate. Never silently
/// repaired.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum GateError {
    #[error("NOT gate requires exactly 1 input, got {0}")]
    NotWrongArity(usize),
    #[error("XOR gate requires exactly 2 inputs, got {0}")]
    XorWrongArity(usize),
    #[error("INHIBIT gate requires exactly 2 inputs, got {0}")]
    InhibitWrongArity(usize),
    #[error("PRIORITY_AND gate requires at least 2 inputs, got {0}")]
    PriorityAndWrongArity(usize),
    #[error("k-of-n gate: k={k} out of range [1, {n}]")]
    KOutOfRange { k: usize, n: usize },
    #[error(transparent)]
    Probability(#[from] ProbabilityError),
}

/// `P(not A) = 1 - P(A)`. A thin, arity-checked wrapper over
/// [`crate::probability::complement`] for gate-dispatch call sites that
/// want the same `&[Probability]` shape as every other gate function here.
pub fn not(inputs: &[Probability]) -> Result<Probability, GateError> {
    if inputs.len() != 1 {
        return Err(GateError::NotWrongArity(inputs.len()));
    }
    Ok(crate::probability::complement(inputs[0]))
}

/// `P(A xor B) = P(A) + P(B) - 2*P(A)*P(B)`, assuming independence.
pub fn xor(inputs: &[Probability]) -> Result<Probability, GateError> {
    if inputs.len() != 2 {
        return Err(GateError::XorWrongArity(inputs.len()));
    }
    let (a, b) = (inputs[0].value(), inputs[1].value());
    Ok(Probability::new(a + b - 2.0 * a * b)?)
}

/// `P(A and B) = P(A) * P(B)` — the INHIBIT gate's own two-input
/// formula (a conditioning event AND'd with a triggering event), kept
/// distinct from [`crate::probability::independent_and_n`] because
/// INHIBIT is defined as exactly-two-input by construction (fault tree
/// semantics: one triggering input, one inhibit condition), not an
/// arbitrary-arity AND.
pub fn inhibit(inputs: &[Probability]) -> Result<Probability, GateError> {
    if inputs.len() != 2 {
        return Err(GateError::InhibitWrongArity(inputs.len()));
    }
    Ok(Probability::new(inputs[0].value() * inputs[1].value())?)
}

/// The probability that at least `k` of `inputs.len()` independent events
/// occur — a k-of-n / VOTING gate, supporting **heterogeneous** per-input
/// probabilities (unlike a [`crate::distribution::Binomial`], which
/// assumes identical `p` per trial). Uses a fast identical-`p` path
/// (binomial sum) when every input matches, and a general Poisson-binomial
/// polynomial-convolution otherwise.
pub fn k_of_n(inputs: &[Probability], k: usize) -> Result<Probability, GateError> {
    let n = inputs.len();
    if k < 1 || k > n {
        return Err(GateError::KOutOfRange { k, n });
    }

    let first = inputs[0].value();
    if inputs.iter().all(|p| (p.value() - first).abs() < 1e-10) {
        let p = first;
        let mut total = 0.0;
        for j in k..=n {
            total += binomial_coeff(n, j) * p.powi(j as i32) * (1.0 - p).powi((n - j) as i32);
        }
        Ok(Probability::new(total.clamp(0.0, 1.0))?)
    } else {
        let mut poly = vec![1.0];
        for input in inputs {
            let p = input.value();
            poly = multiply_polynomial(&poly, &[1.0 - p, p]);
        }
        let mut total = 0.0;
        for j in k..=n {
            if j < poly.len() {
                total += poly[j];
            }
        }
        Ok(Probability::new(total.clamp(0.0, 1.0))?)
    }
}

/// The probability that `n` independent events all occur **in the listed
/// order**, assuming every ordering of occurrence is equally likely:
/// `P = (product of p_i) / n!`. Computed in log space to avoid factorial
/// overflow for large `n`.
pub fn priority_and(inputs: &[Probability]) -> Result<Probability, GateError> {
    let n = inputs.len();
    if n < 2 {
        return Err(GateError::PriorityAndWrongArity(n));
    }
    let mut log_p = 0.0;
    for input in inputs {
        let p = input.value();
        if p <= 0.0 {
            return Ok(Probability::IMPOSSIBLE);
        }
        log_p += p.ln();
    }
    log_p -= ln_factorial(n);
    Ok(Probability::new(log_p.exp().clamp(0.0, 1.0))?)
}

fn ln_factorial(n: usize) -> f64 {
    if n <= 170 {
        let mut f = 1.0f64;
        for i in 2..=n {
            f *= i as f64;
        }
        f.ln()
    } else {
        ln_gamma((n as f64) + 1.0)
    }
}

/// Natural logarithm of the gamma function (Lanczos approximation), giving
/// ln(n!) for integer n+1. Used only for n > 170 where direct products
/// would overflow; ~1e-12 relative accuracy is ample at those magnitudes.
fn ln_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const P: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.5203681218851,
        -1259.1392167224028,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507343278686905,
        -0.13857109526572012,
        9.984_369_578_019_572e-6,
        1.5056327351493116e-7,
    ];

    if x < 0.5 {
        return std::f64::consts::PI.ln()
            - (std::f64::consts::PI * x).sin().ln()
            - ln_gamma(1.0 - x);
    }
    let x_minus_one = x - 1.0;
    let mut a = P[0];
    let t = x_minus_one + G + 0.5;
    for (i, p) in P.iter().enumerate().skip(1) {
        a += p / (x_minus_one + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x_minus_one + 0.5) * t.ln() - t + a.ln()
}

fn binomial_coeff(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    // ln(C(n,k)) = ln(n!) - ln(k!) - ln((n-k)!)
    let ln = ln_factorial(n) - ln_factorial(k) - ln_factorial(n - k);
    ln.exp().round()
}

fn multiply_polynomial(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut result = vec![0.0; a.len() + b.len() - 1];
    for (i, &coeff_a) in a.iter().enumerate() {
        for (j, &coeff_b) in b.iter().enumerate() {
            result[i + j] += coeff_a * coeff_b;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: f64) -> Probability {
        Probability::new(v).unwrap()
    }

    #[test]
    fn not_matches_complement() {
        assert_eq!(not(&[p(0.3)]).unwrap().value(), 0.7);
    }

    #[test]
    fn not_wrong_arity_is_rejected() {
        assert_eq!(not(&[p(0.3), p(0.4)]), Err(GateError::NotWrongArity(2)));
    }

    #[test]
    fn xor_known_value() {
        // 0.2 + 0.3 - 2*0.2*0.3 = 0.38
        let r = xor(&[p(0.2), p(0.3)]).unwrap();
        assert!((r.value() - 0.38).abs() < 1e-12);
    }

    #[test]
    fn inhibit_known_value() {
        let r = inhibit(&[p(0.1), p(0.5)]).unwrap();
        assert!((r.value() - 0.05).abs() < 1e-12);
    }

    #[test]
    fn inhibit_wrong_arity_is_rejected() {
        assert_eq!(inhibit(&[p(0.1)]), Err(GateError::InhibitWrongArity(1)));
    }

    #[test]
    fn k_of_n_identical_p_matches_binomial_sum() {
        // 3 inputs, p=0.5 each, k=2: C(3,2)*0.5^2*0.5 + C(3,3)*0.5^3 = 0.375+0.125=0.5
        let r = k_of_n(&[p(0.5), p(0.5), p(0.5)], 2).unwrap();
        assert!((r.value() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn k_of_n_heterogeneous_p() {
        let a = 0.1;
        let b = 0.2;
        let c = 0.3;
        // P(at least 2 of 3), heterogeneous. Manual Poisson-binomial:
        // P(exactly 2) = ab(1-c)+a(1-b)c+(1-a)bc; P(exactly 3)=abc
        let exactly2 = a * b * (1.0 - c) + a * (1.0 - b) * c + (1.0 - a) * b * c;
        let exactly3 = a * b * c;
        let expected = exactly2 + exactly3;
        let r = k_of_n(&[p(a), p(b), p(c)], 2).unwrap();
        assert!((r.value() - expected).abs() < 1e-9);
    }

    #[test]
    fn k_of_n_k_out_of_range_is_rejected() {
        assert_eq!(
            k_of_n(&[p(0.5), p(0.5)], 3),
            Err(GateError::KOutOfRange { k: 3, n: 2 })
        );
        assert_eq!(
            k_of_n(&[p(0.5), p(0.5)], 0),
            Err(GateError::KOutOfRange { k: 0, n: 2 })
        );
    }

    #[test]
    fn priority_and_two_events_matches_hand_derivation() {
        // Reference from fault_tree.rs's own existing test.
        let r = priority_and(&[p(0.2), p(0.3)]).unwrap();
        assert!((r.value() - (0.2 * 0.3 / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn priority_and_three_events() {
        let r = priority_and(&[p(0.2), p(0.3), p(0.4)]).unwrap();
        assert!((r.value() - (0.2 * 0.3 * 0.4 / 6.0)).abs() < 1e-9);
    }

    #[test]
    fn priority_and_wrong_arity_is_rejected() {
        assert_eq!(
            priority_and(&[p(0.1)]),
            Err(GateError::PriorityAndWrongArity(1))
        );
    }

    #[test]
    fn priority_and_zero_probability_input_short_circuits_to_impossible() {
        let r = priority_and(&[p(0.0), p(0.5)]).unwrap();
        assert_eq!(r, Probability::IMPOSSIBLE);
    }

    #[test]
    fn binomial_coeff_does_not_overflow_for_large_n() {
        // n=200 exceeds the direct-product ln_factorial threshold (170),
        // exercising the ln_gamma fallback path.
        let inputs: Vec<Probability> = (0..200).map(|_| p(0.01)).collect();
        let r = k_of_n(&inputs, 5).unwrap();
        assert!(r.value() >= 0.0 && r.value() <= 1.0);
    }

    #[test]
    fn binomial_coeff_known_value_does_not_overflow_usize() {
        // C(70, 35) overflows usize; the f64/ln-space implementation must
        // still work. Exact value is 112186277816656760000 ≈ 1.121862778e20.
        let c = binomial_coeff(70, 35);
        assert!(c > 0.0);
        assert!(
            (c - 1.121862778e20).abs() / 1.121862778e20 < 1e-6,
            "got {c}"
        );
    }

    #[test]
    fn ln_gamma_consistency() {
        // ln(6!) = ln(720) ≈ 6.5792
        assert!((ln_factorial(6) - 720.0f64.ln()).abs() < 1e-9);
        // Compare log-space binomial against a direct small-n result.
        let small = binomial_coeff(10, 5);
        assert!((small - 252.0).abs() < 1e-6);
    }
}
