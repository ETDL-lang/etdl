use etdl_parser::ast::{EtlDocument, FaultTree, GateType};
use etdl_parser::spanned::SpanKey;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

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

/// `pub(crate)`, not private: `codegen/rust.rs`'s live-reliability
/// registration codegen reuses this exact formula to seed each local
/// basic event's live estimator — the same declared-probability
/// computation, not a second copy that could drift.
pub(crate) fn compute_basic_event_probability(be: &etdl_parser::ast::BasicEvent) -> Result<f64, String> {
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

/// Dispatches each `GateType` to its combinator in `etdl-probability-core`
/// — the single shared implementation of "how a gate combines
/// probabilities" (also used by the runtime live-recombination engine).
/// This function owns only the `GateType` → function mapping and the
/// `f64` <-> `Probability` boundary conversion; the math itself lives in
/// `etdl_probability_core::gate`.
fn compute_gate_probability(
    gate_type: &GateType,
    inputs: &[f64],
    k: Option<u32>,
) -> Result<f64, String> {
    use etdl_probability_core::{gate, Probability};

    let probs: Vec<Probability> = inputs
        .iter()
        .map(|&p| Probability::new(p).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;

    let result = match gate_type {
        GateType::And => {
            etdl_probability_core::independent_and_n(&probs).map_err(|e| e.to_string())?
        }
        GateType::Or => {
            etdl_probability_core::independent_or_n(&probs).map_err(|e| e.to_string())?
        }
        GateType::Not => gate::not(&probs).map_err(|e| e.to_string())?,
        GateType::Xor => gate::xor(&probs).map_err(|e| e.to_string())?,
        GateType::Voting => {
            let k_val = k.ok_or("VOTING gate requires k")? as usize;
            gate::k_of_n(&probs, k_val).map_err(|e| e.to_string())?
        }
        GateType::Inhibit => gate::inhibit(&probs).map_err(|e| e.to_string())?,
        GateType::PriorityAnd => gate::priority_and(&probs).map_err(|e| e.to_string())?,
    };
    Ok(result.value())
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
    sorted_rows.sort();
    sorted_rows.dedup();

    // Drop every row that has a *different* (index-distinct) row as a
    // subset of it — that other row already implies it, so it is
    // redundant. Indices (not `std::ptr::eq` on cloned `Vec`s, which never
    // matches its own source and previously caused every row, including
    // ones with no true superset, to be discarded as "subset of itself")
    // are what make a row distinguishable from itself here.
    let mut keep = vec![true; sorted_rows.len()];
    for i in 0..sorted_rows.len() {
        let set_i: BTreeSet<&String> = sorted_rows[i].iter().collect();
        for j in 0..sorted_rows.len() {
            if i == j || !keep[j] {
                continue;
            }
            let set_j: BTreeSet<&String> = sorted_rows[j].iter().collect();
            if set_i.is_subset(&set_j) {
                keep[j] = false;
            }
        }
    }

    sorted_rows
        .into_iter()
        .zip(keep)
        .filter_map(|(row, k)| k.then_some(row))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use etdl_parser::ast::{BasicEvent, Gate, TopEvent};

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

    fn basic_event(probability: f64) -> BasicEvent {
        BasicEvent {
            description: "d".to_string(),
            probability: Some(probability),
            failure_rate: None,
            mission_time: None,
            undeveloped: None,
            event_type: None,
            message: None,
            extensions: Default::default(),
        }
    }

    // A single OR gate over two basic events has two minimal cut sets, one
    // per input — this is the smallest fixture that distinguishes "correct
    // reduction" from a `minimize_rows` bug that discards every row as a
    // trivial "subset of itself" (a real bug this reproduced: comparing
    // `std::ptr::eq` against a freshly cloned row never matches the row's
    // own list entry, so every row was treated as a redundant superset of
    // itself and the function always returned `Ok(vec![])`).
    #[test]
    fn or_gate_produces_one_cut_set_per_input() {
        let ft = FaultTree {
            top_event: TopEvent {
                id: "Top".to_string(),
                description: "d".to_string(),
                message: None,
                root_cause: "Gate".to_string(),
            },
            gates: Some(BTreeMap::from([(
                "Gate".to_string(),
                Gate {
                    gate_type: GateType::Or,
                    inputs: vec!["A".to_string(), "B".to_string()],
                    k: None,
                    description: None,
                    inhibit_condition: None,
                },
            )])),
            basic_events: BTreeMap::from([
                ("A".to_string(), basic_event(0.01)),
                ("B".to_string(), basic_event(0.01)),
            ]),
            transfers: None,
            description: None,
        };

        let mut cut_sets = enumerate_minimal_cut_sets(&ft).expect("coherent tree");
        for row in &mut cut_sets {
            row.sort();
        }
        cut_sets.sort();

        assert_eq!(cut_sets, vec![vec!["A".to_string()], vec!["B".to_string()]]);
    }

    // An AND gate over two basic events has exactly one minimal cut set
    // containing both — proves the reduction step keeps a single
    // multi-element row instead of also discarding it.
    #[test]
    fn and_gate_produces_one_cut_set_with_both_inputs() {
        let ft = FaultTree {
            top_event: TopEvent {
                id: "Top".to_string(),
                description: "d".to_string(),
                message: None,
                root_cause: "Gate".to_string(),
            },
            gates: Some(BTreeMap::from([(
                "Gate".to_string(),
                Gate {
                    gate_type: GateType::And,
                    inputs: vec!["A".to_string(), "B".to_string()],
                    k: None,
                    description: None,
                    inhibit_condition: None,
                },
            )])),
            basic_events: BTreeMap::from([
                ("A".to_string(), basic_event(0.01)),
                ("B".to_string(), basic_event(0.01)),
            ]),
            transfers: None,
            description: None,
        };

        let cut_sets = enumerate_minimal_cut_sets(&ft).expect("coherent tree");

        assert_eq!(cut_sets.len(), 1, "got {cut_sets:?}");
        let mut only = cut_sets[0].clone();
        only.sort();
        assert_eq!(only, vec!["A".to_string(), "B".to_string()]);
    }
}
