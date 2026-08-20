//! Lightweight runtime evidence collection.
//!
//! The runtime collects **immutable observations** for later offline analysis.
//! It does NOT run Bayesian inference, query reliability databases, run Monte
//! Carlo, or call AI — those are analysis-time concerns (see the
//! `etdl-reliability` crate). This keeps the runtime service-local and
//! lightweight, per the ETDL architecture.

use std::sync::Arc;

/// An immutable reliability observation: what happened, when, and under what
/// conditions. No sensitive payload data by default.
///
/// Identity is the explicit `id` field, never array position: a dataset built
/// from these observations must remain stable under reordering.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReliabilityObservation {
    pub id: String,
    pub event: String,
    pub timestamp: String,
    pub service: Option<String>,
    pub operation: Option<String>,
    pub environment: Option<String>,
    pub deployment: Option<String>,
    /// The software/model version that produced this observation (e.g. a
    /// service semver or the compiled ETDL artifact version). Distinct from
    /// `deployment`, which identifies the deployment slot/environment, not
    /// the code that ran in it.
    pub service_version: Option<String>,
    /// A stable reference to the compiled build/reliability artifact that
    /// generated this observation (e.g. `payment-gateway@1.2.0`), so an
    /// analyst can trace "which model predicted this" without the runtime
    /// carrying the full artifact. See `etdl-build-manifest.json`.
    pub build_ref: Option<String>,
    pub outcome: String,
    pub conditions: Vec<String>,
    pub duration_ms: Option<u64>,
    pub trace_id: Option<String>,
}

impl ReliabilityObservation {
    pub fn new(id: impl Into<String>, event: impl Into<String>) -> Self {
        ReliabilityObservation {
            id: id.into(),
            event: event.into(),
            timestamp: String::new(),
            service: None,
            operation: None,
            environment: None,
            deployment: None,
            service_version: None,
            build_ref: None,
            outcome: String::new(),
            conditions: Vec::new(),
            duration_ms: None,
            trace_id: None,
        }
    }
}

/// Generate a lightweight, collision-resistant observation id. Uses OS
/// randomness with a time+counter fallback (same strategy as
/// [`crate::telemetry::inject_traceparent`]); no heavy dependency is added.
pub fn generate_observation_id() -> String {
    let mut bytes = [0u8; 12];
    if getrandom::getrandom(&mut bytes).is_err() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut seed = nanos ^ counter.wrapping_mul(0x9E3779B97F4A7C15);
        for slot in bytes.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *slot = seed as u8;
        }
    }
    let mut s = String::with_capacity(3 + bytes.len() * 2);
    s.push_str("obs");
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// The current time as an RFC 3339 / ISO-8601 UTC timestamp
/// (`YYYY-MM-DDThh:mm:ssZ`), computed from `SystemTime` without a chrono
/// dependency so the runtime stays lightweight.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    civil_from_unix(secs)
}

/// Convert Unix seconds (UTC, no leap seconds) to an RFC 3339 timestamp using
/// Howard Hinnant's `civil_from_days` algorithm (public domain).
fn civil_from_unix(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hh, mm, ss)
}

/// A destination for observations. Implementations may write JSON Lines, CSV,
/// OpenTelemetry, a database adapter, or a message stream. These are optional
/// adapters; the runtime does not require any of them.
pub trait ObservationSink: Send + Sync {
    fn emit(&self, observation: &ReliabilityObservation);
}

/// A sink that drops observations (default). Enables "no telemetry configured".
#[derive(Debug, Default, Clone)]
pub struct NoopSink;

impl ObservationSink for NoopSink {
    fn emit(&self, _observation: &ReliabilityObservation) {}
}

/// A sink that writes observations as JSON Lines to a `Vec<String>` for tests
/// and simple capture.
#[derive(Debug, Default)]
pub struct CapturingSink {
    lines: std::sync::Mutex<Vec<String>>,
}

impl CapturingSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl ObservationSink for CapturingSink {
    fn emit(&self, observation: &ReliabilityObservation) {
        if let Ok(line) = serde_json::to_string(observation) {
            if let Ok(mut g) = self.lines.lock() {
                g.push(line);
            }
        }
    }
}

/// A sink that appends observations as JSON Lines to a file. Each `emit` is
/// one `write` + `flush` of a single line: no buffering that could lose
/// observations on process termination, no statistics, no aggregation. The
/// analysis layer (`etdl-reliability`) reads the resulting file offline.
pub struct JsonlSink {
    file: std::sync::Mutex<std::fs::File>,
}

impl JsonlSink {
    /// Open (creating if absent, appending if present) a JSON Lines file for
    /// observation capture.
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(JsonlSink {
            file: std::sync::Mutex::new(file),
        })
    }
}

impl ObservationSink for JsonlSink {
    fn emit(&self, observation: &ReliabilityObservation) {
        use std::io::Write;
        let Ok(mut line) = serde_json::to_string(observation) else {
            return;
        };
        line.push('\n');
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
    }
}

/// Shared sink handle used by the runtime.
pub type SharedSink = Arc<dyn ObservationSink>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capturing_sink_records() {
        let sink = CapturingSink::new();
        let obs = ReliabilityObservation::new("obs-1", "failure.network.timeout");
        sink.emit(&obs);
        let lines = sink.lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("failure.network.timeout"));
    }

    #[test]
    fn observation_is_plain_data() {
        let obs = ReliabilityObservation::new("obs-1", "failure.network.timeout");
        assert_eq!(obs.event, "failure.network.timeout");
        assert!(obs.duration_ms.is_none());
    }

    #[test]
    fn generated_ids_are_unique_and_stable_prefix() {
        let a = generate_observation_id();
        let b = generate_observation_id();
        assert_ne!(a, b);
        assert!(a.starts_with("obs"));
        assert_eq!(a.len(), 3 + 24); // "obs" + 12 bytes hex
    }

    #[test]
    fn rfc3339_timestamp_is_well_formed() {
        // 2025-08-18T00:00:00Z == unix 1755475200
        assert_eq!(civil_from_unix(1_755_475_200), "2025-08-18T00:00:00Z");
        // 1970-01-01T00:00:00Z == unix 0 (epoch)
        assert_eq!(civil_from_unix(0), "1970-01-01T00:00:00Z");
        let now = now_rfc3339();
        assert_eq!(now.len(), 20);
        assert!(now.ends_with('Z'));
    }

    #[test]
    fn jsonl_sink_appends_lines_and_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("etdl-jsonl-test-{}", generate_observation_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("observations.jsonl");

        {
            let sink = JsonlSink::open(&path).unwrap();
            sink.emit(&ReliabilityObservation::new("obs-1", "failure.a"));
        }
        {
            // Reopening must append, never truncate history.
            let sink = JsonlSink::open(&path).unwrap();
            sink.emit(&ReliabilityObservation::new("obs-2", "failure.b"));
        }

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("failure.a"));
        assert!(lines[1].contains("failure.b"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
