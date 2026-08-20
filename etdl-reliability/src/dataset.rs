//! Versioned, immutable observation datasets.
//!
//! An [`ObservationDataset`] is the logical unit of "a batch of observations
//! collected under known conditions, over a known period, from a known
//! source". It does not require every observation to live in one physical
//! file — only that the logical dataset is identifiable by `(id, version)`.
//!
//! New observations never mutate an existing dataset: publishing more data is
//! always a new [`ObservationDataset`] value with a new `version`, referring
//! back to what changed via [`DatasetRef`]. Nothing in this module exposes a
//! way to mutate a dataset's observations in place; a "new" dataset is always
//! a distinct value. This is what makes reliability analysis over observation
//! history reproducible: two engineers loading dataset `orders@3` always see
//! exactly the same data.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use etdl_reliability_core::provenance::Provenance;

use crate::observations::{AggregateObservation, ObservationError, TimeInterval};
use crate::probability::TimeBasis;

/// Version of the observation-dataset schema.
pub const DATASET_SCHEMA: &str = "etdl.reliability.observation-dataset/1.0";

fn default_schema() -> String {
    DATASET_SCHEMA.to_string()
}

/// A versioned, immutable collection of [`AggregateObservation`]s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationDataset {
    #[serde(default = "default_schema")]
    pub schema: String,
    pub id: String,
    /// Explicit, monotonically-assigned dataset version (e.g. `"1"`, `"2"`).
    /// Not derived automatically: the engineer/pipeline that publishes a new
    /// version states it, so version numbers stay meaningful and auditable.
    pub version: String,
    pub observations: Vec<AggregateObservation>,
    /// Where this dataset came from (system, pipeline, export job).
    #[serde(default)]
    pub source: Option<String>,
    /// The time span this dataset's observations were collected over.
    #[serde(default)]
    pub collection_period: Option<TimeInterval>,
    /// Conditions common to the whole dataset (e.g. `production`), in
    /// addition to any per-observation conditions.
    #[serde(default)]
    pub conditions: Vec<String>,
    #[serde(default)]
    pub provenance: Option<Provenance>,
}

/// A reference to an immutable dataset (id + version), used for provenance on
/// derived results (aggregates, calibration reports, ...). Never a substitute
/// for loading the dataset itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DatasetRef {
    pub id: String,
    pub version: String,
}

impl std::fmt::Display for DatasetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.id, self.version)
    }
}

/// A problem found while validating a dataset. Validation never silently
/// repairs invalid data; it reports.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DatasetError {
    #[error("dataset id is required")]
    MissingId,
    #[error("dataset version is required")]
    MissingVersion,
    #[error("dataset schema '{found}' is not supported (expected '{expected}')")]
    SchemaMismatch { found: String, expected: String },
    #[error(
        "dataset '{dataset}' is empty; an observation dataset must contain at least one observation"
    )]
    Empty { dataset: String },
    #[error(
        "observation at index {index} in dataset '{dataset}' has no id; array position is never identity"
    )]
    MissingObservationId { dataset: String, index: usize },
    #[error("duplicate observation id '{id}' in dataset '{dataset}'")]
    DuplicateObservationId { dataset: String, id: String },
    #[error("invalid collection period in dataset '{dataset}': {source}")]
    InvalidPeriod {
        dataset: String,
        #[source]
        source: ObservationError,
    },
    #[error("observation '{id}' in dataset '{dataset}' is invalid: {source}")]
    InvalidObservation {
        dataset: String,
        id: String,
        #[source]
        source: ObservationError,
    },
}

impl ObservationDataset {
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        ObservationDataset {
            schema: DATASET_SCHEMA.to_string(),
            id: id.into(),
            version: version.into(),
            observations: Vec::new(),
            source: None,
            collection_period: None,
            conditions: Vec::new(),
            provenance: None,
        }
    }

    /// Validate structure and every contained observation.
    ///
    /// Enforces: matching schema; non-empty id/version; a non-empty
    /// observation list; every observation carrying a non-empty, dataset-wide
    /// unique id (never array position); a structurally valid collection
    /// period; and each observation individually valid.
    pub fn validate(&self) -> Result<(), DatasetError> {
        if self.schema != DATASET_SCHEMA {
            return Err(DatasetError::SchemaMismatch {
                found: self.schema.clone(),
                expected: DATASET_SCHEMA.to_string(),
            });
        }
        if self.id.trim().is_empty() {
            return Err(DatasetError::MissingId);
        }
        if self.version.trim().is_empty() {
            return Err(DatasetError::MissingVersion);
        }
        if self.observations.is_empty() {
            return Err(DatasetError::Empty {
                dataset: self.id.clone(),
            });
        }
        if let Some(period) = &self.collection_period {
            period.validate().map_err(|source| DatasetError::InvalidPeriod {
                dataset: self.id.clone(),
                source,
            })?;
        }

        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for (index, o) in self.observations.iter().enumerate() {
            let id = match o.id.as_deref() {
                Some(id) if !id.trim().is_empty() => id,
                _ => {
                    return Err(DatasetError::MissingObservationId {
                        dataset: self.id.clone(),
                        index,
                    })
                }
            };
            if !seen.insert(id) {
                return Err(DatasetError::DuplicateObservationId {
                    dataset: self.id.clone(),
                    id: id.to_string(),
                });
            }
            o.validate().map_err(|source| DatasetError::InvalidObservation {
                dataset: self.id.clone(),
                id: id.to_string(),
                source,
            })?;
        }
        Ok(())
    }

    /// Look up an observation by its stable id (never by array position).
    pub fn find(&self, id: &str) -> Option<&AggregateObservation> {
        self.observations
            .iter()
            .find(|o| o.id.as_deref() == Some(id))
    }

    /// A reference to this dataset for provenance on derived results.
    pub fn dataset_ref(&self) -> DatasetRef {
        DatasetRef {
            id: self.id.clone(),
            version: self.version.clone(),
        }
    }
}

/// How an [`AggregatedObservation`] was derived, and from what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregationProvenance {
    /// The datasets contributing observations, sorted for determinism.
    pub source_datasets: Vec<DatasetRef>,
    /// The individual observation ids summed, sorted for determinism.
    pub source_observation_ids: Vec<String>,
    /// The aggregation method (currently always `"sum"`).
    pub method: String,
}

/// The result of aggregating compatible observations for one failure mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregatedObservation {
    pub observation: AggregateObservation,
    pub provenance: AggregationProvenance,
}

/// A problem found while aggregating observations. Aggregation never sums
/// observations whose exposure basis or conditions differ; it reports why.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AggregationError {
    #[error("no observations for failure mode '{0}' in the given datasets")]
    NoMatchingObservations(String),
    #[error(
        "cannot aggregate '{failure_mode}': exposure unit mismatch ({a} vs {b}) between \
         observation '{id_a}' and '{id_b}'"
    )]
    IncompatibleExposureUnit {
        failure_mode: String,
        a: TimeBasis,
        b: TimeBasis,
        id_a: String,
        id_b: String,
    },
    #[error(
        "cannot aggregate '{failure_mode}': condition mismatch ({a:?} vs {b:?}) between \
         observation '{id_a}' and '{id_b}'"
    )]
    IncompatibleConditions {
        failure_mode: String,
        a: Vec<String>,
        b: Vec<String>,
        id_a: String,
        id_b: String,
    },
    #[error("observation '{id}' is invalid: {source}")]
    Invalid {
        id: String,
        #[source]
        source: ObservationError,
    },
}

/// Aggregate every observation for `failure_mode` across `datasets`,
/// refusing to combine observations whose exposure unit or conditions are
/// incompatible. The result preserves references to every source dataset and
/// source observation id, so it remains auditable back to its origin.
///
/// Time intervals are merged as `[min(starts), max(ends)]` only when every
/// contributing observation declares one; otherwise the aggregate has no
/// interval, rather than fabricating one.
pub fn aggregate_across(
    datasets: &[&ObservationDataset],
    failure_mode: &str,
) -> Result<AggregatedObservation, AggregationError> {
    let mut matches: Vec<(&ObservationDataset, &AggregateObservation)> = Vec::new();
    for ds in datasets {
        for o in &ds.observations {
            if o.failure_mode == failure_mode {
                matches.push((ds, o));
            }
        }
    }

    let Some((_, first)) = matches.first().copied() else {
        return Err(AggregationError::NoMatchingObservations(
            failure_mode.to_string(),
        ));
    };
    let mut normalized_conditions = first.conditions.clone();
    normalized_conditions.sort();
    let unit = first.exposure_unit;
    let first_id = first.id.clone().unwrap_or_default();

    let mut exposure = 0u64;
    let mut failures = 0u64;
    let mut source_datasets: Vec<DatasetRef> = Vec::new();
    let mut source_ids: Vec<String> = Vec::new();
    let mut all_have_intervals = true;
    let mut starts: Vec<String> = Vec::new();
    let mut ends: Vec<String> = Vec::new();

    for (ds, o) in &matches {
        let id = o.id.clone().unwrap_or_default();
        o.validate().map_err(|source| AggregationError::Invalid {
            id: id.clone(),
            source,
        })?;
        if o.exposure_unit != unit {
            return Err(AggregationError::IncompatibleExposureUnit {
                failure_mode: failure_mode.to_string(),
                a: unit,
                b: o.exposure_unit,
                id_a: first_id.clone(),
                id_b: id,
            });
        }
        let mut cond = o.conditions.clone();
        cond.sort();
        if cond != normalized_conditions {
            return Err(AggregationError::IncompatibleConditions {
                failure_mode: failure_mode.to_string(),
                a: normalized_conditions.clone(),
                b: cond,
                id_a: first_id.clone(),
                id_b: id,
            });
        }

        exposure += o.exposure;
        failures += o.failures;
        let dr = ds.dataset_ref();
        if !source_datasets.contains(&dr) {
            source_datasets.push(dr);
        }
        if !id.is_empty() {
            source_ids.push(id);
        }
        match &o.interval {
            Some(iv) => {
                starts.push(iv.start.clone());
                ends.push(iv.end.clone());
            }
            None => all_have_intervals = false,
        }
    }

    source_datasets.sort();
    source_ids.sort();

    let interval = if all_have_intervals && !starts.is_empty() {
        Some(TimeInterval {
            start: starts.into_iter().min().unwrap(),
            end: ends.into_iter().max().unwrap(),
        })
    } else {
        None
    };

    let agg_id = format!("agg-{:016x}", fnv1a(&source_ids.join(",")));

    let observation = AggregateObservation {
        id: Some(agg_id),
        failure_mode: failure_mode.to_string(),
        exposure,
        failures,
        exposure_unit: unit,
        conditions: normalized_conditions,
        interval,
        source: Some(format!(
            "aggregate of {} dataset(s)",
            source_datasets.len()
        )),
        version: None,
    };
    Ok(AggregatedObservation {
        observation,
        provenance: AggregationProvenance {
            source_datasets,
            source_observation_ids: source_ids,
            method: "sum".to_string(),
        },
    })
}

/// FNV-1a 64-bit, for deterministic content-derived ids. Not cryptographic;
/// collision resistance is not a security property here, only a
/// reproducibility one (same inputs -> same id).
fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agg(id: &str, failures: u64, exposure: u64) -> AggregateObservation {
        AggregateObservation {
            id: Some(id.to_string()),
            failure_mode: "failure.gateway.timeout".into(),
            exposure,
            failures,
            exposure_unit: TimeBasis::PerRequest,
            conditions: vec!["production".into()],
            interval: None,
            source: Some("prod-obs".into()),
            version: None,
        }
    }

    #[test]
    fn dataset_validates_well_formed() {
        let mut ds = ObservationDataset::new("orders", "1");
        ds.observations.push(agg("o1", 37, 100_000));
        assert!(ds.validate().is_ok());
    }

    #[test]
    fn dataset_rejects_missing_observation_id() {
        let mut ds = ObservationDataset::new("orders", "1");
        let mut o = agg("o1", 1, 10);
        o.id = None;
        ds.observations.push(o);
        assert!(matches!(
            ds.validate(),
            Err(DatasetError::MissingObservationId { .. })
        ));
    }

    #[test]
    fn dataset_rejects_duplicate_observation_ids_even_if_reordered() {
        let mut ds1 = ObservationDataset::new("orders", "1");
        ds1.observations
            .push(agg("dup", 1, 10));
        ds1.observations
            .push(agg("dup", 2, 20));
        assert!(matches!(
            ds1.validate(),
            Err(DatasetError::DuplicateObservationId { .. })
        ));

        // Identity survives reordering: the same two (differently valued)
        // observations under swapped positions still collide on id.
        let mut ds2 = ObservationDataset::new("orders", "1");
        ds2.observations.push(agg("dup", 2, 20));
        ds2.observations.push(agg("dup", 1, 10));
        assert!(matches!(
            ds2.validate(),
            Err(DatasetError::DuplicateObservationId { .. })
        ));
    }

    #[test]
    fn dataset_rejects_empty() {
        let ds = ObservationDataset::new("orders", "1");
        assert!(matches!(ds.validate(), Err(DatasetError::Empty { .. })));
    }

    #[test]
    fn v1_is_never_mutated_by_v2() {
        let mut v1 = ObservationDataset::new("orders", "1");
        v1.observations.push(agg("o1", 37, 100_000));
        let v1_snapshot = v1.clone();

        let mut v2 = ObservationDataset::new("orders", "2");
        v2.observations.push(agg("o1", 37, 100_000));
        v2.observations.push(agg("o2", 3, 10_000));

        assert_eq!(v1, v1_snapshot);
        assert_eq!(v1.observations.len(), 1);
        assert_eq!(v2.observations.len(), 2);
    }

    #[test]
    fn aggregate_sums_compatible_observations_across_datasets() {
        let mut ds_a = ObservationDataset::new("region-a", "1");
        ds_a.observations.push(agg("a1", 37, 100_000));
        let mut ds_b = ObservationDataset::new("region-b", "1");
        ds_b.observations.push(agg("b1", 3, 10_000));

        let result = aggregate_across(&[&ds_a, &ds_b], "failure.gateway.timeout").unwrap();
        assert_eq!(result.observation.exposure, 110_000);
        assert_eq!(result.observation.failures, 40);
        assert_eq!(result.provenance.source_datasets.len(), 2);
        assert_eq!(
            result.provenance.source_observation_ids,
            vec!["a1".to_string(), "b1".to_string()]
        );
    }

    #[test]
    fn aggregate_refuses_incompatible_exposure_units() {
        let mut ds = ObservationDataset::new("mixed", "1");
        ds.observations.push(agg("a1", 37, 100_000));
        let mut b = agg("b1", 3, 10_000);
        b.exposure_unit = TimeBasis::PerHour;
        ds.observations.push(b);

        let err = aggregate_across(&[&ds], "failure.gateway.timeout").unwrap_err();
        assert!(matches!(
            err,
            AggregationError::IncompatibleExposureUnit { .. }
        ));
    }

    #[test]
    fn aggregate_refuses_incompatible_conditions() {
        let mut ds = ObservationDataset::new("mixed", "1");
        ds.observations.push(agg("a1", 37, 100_000));
        let mut b = agg("b1", 3, 10_000);
        b.conditions = vec!["high-load".into()];
        ds.observations.push(b);

        let err = aggregate_across(&[&ds], "failure.gateway.timeout").unwrap_err();
        assert!(matches!(
            err,
            AggregationError::IncompatibleConditions { .. }
        ));
    }

    #[test]
    fn aggregate_id_is_deterministic_regardless_of_dataset_order() {
        let mut ds_a = ObservationDataset::new("region-a", "1");
        ds_a.observations.push(agg("a1", 37, 100_000));
        let mut ds_b = ObservationDataset::new("region-b", "1");
        ds_b.observations.push(agg("b1", 3, 10_000));

        let forward = aggregate_across(&[&ds_a, &ds_b], "failure.gateway.timeout").unwrap();
        let backward = aggregate_across(&[&ds_b, &ds_a], "failure.gateway.timeout").unwrap();
        assert_eq!(forward.observation.id, backward.observation.id);
    }

    #[test]
    fn aggregate_no_match_is_explicit_error() {
        let ds = ObservationDataset::new("empty", "1");
        let err = aggregate_across(&[&ds], "failure.nonexistent");
        assert!(matches!(
            err,
            Err(AggregationError::NoMatchingObservations(_))
        ));
    }
}
