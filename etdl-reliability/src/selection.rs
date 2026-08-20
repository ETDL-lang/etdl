//! Deterministic estimate selection and conflict detection.
//!
//! When multiple artifacts (or multiple variants within one artifact) hold
//! estimates for the same failure mode under the same conditions, the selection
//! must be deterministic and never silently pick one. This module provides the
//! selection rules and the conflict policy.

use std::collections::BTreeMap;

use etdl_reliability_core::artifact::ReliabilityArtifact;
use etdl_reliability_core::estimate::ProbabilityEstimate;

/// How to resolve a conflict when two estimates claim the same context.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConflictPolicy {
    /// Any conflict is an error; the caller must resolve it explicitly.
    #[default]
    Error,
    /// The first artifact listed wins (caller-provided priority order).
    FirstWins,
    /// The artifact with the explicitly requested id wins.
    ExplicitArtifact(String),
    /// The estimate with the explicitly requested version wins.
    ExplicitVersion(String),
}

/// The context key that uniquely identifies an estimate's applicability.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EstimateKey {
    pub failure_mode: String,
    pub metric: String,
    /// Sorted conditions.
    pub conditions: Vec<String>,
    pub population: Option<String>,
}

impl EstimateKey {
    pub fn of(estimate: &ProbabilityEstimate) -> Self {
        let mut conditions = estimate.conditions.clone();
        conditions.sort();
        conditions.dedup();
        EstimateKey {
            failure_mode: estimate.event.clone(),
            metric: format!("{:?}", estimate.metric),
            conditions,
            population: estimate.population.clone(),
        }
    }
}

/// A conflict: two or more estimates claim the same context with different
/// values.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimateConflict {
    pub key: EstimateKey,
    /// (artifact_id, version, value) for each conflicting estimate.
    pub candidates: Vec<(String, Option<String>, f64)>,
}

/// Deterministically select an estimate for a failure mode under conditions.
///
/// If exactly one estimate matches, it is returned. If several match with the
/// same value, the first (deterministic order) is returned. If several match
/// with **different** values, the policy decides: `Error` produces a
/// [`SelectionError::Conflict`], `FirstWins` picks the first, and the explicit
/// policies select by artifact id or version.
pub fn select_estimate<'a>(
    estimates: &[(&'a str, &'a ReliabilityArtifact)],
    key: &EstimateKey,
    policy: &ConflictPolicy,
) -> Result<&'a ProbabilityEstimate, SelectionError> {
    let mut matches: Vec<(String, Option<String>, &ProbabilityEstimate)> = Vec::new();
    for (artifact_id, artifact) in estimates {
        for est in artifact.all_for(&key.failure_mode) {
            if EstimateKey::of(est) == *key {
                matches.push((artifact_id.to_string(), est.version.clone(), est));
            }
        }
    }
    if matches.is_empty() {
        return Err(SelectionError::not_found(key));
    }

    // Distinct values?
    let distinct: BTreeMap<u64, ()> = matches
        .iter()
        .map(|(_, _, e)| (e.value.unwrap_or(f64::NAN).to_bits(), ()))
        .collect();
    if distinct.len() > 1 {
        match policy {
            ConflictPolicy::Error => {
                let candidates = matches
                    .iter()
                    .map(|(a, v, e)| (a.clone(), v.clone(), e.value.unwrap_or(f64::NAN)))
                    .collect();
                return Err(SelectionError::Conflict(EstimateConflict {
                    key: key.clone(),
                    candidates,
                }));
            }
            ConflictPolicy::FirstWins => {
                // matches is in deterministic order (artifacts listed in
                // caller order, variants sorted by canonical key).
                let (_, _, est) = &matches[0];
                return Ok(est);
            }
            ConflictPolicy::ExplicitArtifact(id) => {
                if let Some((_, _, est)) = matches.iter().find(|(a, _, _)| a == id) {
                    return Ok(est);
                }
                return Err(SelectionError::RequestedArtifactNotFound(id.clone()));
            }
            ConflictPolicy::ExplicitVersion(v) => {
                if let Some((_, _, est)) =
                    matches.iter().find(|(_, ver, _)| ver.as_deref() == Some(v))
                {
                    return Ok(est);
                }
                return Err(SelectionError::RequestedVersionNotFound(v.clone()));
            }
        }
    }

    // All values identical (or single match): pick first deterministically.
    let (_, _, est) = &matches[0];
    Ok(est)
}

/// Errors from deterministic estimate selection.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SelectionError {
    #[error("no estimate found for key: failure_mode={fm}, metric={metric}, conditions={cond:?}, population={pop:?}")]
    NotFound {
        fm: String,
        metric: String,
        cond: String,
        pop: String,
    },
    #[error("conflicting estimates for the same context: {0:?}")]
    Conflict(EstimateConflict),
    #[error("requested artifact '{0}' has no estimate for this context")]
    RequestedArtifactNotFound(String),
    #[error("requested version '{0}' has no estimate for this context")]
    RequestedVersionNotFound(String),
}

impl SelectionError {
    pub fn not_found(key: &EstimateKey) -> Self {
        SelectionError::NotFound {
            fm: key.failure_mode.clone(),
            metric: key.metric.clone(),
            cond: key.conditions.join(","),
            pop: key.population.clone().unwrap_or_default(),
        }
    }
}

/// Detect all conflicts across a set of artifacts for a failure mode:
/// distinct values for the same `EstimateKey`. Returns a list of conflicts.
pub fn detect_conflicts(
    estimates: &[(&str, &ReliabilityArtifact)],
    failure_mode: &str,
) -> Vec<EstimateConflict> {
    // Group all estimates by key.
    let mut by_key: BTreeMap<EstimateKey, Vec<(String, Option<String>, f64)>> = BTreeMap::new();
    for (artifact_id, artifact) in estimates {
        for est in artifact.all_for(failure_mode) {
            let key = EstimateKey::of(est);
            by_key.entry(key).or_default().push((
                artifact_id.to_string(),
                est.version.clone(),
                est.value.unwrap_or(f64::NAN),
            ));
        }
    }
    by_key
        .into_iter()
        .filter_map(|(key, candidates)| {
            let distinct: BTreeMap<u64, ()> = candidates
                .iter()
                .map(|(_, _, v)| (v.to_bits(), ()))
                .collect();
            if distinct.len() > 1 {
                Some(EstimateConflict { key, candidates })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use etdl_reliability_core::artifact::{declared, ReliabilityArtifact};

    fn est_with(fm: &str, cond: &str, value: f64) -> ProbabilityEstimate {
        let mut e = declared(fm, value);
        e.conditions = vec![cond.to_string()];
        e
    }

    #[test]
    fn selects_exactly_matching_conditions() {
        let mut a = ReliabilityArtifact::new("a1");
        a.add(est_with("failure.x", "production", 0.002)).unwrap();
        a.add(est_with("failure.x", "high-load", 0.008)).unwrap();
        let key = EstimateKey {
            failure_mode: "failure.x".into(),
            metric: "Probability".to_string(),
            conditions: vec!["high-load".into()],
            population: None,
        };
        let est = select_estimate(&[("a1", &a)], &key, &ConflictPolicy::Error).unwrap();
        assert_eq!(est.value, Some(0.008));
    }

    #[test]
    fn conflicting_estimates_error_by_default() {
        let mut a1 = ReliabilityArtifact::new("a1");
        a1.add(est_with("failure.x", "production", 0.01)).unwrap();
        let mut a2 = ReliabilityArtifact::new("a2");
        a2.add(est_with("failure.x", "production", 0.03)).unwrap();
        let key = EstimateKey {
            failure_mode: "failure.x".into(),
            metric: "Probability".to_string(),
            conditions: vec!["production".into()],
            population: None,
        };
        let err =
            select_estimate(&[("a1", &a1), ("a2", &a2)], &key, &ConflictPolicy::Error).unwrap_err();
        assert!(matches!(err, SelectionError::Conflict(_)));
    }

    #[test]
    fn explicit_version_policy_is_deterministic() {
        let mut a = ReliabilityArtifact::new("a");
        let mut e1 = est_with("failure.x", "production", 0.01);
        e1.version = Some("1".into());
        a.add(e1).unwrap();
        let mut e2 = est_with("failure.x", "production", 0.03);
        e2.version = Some("2".into());
        a.add(e2).unwrap();
        let key = EstimateKey {
            failure_mode: "failure.x".into(),
            metric: "Probability".to_string(),
            conditions: vec!["production".into()],
            population: None,
        };
        let est = select_estimate(
            &[("a", &a)],
            &key,
            &ConflictPolicy::ExplicitVersion("2".into()),
        )
        .unwrap();
        assert_eq!(est.value, Some(0.03));
        let est1 = select_estimate(
            &[("a", &a)],
            &key,
            &ConflictPolicy::ExplicitVersion("1".into()),
        )
        .unwrap();
        assert_eq!(est1.value, Some(0.01));
    }

    #[test]
    fn detect_conflicts_lists_them() {
        let mut a1 = ReliabilityArtifact::new("a1");
        a1.add(est_with("failure.x", "production", 0.01)).unwrap();
        let mut a2 = ReliabilityArtifact::new("a2");
        a2.add(est_with("failure.x", "production", 0.03)).unwrap();
        let conflicts = detect_conflicts(&[("a1", &a1), ("a2", &a2)], "failure.x");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].candidates.len(), 2);
    }

    #[test]
    fn same_value_across_artifacts_is_not_a_conflict() {
        let mut a1 = ReliabilityArtifact::new("a1");
        a1.add(est_with("failure.x", "production", 0.02)).unwrap();
        let mut a2 = ReliabilityArtifact::new("a2");
        a2.add(est_with("failure.x", "production", 0.02)).unwrap();
        let key = EstimateKey {
            failure_mode: "failure.x".into(),
            metric: "Probability".to_string(),
            conditions: vec!["production".into()],
            population: None,
        };
        let est =
            select_estimate(&[("a1", &a1), ("a2", &a2)], &key, &ConflictPolicy::Error).unwrap();
        assert_eq!(est.value, Some(0.02));
    }
}
