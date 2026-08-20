use etdl_parser::ast::{EtlDocument, FaultTree, GateType};
use etdl_parser::spanned::SpanKey;
use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::validate::Diagnostic;

pub type FaultTreeProbabilities = BTreeMap<String, f64>;

/// External probability overrides for basic events.
///
/// Keys are compound and unambiguous: `"{fault_tree_id}::{basic_event_id}"`.
/// This prevents same-named basic events in different fault trees from
/// colliding. Values are deterministic probabilities resolved from external
/// reliability sources.
pub type BasicEventOverrides = BTreeMap<String, f64>;

/// Build the override lookup key for a basic event within a fault tree.
pub fn override_key(ft_id: &str, be_id: &str) -> String {
    format!("{}::{}", ft_id, be_id)
}

pub fn resolve_fault_trees(
    doc: &EtlDocument,
    diagnostics: &mut Vec<Diagnostic>,
) -> FaultTreeProbabilities {
    resolve_fault_trees_with_overrides(doc, &BasicEventOverrides::new(), diagnostics)
}

/// Resolve fault trees, honoring external probability overrides for basic
/// events. `overrides` maps compound `"{ft_id}::{be_id}"` keys to deterministic
/// probabilities (e.g. resolved from reliability artifacts). The override takes
/// precedence over a declared `probability`/`failureRate`, mirroring how
/// `onFailureProbabilitySource` is authoritative.
pub fn resolve_fault_trees_with_overrides(
    doc: &EtlDocument,
    overrides: &BasicEventOverrides,
    diagnostics: &mut Vec<Diagnostic>,
) -> FaultTreeProbabilities {
    let mut results = BTreeMap::new();

    let fault_trees = match &doc.fault_trees {
        Some(fts) => fts,
        None => return results,
    };

    for (ft_id, ft) in fault_trees {
        match compute_top_event_probability(ft_id, ft, overrides) {
            Ok(prob) => {
                results.insert(ft_id.clone(), prob);
            }
            Err(e) => {
                diagnostics.push(
                    Diagnostic::error(
                        "V-401",
                        format!("fault tree '{}': error computing probability: {}", ft_id, e),
                    )
                    .at(SpanKey::FaultTree {
                        tree: ft_id.clone(),
                    }),
                );
            }
        }
    }

    results
}

fn compute_top_event_probability(
    ft_id: &str,
    ft: &FaultTree,
    overrides: &BasicEventOverrides,
) -> Result<f64, String> {
    let mut probs: HashMap<String, f64> = HashMap::new();

    for (be_id, be) in &ft.basic_events {
        let prob = if let Some(&override_value) = overrides.get(&override_key(ft_id, be_id)) {
            override_value
        } else {
            compute_basic_event_probability(be)?
        };
        probs.insert(be_id.clone(), prob);
    }

    let gates = match &ft.gates {
        Some(g) => g,
        None => {
            let root_id = &ft.top_event.root_cause;
            if let Some(&prob) = probs.get(root_id) {
                return Ok(prob);
            } else {
                return Err(format!(
                    "topEvent.rootCause '{}' not found in basic events and no gates defined",
                    root_id
                ));
            }
        }
    };

    let order = topological_sort_gates(gates, &ft.top_event.root_cause)?;

    for gate_id in &order {
        let gate = gates
            .get(gate_id)
            .ok_or_else(|| format!("gate '{}' not found during resolution", gate_id))?;

        let input_probs: Vec<f64> = gate
            .inputs
            .iter()
            .map(|input| {
                probs
                    .get(input.as_str())
                    .copied()
                    .ok_or_else(|| format!("probability for '{}' not resolved", input))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let gate_prob = compute_gate_probability(&gate.gate_type, &input_probs, gate.k)?;
        probs.insert(gate_id.clone(), gate_prob);
    }

    let root_id = &ft.top_event.root_cause;
    probs
        .get(root_id)
        .copied()
        .ok_or_else(|| format!("topEvent.rootCause '{}' probability not resolved", root_id))
}

fn compute_basic_event_probability(be: &etdl_parser::ast::BasicEvent) -> Result<f64, String> {
    if let Some(ref failure_rate) = be.failure_rate {
        let mission_time = be
            .mission_time
            .ok_or("failureRate set but missionTime missing")?;
        Ok(1.0 - (-failure_rate * mission_time).exp())
    } else if let Some(prob) = be.probability {
        if !(0.0..=1.0).contains(&prob) {
            return Err(format!("probability {} out of range [0, 1]", prob));
        }
        Ok(prob)
    } else {
        Err("basic event has neither probability nor failureRate".to_string())
    }
}

fn compute_gate_probability(
    gate_type: &GateType,
    inputs: &[f64],
    k: Option<u32>,
) -> Result<f64, String> {
    match gate_type {
        GateType::And => Ok(inputs.iter().product()),
        GateType::Or => {
            let complement: f64 = inputs.iter().map(|p| 1.0 - p).product();
            Ok(1.0 - complement)
        }
        GateType::Not => {
            if inputs.len() != 1 {
                return Err("NOT gate requires exactly 1 input".to_string());
            }
            if inputs[0] < 0.0 || inputs[0] > 1.0 {
                return Err(format!(
                    "NOT gate input probability {} out of range",
                    inputs[0]
                ));
            }
            Ok(1.0 - inputs[0])
        }
        GateType::Xor => {
            if inputs.len() != 2 {
                return Err("XOR gate requires exactly 2 inputs".to_string());
            }
            Ok(inputs[0] + inputs[1] - 2.0 * inputs[0] * inputs[1])
        }
        GateType::Voting => {
            let k_val = k.ok_or("VOTING gate requires k")? as usize;
            let n = inputs.len();

            if k_val < 1 || k_val > n {
                return Err(format!("VOTING gate: k={} out of range [1, {}]", k_val, n));
            }

            if inputs.iter().all(|&p| (p - inputs[0]).abs() < 1e-10) {
                let p = inputs[0].clamp(0.0, 1.0);
                let mut total = 0.0;
                for j in k_val..=n {
                    total +=
                        binomial_coeff(n, j) * p.powi(j as i32) * (1.0 - p).powi((n - j) as i32);
                }
                Ok(total.clamp(0.0, 1.0))
            } else {
                let mut poly = vec![1.0];
                for &p in inputs {
                    poly = multiply_polynomial(&poly, &[1.0 - p, p]);
                }
                let mut total = 0.0;
                for j in k_val..=n {
                    if j < poly.len() {
                        total += poly[j];
                    }
                }
                Ok(total.clamp(0.0, 1.0))
            }
        }
        GateType::Inhibit => {
            if inputs.len() != 2 {
                return Err("INHIBIT gate requires exactly 2 inputs".to_string());
            }
            Ok(inputs[0] * inputs[1])
        }
        GateType::PriorityAnd => {
            let n = inputs.len();
            if n < 2 {
                return Err("PRIORITY_AND gate requires at least 2 inputs".to_string());
            }
            // All n inputs must occur in the listed order. Assuming each
            // ordering is equally likely: P = (prod p_i) / n!.
            // Computed in log space to avoid factorial overflow for large n.
            let mut log_p = 0.0;
            for p in inputs {
                let p = (*p).clamp(0.0, 1.0);
                if p <= 0.0 {
                    return Ok(0.0);
                }
                log_p += p.ln();
            }
            log_p -= ln_factorial(n);
            Ok(log_p.exp().clamp(0.0, 1.0))
        }
    }
}

/// ln(n!) computed without overflow: direct f64 product for n ≤ 170 (where
/// n! still fits in f64), and the log-gamma approximation beyond (where the
/// exact value is astronomically small and precision is irrelevant).
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
/// ln(n!) for integer n+1. Used only for n > 170 where direct products would
/// overflow; ~1e-12 relative accuracy is ample at those magnitudes.
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
    for i in 1..9 {
        a += P[i] / (x_minus_one + i as f64);
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

fn topological_sort_gates(
    gates: &BTreeMap<String, etdl_parser::ast::Gate>,
    root_id: &str,
) -> Result<Vec<String>, String> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for gate_id in gates.keys() {
        in_degree.entry(gate_id.as_str()).or_insert(0);
        adj.entry(gate_id.as_str()).or_default();
    }

    for (gate_id, gate) in gates {
        for input in &gate.inputs {
            if gates.contains_key(input.as_str()) {
                adj.entry(input.as_str())
                    .or_default()
                    .push(gate_id.as_str());
                *in_degree.entry(gate_id.as_str()).or_insert(0) += 1;
            }
        }
    }

    let mut queue: VecDeque<&str> = VecDeque::new();
    // Iterate gates in sorted (BTreeMap) order for deterministic output.
    for gate_id in gates.keys() {
        let deg = in_degree.get(gate_id.as_str()).copied().unwrap_or(0);
        if deg == 0 {
            queue.push_back(gate_id.as_str());
        }
    }

    let mut order = Vec::new();
    while let Some(id) = queue.pop_front() {
        order.push(id.to_string());
        if let Some(children) = adj.get(id) {
            for &child in children {
                if let Some(deg) = in_degree.get_mut(child) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }
    }

    if order.len() != gates.len() {
        return Err("cycle detected in fault tree gates (V-403)".to_string());
    }

    if !order.contains(&root_id.to_string()) {
        order.push(root_id.to_string());
    }

    Ok(order)
}

/// Maximum number of cut-set rows MOCUS will produce before aborting. Cut-set
/// enumeration has exponential worst-case output (ETDL §8.6), so an
/// implementation-defined cap is required; exceeding it is an error rather than
/// unbounded memory growth.
pub const MAX_CUT_SET_ROWS: usize = 100_000;

pub fn enumerate_minimal_cut_sets(ft: &FaultTree) -> Result<Vec<Vec<String>>, String> {
    let gates = match &ft.gates {
        Some(g) => g,
        None => {
            return Ok(vec![vec![ft.top_event.root_cause.clone()]]);
        }
    };

    for gate in gates.values() {
        if matches!(gate.gate_type, GateType::Not | GateType::Xor) {
            return Err(
                "cannot enumerate cut sets for non-coherent fault tree (contains NOT or XOR gate)"
                    .to_string(),
            );
        }
    }

    let mut rows: Vec<Vec<String>> = vec![vec![ft.top_event.root_cause.clone()]];

    let mut changed = true;
    while changed {
        changed = false;
        let mut new_rows = Vec::new();

        for row in &rows {
            if new_rows.len() > MAX_CUT_SET_ROWS {
                return Err(format!(
                    "cut set enumeration exceeded maximum row count {}; tree too large",
                    MAX_CUT_SET_ROWS
                ));
            }

            let gate_positions: Vec<(usize, &str)> = row
                .iter()
                .enumerate()
                .filter(|(_, item)| gates.contains_key(item.as_str()))
                .map(|(i, item)| (i, item.as_str()))
                .collect();

            if gate_positions.is_empty() {
                new_rows.push(row.clone());
                continue;
            }

            changed = true;
            let (pos, gate_id) = gate_positions[0];
            let gate = &gates[gate_id];

            match gate.gate_type {
                GateType::Or => {
                    for input in &gate.inputs {
                        let mut new_row = row.clone();
                        new_row.remove(pos);
                        new_row.insert(pos, input.clone());
                        new_rows.push(new_row);
                    }
                }
                GateType::And | GateType::Inhibit | GateType::PriorityAnd => {
                    let mut new_row = row.clone();
                    new_row.remove(pos);
                    for (offset, input) in gate.inputs.iter().enumerate() {
                        new_row.insert(pos + offset, input.clone());
                    }
                    new_rows.push(new_row);
                }
                GateType::Voting => {
                    let k = gate.k.unwrap_or(1) as usize;
                    let combinations = generate_combinations(&gate.inputs, k);
                    for combo in &combinations {
                        let mut new_row = row.clone();
                        new_row.remove(pos);
                        for (offset, input) in combo.iter().enumerate() {
                            new_row.insert(pos + offset, input.clone());
                        }
                        new_rows.push(new_row);
                    }
                }
                _ => {
                    return Err(format!(
                        "unexpected gate type {:?} in cut set enumeration",
                        gate.gate_type
                    ));
                }
            }
        }

        rows = new_rows;
        rows = minimize_rows(rows);
    }

    Ok(rows)
}

fn generate_combinations<T: Clone>(items: &[T], k: usize) -> Vec<Vec<T>> {
    if k == 0 {
        return vec![vec![]];
    }
    if items.is_empty() {
        return vec![];
    }

    let mut result = Vec::new();
    let first = &items[0];
    let rest = &items[1..];

    for mut combo in generate_combinations(rest, k - 1) {
        let mut new_combo = vec![first.clone()];
        new_combo.append(&mut combo);
        result.push(new_combo);
    }

    for combo in generate_combinations(rest, k) {
        result.push(combo);
    }

    result
}

fn minimize_rows(rows: Vec<Vec<String>>) -> Vec<Vec<String>> {
    let mut sorted_rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|mut row| {
            row.sort();
            row.dedup();
            row
        })
        .collect();

    let mut i = 0;
    while i < sorted_rows.len() {
        let row_i = sorted_rows[i].clone();
        sorted_rows.retain(|row_j| {
            if std::ptr::eq(row_j, &row_i) {
                return true;
            }
            let set_i: std::collections::BTreeSet<_> = row_i.iter().collect();
            let set_j: std::collections::BTreeSet<_> = row_j.iter().collect();
            !set_i.is_subset(&set_j)
        });
        i += 1;
    }

    sorted_rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inhibit_gate_is_product() {
        let p = compute_gate_probability(&GateType::Inhibit, &[0.1, 0.5], None).unwrap();
        assert!((p - 0.05).abs() < 1e-12);
    }

    #[test]
    fn voting_heterogeneous_matches_binomial() {
        // 2-of-3 with equal probabilities should equal the binomial tail.
        let p = compute_gate_probability(&GateType::Voting, &[0.5, 0.5, 0.5], Some(2)).unwrap();
        let expected = 0.5; // P(X>=2) for Bin(3,0.5)
        assert!((p - expected).abs() < 1e-9, "got {}", p);
    }

    #[test]
    fn voting_heterogeneous_polynomial() {
        // 2-of-3 with distinct probabilities: compute via the generating
        // polynomial directly.
        let a = 0.1;
        let b = 0.2;
        let c = 0.3;
        let p = compute_gate_probability(&GateType::Voting, &[a, b, c], Some(2)).unwrap();
        let expected = a * b * (1.0 - c) + a * (1.0 - b) * c + (1.0 - a) * b * c + a * b * c;
        assert!((p - expected).abs() < 1e-9, "got {}", p);
    }

    #[test]
    fn binomial_coeff_does_not_overflow() {
        // C(70, 35) overflows usize; the f64 implementation must still work.
        let c = binomial_coeff(70, 35);
        assert!(c > 0.0);
        // Exact value is 112186277816656760000 ≈ 1.121862778e20.
        assert!(
            (c - 1.121862778e20).abs() / 1.121862778e20 < 1e-6,
            "got {}",
            c
        );
    }

    #[test]
    fn priority_and_large_n_no_overflow() {
        // n inputs of probability 1.0 give P = 1/n!; this must not overflow
        // the u64 path (20! fits, 21! does not).
        let twenty = vec![1.0; 20];
        let p20 = compute_gate_probability(&GateType::PriorityAnd, &twenty, None).unwrap();
        let exact_20 = (1u64..=20).fold(1.0f64, |acc, i| acc * i as f64);
        assert!((p20 - 1.0 / exact_20).abs() < 1e-20, "got {}", p20);

        let twenty_one = vec![1.0; 21];
        let p21 = compute_gate_probability(&GateType::PriorityAnd, &twenty_one, None).unwrap();
        // 21! ≈ 5.109e19, so P ≈ 1.957e-20 (computed via direct f64 product).
        let exact_21 = (1u64..=21).fold(1.0f64, |acc, i| acc * i as f64);
        assert!((p21 - 1.0 / exact_21).abs() < 1e-20, "got {}", p21);
        // Sanity: the results are tiny but positive numbers.
        assert!(p20 > 0.0 && p20 < 1e-15);
        assert!(p21 > 0.0 && p21 < 1e-15);
    }

    #[test]
    fn ln_gamma_consistency() {
        // ln(6!) = ln(720) ≈ 6.5792
        assert!((ln_factorial(6) - 720.0f64.ln()).abs() < 1e-9);
        // Large n: compare log-space binomial against direct small-n result.
        let small = binomial_coeff(10, 5);
        assert!((small - 252.0).abs() < 1e-6);
    }

    #[test]
    fn inhibit_requires_two_inputs() {
        assert!(compute_gate_probability(&GateType::Inhibit, &[0.1], None).is_err());
    }

    #[test]
    fn priority_and_uses_uniform_ordering() {
        // P(A then B) = (0.2 * 0.3) / 2! = 0.03
        let p = compute_gate_probability(&GateType::PriorityAnd, &[0.2, 0.3], None).unwrap();
        assert!((p - 0.03).abs() < 1e-12);
    }

    #[test]
    fn priority_and_three_inputs() {
        // (0.2 * 0.3 * 0.4) / 3! = 0.024 / 6 = 0.004
        let p = compute_gate_probability(&GateType::PriorityAnd, &[0.2, 0.3, 0.4], None).unwrap();
        assert!((p - 0.004).abs() < 1e-12);
    }

    #[test]
    fn priority_and_requires_two_inputs() {
        assert!(compute_gate_probability(&GateType::PriorityAnd, &[0.1], None).is_err());
    }
}
