//! [`Probability`]: a validated scalar in `[0, 1]`, and explicit composition
//! operations over it.
//!
//! `Probability` is deliberately the *only* thing this type represents: a
//! plain mathematical probability. It carries no uncertainty, no evidence,
//! no method, and no provenance — those belong to a richer estimate type
//! (the reliability domain's own `ProbabilityEstimate`, in
//! `etdl-reliability-core`, remains authoritative for that; see
//! `docs/reference/standard-probability-library.md`'s "Probability vs.
//! ProbabilityEstimate" section for why this crate does not introduce a
//! second, competing estimate type).

use serde::{Deserialize, Serialize};

/// A probability: `0 <= p <= 1`. Constructed only through [`Probability::new`],
/// which validates and never repairs an invalid input.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Probability(f64);

/// A problem constructing or combining probabilities. Never silently
/// repaired (no clamping `-0.1` to `0`, no clamping `1.2` to `1`).
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum ProbabilityError {
    #[error("probability value {0} is not finite (NaN or infinity)")]
    NotFinite(f64),
    #[error("probability value {0} is outside [0, 1]")]
    OutOfRange(f64),
    #[error("conditional probability is undefined: P(B) = 0")]
    ConditioningOnImpossibleEvent,
    #[error("Bayes' rule is undefined: P(B) = 0")]
    BayesZeroDenominator,
    #[error("mutually-exclusive OR is invalid: P(A) + P(B) = {0} exceeds 1")]
    MutuallyExclusiveSumExceedsOne(f64),
    #[error("independent_and_n/independent_or_n require at least one probability")]
    EmptyCombination,
}

impl Probability {
    /// The only constructor. Rejects NaN/infinite and out-of-range values;
    /// never clamps.
    pub fn new(p: f64) -> Result<Self, ProbabilityError> {
        if !p.is_finite() {
            return Err(ProbabilityError::NotFinite(p));
        }
        if !(0.0..=1.0).contains(&p) {
            return Err(ProbabilityError::OutOfRange(p));
        }
        Ok(Probability(p))
    }

    /// `P = 0`, exactly.
    pub const IMPOSSIBLE: Probability = Probability(0.0);
    /// `P = 1`, exactly.
    pub const CERTAIN: Probability = Probability(1.0);

    /// The underlying scalar value.
    pub fn value(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Probability {
    type Error = ProbabilityError;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Probability::new(value)
    }
}

impl From<Probability> for f64 {
    fn from(p: Probability) -> f64 {
        p.0
    }
}

impl std::fmt::Display for Probability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `P(not A) = 1 - P(A)`. Always well-defined for a valid [`Probability`];
/// the result is itself always in `[0, 1]` (no separate validation needed —
/// this is provable from `A` already being in `[0, 1]`, and is exercised by
/// a boundary test at `P=0`/`P=1`).
pub fn complement(a: Probability) -> Probability {
    Probability(1.0 - a.0)
}

/// `P(A and B) = P(A) * P(B)`, **assuming A and B are independent**. The
/// function name states the assumption; it is never inferred. See
/// [`independent_and_n`] for more than two events.
pub fn independent_and(a: Probability, b: Probability) -> Probability {
    Probability(a.0 * b.0)
}

/// `P(A1 and A2 and ... and An) = P(A1) * P(A2) * ... * P(An)`, assuming
/// mutual independence. Errors on an empty slice (there is no meaningful
/// "and of nothing").
pub fn independent_and_n(events: &[Probability]) -> Result<Probability, ProbabilityError> {
    if events.is_empty() {
        return Err(ProbabilityError::EmptyCombination);
    }
    Ok(Probability(events.iter().fold(1.0, |acc, p| acc * p.0)))
}

/// `P(A or B) = P(A) + P(B) - P(A)*P(B)`, **assuming A and B are
/// independent**. This is the general inclusion-exclusion formula
/// specialized to independence; it is distinct from
/// [`mutually_exclusive_or`], and the two must never be confused —
/// independence and mutual exclusivity are different (in fact,
/// incompatible unless one event has probability 0) assumptions.
pub fn independent_or(a: Probability, b: Probability) -> Probability {
    Probability(a.0 + b.0 - a.0 * b.0)
}

/// `P(A1 or A2 or ... or An)` assuming mutual independence, computed as
/// `1 - product(1 - P(Ai))` (the numerically stable form — avoids the
/// combinatorial blow-up of expanding inclusion-exclusion for n > 2).
/// Errors on an empty slice.
pub fn independent_or_n(events: &[Probability]) -> Result<Probability, ProbabilityError> {
    if events.is_empty() {
        return Err(ProbabilityError::EmptyCombination);
    }
    let product_of_complements = events.iter().fold(1.0, |acc, p| acc * (1.0 - p.0));
    Ok(Probability(1.0 - product_of_complements))
}

/// `P(A or B) = P(A) + P(B)`, valid **only when A and B are mutually
/// exclusive** (cannot both occur). Rejects inputs whose sum exceeds 1 —
/// mutually exclusive events can never have a combined probability above 1,
/// so a sum exceeding 1 means the mutual-exclusivity assumption itself is
/// false for these inputs, and the function refuses to silently produce an
/// out-of-range result.
pub fn mutually_exclusive_or(a: Probability, b: Probability) -> Result<Probability, ProbabilityError> {
    let sum = a.0 + b.0;
    if sum > 1.0 {
        return Err(ProbabilityError::MutuallyExclusiveSumExceedsOne(sum));
    }
    Ok(Probability(sum))
}

/// `P(A | B) = P(A and B) / P(B)` — conditional probability, computed
/// directly from the joint and marginal probabilities. This is **not**
/// derived by assuming independence (if A and B were independent,
/// `P(A|B) = P(A)`; this function makes no such assumption and requires the
/// joint probability explicitly). Undefined (and rejected, not silently
/// approximated) when `P(B) = 0`.
pub fn conditional(
    joint_a_and_b: Probability,
    marginal_b: Probability,
) -> Result<Probability, ProbabilityError> {
    if marginal_b.0 == 0.0 {
        return Err(ProbabilityError::ConditioningOnImpossibleEvent);
    }
    Probability::new(joint_a_and_b.0 / marginal_b.0)
}

/// Bayes' rule: `P(A | B) = P(B | A) * P(A) / P(B)`. Rejects `P(B) = 0`
/// (undefined) rather than returning NaN or infinity. Does not itself
/// validate that `P(B|A)*P(A) <= P(B)` beyond the final range check on the
/// result — an inconsistent set of inputs (violating the law of total
/// probability) surfaces as an ordinary [`ProbabilityError::OutOfRange`] on
/// the result, not a silently wrong number.
pub fn bayes(
    likelihood_b_given_a: Probability,
    prior_a: Probability,
    marginal_b: Probability,
) -> Result<Probability, ProbabilityError> {
    if marginal_b.0 == 0.0 {
        return Err(ProbabilityError::BayesZeroDenominator);
    }
    Probability::new(likelihood_b_given_a.0 * prior_a.0 / marginal_b.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: f64) -> Probability {
        Probability::new(v).unwrap()
    }

    #[test]
    fn rejects_negative_without_clamping() {
        assert_eq!(
            Probability::new(-0.1),
            Err(ProbabilityError::OutOfRange(-0.1))
        );
    }

    #[test]
    fn rejects_above_one_without_clamping() {
        assert_eq!(
            Probability::new(1.2),
            Err(ProbabilityError::OutOfRange(1.2))
        );
    }

    #[test]
    fn rejects_nan_and_infinity() {
        assert!(matches!(
            Probability::new(f64::NAN),
            Err(ProbabilityError::NotFinite(_))
        ));
        assert!(matches!(
            Probability::new(f64::INFINITY),
            Err(ProbabilityError::NotFinite(_))
        ));
    }

    #[test]
    fn accepts_boundary_zero_and_one() {
        assert_eq!(Probability::new(0.0).unwrap().value(), 0.0);
        assert_eq!(Probability::new(1.0).unwrap().value(), 1.0);
    }

    #[test]
    fn complement_known_value() {
        assert_eq!(complement(p(0.2)).value(), 0.8);
    }

    #[test]
    fn complement_boundary() {
        assert_eq!(complement(Probability::IMPOSSIBLE), Probability::CERTAIN);
        assert_eq!(complement(Probability::CERTAIN), Probability::IMPOSSIBLE);
    }

    #[test]
    fn independent_and_known_value() {
        // Reference: 0.2 * 0.3 = 0.06
        let r = independent_and(p(0.2), p(0.3));
        assert!((r.value() - 0.06).abs() < 1e-12);
    }

    #[test]
    fn independent_or_known_value() {
        // Reference: 0.2 + 0.3 - 0.2*0.3 = 0.44
        let r = independent_or(p(0.2), p(0.3));
        assert!((r.value() - 0.44).abs() < 1e-12);
    }

    #[test]
    fn independent_or_n_matches_pairwise_for_three_events() {
        let a = p(0.1);
        let b = p(0.2);
        let c = p(0.3);
        let pairwise = independent_or(independent_or(a, b), c);
        let n_ary = independent_or_n(&[a, b, c]).unwrap();
        assert!((pairwise.value() - n_ary.value()).abs() < 1e-12);
    }

    #[test]
    fn independent_and_n_matches_pairwise_for_three_events() {
        let a = p(0.5);
        let b = p(0.4);
        let c = p(0.3);
        let pairwise = independent_and(independent_and(a, b), c);
        let n_ary = independent_and_n(&[a, b, c]).unwrap();
        assert!((pairwise.value() - n_ary.value()).abs() < 1e-12);
    }

    #[test]
    fn empty_combination_is_an_explicit_error() {
        assert_eq!(
            independent_and_n(&[]),
            Err(ProbabilityError::EmptyCombination)
        );
        assert_eq!(
            independent_or_n(&[]),
            Err(ProbabilityError::EmptyCombination)
        );
    }

    #[test]
    fn mutually_exclusive_or_known_value() {
        let r = mutually_exclusive_or(p(0.2), p(0.3)).unwrap();
        assert!((r.value() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn mutually_exclusive_or_rejects_sum_exceeding_one() {
        assert_eq!(
            mutually_exclusive_or(p(0.7), p(0.5)),
            Err(ProbabilityError::MutuallyExclusiveSumExceedsOne(1.2))
        );
    }

    #[test]
    fn mutually_exclusive_and_independent_or_diverge_for_the_same_inputs() {
        // The whole point of keeping them distinct: for the same P(A), P(B),
        // the two formulas give different answers whenever both are
        // positive.
        let a = p(0.2);
        let b = p(0.3);
        let me = mutually_exclusive_or(a, b).unwrap();
        let ind = independent_or(a, b);
        assert!((me.value() - 0.5).abs() < 1e-12);
        assert!((ind.value() - 0.44).abs() < 1e-12);
        assert_ne!(me.value(), ind.value());
    }

    #[test]
    fn conditional_known_value() {
        // P(A and B) = 0.1, P(B) = 0.4 -> P(A|B) = 0.25
        let r = conditional(p(0.1), p(0.4)).unwrap();
        assert!((r.value() - 0.25).abs() < 1e-12);
    }

    #[test]
    fn conditional_on_impossible_event_is_rejected() {
        assert_eq!(
            conditional(p(0.1), Probability::IMPOSSIBLE),
            Err(ProbabilityError::ConditioningOnImpossibleEvent)
        );
    }

    #[test]
    fn conditional_is_not_silently_computed_as_independence() {
        // If conditional() silently assumed independence, P(A|B) would
        // just equal P(A) = 0.5 regardless of the joint. It must not.
        let joint = p(0.1);
        let marginal_b = p(0.4);
        let r = conditional(joint, marginal_b).unwrap();
        assert_ne!(r.value(), 0.5);
    }

    #[test]
    fn bayes_known_value() {
        // Classic textbook example: a disease with prevalence 1%, test
        // sensitivity 90% (P(pos|disease)), test false-positive rate 5%
        // (P(pos|no disease) = 0.05).
        // P(pos) = 0.9*0.01 + 0.05*0.99 = 0.009 + 0.0495 = 0.0585
        // P(disease|pos) = 0.9*0.01 / 0.0585 = 0.15384615...
        let p_pos = p(0.9 * 0.01 + 0.05 * 0.99);
        let r = bayes(p(0.9), p(0.01), p_pos).unwrap();
        assert!((r.value() - 0.153846153846).abs() < 1e-9);
    }

    #[test]
    fn bayes_zero_denominator_is_rejected() {
        assert_eq!(
            bayes(p(0.5), p(0.5), Probability::IMPOSSIBLE),
            Err(ProbabilityError::BayesZeroDenominator)
        );
    }

    #[test]
    fn bayes_inconsistent_inputs_surface_as_out_of_range_not_silent_wrong_number() {
        // P(B|A)=1, P(A)=1, P(B)=0.5 implies P(A|B) = 1/0.5 = 2, invalid.
        let err = bayes(Probability::CERTAIN, Probability::CERTAIN, p(0.5)).unwrap_err();
        assert!(matches!(err, ProbabilityError::OutOfRange(_)));
    }

    #[test]
    fn serde_roundtrips_as_a_bare_number() {
        let v = p(0.25);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "0.25");
        let back: Probability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn serde_rejects_out_of_range_on_deserialize() {
        let result: Result<Probability, _> = serde_json::from_str("1.5");
        assert!(result.is_err());
    }
}
