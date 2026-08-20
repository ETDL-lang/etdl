//! `cargo run -p etdl-probability-core --example composition`
//!
//! Basic probability composition — complement, independent AND/OR,
//! mutually-exclusive OR, conditional probability, Bayes' rule.
//!
//! This is a **Rust** example, not an `.etdl` file, because ETDL has no
//! expression syntax for calling these functions — see
//! `examples/probability/basic.etdl` for the ETDL-source half
//! (`std.probability`'s reusable constants) and
//! `docs/reference/standard-probability-library.md` for the full
//! explanation. This is the counterpart a Rust-implemented compiler
//! extension or future domain library would actually call.

use etdl_probability_core::{
    bayes, complement, conditional, independent_and, independent_or, mutually_exclusive_or,
    Probability,
};

fn main() {
    let gateway_timeout = Probability::new(0.2).unwrap();
    let database_unavailable = Probability::new(0.3).unwrap();

    println!("P(GatewayTimeout)      = {gateway_timeout}");
    println!("P(DatabaseUnavailable) = {database_unavailable}");

    // Complement.
    println!(
        "P(not GatewayTimeout)  = {}",
        complement(gateway_timeout)
    );

    // Independent AND/OR — the assumption is in the function name, never
    // inferred.
    println!(
        "P(GatewayTimeout AND DatabaseUnavailable), assuming independence = {}",
        independent_and(gateway_timeout, database_unavailable)
    );
    println!(
        "P(GatewayTimeout OR DatabaseUnavailable), assuming independence  = {}",
        independent_or(gateway_timeout, database_unavailable)
    );

    // Mutually exclusive OR is a DIFFERENT formula and a different
    // assumption — never confused with independence.
    let outcome_a = Probability::new(0.2).unwrap();
    let outcome_b = Probability::new(0.3).unwrap();
    println!(
        "P(OutcomeA OR OutcomeB), assuming mutual exclusivity = {}",
        mutually_exclusive_or(outcome_a, outcome_b).unwrap()
    );

    // Conditional probability — requires the joint probability explicitly,
    // never silently derived by assuming independence.
    let joint = Probability::new(0.1).unwrap();
    let marginal_timeout = gateway_timeout;
    println!(
        "P(DatabaseUnavailable | GatewayTimeout) = {}",
        conditional(joint, marginal_timeout).unwrap()
    );

    // Bayes' rule: the classic diagnostic-test example. P(positive test) is
    // the sum of two MUTUALLY EXCLUSIVE contributions (a patient either has
    // the disease or doesn't) — mutually_exclusive_or is the correct
    // combinator here, not independent_or (these two contributions are not
    // independent events at all; they partition the sample space).
    let prevalence = Probability::new(0.01).unwrap();
    let sensitivity = Probability::new(0.9).unwrap(); // P(positive | disease)
    let false_positive_rate = Probability::new(0.05).unwrap(); // P(positive | no disease)
    let true_positive_contribution =
        Probability::new(sensitivity.value() * prevalence.value()).unwrap();
    let false_positive_contribution =
        Probability::new(false_positive_rate.value() * complement(prevalence).value()).unwrap();
    let p_positive =
        mutually_exclusive_or(true_positive_contribution, false_positive_contribution).unwrap();
    println!("P(positive test) = {p_positive}");

    let posterior = bayes(sensitivity, prevalence, p_positive).unwrap();
    println!("P(disease | positive test) = {posterior}");
}
