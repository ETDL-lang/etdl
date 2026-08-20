//! Reliability artifacts and external probability resolution.
//!
//! A **reliability artifact** is an external, non-executable, versioned file
//! (JSON or YAML) containing probability estimates with provenance. The ETDL
//! compiler resolves these to deterministic scalars at build time; nothing is
//! resolved at runtime.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::estimate::{ProbabilityEstimate, ProbabilityState};
use crate::provenance::Provenance;
use crate::ReliabilityError;

/// Version of the artifact schema.
pub const ARTIFACT_SCHEMA: &str = "etdl.reliability.artifact/1.0";

/// A versioned reliability artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityArtifact {
    pub schema: String,
    pub id: String,
    pub version: Option<String>,
    /// Estimates keyed by event id. Each entry is the "primary" estimate for
    /// that event (the one without conditions, when present).
    pub estimates: BTreeMap<String, ProbabilityEstimate>,
    /// All estimates for an event id, including condition/version variants.
    /// Keyed by event id; each entry is sorted deterministically by canonical
    /// key. Populated from new estimates; empty for legacy artifacts.
    #[serde(default)]
    pub variants: BTreeMap<String, Vec<ProbabilityEstimate>>,
}

impl ReliabilityArtifact {
    pub fn new(id: impl Into<String>) -> Self {
        ReliabilityArtifact {
            schema: ARTIFACT_SCHEMA.to_string(),
            id: id.into(),
            version: None,
            estimates: BTreeMap::new(),
            variants: BTreeMap::new(),
        }
    }

    /// Add an estimate. Never silently overwrites: an estimate with the same
    /// event id and the same canonical key is a duplicate and is rejected;
    /// an estimate with a different canonical key (different conditions,
    /// population, metric, time basis, or version) is stored as a variant.
    pub fn add(&mut self, estimate: ProbabilityEstimate) -> Result<(), ReliabilityError> {
        let event = estimate.event.clone();
        let key = estimate.key();

        if let Some(variants) = self.variants.get_mut(&event) {
            if variants.iter().any(|e| e.key() == key) {
                return Err(ReliabilityError::DuplicateEstimate { event, key });
            }
            variants.push(estimate);
            variants.sort_by_key(|e| e.key());
            // The primary is the estimate with no conditions (if any), else
            // the first variant deterministically.
            let primary = variants
                .iter()
                .find(|e| e.conditions.is_empty())
                .or_else(|| variants.first())
                .expect("variants non-empty");
            self.estimates.insert(event, primary.clone());
            return Ok(());
        }

        self.variants.insert(event.clone(), vec![estimate.clone()]);
        self.estimates.insert(event, estimate);
        Ok(())
    }

    /// The primary estimate for an event id (used by the compiler resolution
    /// path). Returns `None` if the event is unknown.
    pub fn get(&self, event: &str) -> Option<&ProbabilityEstimate> {
        self.estimates.get(event)
    }

    /// All estimates for an event id, including variants, sorted by canonical
    /// key. Empty if the event is unknown.
    pub fn all_for(&self, event: &str) -> Vec<&ProbabilityEstimate> {
        self.variants
            .get(event)
            .map(|v| v.iter().collect())
            .unwrap_or_else(|| {
                self.estimates
                    .get(event)
                    .map(|e| vec![e])
                    .unwrap_or_default()
            })
    }

    /// Deterministically select the best estimate for an event id under a
    /// given set of conditions and population.
    ///
    /// Precedence: exact conditions match, then a variant with a subset of the
    /// requested conditions, then the primary (unconditional) estimate.
    pub fn select<'a>(
        &'a self,
        event: &str,
        conditions: &[String],
        population: Option<&str>,
    ) -> Option<&'a ProbabilityEstimate> {
        let variants = self.all_for(event);
        if variants.is_empty() {
            return None;
        }
        let requested: Vec<&str> = conditions.iter().map(|s| s.as_str()).collect();

        // 1. Exact conditions + population.
        let exact = variants
            .iter()
            .find(|e| {
                e.conditions.iter().map(|s| s.as_str()).collect::<Vec<_>>() == requested
                    && e.population.as_deref() == population
            })
            .copied();
        if let Some(e) = exact {
            return Some(e);
        }
        // 2. Exact conditions (any population).
        let exact_any = variants
            .iter()
            .find(|e| e.conditions.iter().map(|s| s.as_str()).collect::<Vec<_>>() == requested)
            .copied();
        if let Some(e) = exact_any {
            return Some(e);
        }
        // 3. A variant whose conditions are a subset of the requested set.
        let subset = variants
            .iter()
            .find(|e| {
                !e.conditions.is_empty()
                    && e.conditions.iter().all(|c| requested.contains(&c.as_str()))
            })
            .copied();
        if let Some(e) = subset {
            return Some(e);
        }
        // 4. Primary (unconditional).
        variants
            .iter()
            .find(|e| e.conditions.is_empty())
            .copied()
            .or_else(|| variants.first().copied())
    }

    /// Parse from JSON.
    pub fn from_json(s: &str) -> Result<Self, ReliabilityError> {
        let a: ReliabilityArtifact = serde_json::from_str(s)
            .map_err(|e| ReliabilityError::MalformedArtifact(e.to_string()))?;
        a.check_schema()?;
        Ok(a)
    }

    /// Parse from YAML.
    pub fn from_yaml(s: &str) -> Result<Self, ReliabilityError> {
        let a: ReliabilityArtifact = serde_yaml::from_str(s)
            .map_err(|e| ReliabilityError::MalformedArtifact(e.to_string()))?;
        a.check_schema()?;
        Ok(a)
    }

    fn check_schema(&self) -> Result<(), ReliabilityError> {
        if self.schema != ARTIFACT_SCHEMA {
            return Err(ReliabilityError::SchemaVersionMismatch {
                found: self.schema.clone(),
                expected: ARTIFACT_SCHEMA.to_string(),
            });
        }
        Ok(())
    }
}

/// The context for resolving a probability source.
#[derive(Debug, Clone)]
pub struct ResolutionContext {
    /// The directory the `.etdl` document lives in, for relative artifact paths.
    pub base_dir: std::path::PathBuf,
    /// Known reliability artifacts loaded from the document's `x-reliability`
    /// sources section.
    pub artifacts: BTreeMap<String, ReliabilityArtifact>,
}

impl ResolutionContext {
    pub fn new(base_dir: impl Into<std::path::PathBuf>) -> Self {
        ResolutionContext {
            base_dir: base_dir.into(),
            artifacts: BTreeMap::new(),
        }
    }
}

/// A resolved deterministic probability plus its provenance, used by the
/// compiler to emit constants and the build manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedProbability {
    pub value: f64,
    pub estimate_id: String,
    pub artifact_id: String,
    pub artifact_version: Option<String>,
}

/// Provider abstraction: resolve an estimate from a source. Providers must not
/// require network access for the build path.
pub trait ProbabilityProvider {
    fn resolve(
        &self,
        source: &crate::probability::ProbabilitySource,
        estimate: &ProbabilityEstimate,
    ) -> Result<f64, ReliabilityError>;
}

/// A provider that returns the estimate's declared scalar.
#[derive(Debug, Default)]
pub struct LiteralProvider;

impl ProbabilityProvider for LiteralProvider {
    fn resolve(
        &self,
        _source: &crate::probability::ProbabilitySource,
        estimate: &ProbabilityEstimate,
    ) -> Result<f64, ReliabilityError> {
        estimate.resolved_probability()
    }
}

/// A test/mock provider that returns a scripted value, proving the provider
/// abstraction is decoupled from any concrete artifact representation.
#[derive(Debug)]
pub struct MockProbabilityProvider {
    /// Scripted value returned for any estimate.
    pub value: f64,
}

impl ProbabilityProvider for MockProbabilityProvider {
    fn resolve(
        &self,
        _source: &crate::probability::ProbabilitySource,
        _estimate: &ProbabilityEstimate,
    ) -> Result<f64, ReliabilityError> {
        if (0.0..=1.0).contains(&self.value) {
            Ok(self.value)
        } else {
            Err(ReliabilityError::OutOfRange(self.value))
        }
    }
}

/// The outcome of resolving a named estimate from an artifact.
///
/// The policy governs only `Unknown`: an estimate that exists but carries no
/// deterministic value. A *missing* estimate id is always an error — it means
/// the document references something the artifact does not contain.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveOutcome {
    /// A deterministic scalar with provenance, usable as a fault-tree override.
    Resolved(ResolvedProbability),
    /// The estimate exists but has no value (`unknown: true` / uncertainty-only).
    /// Callers decide what to do via `UnknownProbabilityPolicy`.
    Unknown { event: String },
    /// No estimate with that id exists in the artifact. Always an error.
    Missing { event: String },
}

impl ResolveOutcome {
    /// The probability for the build manifest, if this is `Resolved`.
    pub fn probability(&self) -> Option<&ResolvedProbability> {
        match self {
            ResolveOutcome::Resolved(r) => Some(r),
            _ => None,
        }
    }
}

/// Resolves a named estimate from an artifact, applying the unknown-probability
/// policy.
#[derive(Debug)]
pub struct ArtifactResolver {
    pub unknown_policy: UnknownProbabilityPolicy,
}

/// What to do when an estimate exists but has no deterministic value.
///
/// - `Error`: hard error; the build must not produce a number.
/// - `Warning`: warn and treat the estimate as unresolved (fault tree falls
///   back to the document's declared probability).
/// - `Allow`: silently treat the estimate as unresolved.
///
/// In no case is an unknown probability turned into a scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnknownProbabilityPolicy {
    Error,
    #[default]
    Warning,
    Allow,
}

impl Default for ArtifactResolver {
    fn default() -> Self {
        ArtifactResolver {
            unknown_policy: UnknownProbabilityPolicy::Warning,
        }
    }
}

impl ArtifactResolver {
    pub fn new(unknown_policy: UnknownProbabilityPolicy) -> Self {
        ArtifactResolver { unknown_policy }
    }

    /// Resolve `event` from `artifact` to a deterministic probability, or
    /// report whether the estimate is unknown-valued or missing.
    ///
    /// Unknown probabilities are never silently turned into zero.
    pub fn resolve(
        &self,
        artifact: &ReliabilityArtifact,
        event: &str,
    ) -> Result<ResolveOutcome, ReliabilityError> {
        let estimate = match artifact.get(event) {
            Some(e) => e,
            None => {
                return Ok(ResolveOutcome::Missing {
                    event: event.to_string(),
                })
            }
        };

        if estimate.is_unknown() {
            return Ok(ResolveOutcome::Unknown {
                event: event.to_string(),
            });
        }

        match estimate.resolved_probability() {
            Ok(value) => Ok(ResolveOutcome::Resolved(ResolvedProbability {
                value,
                estimate_id: estimate.event.clone(),
                artifact_id: artifact.id.clone(),
                artifact_version: artifact.version.clone(),
            })),
            // A non-probability metric or out-of-range value is a hard error
            // regardless of policy: the artifact itself is malformed for the
            // deterministic path.
            Err(e) => Err(e),
        }
    }
}

/// Convenience: a probability estimate in literal (declared) form.
pub fn declared(event: impl Into<String>, value: f64) -> ProbabilityEstimate {
    ProbabilityEstimate::new(event, ProbabilityState::Declared, value)
}

/// Convenience: a probability estimate with provenance.
pub fn with_provenance(
    event: impl Into<String>,
    state: ProbabilityState,
    value: f64,
    provenance: Provenance,
) -> ProbabilityEstimate {
    let mut e = ProbabilityEstimate::new(event, state, value);
    e.provenance = Some(provenance);
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_artifact() -> ReliabilityArtifact {
        let mut a = ReliabilityArtifact::new("payment-gateway");
        a.version = Some("1.2.0".into());
        a.add(declared("failure.gateway.timeout", 0.0027)).unwrap();
        a
    }

    #[test]
    fn artifact_roundtrips_json() {
        let a = sample_artifact();
        let s = serde_json::to_string(&a).unwrap();
        let b = ReliabilityArtifact::from_json(&s).unwrap();
        assert_eq!(
            b.get("failure.gateway.timeout").unwrap().value,
            Some(0.0027)
        );
    }

    #[test]
    fn resolver_returns_value_and_provenance() {
        let a = sample_artifact();
        let r = ArtifactResolver::new(UnknownProbabilityPolicy::Error);
        let outcome = r.resolve(&a, "failure.gateway.timeout").unwrap();
        let ResolveOutcome::Resolved(resolved) = outcome else {
            panic!("expected Resolved, got {outcome:?}");
        };
        assert_eq!(resolved.value, 0.0027);
        assert_eq!(resolved.artifact_id, "payment-gateway");
        assert_eq!(resolved.artifact_version.as_deref(), Some("1.2.0"));
    }

    #[test]
    fn resolver_missing_estimate_is_missing_not_error() {
        let a = sample_artifact();
        let r = ArtifactResolver::new(UnknownProbabilityPolicy::Error);
        match r.resolve(&a, "failure.missing").unwrap() {
            ResolveOutcome::Missing { event } => assert_eq!(event, "failure.missing"),
            other => panic!("expected Missing, got {other:?}"),
        }
    }

    #[test]
    fn resolver_unknown_value_is_unknown_outcome() {
        let mut a = sample_artifact();
        a.add(ProbabilityEstimate::unknown("failure.unknown"))
            .unwrap();
        let r = ArtifactResolver::new(UnknownProbabilityPolicy::Error);
        match r.resolve(&a, "failure.unknown").unwrap() {
            ResolveOutcome::Unknown { event } => assert_eq!(event, "failure.unknown"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn resolver_rejects_malformed_value_regardless_of_policy() {
        let mut a = sample_artifact();
        a.add(declared("failure.bad", 1.5)).unwrap(); // out of range
        let r = ArtifactResolver::new(UnknownProbabilityPolicy::Allow);
        assert!(r.resolve(&a, "failure.bad").is_err());
    }

    #[test]
    fn unknown_never_becomes_scalar() {
        // Unknown must never be converted to a number under any policy.
        let mut a = sample_artifact();
        a.add(ProbabilityEstimate::unknown("failure.unk")).unwrap();
        for policy in [
            UnknownProbabilityPolicy::Error,
            UnknownProbabilityPolicy::Warning,
            UnknownProbabilityPolicy::Allow,
        ] {
            let r = ArtifactResolver::new(policy);
            match r.resolve(&a, "failure.unk").unwrap() {
                ResolveOutcome::Unknown { .. } => {}
                other => panic!("policy {policy:?} turned unknown into {other:?}"),
            }
        }
    }

    #[test]
    fn schema_mismatch_rejected() {
        let a = sample_artifact();
        let bad = serde_json::to_string(&a)
            .unwrap()
            .replace(ARTIFACT_SCHEMA, "etdl.reliability.artifact/0.9");
        assert!(ReliabilityArtifact::from_json(&bad).is_err());
    }

    #[test]
    fn provider_abstraction_is_decoupled() {
        // The compiler depends on ProbabilityProvider, not on concrete artifact
        // representation. A mock provider proves the seam is usable by any
        // future provider (database, HTTP, external calculation, ...).
        let mock = MockProbabilityProvider { value: 0.42 };
        let est = declared("failure.x", 0.1); // mock ignores the estimate
        let resolved = mock
            .resolve(
                &crate::probability::ProbabilitySource::ExternalCalculation,
                &est,
            )
            .unwrap();
        assert_eq!(resolved, 0.42);

        let bad = MockProbabilityProvider { value: 1.5 };
        assert!(bad
            .resolve(
                &crate::probability::ProbabilitySource::ExternalCalculation,
                &est
            )
            .is_err());
    }

    #[test]
    fn literal_provider_returns_declared() {
        let provider = LiteralProvider;
        let est = declared("failure.x", 0.007);
        let v = provider
            .resolve(&crate::probability::ProbabilitySource::Literal, &est)
            .unwrap();
        assert_eq!(v, 0.007);
    }

    fn conditional(mut e: ProbabilityEstimate, cond: &str, value: f64) -> ProbabilityEstimate {
        e.conditions = vec![cond.to_string()];
        e.value = Some(value);
        e
    }

    #[test]
    fn multiple_conditions_are_preserved() {
        let mut a = ReliabilityArtifact::new("svc");
        a.add(conditional(
            declared("failure.gateway.timeout", 0.001),
            "production",
            0.001,
        ))
        .unwrap();
        a.add(conditional(
            declared("failure.gateway.timeout", 0.008),
            "high-load",
            0.008,
        ))
        .unwrap();
        a.add(conditional(
            declared("failure.gateway.timeout", 0.0005),
            "region=us-east",
            0.0005,
        ))
        .unwrap();

        // get() returns the primary (last added, which had no conditions... but
        // here all have conditions, so first deterministically).
        assert_eq!(a.all_for("failure.gateway.timeout").len(), 3);

        let selected = a
            .select("failure.gateway.timeout", &["high-load".to_string()], None)
            .unwrap();
        assert_eq!(selected.value, Some(0.008));

        let region = a
            .select(
                "failure.gateway.timeout",
                &["region=us-east".to_string()],
                None,
            )
            .unwrap();
        assert_eq!(region.value, Some(0.0005));
    }

    #[test]
    fn duplicate_estimate_is_rejected() {
        let mut a = ReliabilityArtifact::new("svc");
        a.add(declared("failure.x", 0.01)).unwrap();
        let err = a.add(declared("failure.x", 0.02)).unwrap_err();
        assert!(matches!(err, ReliabilityError::DuplicateEstimate { .. }));
        // Original preserved.
        assert_eq!(a.get("failure.x").unwrap().value, Some(0.01));
    }

    #[test]
    fn select_returns_primary_when_no_conditions() {
        let mut a = ReliabilityArtifact::new("svc");
        a.add(declared("failure.x", 0.01)).unwrap();
        let e = a
            .select("failure.x", &["irrelevant".to_string()], None)
            .unwrap();
        assert_eq!(e.value, Some(0.01));
    }
}
