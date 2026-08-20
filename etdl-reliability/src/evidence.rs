//! Reliability evidence primitives.

use serde::{Deserialize, Serialize};

use crate::observation::ReliabilityObservation;
use crate::observations::{AggregateObservation, TimeInterval};
use crate::probability::TimeBasis;

/// Aggregated evidence about a failure mode: immutable observations plus a
/// counter. Rewriting history is forbidden; the counter is derived.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub event: String,
    pub observations: Vec<ReliabilityObservation>,
    pub successes: u64,
    pub failures: u64,
}

impl Evidence {
    pub fn new(event: impl Into<String>) -> Self {
        Evidence {
            event: event.into(),
            observations: Vec::new(),
            successes: 0,
            failures: 0,
        }
    }

    /// Append an observation; returns the derived totals.
    pub fn append(&mut self, o: ReliabilityObservation) -> (u64, u64) {
        if o.outcome.eq_ignore_ascii_case("failed") || o.outcome.eq_ignore_ascii_case("failure") {
            self.failures += 1;
        } else {
            self.successes += 1;
        }
        self.observations.push(o);
        (self.successes, self.failures)
    }

    pub fn total(&self) -> u64 {
        self.successes + self.failures
    }

    /// Empirical probability of failure, if any observations exist.
    pub fn empirical_failure_probability(&self) -> Option<f64> {
        if self.total() == 0 {
            None
        } else {
            Some(self.failures as f64 / self.total() as f64)
        }
    }

    /// Bridge raw per-occurrence evidence into an [`AggregateObservation`]
    /// (the counted, exposure-explicit form the estimators and calibration
    /// consume). The exposure unit is not inferred: the caller states what
    /// each occurrence represents (e.g. one request), since a bare
    /// per-occurrence record does not self-describe its exposure basis.
    pub fn to_aggregate_observation(
        &self,
        id: impl Into<String>,
        exposure_unit: TimeBasis,
        conditions: Vec<String>,
        interval: Option<TimeInterval>,
        source: Option<String>,
    ) -> AggregateObservation {
        AggregateObservation {
            id: Some(id.into()),
            failure_mode: self.event.clone(),
            exposure: self.total(),
            failures: self.failures,
            exposure_unit,
            conditions,
            interval,
            source,
            version: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_appends_immutably() {
        let mut e = Evidence::new("failure.x");
        let (s, f) = e.append(ReliabilityObservation::new("o1", "failure.x", "t").outcome_failed());
        assert_eq!((s, f), (0, 1));
        let _ = e.append(ReliabilityObservation::new("o2", "failure.x", "t").outcome_succeeded());
        assert_eq!(e.total(), 2);
        assert_eq!(e.empirical_failure_probability(), Some(0.5));
    }

    #[test]
    fn bridges_to_aggregate_observation() {
        let mut e = Evidence::new("failure.gateway.timeout");
        for _ in 0..37 {
            e.append(ReliabilityObservation::new("o", "failure.gateway.timeout", "t").outcome_failed());
        }
        for _ in 0..(100_000 - 37) {
            e.append(
                ReliabilityObservation::new("o", "failure.gateway.timeout", "t")
                    .outcome_succeeded(),
            );
        }
        let agg = e.to_aggregate_observation(
            "agg-1",
            TimeBasis::PerRequest,
            vec!["production".into()],
            None,
            Some("runtime-capture".into()),
        );
        assert_eq!(agg.id.as_deref(), Some("agg-1"));
        assert_eq!(agg.exposure, 100_000);
        assert_eq!(agg.failures, 37);
        assert!(agg.validate().is_ok());
    }
}
