//! Importance analysis: mathematical validation.
//!
//! Every measure is checked against a hand-derived closed-form result, with
//! the derivation written out in the test. A test that only asserts "the
//! number is positive" would not catch a wrong definition, which is the
//! failure mode that matters here.

use etdl_reliability::analysis::dependence::{
    importance, importance_result, is_coherent, CommonCause, DependencyModel, FaultTreeSpec,
    GateKind, GateSpec, IndependenceAssumption,
};
use etdl_reliability::analysis::ImportanceMetric;

fn or_tree(a: f64, b: f64) -> FaultTreeSpec {
    let mut tree = FaultTreeSpec::new("TOP");
    tree.leaves.insert("A".into(), a);
    tree.leaves.insert("B".into(), b);
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::Or,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    tree
}

fn and_tree(a: f64, b: f64) -> FaultTreeSpec {
    let mut tree = FaultTreeSpec::new("TOP");
    tree.leaves.insert("A".into(), a);
    tree.leaves.insert("B".into(), b);
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::And,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    tree
}

/// Two-event OR model, fully derived by hand.
///
/// ```text
/// qA = 0.02, qB = 0.05, independent
/// P(TOP) = 1 - (1-qA)(1-qB) = 1 - 0.98*0.95 = 1 - 0.931 = 0.069
///
/// P(TOP | A=1) = 1                    (OR is satisfied by A alone)
/// P(TOP | A=0) = qB = 0.05
///
/// Birnbaum(A)       = 1 - 0.05                    = 0.95
/// Fussell-Vesely(A) = (0.069 - 0.05) / 0.069      = 0.2753623188...
/// Criticality(A)    = 0.95 * 0.02 / 0.069         = 0.2753623188...
/// RAW(A)            = 1 / 0.069                   = 14.4927536...
/// RRW(A)            = 0.069 / 0.05                = 1.38
/// ```
///
/// Criticality and Fussell-Vesely coincide here; that is a property of a
/// two-event OR (`I_B(A)*qA = (1-qB)*qA = P - qB`), not an implementation
/// accident, and it is a useful cross-check on both formulas.
#[test]
fn two_event_or_matches_hand_derivation() {
    let tree = or_tree(0.02, 0.05);
    let model = DependencyModel::independent();
    let result = importance_result(&tree, &model).unwrap();

    let p_top = 1.0 - 0.98 * 0.95;
    assert!((result.baseline_top_probability - p_top).abs() < 1e-15);
    assert!((p_top - 0.069).abs() < 1e-15);
    assert!(result.coherent);

    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    assert!((a.top_given_occurred - 1.0).abs() < 1e-15);
    assert!((a.top_given_absent - 0.05).abs() < 1e-15);
    assert!((a.birnbaum - 0.95).abs() < 1e-12, "Birnbaum {}", a.birnbaum);
    assert!(
        (a.fussell_vesely.unwrap() - (0.069 - 0.05) / 0.069).abs() < 1e-12,
        "FV {:?}",
        a.fussell_vesely
    );
    assert!(
        (a.criticality.unwrap() - 0.95 * 0.02 / 0.069).abs() < 1e-12,
        "CI {:?}",
        a.criticality
    );
    assert!((a.raw - 1.0 / 0.069).abs() < 1e-9, "RAW {}", a.raw);
    assert!((a.rrw - 0.069 / 0.05).abs() < 1e-12, "RRW {}", a.rrw);

    // The two coincide for a two-event OR.
    assert!((a.fussell_vesely.unwrap() - a.criticality.unwrap()).abs() < 1e-12);

    let b = result.entries.iter().find(|e| e.id == "B").unwrap();
    // Birnbaum(B) = 1 - qA = 0.98. B has the larger probability, so it also has
    // the larger Fussell-Vesely, but the SMALLER Birnbaum ordering is reversed
    // relative to A: importance and probability are not the same ranking.
    assert!((b.birnbaum - 0.98).abs() < 1e-12);
    assert!(b.birnbaum > a.birnbaum);
    assert!(b.fussell_vesely.unwrap() > a.fussell_vesely.unwrap());
}

/// Birnbaum importance equals the partial derivative for a coherent tree with
/// independent basic events, because `P(top)` is multilinear in the `q_i`.
///
/// ```text
/// P = 1 - (1-qA)(1-qB)  =>  dP/dqA = 1 - qB
/// ```
///
/// The implementation computes the conditioning form, so agreement with a
/// numerical derivative is a genuine check that the conditioning form is the
/// correct formal definition rather than an approximation of it.
#[test]
fn birnbaum_equals_the_partial_derivative() {
    let model = DependencyModel::independent();
    for (qa, qb) in [(0.02, 0.05), (0.3, 0.4), (1e-6, 1e-3)] {
        let entries = importance(&or_tree(qa, qb), &model).unwrap();
        let a = entries.iter().find(|e| e.id == "A").unwrap();

        // Central difference on the closed form.
        let h = 1e-7;
        let p = |x: f64| 1.0 - (1.0 - x) * (1.0 - qb);
        let numeric = (p(qa + h) - p(qa - h)) / (2.0 * h);

        assert!(
            (a.birnbaum - numeric).abs() < 1e-6,
            "qA={qa}: conditioning {} vs derivative {numeric}",
            a.birnbaum
        );
        assert!((a.birnbaum - (1.0 - qb)).abs() < 1e-12);
    }
}

/// AND model, derived by hand.
///
/// ```text
/// qA = 0.1, qB = 0.2, P(TOP) = 0.02
/// P(TOP | A=1) = qB = 0.2 ; P(TOP | A=0) = 0
/// Birnbaum(A) = 0.2 ; FV(A) = (0.02 - 0)/0.02 = 1 ; RRW(A) = inf
/// ```
///
/// Fussell-Vesely of 1 is correct and meaningful: in an AND model every basic
/// event appears in the single minimal cut set, so eliminating any one of them
/// eliminates the top event entirely.
#[test]
fn and_model_gives_unit_fussell_vesely_and_infinite_rrw() {
    let tree = and_tree(0.1, 0.2);
    let model = DependencyModel::independent();
    let result = importance_result(&tree, &model).unwrap();
    assert!((result.baseline_top_probability - 0.02).abs() < 1e-15);

    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    assert!((a.birnbaum - 0.2).abs() < 1e-15);
    assert!((a.top_given_absent - 0.0).abs() < 1e-15);
    assert!((a.fussell_vesely.unwrap() - 1.0).abs() < 1e-15);
    assert!(a.rrw.is_infinite());
    // Criticality(A) = 0.2 * 0.1 / 0.02 = 1.0
    assert!((a.criticality.unwrap() - 1.0).abs() < 1e-15);
}

/// A common cause with a *smaller* probability than an independent event can
/// still be the more important entity, because it defeats redundancy.
///
/// ```text
/// TOP = AND(A, B)      qA = qB = 0.05
/// Common cause C (P = 0.01) affects both A and B.
///
/// Conditioning on C:
///   C occurs   (p = 0.01):  A = B = 1  ->  P(TOP | C) = 1
///   C absent   (p = 0.99):  q' = (0.05 - 0.01)/0.99 = 0.040404...
///                           P(TOP | not C) = q'^2 = 0.001632...
///   P(TOP) = 0.01*1 + 0.99*0.00163249... = 0.011616...
///
/// Importance of C conditions on C itself, not on the leaves it affects:
///   P(TOP | C occurs) = 1                        (both components down)
///   P(TOP | C absent) = q'^2 = 0.00163249...     (residual paths remain)
///   Birnbaum(C) = 1 - 0.00163249... = 0.99836750...
///
/// Conditioning on "C absent" must leave A and B at their RESIDUAL
/// probability q' = 0.0404..., not at their full 0.05: the shared path has
/// been removed, not the independent ones.
///
/// Birnbaum(A) = P(TOP | A=1) - P(TOP | A=0) is far smaller, because B must
/// still fail for the AND to be satisfied.
/// ```
///
/// Without the common cause the AND would give 0.0025; the shared cause raises
/// the top event by roughly 4.6x. That is the entire engineering point of
/// modelling it.
#[test]
fn common_cause_outranks_a_larger_independent_event() {
    let tree = and_tree(0.05, 0.05);
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![CommonCause::new(
            "SharedPowerFailure",
            0.01,
            vec!["A".into(), "B".into()],
        )],
        ..Default::default()
    };
    let result = importance_result(&tree, &model).unwrap();

    let residual = (0.05 - 0.01) / 0.99;
    let expected_top = 0.01 * 1.0 + 0.99 * residual * residual;
    assert!(
        (result.baseline_top_probability - expected_top).abs() < 1e-12,
        "top {} vs {expected_top}",
        result.baseline_top_probability
    );
    // The independence answer would have been 0.0025.
    assert!(result.baseline_top_probability > 4.0 * 0.0025);

    let cc = result
        .entries
        .iter()
        .find(|e| e.id == "SharedPowerFailure")
        .unwrap();
    let a = result.entries.iter().find(|e| e.id == "A").unwrap();

    assert!(cc.is_common_cause);
    assert_eq!(cc.entity_kind, "common-cause");
    assert_eq!(a.entity_kind, "basic-event");
    // The result identifies the actual model entity, not the leaves it affects.
    assert_eq!(cc.id, "SharedPowerFailure");
    assert_eq!(cc.event_probability, Some(0.01));

    assert!((cc.top_given_occurred - 1.0).abs() < 1e-12);
    assert!(
        (cc.top_given_absent - residual * residual).abs() < 1e-12,
        "P(TOP | C absent) must use the residual probability: {} vs {}",
        cc.top_given_absent,
        residual * residual
    );
    assert!(
        (cc.birnbaum - (1.0 - residual * residual)).abs() < 1e-12,
        "CCF Birnbaum {}",
        cc.birnbaum
    );
    assert!(
        cc.birnbaum > a.birnbaum,
        "the common cause (q=0.01) must outrank the larger independent event \
         (q=0.05): {} vs {}",
        cc.birnbaum,
        a.birnbaum
    );
    // Ranking is by Birnbaum descending, so the common cause leads.
    assert_eq!(result.entries[0].id, "SharedPowerFailure");
}

/// Fussell-Vesely and criticality are undefined for a non-coherent tree and
/// are reported as absent rather than computed anyway.
#[test]
fn non_coherent_tree_withholds_coherence_dependent_measures() {
    let mut tree = FaultTreeSpec::new("TOP");
    tree.leaves.insert("A".into(), 0.3);
    tree.leaves.insert("B".into(), 0.4);
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::Xor,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    assert!(!is_coherent(&tree));

    let result = importance_result(&tree, &DependencyModel::independent()).unwrap();
    assert!(!result.coherent);
    for e in &result.entries {
        assert!(e.fussell_vesely.is_none(), "FV must not be reported");
        assert!(e.criticality.is_none(), "criticality must not be reported");
        // Birnbaum stays well defined for a non-coherent tree.
        assert!(e.birnbaum.is_finite());
    }
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "RA008" && d.message.contains("coherent")));
    assert!(!result.measures.contains(&"fussell-vesely".to_string()));
    assert!(result.measures.contains(&"birnbaum".to_string()));
}

/// Edge cases: q = 0, q = 1, and a zero top event.
#[test]
fn boundary_probabilities_are_handled_without_fabrication() {
    let model = DependencyModel::independent();

    // q = 0: the event cannot occur, but its Birnbaum importance is still the
    // structural quantity 1 - qB. Importance is not a probability.
    let entries = importance(&or_tree(0.0, 0.05), &model).unwrap();
    let a = entries.iter().find(|e| e.id == "A").unwrap();
    assert!((a.birnbaum - 0.95).abs() < 1e-12);
    // ...while its criticality IS zero, because criticality weights by q.
    assert!((a.criticality.unwrap() - 0.0).abs() < 1e-15);
    assert!((a.fussell_vesely.unwrap() - 0.0).abs() < 1e-15);

    // q = 1: the top event is certain.
    let entries = importance(&or_tree(1.0, 0.05), &model).unwrap();
    let a = entries.iter().find(|e| e.id == "A").unwrap();
    assert!((a.raw - 1.0).abs() < 1e-12);

    // Zero top event: normalised measures are withheld.
    let result = importance_result(&and_tree(0.0, 0.5), &model).unwrap();
    assert_eq!(result.baseline_top_probability, 0.0);
    assert!(result.diagnostics.iter().any(|d| d.code == "RA009"));
    for e in &result.entries {
        assert!(e.fussell_vesely.is_none());
        assert!(e.criticality.is_none());
    }
}

/// A single-event tree with no gates.
#[test]
fn single_event_tree() {
    let mut tree = FaultTreeSpec::new("A");
    tree.leaves.insert("A".into(), 0.25);
    let result = importance_result(&tree, &DependencyModel::independent()).unwrap();
    assert!((result.baseline_top_probability - 0.25).abs() < 1e-15);
    let a = &result.entries[0];
    // P(top|A=1) = 1, P(top|A=0) = 0 -> Birnbaum = 1, FV = 1.
    assert!((a.birnbaum - 1.0).abs() < 1e-15);
    assert!((a.fussell_vesely.unwrap() - 1.0).abs() < 1e-15);
    // Criticality = I_B * q / P = 1 * 0.25 / 0.25 = 1.
    assert!((a.criticality.unwrap() - 1.0).abs() < 1e-15);
}

/// A 2-of-3 voting gate, derived by hand.
///
/// ```text
/// q = 0.1 for all three inputs.
/// P(2oo3) = 3q^2(1-q) + q^3 = 3*0.01*0.9 + 0.001 = 0.027 + 0.001 = 0.028
///
/// P(TOP | A=1) = P(at least 1 of B,C) = 1-(1-q)^2 = 1 - 0.81 = 0.19
/// P(TOP | A=0) = P(both B and C)      = q^2       = 0.01
/// Birnbaum(A)  = 0.19 - 0.01 = 0.18
/// ```
#[test]
fn voting_gate_matches_hand_derivation() {
    let mut tree = FaultTreeSpec::new("TOP");
    for id in ["A", "B", "C"] {
        tree.leaves.insert(id.into(), 0.1);
    }
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::Voting,
            inputs: vec!["A".into(), "B".into(), "C".into()],
            k: Some(2),
        },
    );
    let result = importance_result(&tree, &DependencyModel::independent()).unwrap();
    assert!(
        (result.baseline_top_probability - 0.028).abs() < 1e-12,
        "2oo3 top {}",
        result.baseline_top_probability
    );
    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    assert!((a.top_given_occurred - 0.19).abs() < 1e-12);
    assert!((a.top_given_absent - 0.01).abs() < 1e-12);
    assert!((a.birnbaum - 0.18).abs() < 1e-12, "Birnbaum {}", a.birnbaum);
}

/// Nested gates: OR(AND(A,B), C).
///
/// ```text
/// qA=0.1 qB=0.2 qC=0.05
/// G = AND(A,B) = 0.02
/// TOP = OR(G, C) = 1 - (1-0.02)(1-0.05) = 1 - 0.98*0.95 = 0.069
/// P(TOP|A=1) = 1 - (1-qB)(1-qC) = 1 - 0.8*0.95 = 0.24
/// P(TOP|A=0) = qC = 0.05
/// Birnbaum(A) = 0.19
/// ```
#[test]
fn nested_gates_match_hand_derivation() {
    let mut tree = FaultTreeSpec::new("TOP");
    tree.leaves.insert("A".into(), 0.1);
    tree.leaves.insert("B".into(), 0.2);
    tree.leaves.insert("C".into(), 0.05);
    tree.gates.insert(
        "G".into(),
        GateSpec {
            kind: GateKind::And,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::Or,
            inputs: vec!["G".into(), "C".into()],
            k: None,
        },
    );
    let result = importance_result(&tree, &DependencyModel::independent()).unwrap();
    assert!((result.baseline_top_probability - 0.069).abs() < 1e-12);
    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    assert!((a.top_given_occurred - 0.24).abs() < 1e-12);
    assert!((a.top_given_absent - 0.05).abs() < 1e-12);
    assert!((a.birnbaum - 0.19).abs() < 1e-12);
    // C is the single most important: Birnbaum(C) = 1 - 0.02 = 0.98.
    let c = result.entries.iter().find(|e| e.id == "C").unwrap();
    assert!((c.birnbaum - 0.98).abs() < 1e-12);
    assert_eq!(result.entries[0].id, "C");
}

/// Rare-event range: importance must stay accurate at 1e-3 through 1e-12.
#[test]
fn rare_event_importance_is_accurate() {
    let model = DependencyModel::independent();
    for exp in [3i32, 6, 9, 12] {
        let q = 10f64.powi(-exp);
        let entries = importance(&or_tree(q, q), &model).unwrap();
        let a = entries.iter().find(|e| e.id == "A").unwrap();
        // Birnbaum(A) = 1 - qB exactly.
        let expected = 1.0 - q;
        assert!(
            (a.birnbaum - expected).abs() < 1e-15,
            "1e-{exp}: Birnbaum {} vs {expected}",
            a.birnbaum
        );
        // FV(A) = (P - qB)/P where P = 1-(1-q)^2 = 2q - q^2.
        let p = 2.0 * q - q * q;
        let fv_expected = (p - q) / p;
        let fv = a.fussell_vesely.unwrap();
        assert!(
            (fv - fv_expected).abs() < 1e-9,
            "1e-{exp}: FV {fv} vs {fv_expected}"
        );
        // For two equal rare events FV -> 1/2 as q -> 0.
        assert!((fv - 0.5).abs() < 1e-3);
    }
}

/// The result identifies the metric, the entity and the method — never a bare
/// float.
#[test]
fn result_carries_metric_identity_and_provenance() {
    let result = importance_result(&or_tree(0.02, 0.05), &DependencyModel::independent()).unwrap();
    assert_eq!(result.schema, "etdl.reliability.importance-result/1.0");
    assert_eq!(result.top_event, "TOP");
    assert_eq!(result.method, "exact-conditioning");
    assert_eq!(result.method_version, "1");
    assert!(result.measures.contains(&ImportanceMetric::Birnbaum.name().to_string()));
    assert!(result
        .measures
        .contains(&ImportanceMetric::FussellVesely.name().to_string()));
    assert!(!result.assumptions.is_empty());
    assert!(result
        .assumptions
        .iter()
        .any(|a| a.contains("not a probability")));

    // Ranking by a named measure is available and deterministic.
    let ranked = result.ranked_by("birnbaum");
    assert_eq!(ranked.len(), 2);
    assert!(ranked[0].1 >= ranked[1].1);
    assert!(result.ranked_by("no-such-measure").is_empty());
}

/// A declared conditional probability is refused, not silently ignored.
#[test]
fn unsupported_dependency_is_refused_for_importance() {
    use etdl_reliability::analysis::dependence::ConditionalProbability;
    let tree = or_tree(0.02, 0.05);
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
    let err = importance_result(&tree, &model).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("RA001"), "diagnostic code missing: {msg}");
    assert!(msg.contains("conditional probability"), "{msg}");
}
