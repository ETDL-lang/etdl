//! Sensitivity analysis: mathematical validation.
//!
//! Sensitivity answers "if this input probability were different, how
//! different would the top event be?". It is a *parameter* question. The tests
//! here check the direction, the magnitude, the two-sided behaviour, the
//! elasticity, and the refusal to report a normalised value when its
//! denominator is degenerate.

use etdl_reliability::analysis::dependence::{
    sensitivity, sensitivity_result, CommonCause, DependencyModel, FaultTreeSpec, GateKind,
    GateSpec, IndependenceAssumption,
};

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

/// A simple OR model: perturbing P(A) upward must raise the top event, and
/// downward must lower it, by an exactly predictable amount.
///
/// ```text
/// qA = 0.01, qB = 0.02, delta = 0.001
/// P        = 1 - 0.99   * 0.98 = 0.0298
/// P(q+)    = 1 - 0.989  * 0.98 = 0.03078
/// P(q-)    = 1 - 0.991  * 0.98 = 0.02882
/// dP+ = +0.00098 = +delta * (1 - qB)
/// dP- = -0.00098 = -delta * (1 - qB)
/// ```
///
/// For an OR of independent events the response is exactly linear in each
/// input, so the two legs mirror each other precisely.
#[test]
fn or_model_moves_in_the_expected_direction_by_the_expected_amount() {
    let tree = or_tree(0.01, 0.02);
    let model = DependencyModel::independent();
    let delta = 0.001;
    let result = sensitivity_result(&tree, &model, delta).unwrap();

    let base_top = 1.0 - 0.99 * 0.98;
    assert!((result.top_baseline - base_top).abs() < 1e-15);

    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    let up = a.increase.as_ref().unwrap();
    let down = a.decrease.as_ref().unwrap();

    assert!(up.applied && down.applied);
    assert!((up.input_perturbed - 0.011).abs() < 1e-15);
    assert!((down.input_perturbed - 0.009).abs() < 1e-15);

    let expected = delta * (1.0 - 0.02);
    assert!(
        (up.delta - expected).abs() < 1e-15,
        "increase leg {} vs {expected}",
        up.delta
    );
    assert!(
        (down.delta + expected).abs() < 1e-15,
        "decrease leg {} vs {}",
        down.delta,
        -expected
    );
    assert!(up.delta > 0.0, "increasing P(A) must raise P(TOP)");
    assert!(down.delta < 0.0, "decreasing P(A) must lower P(TOP)");

    // OR of independents is linear, so the legs mirror.
    assert_eq!(a.two_sided_symmetric, Some(true));
}

/// Elasticity `(dP/P) / (dq/q)` for the same model.
///
/// ```text
/// dP/P = 0.00098 / 0.0298 = 0.0328859...
/// dq/q = 0.001   / 0.01   = 0.1
/// elasticity = 0.328859...
/// ```
///
/// Interpretation: a 1% relative increase in P(A) raises P(TOP) by about
/// 0.33%. A is a minority contributor here, and the elasticity says so on a
/// scale that is comparable across inputs of very different magnitudes.
#[test]
fn relative_sensitivity_is_the_elasticity() {
    let result = sensitivity_result(&or_tree(0.01, 0.02), &DependencyModel::independent(), 0.001)
        .unwrap();
    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    let top = 1.0 - 0.99 * 0.98;
    let expected = (0.001 * 0.98 / top) / (0.001 / 0.01);
    let got = a.relative_sensitivity.unwrap();
    assert!((got - expected).abs() < 1e-12, "elasticity {got} vs {expected}");
    assert!((got - 0.328859).abs() < 1e-5, "elasticity {got}");

    // B has the larger probability, so also the larger elasticity.
    let b = result.entries.iter().find(|e| e.id == "B").unwrap();
    assert!(b.relative_sensitivity.unwrap() > got);
}

/// Elasticity is withheld, not fudged, when a denominator is zero.
#[test]
fn zero_denominators_yield_no_relative_sensitivity() {
    // Baseline input probability zero.
    let result =
        sensitivity_result(&or_tree(0.0, 0.02), &DependencyModel::independent(), 1e-3).unwrap();
    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    assert!(a.relative_sensitivity.is_none());
    // Absolute sensitivity is still reported.
    assert!(a.increase.as_ref().unwrap().delta > 0.0);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "RA006" && d.subject.as_deref() == Some("A")));

    // Zero top-event probability: an AND with a zero input.
    let mut tree = FaultTreeSpec::new("TOP");
    tree.leaves.insert("A".into(), 0.0);
    tree.leaves.insert("B".into(), 0.0);
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::And,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    let result = sensitivity_result(&tree, &DependencyModel::independent(), 1e-3).unwrap();
    assert_eq!(result.top_baseline, 0.0);
    for e in &result.entries {
        assert!(e.relative_sensitivity.is_none());
    }
}

/// Perturbations that would leave [0,1] are clamped, and a leg that cannot
/// move is reported as not applied rather than silently dropped.
#[test]
fn boundary_inputs_report_unapplied_legs() {
    // q = 1: cannot increase.
    let result =
        sensitivity_result(&or_tree(1.0, 0.02), &DependencyModel::independent(), 1e-3).unwrap();
    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    let up = a.increase.as_ref().unwrap();
    assert!(!up.applied, "increase from q=1 cannot be applied");
    assert_eq!(up.delta, 0.0);
    assert!(a.decrease.as_ref().unwrap().applied);
    // Only one leg moved, so symmetry is unknown rather than assumed.
    assert_eq!(a.two_sided_symmetric, None);

    // q = 0: cannot decrease.
    let result =
        sensitivity_result(&or_tree(0.0, 0.02), &DependencyModel::independent(), 1e-3).unwrap();
    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    assert!(!a.decrease.as_ref().unwrap().applied);
    assert!(a.increase.as_ref().unwrap().applied);
}

/// A voting gate is nonlinear, so the two legs must not mirror each other.
/// This is exactly why the analysis evaluates both directions instead of
/// assuming symmetry.
///
/// ```text
/// 2-of-3 with q = 0.5 each: P = 3q^2(1-q) + q^3 = 0.5
/// Perturb A by +/- 0.2 while B and C stay at 0.5:
///   P(TOP) = qA * [1-(1-q)^2] + (1-qA) * q^2      (A in / A out)
///          = qA * 0.75 + (1-qA) * 0.25
/// which IS linear in qA. So the asymmetry must be sought where the
/// perturbed input appears more than once - here, in a common cause.
/// ```
#[test]
fn voting_gate_response_is_linear_in_a_single_input() {
    let mut tree = FaultTreeSpec::new("TOP");
    for id in ["A", "B", "C"] {
        tree.leaves.insert(id.into(), 0.5);
    }
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::Voting,
            inputs: vec!["A".into(), "B".into(), "C".into()],
            k: Some(2),
        },
    );
    let result = sensitivity_result(&tree, &DependencyModel::independent(), 0.2).unwrap();
    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    let up = a.increase.as_ref().unwrap();
    let down = a.decrease.as_ref().unwrap();
    // P = 0.25 + 0.5*qA, so dP = +/- 0.1 for a 0.2 perturbation.
    assert!((up.delta - 0.1).abs() < 1e-12, "up {}", up.delta);
    assert!((down.delta + 0.1).abs() < 1e-12, "down {}", down.delta);
    assert_eq!(a.two_sided_symmetric, Some(true));
}

/// A common cause makes the response genuinely asymmetric, because the cause's
/// probability enters the conditioning sum more than once.
///
/// TOP = AND(A, B) with a shared cause C. Raising P(C) pushes probability mass
/// into the state where both components are down (top = 1) while simultaneously
/// lowering the residual independent probabilities. The two effects do not
/// cancel linearly, so the up and down legs differ.
#[test]
fn common_cause_response_is_asymmetric() {
    let mut tree = FaultTreeSpec::new("TOP");
    tree.leaves.insert("A".into(), 0.2);
    tree.leaves.insert("B".into(), 0.2);
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::And,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        common_causes: vec![CommonCause::new("C", 0.05, vec!["A".into(), "B".into()])],
        ..Default::default()
    };
    let result = sensitivity_result(&tree, &model, 0.04).unwrap();
    let c = result.entries.iter().find(|e| e.id == "C").unwrap();
    assert!(c.is_common_cause);
    assert_eq!(c.entity_kind, "common-cause");
    let up = c.increase.as_ref().unwrap();
    let down = c.decrease.as_ref().unwrap();
    assert!(up.applied && down.applied);
    assert!(up.delta > 0.0, "raising the shared cause raises the top event");
    assert!(down.delta < 0.0);
    assert_eq!(
        c.two_sided_symmetric,
        Some(false),
        "common-cause response must be reported as asymmetric: up {} down {}",
        up.delta,
        down.delta
    );
}

/// A common cause already at its structural ceiling cannot be raised, and that
/// is reported rather than silently producing a zero delta.
#[test]
fn common_cause_at_its_ceiling_reports_an_unapplied_leg() {
    let mut tree = FaultTreeSpec::new("TOP");
    tree.leaves.insert("A".into(), 0.05);
    tree.leaves.insert("B".into(), 0.30);
    tree.gates.insert(
        "TOP".into(),
        GateSpec {
            kind: GateKind::And,
            inputs: vec!["A".into(), "B".into()],
            k: None,
        },
    );
    let model = DependencyModel {
        independence: IndependenceAssumption::NotAssumed,
        // C is already equal to the smallest affected probability (0.05).
        common_causes: vec![CommonCause::new("C", 0.05, vec!["A".into(), "B".into()])],
        ..Default::default()
    };
    let result = sensitivity_result(&tree, &model, 0.01).unwrap();
    let c = result.entries.iter().find(|e| e.id == "C").unwrap();
    assert!(!c.increase.as_ref().unwrap().applied);
    assert!(c.decrease.as_ref().unwrap().applied);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.subject.as_deref() == Some("C") && d.message.contains("ceiling")));
}

/// Rare-event inputs keep their sensitivity accurate.
#[test]
fn rare_event_sensitivity_is_accurate() {
    let model = DependencyModel::independent();
    for exp in [3i32, 6, 9, 12] {
        let q = 10f64.powi(-exp);
        let delta = q / 10.0;
        let entries = sensitivity(&or_tree(q, q), &model, delta).unwrap();
        let a = entries.iter().find(|e| e.id == "A").unwrap();
        // dP = delta * (1 - qB) exactly.
        let expected = delta * (1.0 - q);
        let got = a.increase.as_ref().unwrap().delta;
        let rel = (got - expected).abs() / expected;
        assert!(
            rel < 1e-9,
            "1e-{exp}: relative error {rel} (got {got}, expected {expected})"
        );
    }
}

/// The result preserves identity, method, direction and provenance — never a
/// bare number.
#[test]
fn result_preserves_identity_and_method() {
    let result =
        sensitivity_result(&or_tree(0.01, 0.02), &DependencyModel::independent(), 1e-3).unwrap();
    assert_eq!(result.schema, "etdl.reliability.sensitivity-result/1.0");
    assert_eq!(result.top_event, "TOP");
    assert_eq!(result.method, "finite-perturbation/absolute/two-sided");
    assert_eq!(result.method_version, "1");
    assert!((result.perturbation - 1e-3).abs() < 1e-18);
    assert!(result.assumptions.iter().any(|a| a.contains("one input at a time")));
    assert!(result.assumptions.iter().any(|a| a.contains("elasticity")));

    let a = result.entries.iter().find(|e| e.id == "A").unwrap();
    assert_eq!(a.id, "A");
    assert_eq!(a.entity_kind, "basic-event");
    assert!((a.baseline - 0.01).abs() < 1e-18);
    assert_eq!(a.increase.as_ref().unwrap().direction, "increase");
    assert_eq!(a.decrease.as_ref().unwrap().direction, "decrease");
    assert!(a.method.contains("two-sided"));
    // Ranked by |delta| descending: B moves the top event more than A.
    assert_eq!(result.entries[0].id, "B");
}

/// An invalid perturbation is rejected instead of being silently replaced.
#[test]
fn invalid_perturbation_is_rejected() {
    let tree = or_tree(0.01, 0.02);
    let model = DependencyModel::independent();
    for delta in [0.0, -1e-3, f64::NAN, f64::INFINITY] {
        assert!(
            sensitivity_result(&tree, &model, delta).is_err(),
            "perturbation {delta} must be rejected"
        );
    }
}

/// Sensitivity and importance are different numbers for the same input, and
/// neither is derivable from the other by inspection.
#[test]
fn sensitivity_is_not_importance() {
    use etdl_reliability::analysis::dependence::importance;
    let tree = or_tree(0.01, 0.02);
    let model = DependencyModel::independent();

    let imp = importance(&tree, &model).unwrap();
    let sens = sensitivity(&tree, &model, 1e-3).unwrap();

    let a_imp = imp.iter().find(|e| e.id == "A").unwrap();
    let a_sens = sens.iter().find(|e| e.id == "A").unwrap();

    // Birnbaum(A) = 1 - qB = 0.98; sensitivity delta = 9.8e-4.
    assert!((a_imp.birnbaum - 0.98).abs() < 1e-12);
    assert!((a_sens.delta - 9.8e-4).abs() < 1e-15);
    // They differ by the perturbation size; the analysis reports both rather
    // than deriving one from the other and calling it the same thing.
    assert!((a_imp.birnbaum - a_sens.delta).abs() > 0.9);

    // Importance ranks B above A (0.99 vs 0.98) and so does sensitivity here,
    // but the VALUES are on different scales and must never be compared across
    // metrics.
    assert!(imp[0].id == "B" && sens[0].id == "B");
}
