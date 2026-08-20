//! Probability metrics, units, and sources.

use serde::{Deserialize, Serialize};

/// The kind of quantity a probability estimate represents. These are distinct
/// and MUST NOT be silently converted between each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ProbabilityMetric {
    /// Probability of an event (dimensionless, in `[0,1]`).
    #[default]
    Probability,
    /// Failure rate λ (per unit time).
    FailureRate,
    /// Rate of occurrence (e.g. per request).
    EventFrequency,
    /// Availability (dimensionless, in `[0,1]`).
    Availability,
    /// Mean time between failures.
    MeanTimeBetweenFailures,
    /// Mean time to failure.
    MeanTimeToFailure,
    /// Mean time to repair.
    MeanTimeToRepair,
}

impl ProbabilityMetric {
    /// Whether this metric is a dimensionless probability in `[0,1]`.
    pub fn is_probability_like(self) -> bool {
        matches!(
            self,
            ProbabilityMetric::Probability | ProbabilityMetric::Availability
        )
    }

    /// Whether the metric is a rate expressed over a time basis (per-hour,
    /// per-mission, ...). Rates MUST be accompanied by a `time_basis`.
    pub fn requires_time_basis(self) -> bool {
        matches!(
            self,
            ProbabilityMetric::FailureRate
                | ProbabilityMetric::EventFrequency
                | ProbabilityMetric::MeanTimeBetweenFailures
                | ProbabilityMetric::MeanTimeToFailure
                | ProbabilityMetric::MeanTimeToRepair
        )
    }

    /// The unit family of the metric, for documentation and diagnostics.
    pub fn unit_family(self) -> &'static str {
        match self {
            ProbabilityMetric::Probability => "dimensionless [0,1]",
            ProbabilityMetric::FailureRate => "failures per time unit",
            ProbabilityMetric::EventFrequency => "events per exposure unit",
            ProbabilityMetric::Availability => "dimensionless [0,1]",
            ProbabilityMetric::MeanTimeBetweenFailures => "time unit",
            ProbabilityMetric::MeanTimeToFailure => "time unit",
            ProbabilityMetric::MeanTimeToRepair => "time unit",
        }
    }
}

/// The exposure/denominator a probability or rate is defined over.
///
/// A probability is meaningless without knowing what the denominator
/// represents; mixing populations silently is forbidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum TimeBasis {
    PerRequest,
    PerOperation,
    PerTransaction,
    PerMessage,
    PerHour,
    PerMission,
    PerDeployment,
    PerServiceInstance,
    /// An explicit but unclassified basis; the caller must supply units text.
    #[default]
    Other,
}

impl std::fmt::Display for TimeBasis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TimeBasis::PerRequest => "per-request",
            TimeBasis::PerOperation => "per-operation",
            TimeBasis::PerTransaction => "per-transaction",
            TimeBasis::PerMessage => "per-message",
            TimeBasis::PerHour => "per-hour",
            TimeBasis::PerMission => "per-mission",
            TimeBasis::PerDeployment => "per-deployment",
            TimeBasis::PerServiceInstance => "per-service-instance",
            TimeBasis::Other => "other",
        };
        f.write_str(s)
    }
}

/// Where a probability estimate came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum ProbabilitySource {
    #[default]
    Literal,
    EngineeringAssumption,
    Measurement,
    HistoricalData,
    Prediction,
    StatisticalModel,
    ExternalCalculation,
    VendorData,
    Simulation,
    Inference,
    Unknown,
}

impl ProbabilitySource {
    pub fn label(&self) -> &'static str {
        match self {
            ProbabilitySource::Literal => "declared",
            ProbabilitySource::EngineeringAssumption => "assumed",
            ProbabilitySource::Measurement => "measured",
            ProbabilitySource::HistoricalData => "measured",
            ProbabilitySource::Prediction => "predicted",
            ProbabilitySource::StatisticalModel => "estimated",
            ProbabilitySource::ExternalCalculation => "imported",
            ProbabilitySource::VendorData => "imported",
            ProbabilitySource::Simulation => "estimated",
            ProbabilitySource::Inference => "inferred",
            ProbabilitySource::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_classification() {
        assert!(ProbabilityMetric::Probability.is_probability_like());
        assert!(ProbabilityMetric::Availability.is_probability_like());
        assert!(!ProbabilityMetric::FailureRate.is_probability_like());
    }

    #[test]
    fn time_basis_display() {
        assert_eq!(TimeBasis::PerRequest.to_string(), "per-request");
        assert_eq!(TimeBasis::PerMission.to_string(), "per-mission");
    }
}
