//! End-to-end: `std.probability` (`etdl-probability-core`) -> the existing
//! reliability layer (`ProbabilityEstimate` -> `ReliabilityArtifact` ->
//! `ArtifactResolver`), without changing any existing reliability
//! behavior. Proves the dependency direction works in practice: this test
//! constructs a validated `Probability` first, adapts it into the
//! reliability domain's own estimate type, and lets the *existing*,
//! unmodified reliability artifact/resolution machinery take over from
//! there.

use etdl_probability_core::distribution::Beta;
use etdl_probability_core::Probability;
use etdl_reliability::probability_adapter::estimate_from_probability;
use etdl_reliability_core::artifact::{ArtifactResolver, ReliabilityArtifact, ResolveOutcome, UnknownProbabilityPolicy};
use etdl_reliability_core::estimate::ProbabilityState;

#[test]
fn validated_probability_flows_into_an_existing_reliability_artifact() {
    // Step 1: a plain, validated Probability -- std.probability's own
    // native layer, nothing reliability-specific about it yet.
    let p = Probability::new(0.0024).unwrap();

    // Step 2: the adapter converts it into the reliability domain's own
    // ProbabilityEstimate. Nothing about ProbabilityEstimate itself
    // changed -- this is the same type, same fields, same semantics as
    // before this task.
    let estimate = estimate_from_probability(
        "failure.gateway.timeout",
        ProbabilityState::Declared,
        p,
    );
    assert_eq!(estimate.value, Some(0.0024));

    // Step 3: from here on, this is the *existing*, unmodified reliability
    // pipeline: build an artifact, resolve it. Nothing below this line
    // knows or cares that the value originated from etdl-probability-core.
    let mut artifact = ReliabilityArtifact::new("payment-gateway");
    artifact.version = Some("1.0.0".to_string());
    artifact.add(estimate).unwrap();

    let resolver = ArtifactResolver::new(UnknownProbabilityPolicy::Error);
    let outcome = resolver
        .resolve(&artifact, "failure.gateway.timeout")
        .unwrap();

    let ResolveOutcome::Resolved(resolved) = outcome else {
        panic!("expected Resolved, got {outcome:?}");
    };
    assert_eq!(resolved.value, 0.0024);
    assert_eq!(resolved.artifact_id, "payment-gateway");
}

#[test]
fn beta_posterior_mean_from_probability_core_is_a_valid_reliability_estimate() {
    // A Beta-Binomial-style posterior computed via etdl-probability-core's
    // distribution math (independent of etdl-reliability's own
    // Beta-Binomial estimator) still produces a value the existing
    // artifact/resolver machinery accepts without any special-casing.
    let posterior = Beta::new(1.0 + 37.0, 1.0 + 99_963.0).unwrap(); // 37 failures / 100000
    let p = Probability::new(posterior.mean()).unwrap();

    let estimate = estimate_from_probability(
        "failure.gateway.timeout",
        ProbabilityState::Estimated,
        p,
    );

    let mut artifact = ReliabilityArtifact::new("payment-gateway");
    artifact.add(estimate).unwrap();
    let resolver = ArtifactResolver::new(UnknownProbabilityPolicy::Error);
    let outcome = resolver
        .resolve(&artifact, "failure.gateway.timeout")
        .unwrap();
    assert!(matches!(outcome, ResolveOutcome::Resolved(_)));
}

#[test]
fn invalid_probability_is_rejected_before_it_ever_reaches_the_artifact() {
    // etdl-probability-core's own validation runs first; an invalid value
    // never even gets the chance to become a ProbabilityEstimate.
    assert!(Probability::new(1.5).is_err());
    assert!(Probability::new(-0.1).is_err());
}
