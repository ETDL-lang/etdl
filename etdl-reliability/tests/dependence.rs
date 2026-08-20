//! Dependency-aware fault-tree analysis tests.
//!
//! These verify the mathematical correctness of common-cause handling,
//! conditional probability, uncertainty, importance, and sensitivity. They also
//! verify that independent models produce exactly the classic results.

use etdl_reliability::analysis::dependence::{
    analyze, importance, run_monte_carlo, sensitivity, BetaFactor, CommonCause,
    ConditionalProbability, DependencyEdge, DependencyEvaluator, DependencyKind, DependencyModel,
    FaultTreeSpec, GateKind, GateSpec, IndependenceAssumption, MonteCarloConfig,
};
use std::collections::BTreeMap;

fn tree_ab() -> FaultTreeSpec {
    // OR(A, B) with A=0.008, B=0.005
    let mut tree = FaultTreeSpec::new("top");
    tree.leaves.insert("A".into(), 0.008);
    tree.leaves.insert("B".into(), 0.005);
    tree.gates.insert(
        "top".into(),
        GateSpec {
            kind: GateKind::Or,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    tree
}

fn tree_and_ab() -> FaultTreeSpec {
    // AND(A, B) with A=0.008, B=0.005
    let mut tree = FaultTreeSpec::new("top");
    tree.leaves.insert("A".into(), 0.008);
    tree.leaves.insert("B".into(), 0.005);
    tree.gates.insert(
        "top".into(),
        GateSpec {
            kind: GateKind::And,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    tree
}

// ---- independence regression (must match classic math) ---------------------

#[test]
fn independent_or_matches_classic() {
    let tree = tree_ab();
    let model = DependencyModel::independent();
    let ev = DependencyEvaluator::new(&tree, &model);
    let p = ev.point_estimate().unwrap();
    let expected = 1.0 - (1.0 - 0.008) * (1.0 - 0.005);
    assert!((p - expected).abs() < 1e-12, "got {p}, expected {expected}");
}

#[test]
fn independent_and_matches_classic() {
    let tree = tree_and_ab();
    let model = DependencyModel::independent();
    let ev = DependencyEvaluator::new(&tree, &model);
    let p = ev.point_estimate().unwrap();
    let expected = 0.008 * 0.005;
    assert!((p - expected).abs() < 1e-12, "got {p}, expected {expected}");
}

// ---- common-cause double counting ------------------------------------------

#[test]
fn common_cause_or_is_not_independent_product() {
    // A and B both caused by C (shared). Model: A = ind_A OR C, B = ind_B OR C.
    // P(A) = 0.008, P(B) = 0.005, P(C) = 0.003.
    // P(ind_A) = (P(A) - P(C)) / (1 - P(C)) = (0.008-0.003)/0.997 = 0.005015...
    // P(ind_B) = (0.005-0.003)/0.997 = 0.002006...
    // P(top = A OR B) = P(A) + P(B) - P(A AND B)
    // P(A AND B) = P(C) + (1-P(C)) * P(ind_A)*P(ind_B)
    //   = 0.003 + 0.997 * 0.005015*0.002006 ≈ 0.003 + 0.997*1.006e-5 ≈ 0.00301003
    // So P(top) ≈ 0.008+0.005 - 0.00301003 = 0.00998997
    let tree = tree_ab();
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![CommonCause {
            id: "C".into(),
            ontology_id: Some("failure.network.unreachable".into()),
            probability: 0.003,
            affects: vec!["A".into(), "B".into()],
            evidence: vec!["shared network".into()],
            source: Some("engineering".into()),
            assumptions: vec!["independent residual".into()],
        }],
        conditional: vec![],
        edges: vec![],
    };
    let ev = DependencyEvaluator::new(&tree, &model);
    let p = ev.point_estimate().unwrap();

    // Hand-computed.
    let ind_a = (0.008 - 0.003) / (1.0 - 0.003);
    let ind_b = (0.005 - 0.003) / (1.0 - 0.003);
    let p_and = 0.003 + (1.0 - 0.003) * ind_a * ind_b;
    let expected = 0.008 + 0.005 - p_and;
    assert!((p - expected).abs() < 1e-9, "got {p}, expected {expected}");

    // The naive independent product would give a DIFFERENT (wrong) answer.
    let naive = 1.0 - (1.0 - 0.008) * (1.0 - 0.005);
    assert!(
        (p - naive).abs() > 1e-6,
        "must differ from naive independence"
    );
}

#[test]
fn common_cause_double_counting_is_prevented() {
    // Same as above but with a common cause that also appears in an AND.
    // A = C OR ind_A, B = C OR ind_B. Top = A AND B.
    // P(top) = P(A AND B) = P(C) + (1-P(C))*P(ind_A)*P(ind_B)
    let tree = tree_and_ab();
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![CommonCause::new("C", 0.003, vec!["A".into(), "B".into()])],
        conditional: vec![],
        edges: vec![],
    };
    let ev = DependencyEvaluator::new(&tree, &model);
    let p = ev.point_estimate().unwrap();
    let ind_a = (0.008 - 0.003) / (1.0 - 0.003);
    let ind_b = (0.005 - 0.003) / (1.0 - 0.003);
    let expected = 0.003 + (1.0 - 0.003) * ind_a * ind_b;
    assert!((p - expected).abs() < 1e-9, "got {p}, expected {expected}");

    // The naive AND would give 0.008*0.005 = 4e-5 — massively wrong.
    let naive = 0.008 * 0.005;
    assert!(p > naive * 10.0, "double counting must be prevented");
}

#[test]
fn beta_factor_model_is_valid() {
    // β = 0.1, λ_total = 1e-3 /hour, t = 24h
    let bf = BetaFactor {
        beta: 0.1,
        lambda_total: 1e-3,
        mission_time: 24.0,
    };
    let p_ccf = bf.common_cause_probability().unwrap();
    let expected = -(-0.1_f64 * 1e-3 * 24.0).exp_m1();
    assert!((p_ccf - expected).abs() < 1e-12);
    // β=0 → no common cause; β=1 → total rate is common cause.
    assert!(
        (BetaFactor {
            beta: 0.0,
            lambda_total: 1e-3,
            mission_time: 24.0
        })
        .common_cause_probability()
        .unwrap()
            == 0.0
    );
    // β=1 → total rate is common cause.
    let p1 = BetaFactor {
        beta: 1.0,
        lambda_total: 1e-3,
        mission_time: 24.0,
    }
    .common_cause_probability()
    .unwrap();
    let expected1 = -(-1e-3_f64 * 24.0).exp_m1();
    assert!((p1 - expected1).abs() < 1e-12);
    assert!(BetaFactor {
        beta: 1.5,
        lambda_total: 1e-3,
        mission_time: 24.0
    }
    .common_cause_probability()
    .is_err());
}

// ---- conditional probability -------------------------------------------------

#[test]
fn conditional_probability_is_represented_and_validated() {
    let tree = tree_ab();
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![],
        conditional: vec![ConditionalProbability {
            event: "A".into(),
            given: "B".into(),
            probability: 0.5,
        }],
        edges: vec![],
    };
    // Validation must pass: both A and B are known leaves.
    assert!(model.validate(&tree).is_ok());

    // An out-of-range conditional is rejected.
    let bad = DependencyModel {
        conditional: vec![ConditionalProbability {
            event: "A".into(),
            given: "B".into(),
            probability: 1.5,
        }],
        ..model.clone()
    };
    assert!(bad.validate(&tree).is_err());
}

#[test]
fn conditional_unknown_given_rejected() {
    let tree = tree_ab();
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![],
        conditional: vec![ConditionalProbability {
            event: "A".into(),
            given: "nonexistent".into(),
            probability: 0.5,
        }],
        edges: vec![],
    };
    assert!(model.validate(&tree).is_err());
}

// ---- validation --------------------------------------------------------------

#[test]
fn validation_rejects_problematic_models() {
    let tree = tree_ab();

    // Common cause without affected events.
    let empty = DependencyModel {
        common_causes: vec![CommonCause::new("C", 0.001, vec![])],
        ..Default::default()
    };
    assert!(empty.validate(&tree).is_err());

    // Common cause probability out of range.
    let bad_p = DependencyModel {
        common_causes: vec![CommonCause::new("C", 1.5, vec!["A".into()])],
        ..Default::default()
    };
    assert!(bad_p.validate(&tree).is_err());

    // Common cause exceeds the affected event probability.
    let exceeds = DependencyModel {
        common_causes: vec![CommonCause::new("C", 0.5, vec!["A".into()])],
        ..Default::default()
    };
    assert!(exceeds.validate(&tree).is_err());

    // Unknown affected event.
    let unknown = DependencyModel {
        common_causes: vec![CommonCause::new("C", 0.001, vec!["ZZZ".into()])],
        ..Default::default()
    };
    assert!(unknown.validate(&tree).is_err());

    // Cycle in dependency edges.
    let cycle = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        edges: vec![
            DependencyEdge {
                from: "A".into(),
                to: "B".into(),
                kind: DependencyKind::DependsOn,
            },
            DependencyEdge {
                from: "B".into(),
                to: "A".into(),
                kind: DependencyKind::DependsOn,
            },
        ],
        ..Default::default()
    };
    assert!(cycle.validate(&tree).is_err());
}

#[test]
fn validation_rejects_nan_and_out_of_range_leaves() {
    let mut tree = tree_ab();
    tree.leaves.insert("A".into(), f64::NAN);
    let model = DependencyModel::independent();
    assert!(model.validate(&tree).is_ok()); // model validation doesn't check leaves
    let ev = DependencyEvaluator::new(&tree, &model);
    assert!(ev.point_estimate().is_err());
}

// ---- importance ---------------------------------------------------------------

#[test]
fn birnbaum_importance_is_correct_for_or() {
    let tree = tree_ab();
    let model = DependencyModel::independent();
    let entries = importance(&tree, &model).unwrap();
    // For OR(A,B): Birnbaum(A) = P(top|A=1) - P(top|A=0) = 1 - P(B) = 0.995
    let a = entries
        .iter()
        .find(|e| e.id == "A" && !e.is_common_cause)
        .unwrap();
    assert!(
        (a.birnbaum - (1.0 - 0.005)).abs() < 1e-9,
        "got {}",
        a.birnbaum
    );
    // RAW(A) = P(top|A=1)/P(top)
    let top = 1.0 - (1.0 - 0.008) * (1.0 - 0.005);
    assert!((a.raw - 1.0 / top).abs() < 1e-9, "got {}", a.raw);
}

#[test]
fn importance_includes_common_causes() {
    let tree = tree_ab();
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![CommonCause::new("C", 0.003, vec!["A".into(), "B".into()])],
        ..Default::default()
    };
    let entries = importance(&tree, &model).unwrap();
    assert!(
        entries.iter().any(|e| e.is_common_cause && e.id == "C"),
        "common cause must appear in importance"
    );
}

// ---- sensitivity ---------------------------------------------------------------

#[test]
fn sensitivity_preserves_input_identity_and_delta() {
    let tree = tree_ab();
    let model = DependencyModel::independent();
    let entries = sensitivity(&tree, &model, 0.001).unwrap();
    let a = entries.iter().find(|e| e.id == "A").unwrap();
    assert_eq!(a.id, "A");
    assert!((a.baseline - 0.008).abs() < 1e-12);
    // delta = P(top|A=0.009) - P(top|A=0.008), with B fixed.
    let expected = (1.0 - (1.0 - 0.009) * (1.0 - 0.005)) - (1.0 - (1.0 - 0.008) * (1.0 - 0.005));
    assert!(
        (a.delta - expected).abs() < 1e-12,
        "got {}, expected {}",
        a.delta,
        expected
    );
}

// ---- full analysis -------------------------------------------------------------

#[test]
fn full_analysis_is_traceable_and_reproducible() {
    let tree = tree_ab();
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![CommonCause::new("C", 0.003, vec!["A".into(), "B".into()])],
        ..Default::default()
    };
    let config = MonteCarloConfig {
        samples: 2000,
        seed: 7,
        level: 0.95,
    };
    let result = analyze(&tree, &model, &config).unwrap();
    assert_eq!(result.schema, "etdl.reliability.analysis-result/1.0");
    assert_eq!(result.independence, "not-assumed");
    assert_eq!(result.dependencies.len(), 1);
    assert!(result.assumptions.iter().any(|a| a.contains("NOT assumed")));
    assert!(result.point_estimate > 0.0 && result.point_estimate < 1.0);
    let mc = result.uncertainty.as_ref().unwrap();
    assert_eq!(mc.samples, 2000);
    assert_eq!(mc.seed, 7);
    assert!(mc.lower <= mc.median && mc.median <= mc.upper);
}

#[test]
fn monte_carlo_is_reproducible_with_seed() {
    let tree = tree_ab();
    let model = DependencyModel::independent();
    let config = MonteCarloConfig {
        samples: 500,
        seed: 99,
        level: 0.95,
    };
    let a = run_monte_carlo(&tree, &model, &BTreeMap::new(), &config).unwrap();
    let b = run_monte_carlo(&tree, &model, &BTreeMap::new(), &config).unwrap();
    assert_eq!(a.mean, b.mean);
    assert_eq!(a.lower, b.lower);
    assert_eq!(a.upper, b.upper);
}

#[test]
fn monte_carlo_mean_approaches_independent_point() {
    let tree = tree_ab();
    let model = DependencyModel::independent();
    let config = MonteCarloConfig {
        samples: 20_000,
        seed: 1,
        level: 0.95,
    };
    let mc = run_monte_carlo(&tree, &model, &BTreeMap::new(), &config).unwrap();
    let expected = 1.0 - (1.0 - 0.008) * (1.0 - 0.005);
    assert!(
        (mc.mean - expected).abs() < 1e-3,
        "mean {} too far from point {}",
        mc.mean,
        expected
    );
}

// ---- regression: existing examples still compile ------------------------------

#[test]
fn regression_worked_example_unchanged() {
    // The ETDL worked example: OR(0.008, 0.005027) ≈ 0.012987.
    let mut tree = FaultTreeSpec::new("PaymentGatewayFailure");
    tree.leaves.insert("GatewayTimeout".into(), 0.008);
    tree.leaves.insert("GatewayUnreachable".into(), 0.005027);
    tree.gates.insert(
        "PaymentGatewayFailure".into(),
        GateSpec {
            kind: GateKind::Or,
            inputs: vec!["GatewayTimeout".into(), "GatewayUnreachable".into()],
            k: None,
        },
    );
    let model = DependencyModel::independent();
    let ev = DependencyEvaluator::new(&tree, &model);
    let p = ev.point_estimate().unwrap();
    let expected = 1.0 - (1.0 - 0.008) * (1.0 - 0.005027);
    assert!((p - expected).abs() < 1e-6);
    assert!((p - 0.012987).abs() < 0.00001, "got {p}");
}
