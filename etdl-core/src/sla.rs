use std::collections::{HashMap, VecDeque};

const DEFAULT_WINDOW_SIZE: usize = 1000;
const DEFAULT_DEVIATION_THRESHOLD: f64 = 0.10;
const MIN_OBSERVATIONS: usize = 10;

/// Tracks observed outcome frequencies per node against the declared
/// probabilities, and reports SLA anomalies when the divergence exceeds a
/// threshold (ETDL §9.3).
///
/// The observed frequency of an outcome is the fraction of the node's
/// evaluations in the rolling window that produced that outcome. This gives a
/// meaningful comparison against the declared probability: if a branch declares
/// `SUCCESS = 0.95` but only 60% of evaluations actually succeed, the deviation
/// exceeds the threshold and an anomaly is reported.
pub struct SlaTracker {
    window_size: usize,
    deviation_threshold: f64,
    /// node id -> (rolling outcome labels, bounded to window_size)
    windows: HashMap<String, VecDeque<Option<String>>>,
    /// (node id, outcome) -> declared probability
    expected: HashMap<(String, String), f64>,
}

impl SlaTracker {
    pub fn new() -> Self {
        SlaTracker {
            window_size: Self::env_window_size(),
            deviation_threshold: Self::env_deviation_threshold(),
            windows: HashMap::new(),
            expected: HashMap::new(),
        }
    }

    pub fn with_config(window_size: usize, deviation_threshold: f64) -> Self {
        SlaTracker {
            window_size,
            deviation_threshold,
            windows: HashMap::new(),
            expected: HashMap::new(),
        }
    }

    /// Record one evaluation of `node_id`.
    ///
    /// `occurred` is true when `outcome` was actually observed (the branch was
    /// taken / the failure happened); false otherwise. The declared probability
    /// for the outcome is remembered (the most recent value wins).
    ///
    /// Returns `true` when the observed frequency for `outcome` deviates from
    /// the declared probability by more than the threshold (with enough
    /// observations to be meaningful).
    pub fn record(
        &mut self,
        node_id: &str,
        outcome: &str,
        declared_probability: f64,
        occurred: bool,
    ) -> bool {
        self.expected.insert(
            (node_id.to_string(), outcome.to_string()),
            declared_probability,
        );

        let window = self.windows.entry(node_id.to_string()).or_default();
        window.push_back(occurred.then(|| outcome.to_string()));
        if window.len() > self.window_size {
            window.pop_front();
        }

        self.is_anomaly(node_id, outcome, declared_probability)
    }

    fn is_anomaly(&self, node_id: &str, outcome: &str, declared: f64) -> bool {
        let window = match self.windows.get(node_id) {
            Some(w) => w,
            None => return false,
        };
        if window.len() < MIN_OBSERVATIONS {
            return false;
        }
        let observed = self.observed_frequency(node_id, outcome);
        (observed - declared).abs() > self.deviation_threshold
    }

    /// The fraction of the node's rolling window evaluations that produced
    /// `outcome`. Returns 0.0 when there are no observations yet.
    pub fn observed_frequency(&self, node_id: &str, outcome: &str) -> f64 {
        match self.windows.get(node_id) {
            Some(window) if !window.is_empty() => {
                let total = window.len() as f64;
                let hits = window
                    .iter()
                    .filter(|label| label.as_deref() == Some(outcome))
                    .count() as f64;
                hits / total
            }
            _ => 0.0,
        }
    }

    /// The most recently declared probability for `(node_id, outcome)`, if any.
    pub fn declared_probability(&self, node_id: &str, outcome: &str) -> Option<f64> {
        self.expected
            .get(&(node_id.to_string(), outcome.to_string()))
            .copied()
    }

    pub fn total_evaluations(&self) -> usize {
        self.windows.values().map(|w| w.len()).sum()
    }

    fn env_window_size() -> usize {
        std::env::var("ETDL_SLA_WINDOW")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_WINDOW_SIZE)
    }

    fn env_deviation_threshold() -> f64 {
        std::env::var("ETDL_SLA_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_DEVIATION_THRESHOLD)
    }
}

impl Default for SlaTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sla_tracking() {
        let mut tracker = SlaTracker::new();
        // 95 of 100 evaluations succeed -> observed ~0.95 matches declared 0.95.
        for i in 0..100 {
            tracker.record("barrier_1", "SUCCESS", 0.95, i < 95);
        }
        let freq = tracker.observed_frequency("barrier_1", "SUCCESS");
        assert!((freq - 0.95).abs() < 0.01, "got {}", freq);
        assert_eq!(tracker.total_evaluations(), 100);
    }

    #[test]
    fn test_sla_anomaly_detection() {
        let mut tracker = SlaTracker::with_config(100, 0.10);
        // Declared 0.95 but only 60% succeed -> anomaly.
        let mut anomaly_detected = false;
        for i in 0..80 {
            let is_anomaly = tracker.record("barrier_1", "SUCCESS", 0.95, i < 48);
            if is_anomaly {
                anomaly_detected = true;
            }
        }
        assert!(anomaly_detected);
    }

    #[test]
    fn no_anomaly_when_matching_declared() {
        let mut tracker = SlaTracker::with_config(100, 0.10);
        let mut anomaly = false;
        for i in 0..100 {
            anomaly |= tracker.record("b", "SUCCESS", 0.5, i % 2 == 0);
        }
        assert!(!anomaly, "matching declared should not alarm");
    }

    #[test]
    fn window_is_bounded() {
        let mut tracker = SlaTracker::with_config(50, 0.10);
        for i in 0..200 {
            tracker.record("b", "SUCCESS", 0.5, i % 2 == 0);
        }
        assert_eq!(tracker.total_evaluations(), 50);
    }
}
