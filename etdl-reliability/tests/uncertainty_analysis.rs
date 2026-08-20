//! Uncertainty propagation: statistical validation.
//!
//! These tests check propagation against distributions whose moments are known
//! in closed form, verify seed reproducibility and sample-count handling, and
//! confirm that the implementation refuses to describe an interval as
//! something it is not.

use std::collections::BTreeMap;

use etdl_reliability::analysis::dependence::{
    analyze_with, propagate, AnalysisOptions, CommonCause, DependencyModel, FaultTreeSpec,
    GateKind, GateSpec, IndependenceAssumption, InputUncertainty, IntervalMeaning,
    MonteCarloConfig, PropagationSemantics,
};
use etdl_reliability::uncertainty::{ConfidenceInterval, Interval, Uncertainty};

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

fn config(samples: usize, seed: u64) -> MonteCarloConfig {
    MonteCarloConfig {
        samples,
        seed,
        level: 0.95,
    }
}

/// Beta inputs, propagated through an OR gate, against the closed form.
///
/// ```text
/// A ~ Beta(2, 198)   E[A] = 2/200   = 0.01     Var = ab/((a+b)^2(a+b+1))
/// B ~ Beta(4, 196)   E[B] = 4/200   = 0.02
///
/// TOP = 1 - (1-A)(1-B) = A + B - AB
/// A and B independent, so
///   E[TOP] = E[A] + E[B] - E[A]E[B] = 0.01 + 0.02 - 0.0002 = 0.0298
/// ```
///
/// The propagated mean must land on 0.0298 within Monte Carlo error, and the
/// interval must actually bracket it — this is what distinguishes real
/// propagation from evaluating the point estimate and attaching the input
/// interval to the answer.
#[test]
fn beta_inputs_propagate_to_the_closed_form_mean() {
    let mut laws = BTreeMap::new();
    laws.insert(
        "A".to_string(),
        InputUncertainty::Beta {
            alpha: 2.0,
            beta: 198.0,
        },
    );
    laws.insert(
        "B".to_string(),
        InputUncertainty::Beta {
            alpha: 4.0,
            beta: 196.0,
        },
    );

    let (r, _) = propagate(
        &or_tree(0.01, 0.02),
        &DependencyModel::independent(),
        &laws,
        &config(50_000, 2024),
    )
    .unwrap();

    let expected_mean = 0.01 + 0.02 - 0.01 * 0.02;
    // Closed-form variance of TOP = A + B - AB, to first order the sum of the
    // input variances; the standard error of the mean bounds the check.
    assert!(
        (r.mean - expected_mean).abs() < 6.0 * r.convergence.standard_error,
        "mean {} vs {expected_mean} (se {})",
        r.mean,
        r.convergence.standard_error
    );

    // The interval must have real width and bracket the point estimate.
    assert!(r.upper > r.lower, "interval must not be degenerate");
    assert!(r.lower < expected_mean && expected_mean < r.upper);
    assert_eq!(r.variable_inputs, 2);
    assert!(r.std_dev > 0.0);

    // Ordering invariants.
    assert!(r.min <= r.lower && r.lower <= r.median);
    assert!(r.median <= r.upper && r.upper <= r.max);
    assert!(r.quantiles["0.5000"] == r.median);
}

/// A single uniform input has an exactly known propagated distribution.
///
/// ```text
/// A ~ Uniform[0.00, 0.04], B fixed at 0
/// TOP = A, so the propagated distribution IS Uniform[0, 0.04].
/// E[TOP] = 0.02 ; sd = 0.04/sqrt(12) = 0.011547...
/// 95% central interval = [0.001, 0.039]
/// ```
///
/// Checking against the exact quantiles, not a broad plausible range, is what
/// makes this a real test of the sampler and the quantile definition.
#[test]
fn uniform_input_reproduces_its_own_quantiles() {
    let mut laws = BTreeMap::new();
    laws.insert(
        "A".to_string(),
        InputUncertainty::Uniform {
            lower: 0.0,
            upper: 0.04,
        },
    );
    laws.insert("B".to_string(), InputUncertainty::fixed(0.0));

    let (r, _) = propagate(
        &or_tree(0.02, 0.0),
        &DependencyModel::independent(),
        &laws,
        &config(200_000, 7),
    )
    .unwrap();

    assert!((r.mean - 0.02).abs() < 1e-3, "mean {}", r.mean);
    let exact_sd = 0.04 / 12f64.sqrt();
    assert!((r.std_dev - exact_sd).abs() < 1e-3, "sd {}", r.std_dev);
    assert!((r.lower - 0.001).abs() < 5e-4, "2.5% quantile {}", r.lower);
    assert!((r.upper - 0.039).abs() < 5e-4, "97.5% quantile {}", r.upper);
    assert!((r.median - 0.02).abs() < 5e-4, "median {}", r.median);
    // Samples must respect the declared support.
    assert!(r.min >= 0.0 && r.max <= 0.04 + 1e-12);
}

/// Reproducibility: same model, inputs, seed, sample count and analyzer
/// version give bit-identical results; a different seed does not.
#[test]
fn seeded_runs_are_bit_reproducible() {
    let mut laws = BTreeMap::new();
    laws.insert(
        "A".to_string(),
        InputUncertainty::Beta {
            alpha: 2.0,
            beta: 198.0,
        },
    );
    let tree = or_tree(0.01, 0.02);
    let model = DependencyModel::independent();

    let (a, _) = propagate(&tree, &model, &laws, &config(3_000, 99)).unwrap();
    let (b, _) = propagate(&tree, &model, &laws, &config(3_000, 99)).unwrap();
    assert_eq!(a.mean.to_bits(), b.mean.to_bits());
    assert_eq!(a.lower.to_bits(), b.lower.to_bits());
    assert_eq!(a.upper.to_bits(), b.upper.to_bits());
    assert_eq!(a.std_dev.to_bits(), b.std_dev.to_bits());
    assert_eq!(a.quantiles, b.quantiles);

    let (c, _) = propagate(&tree, &model, &laws, &config(3_000, 100)).unwrap();
    assert_ne!(a.mean.to_bits(), c.mean.to_bits());

    // The sampler identity is recorded so a future run can be matched to this
    // one.
    assert_eq!(a.sampler, "xorshift64star");
    assert_eq!(a.sampler_version, "1");
    assert_eq!(a.method, "monte-carlo-propagation");
    assert_eq!(a.seed, 99);
}

/// Sample count is honoured exactly and validated.
#[test]
fn sample_count_is_respected_and_validated() {
    let mut laws = BTreeMap::new();
    laws.insert(
        "A".to_string(),
        InputUncertainty::Beta {
            alpha: 2.0,
            beta: 198.0,
        },
    );
    let tree = or_tree(0.01, 0.02);
    let model = DependencyModel::independent();

    for n in [1usize, 2, 999, 5_000] {
        let (r, diags) = propagate(&tree, &model, &laws, &config(n, 5)).unwrap();
        assert_eq!(r.samples, n);
        if n < 1_000 {
            assert!(
                diags.iter().any(|d| d.code == "RA003"),
                "{n} samples must raise an insufficient-samples diagnostic"
            );
        }
    }

    assert!(propagate(&tree, &model, &laws, &config(0, 5)).is_err());
}

/// Confidence and credible inputs are not silently merged, and the output
/// never claims coverage it does not have.
#[test]
fn output_semantics_are_stated_honestly() {
    let tree = or_tree(0.01, 0.02);
    let model = DependencyModel::independent();

    // All-Beta inputs: the output is a credible interval.
    let mut credible = BTreeMap::new();
    credible.insert(
        "A".to_string(),
        InputUncertainty::Beta {
            alpha: 2.0,
            beta: 198.0,
        },
    );
    credible.insert(
        "B".to_string(),
        InputUncertainty::Beta {
            alpha: 4.0,
            beta: 196.0,
        },
    );
    let (r, _) = propagate(&tree, &model, &credible, &config(2_000, 1)).unwrap();
    assert_eq!(r.semantics, PropagationSemantics::PropagatedCredible);
    assert!(r.interpretation.contains("credible"));

    // All-confidence inputs: the output explicitly disclaims coverage.
    let mut frequentist = BTreeMap::new();
    for (id, lo, hi) in [("A", 0.005, 0.015), ("B", 0.015, 0.025)] {
        frequentist.insert(
            id.to_string(),
            InputUncertainty::NormalFromInterval {
                lower: lo,
                upper: hi,
                level: 0.95,
                meaning: IntervalMeaning::Confidence,
            },
        );
    }
    let (r, _) = propagate(&tree, &model, &frequentist, &config(2_000, 1)).unwrap();
    assert_eq!(
        r.semantics,
        PropagationSemantics::PropagatedFrequentistInputs
    );
    assert!(
        r.interpretation.contains("NOT a confidence interval"),
        "output must disclaim frequentist coverage: {}",
        r.interpretation
    );

    // Mixed inputs: no single interpretation is claimed.
    let mut mixed = credible.clone();
    mixed.insert("B".to_string(), frequentist["B"].clone());
    let (r, _) = propagate(&tree, &model, &mixed, &config(2_000, 1)).unwrap();
    assert_eq!(r.semantics, PropagationSemantics::PropagatedMixed);
    assert!(r.interpretation.contains("not a guaranteed range"));
}

/// The declared uncertainty of an estimate converts into a sampling law, and
/// unsupported forms are refused rather than approximated.
#[test]
fn declared_uncertainty_converts_or_is_refused() {
    let ci = Uncertainty::ConfidenceInterval(ConfidenceInterval::new(0.9, 0.001, 0.004));
    let law = InputUncertainty::from_declared(&ci, 0.002).unwrap();
    assert!(law.is_variable());
    assert!(law.describe().contains("confidence"));
    assert!(law.describe().contains("90%"));

    let plain = Uncertainty::Interval(Interval::new(0.001, 0.004));
    let law = InputUncertainty::from_declared(&plain, 0.002).unwrap();
    assert_eq!(
        law,
        InputUncertainty::Uniform {
            lower: 0.001,
            upper: 0.004
        }
    );

    // A one-sided bound has no distribution and is refused.
    let bound = Uncertainty::LowerBound(etdl_reliability::uncertainty::LowerBound { value: 0.001 });
    assert!(InputUncertainty::from_declared(&bound, 0.002).is_err());
}

/// Inputs with no declared uncertainty contribute nothing, and that is
/// reported as a modelling gap rather than presented as certainty.
#[test]
fn missing_uncertainty_is_a_reported_gap() {
    let mut laws = BTreeMap::new();
    laws.insert(
        "A".to_string(),
        InputUncertainty::Beta {
            alpha: 2.0,
            beta: 198.0,
        },
    );
    let (r, diags) = propagate(
        &or_tree(0.01, 0.02),
        &DependencyModel::independent(),
        &laws,
        &config(2_000, 3),
    )
    .unwrap();
    assert_eq!(r.variable_inputs, 1);
    assert!(diags
        .iter()
        .any(|d| d.code == "RA013" && d.subject.as_deref() == Some("B")));
}

/// A dependent model warns that parameter-uncertainty correlation is not
/// modelled. The limitation is documented, not fabricated away.
#[test]
fn correlated_parameter_uncertainty_is_declared_unmodelled() {
    let mut tree = FaultTreeSpec::new("TOP");
    tree.leaves.insert("A".into(), 0.05);
    tree.leaves.insert("B".into(), 0.05);
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
        common_causes: vec![CommonCause::new("C", 0.01, vec!["A".into(), "B".into()])],
        ..Default::default()
    };
    let mut laws = BTreeMap::new();
    for id in ["A", "B"] {
        laws.insert(
            id.to_string(),
            InputUncertainty::Beta {
                alpha: 5.0,
                beta: 95.0,
            },
        );
    }
    let (_, diags) = propagate(&tree, &model, &laws, &config(2_000, 11)).unwrap();
    assert!(
        diags.iter().any(|d| d.code == "RA007"),
        "dependent model must warn that correlated parameter uncertainty is unmodelled"
    );
}

/// Convergence is never claimed merely because samples were executed.
#[test]
fn convergence_is_assessed_not_assumed() {
    let mut laws = BTreeMap::new();
    laws.insert(
        "A".to_string(),
        InputUncertainty::Beta {
            alpha: 1.0,
            beta: 99.0,
        },
    );
    let tree = or_tree(0.01, 0.02);
    let model = DependencyModel::independent();

    let (small, diags) = propagate(&tree, &model, &laws, &config(50, 4)).unwrap();
    assert!(!small.convergence.stable);
    assert!(diags.iter().any(|d| d.code == "RA004" || d.code == "RA003"));

    let (large, _) = propagate(&tree, &model, &laws, &config(100_000, 4)).unwrap();
    // More samples must reduce the standard error, roughly as 1/sqrt(n).
    assert!(large.convergence.standard_error < small.convergence.standard_error);
    assert!(large.convergence.relative_standard_error < 0.01);
    assert!(large.convergence.criterion.contains("not a convergence proof"));
}

/// Rare-event inputs propagate without collapsing to zero.
#[test]
fn rare_event_inputs_propagate() {
    let mut laws = BTreeMap::new();
    // Beta(1, 1e6) has mean 1e-6.
    laws.insert(
        "A".to_string(),
        InputUncertainty::Beta {
            alpha: 1.0,
            beta: 1_000_000.0,
        },
    );
    laws.insert(
        "B".to_string(),
        InputUncertainty::Beta {
            alpha: 1.0,
            beta: 1_000_000_000.0,
        },
    );
    let (r, _) = propagate(
        &or_tree(1e-6, 1e-9),
        &DependencyModel::independent(),
        &laws,
        &config(20_000, 8),
    )
    .unwrap();
    // E[TOP] ~ 1e-6 + 1e-9.
    assert!(r.mean > 5e-7 && r.mean < 2e-6, "rare-event mean {}", r.mean);
    assert!(r.upper > r.lower);
    assert!(r.min >= 0.0);
    assert!(r.max < 1e-4);
}

/// End to end through `analyze_with`, including the uncertainty contribution
/// ranking. The ranking must identify the input whose uncertainty actually
/// drives the output — which is not the same as the most important input.
#[test]
fn uncertainty_ranking_identifies_the_dominant_uncertainty_source() {
    // A is the larger contributor but is precisely known.
    // B is smaller but very poorly known.
    let tree = or_tree(0.02, 0.01);
    let mut laws = BTreeMap::new();
    laws.insert(
        "A".to_string(),
        InputUncertainty::Beta {
            alpha: 2_000.0,
            beta: 98_000.0,
        },
    ); // mean 0.02, very tight
    laws.insert(
        "B".to_string(),
        InputUncertainty::Beta {
            alpha: 1.0,
            beta: 99.0,
        },
    ); // mean 0.01, very wide

    let options = AnalysisOptions {
        monte_carlo: Some(config(20_000, 31)),
        inputs: laws,
        compute_uncertainty_ranking: true,
        ..Default::default()
    };
    let result = analyze_with(&tree, &DependencyModel::independent(), &options).unwrap();

    let ranking = result.uncertainty_ranking.as_ref().unwrap();
    assert_eq!(ranking.entries.len(), 2);
    assert_eq!(
        ranking.entries[0].id, "B",
        "the poorly known input must dominate output uncertainty"
    );
    assert!(ranking.entries[0].variance_share > 0.9);
    assert!(ranking.entries[0].above_noise_floor);
    assert!(ranking.entries[1].variance_share < 0.1);

    // Importance ranks the other way round: A contributes more probability.
    let importance = &result.importance;
    let a_imp = importance.iter().find(|e| e.id == "A").unwrap();
    let b_imp = importance.iter().find(|e| e.id == "B").unwrap();
    assert!(
        a_imp.fussell_vesely.unwrap() > b_imp.fussell_vesely.unwrap(),
        "A must be the more important contributor while B dominates uncertainty"
    );

    // The metric must be labelled so it is never read as importance.
    assert!(ranking.metric.contains("NOT an importance measure"));
    assert!(ranking.method.contains("common-random-numbers"));
}

/// Analysis results are identified by content, so re-running the same analysis
/// yields the same id and changing any input yields a different one.
#[test]
fn analysis_identity_is_content_derived() {
    let tree = or_tree(0.01, 0.02);
    let model = DependencyModel::independent();
    let options = AnalysisOptions {
        monte_carlo: Some(config(1_000, 5)),
        ..Default::default()
    };
    let a = analyze_with(&tree, &model, &options).unwrap();
    let b = analyze_with(&tree, &model, &options).unwrap();
    assert_eq!(a.analysis_id, b.analysis_id);

    // A different input value changes the identity.
    let c = analyze_with(&or_tree(0.011, 0.02), &model, &options).unwrap();
    assert_ne!(a.analysis_id, c.analysis_id);

    // So does a different seed.
    let mut other = options.clone();
    other.monte_carlo = Some(config(1_000, 6));
    let d = analyze_with(&tree, &model, &other).unwrap();
    assert_ne!(a.analysis_id, d.analysis_id);

    // The input snapshot records what was actually analysed.
    assert_eq!(a.inputs.basic_event_count, 2);
    assert_eq!(a.inputs.gate_count, 1);
    assert_eq!(a.inputs.top_event, "TOP");
    let snap = a.inputs.inputs.iter().find(|i| i.id == "A").unwrap();
    assert_eq!(snap.value, 0.01);
    assert_eq!(snap.kind, "basic-event");

    // Provenance carries everything needed to reproduce the run.
    assert_eq!(a.provenance.seed, Some(5));
    assert_eq!(a.provenance.samples, Some(1_000));
    assert_eq!(a.provenance.sampler.as_deref(), Some("xorshift64star"));
    assert_eq!(a.schema_version, "1.1");
}

/// Analysis with no Monte Carlo configuration performs no sampling at all.
#[test]
fn propagation_is_opt_in() {
    let options = AnalysisOptions {
        monte_carlo: None,
        ..Default::default()
    };
    let result = analyze_with(
        &or_tree(0.01, 0.02),
        &DependencyModel::independent(),
        &options,
    )
    .unwrap();
    assert!(result.uncertainty.is_none());
    assert!(result.provenance.seed.is_none());
    assert!(result.provenance.propagation_method.is_none());
    // The deterministic answer is still produced, unchanged.
    let expected = 1.0 - 0.99 * 0.98;
    assert!((result.point_estimate - expected).abs() < 1e-15);
    assert!((result.deterministic_result - expected).abs() < 1e-15);
}
