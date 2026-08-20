//! End-to-end worked example, and the before/after mitigation comparison.
//!
//! This is the documented example referenced by
//! `docs/reliability/uncertainty-importance-sensitivity.md`. It walks the full
//! pipeline for one service:
//!
//! ```text
//! service -> failure modes -> estimated probabilities -> uncertainty
//!         -> dependency / common-cause model -> fault tree
//!         -> top-event probability -> uncertainty propagation
//!         -> importance -> sensitivity -> analysis artifact
//!         -> engineer selects a deterministic result -> ReliabilityArtifact
//! ```
//!
//! and then answers the two engineering questions the analysis exists for:
//!
//! 1. Which failure should the team investigate first?
//! 2. Which probability estimate contributes most to the uncertainty?
//!
//! The answers are different events, which is the whole point.

use std::collections::BTreeMap;

use etdl_reliability::analysis::dependence::{
    analyze_with, compare, AnalysisMetadata, AnalysisOptions, ArtifactRef, CommonCause,
    DependencyModel, FaultTreeSpec, GateKind, GateSpec, IndependenceAssumption, InputUncertainty,
    MonteCarloConfig,
};
use etdl_reliability::artifact::{declared, ReliabilityArtifact};

/// The service model.
///
/// ```text
/// SystemUnavailable = OR( GatewayFailure, DatabaseFailure )
///   GatewayFailure  = OR( GatewayTimeout, GatewayUnreachable )
///   DatabaseFailure = AND( DbPrimaryDown, DbReplicaDown )
///
/// GatewayUnreachable and DbReplicaDown share a network fabric:
///   SharedNetworkFailure (P = 2.0e-4) affects both.
/// ```
fn service_tree(gateway_timeout: f64) -> FaultTreeSpec {
    let mut tree = FaultTreeSpec::new("SystemUnavailable");
    tree.leaves.insert("GatewayTimeout".into(), gateway_timeout);
    tree.leaves.insert("GatewayUnreachable".into(), 1.2e-3);
    tree.leaves.insert("DbPrimaryDown".into(), 4.0e-3);
    tree.leaves.insert("DbReplicaDown".into(), 5.0e-3);

    tree.gates.insert(
        "GatewayFailure".into(),
        GateSpec {
            kind: GateKind::Or,
            inputs: vec!["GatewayTimeout".into(), "GatewayUnreachable".into()],
            k: None,
        },
    );
    tree.gates.insert(
        "DatabaseFailure".into(),
        GateSpec {
            kind: GateKind::And,
            inputs: vec!["DbPrimaryDown".into(), "DbReplicaDown".into()],
            k: None,
        },
    );
    tree.gates.insert(
        "SystemUnavailable".into(),
        GateSpec {
            kind: GateKind::Or,
            inputs: vec!["GatewayFailure".into(), "DatabaseFailure".into()],
            k: None,
        },
    );
    tree
}

fn service_model() -> DependencyModel {
    let mut cc = CommonCause::new(
        "SharedNetworkFailure",
        2.0e-4,
        vec!["GatewayUnreachable".into(), "DbReplicaDown".into()],
    );
    cc.ontology_id = Some("failure.network.unreachable".into());
    cc.source = Some("post-incident review INC-2291".into());
    cc.assumptions = vec![
        "the gateway and the database replica share one top-of-rack switch".to_string(),
    ];
    DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![cc],
        ..Default::default()
    }
}

/// Declared uncertainty for each estimate, as Beta posteriors.
///
/// Each mean equals the corresponding point probability exactly, so the
/// propagated mean is comparable with the point estimate. What differs is how
/// *tightly* each is known:
///
/// ```text
/// GatewayTimeout      Beta(10000, 990000)  mean 1.0e-2  sd 9.9e-5   heavily instrumented
/// GatewayUnreachable  Beta(2.4,   1997.6)  mean 1.2e-3  sd 7.7e-4   barely observed
/// DbPrimaryDown       Beta(4,     996)     mean 4.0e-3  sd 2.0e-3
/// DbReplicaDown       Beta(5,     995)     mean 5.0e-3  sd 2.2e-3
/// ```
///
/// GatewayTimeout is the dominant *contributor* and the best *measured*.
/// GatewayUnreachable contributes far less probability but is known an order
/// of magnitude less precisely, and sits on the same OR path, so it drives the
/// width of the answer. DbPrimaryDown has a wide posterior but enters through
/// an AND, so its influence on the top event is scaled down by its redundant
/// partner and it barely matters either way.
fn service_uncertainty(gateway_timeout_posterior: (f64, f64)) -> BTreeMap<String, InputUncertainty> {
    let mut laws = BTreeMap::new();
    laws.insert(
        "GatewayTimeout".to_string(),
        InputUncertainty::Beta {
            alpha: gateway_timeout_posterior.0,
            beta: gateway_timeout_posterior.1,
        },
    );
    laws.insert(
        "GatewayUnreachable".to_string(),
        InputUncertainty::Beta {
            alpha: 2.4,
            beta: 1_997.6,
        },
    );
    laws.insert(
        "DbPrimaryDown".to_string(),
        InputUncertainty::Beta {
            alpha: 4.0,
            beta: 996.0,
        },
    );
    laws.insert(
        "DbReplicaDown".to_string(),
        InputUncertainty::Beta {
            alpha: 5.0,
            beta: 995.0,
        },
    );
    laws
}

fn options(
    laws: BTreeMap<String, InputUncertainty>,
    model_version: &str,
) -> AnalysisOptions {
    options_with(laws, model_version, false)
}

fn options_with(
    laws: BTreeMap<String, InputUncertainty>,
    model_version: &str,
    ranking: bool,
) -> AnalysisOptions {
    AnalysisOptions {
        monte_carlo: Some(MonteCarloConfig {
            samples: 20_000,
            seed: 20_260_818,
            level: 0.95,
        }),
        inputs: laws,
        perturbation: 1e-4,
        compute_importance: true,
        compute_sensitivity: true,
        compute_uncertainty_ranking: ranking,
        metadata: AnalysisMetadata {
            model_id: "checkout-service".to_string(),
            model_version: Some(model_version.to_string()),
            ontology_version: Some("etdl.reliability.ontology/1.0".to_string()),
            artifacts: vec![ArtifactRef::new("checkout-estimates")
                .with_version("3.1.0")
                .with_role("probability-estimates")],
            ..Default::default()
        },
    }
}

/// The full pipeline, verified end to end.
#[test]
fn end_to_end_analysis_answers_both_engineering_questions() {
    let tree = service_tree(0.01);
    let model = service_model();
    let result = analyze_with(
        &tree,
        &model,
        &options_with(service_uncertainty((10_000.0, 990_000.0)), "1.0.0", true),
    )
    .unwrap();

    // ---- the point estimate, checked against the hand computation ---------
    //
    // Conditioning on SharedNetworkFailure (p = 2e-4):
    //   present (2e-4):  GatewayUnreachable = DbReplicaDown = 1
    //                    GatewayFailure = 1  ->  TOP = 1
    //   absent (0.9998): GatewayUnreachable' = (1.2e-3 - 2e-4)/0.9998
    //                    DbReplicaDown'      = (5.0e-3 - 2e-4)/0.9998
    //                    GatewayFailure  = 1 - (1-0.01)(1-GU')
    //                    DatabaseFailure = 4e-3 * DR'
    //                    TOP = 1 - (1-GF)(1-DF)
    let p_cc = 2.0e-4;
    let gu = (1.2e-3 - p_cc) / (1.0 - p_cc);
    let dr = (5.0e-3 - p_cc) / (1.0 - p_cc);
    let gf = 1.0 - (1.0 - 0.01) * (1.0 - gu);
    let df = 4.0e-3 * dr;
    let top_absent = 1.0 - (1.0 - gf) * (1.0 - df);
    let expected = p_cc * 1.0 + (1.0 - p_cc) * top_absent;
    assert!(
        (result.point_estimate - expected).abs() < 1e-15,
        "top {} vs hand-derived {expected}",
        result.point_estimate
    );
    // Sanity: the system is roughly 1.1% likely to be unavailable.
    assert!(result.point_estimate > 0.010 && result.point_estimate < 0.013);

    // ---- uncertainty -------------------------------------------------------
    let mc = result.uncertainty.as_ref().unwrap();
    assert!(mc.lower < result.point_estimate && result.point_estimate < mc.upper);
    assert_eq!(mc.variable_inputs, 4);
    assert_eq!(mc.samples, 20_000);
    assert_eq!(mc.seed, 20_260_818);

    // ---- question 1: which failure to investigate first? -------------------
    //
    // Answered by importance. Fussell-Vesely is the natural prioritisation
    // measure here because it is the share of top-event probability that
    // disappears if the event is eliminated, which is what "fix this first"
    // means operationally.
    let imp = result.importance_result.as_ref().unwrap();
    let by_fv = imp.ranked_by("fussell-vesely");
    assert_eq!(
        by_fv[0].0.id, "GatewayTimeout",
        "the highest Fussell-Vesely contributor should be the gateway timeout, got {}",
        by_fv[0].0.id
    );
    assert!(by_fv[0].1 > 0.8, "FV {}", by_fv[0].1);
    let first = &result.importance[0];
    // The common cause is present in the ranking as its own entity.
    let cc = result
        .importance
        .iter()
        .find(|e| e.id == "SharedNetworkFailure")
        .expect("the common cause must be eligible for importance analysis");
    assert!(cc.is_common_cause);
    assert_eq!(cc.entity_kind, "common-cause");
    // This is the §24 case: a shared cause with a much SMALLER probability
    // outranks a larger independent hardware failure, because it defeats the
    // database redundancy outright rather than consuming one of its two legs.
    //
    //   SharedNetworkFailure  q = 2.0e-4   Birnbaum ~ 0.99
    //   DbPrimaryDown         q = 4.0e-3   Birnbaum ~ 5e-3
    //
    // Ranking by probability alone would have inverted this.
    let db_primary = result
        .importance
        .iter()
        .find(|e| e.id == "DbPrimaryDown")
        .unwrap();
    assert!(cc.event_probability.unwrap() < db_primary.event_probability.unwrap() / 10.0);
    assert!(
        cc.birnbaum > db_primary.birnbaum * 100.0,
        "the shared cause ({}) must dominate the larger independent failure ({})",
        cc.birnbaum,
        db_primary.birnbaum
    );
    // Against the gateway timeout, which sits directly under the top OR, the
    // shared cause is comparable but does not lead: an event that alone
    // guarantees the top event cannot be outranked on Birnbaum.
    assert!(cc.birnbaum > 0.9 * first.birnbaum);

    // ---- question 2: which estimate drives the uncertainty? ----------------
    let ranking = result.uncertainty_ranking.as_ref().unwrap();
    let top_uncertainty = &ranking.entries[0];
    assert_eq!(
        top_uncertainty.id, "GatewayUnreachable",
        "the least precisely known estimate on the dominant path should drive the \
         output interval, got {}",
        top_uncertainty.id
    );
    assert!(top_uncertainty.variance_share > 0.5);
    assert!(top_uncertainty.above_noise_floor);

    // The two questions have DIFFERENT answers. GatewayTimeout is what to fix;
    // GatewayUnreachable is what to measure. Collapsing importance and
    // uncertainty contribution into one score would have hidden this.
    assert_ne!(by_fv[0].0.id, ranking.entries[0].id);

    // And DbPrimaryDown, despite the widest posterior of the four, is near the
    // bottom of the uncertainty ranking: a wide input that the structure
    // damps out does not drive the answer.
    let db = ranking
        .entries
        .iter()
        .find(|e| e.id == "DbPrimaryDown")
        .unwrap();
    assert!(
        db.variance_share < 0.05,
        "DbPrimaryDown variance share {}",
        db.variance_share
    );

    // ---- sensitivity -------------------------------------------------------
    let sens = result
        .sensitivity
        .iter()
        .find(|e| e.id == "GatewayTimeout")
        .unwrap();
    assert!(sens.increase.as_ref().unwrap().delta > 0.0);
    assert!(sens.decrease.as_ref().unwrap().delta < 0.0);
    assert!(sens.relative_sensitivity.unwrap() > 0.0);

    // ---- the artifact is traceable ----------------------------------------
    assert_eq!(result.model_id, "checkout-service");
    assert_eq!(result.model_version.as_deref(), Some("1.0.0"));
    assert_eq!(result.inputs.artifacts.len(), 1);
    assert_eq!(result.inputs.artifacts[0].id, "checkout-estimates");
    assert_eq!(result.inputs.artifacts[0].version.as_deref(), Some("3.1.0"));
    assert_eq!(
        result.provenance.ontology_version.as_deref(),
        Some("etdl.reliability.ontology/1.0")
    );
    assert!(result.assumptions.iter().any(|a| a.contains("top-of-rack")));

    // ---- the engineer selects a deterministic value -----------------------
    // The analysis does not decide. An engineer reads it and writes a chosen
    // scalar into a reliability artifact, which is what the compiler consumes.
    let mut artifact = ReliabilityArtifact::new("checkout-estimates");
    artifact.version = Some("3.2.0".into());
    artifact
        .add(declared("failure.system.unavailable", result.point_estimate))
        .unwrap();
    let resolved = artifact
        .get("failure.system.unavailable")
        .unwrap()
        .resolved_probability()
        .unwrap();
    assert_eq!(resolved, result.point_estimate);

    // The rendered report names its metrics rather than emitting bare floats.
    let text = result.render();
    assert!(text.contains("Dominant Contributors (Birnbaum importance)"));
    assert!(text.contains("Uncertainty Contribution (NOT importance)"));
    assert!(text.contains("metric: birnbaum"));
    assert!(text.contains("Assumptions:"));
}

/// Before/after mitigation, the documented comparison example.
///
/// The mitigation is a gateway timeout fix: `GatewayTimeout` drops from 0.01 to
/// 0.002, and the improved instrumentation also tightens its posterior. Nothing
/// else about the model changes.
#[test]
fn before_and_after_mitigation() {
    let model = service_model();

    let before = analyze_with(
        &service_tree(0.01),
        &model,
        &options(service_uncertainty((10_000.0, 990_000.0)), "1.0.0"),
    )
    .unwrap();

    // The fix also improved instrumentation, so the posterior tightens as well
    // as shifting: two modifications to one input.
    let after = analyze_with(
        &service_tree(0.002),
        &model,
        &options(service_uncertainty((2_000.0, 998_000.0)), "1.1.0"),
    )
    .unwrap();

    // The top event falls substantially.
    assert!(after.point_estimate < before.point_estimate);
    let reduction =
        (before.point_estimate - after.point_estimate) / before.point_estimate;
    assert!(
        reduction > 0.6,
        "the mitigation should cut the top event by more than 60%, got {:.1}%",
        reduction * 100.0
    );

    let comparison = compare(&before, &after);
    assert_eq!(comparison.schema, "etdl.reliability.analysis-comparison/1.0");
    assert_eq!(comparison.top_event, "SystemUnavailable");
    assert!(comparison.absolute_change < 0.0);
    assert!(comparison.relative_change.unwrap() < -0.6);

    // The changed input is identified with its before and after values.
    let change = comparison
        .input_changes
        .iter()
        .find(|c| c.id == "GatewayTimeout")
        .expect("the mitigated input must appear in the comparison");
    assert_eq!(change.before, Some(0.01));
    assert_eq!(change.after, Some(0.002));
    assert_eq!(change.change, "changed-and-uncertainty-changed");
    assert!(change.relative_change.unwrap() < -0.79);

    // Both the probability AND the uncertainty of that input changed, so the
    // comparison must NOT attribute the outcome to a single modification.
    assert!(
        !comparison.single_change,
        "two simultaneous modifications must defeat single-change attribution"
    );
    assert!(
        comparison.causal_attribution.contains("NOT attributed"),
        "attribution text: {}",
        comparison.causal_attribution
    );
    assert!(comparison.causal_attribution.contains("two ways at once"));

    // Uncertainty shrinks too.
    let (bl, bu) = comparison.before_interval.unwrap();
    let (al, au) = comparison.after_interval.unwrap();
    assert!(au - al < bu - bl, "the interval should narrow after mitigation");

    // The priority picture shifts: GatewayTimeout's share of the top-event
    // probability falls sharply and GatewayUnreachable's rises, so the "what
    // to work on next" answer changes even though the Birnbaum ordering of
    // near-certain OR-path events does not permute.
    let fv = |r: &etdl_reliability::analysis::dependence::AnalysisResult, id: &str| -> f64 {
        r.importance
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .fussell_vesely
            .unwrap()
    };
    let gt_before = fv(&before, "GatewayTimeout");
    let gt_after = fv(&after, "GatewayTimeout");
    let gu_before = fv(&before, "GatewayUnreachable");
    let gu_after = fv(&after, "GatewayUnreachable");
    assert!(
        gt_after < gt_before - 0.2,
        "GatewayTimeout FV should fall materially: {gt_before} -> {gt_after}"
    );
    assert!(
        gu_after > gu_before * 2.0,
        "GatewayUnreachable FV should rise: {gu_before} -> {gu_after}"
    );
    // Each share stays a proper fraction of the top-event probability. Note
    // that Fussell-Vesely shares do NOT partition it: they overlap wherever
    // cut sets overlap, and the total over all entities is not one. Rendering
    // them as a pie chart would be a misreading of the measure.
    for v in [gt_before, gt_after, gu_before, gu_after] {
        assert!((0.0..=1.0).contains(&v), "FV out of range: {v}");
    }
    let total_after: f64 = after
        .importance
        .iter()
        .filter_map(|e| e.fussell_vesely)
        .sum();
    assert!(
        total_after > 0.0,
        "Fussell-Vesely shares are reported per entity, not as a partition \
         (total here: {total_after})"
    );

    // Results are immutable: the "before" analysis is untouched and keeps its
    // own identity.
    assert_ne!(before.analysis_id, after.analysis_id);
    assert_eq!(before.model_version.as_deref(), Some("1.0.0"));
    assert_eq!(after.model_version.as_deref(), Some("1.1.0"));

    let text = comparison.render();
    assert!(text.contains("Reliability Analysis Comparison"));
    assert!(text.contains("Attribution:"));
}

/// A comparison in which exactly one input changed DOES attribute the outcome
/// to it — under the model, and with the limits of that claim stated.
#[test]
fn single_input_change_is_attributed() {
    let model = service_model();
    let laws = service_uncertainty((10_000.0, 990_000.0));
    let before = analyze_with(&service_tree(0.01), &model, &options(laws.clone(), "1.0.0")).unwrap();
    let after = analyze_with(&service_tree(0.002), &model, &options(laws, "1.0.0")).unwrap();

    let comparison = compare(&before, &after);
    assert_eq!(comparison.input_changes.len(), 1);
    assert!(comparison.single_change);
    assert!(comparison
        .causal_attribution
        .contains("exactly one input changed"));
    // Even then, the claim is scoped to the model.
    assert!(comparison
        .causal_attribution
        .contains("whether the real system changed the same way"));
}

/// Two identical analyses compare to no change at all.
#[test]
fn identical_analyses_compare_clean() {
    let model = service_model();
    let opts = options(service_uncertainty((10_000.0, 990_000.0)), "1.0.0");
    let a = analyze_with(&service_tree(0.01), &model, &opts).unwrap();
    let b = analyze_with(&service_tree(0.01), &model, &opts).unwrap();
    assert_eq!(a.analysis_id, b.analysis_id);

    let comparison = compare(&a, &b);
    assert_eq!(comparison.absolute_change, 0.0);
    assert!(comparison.input_changes.is_empty());
    assert!(comparison.assumption_changes.is_empty());
    assert!(comparison.method_changes.is_empty());
    assert!(comparison.importance_rank_changes.is_empty());
    assert!(comparison.causal_attribution.contains("no inputs"));
}

/// The analysis result serialises to stable JSON with explicit metric names,
/// not Rust `Debug` output.
#[test]
fn json_output_is_stable_and_named() {
    let result = analyze_with(
        &service_tree(0.01),
        &service_model(),
        &options(service_uncertainty((10_000.0, 990_000.0)), "1.0.0"),
    )
    .unwrap();

    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["schema"], "etdl.reliability.analysis-result/1.0");
    assert_eq!(json["schemaVersion"], serde_json::Value::Null); // camelCase is not used
    assert_eq!(json["schema_version"], "1.1");
    assert!(json["analysis_id"].as_str().unwrap().starts_with("ana-"));
    assert_eq!(json["model_id"], "checkout-service");
    assert_eq!(json["analyzer"], "etdl-reliability");

    // Metrics are named fields, never positional floats.
    let entry = &json["importance"][0];
    assert!(entry["birnbaum"].is_number());
    assert!(entry["entity_kind"].is_string());
    assert!(entry["top_given_occurred"].is_number());

    let mc = &json["uncertainty"];
    assert_eq!(mc["method"], "monte-carlo-propagation");
    assert_eq!(mc["sampler"], "xorshift64star");
    assert!(mc["semantics"].is_string());
    assert!(mc["convergence"]["stable"].is_boolean());

    // Round-trips.
    let back: etdl_reliability::analysis::dependence::AnalysisResult =
        serde_json::from_value(json).unwrap();
    assert_eq!(back.analysis_id, result.analysis_id);
    assert_eq!(back.point_estimate, result.point_estimate);
}
