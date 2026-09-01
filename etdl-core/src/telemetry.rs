use std::fmt;

#[derive(Debug)]
pub struct Error {
    message: String,
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Error {
            message: msg.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::new(s)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::new(format!("serialization error: {}", e))
    }
}

impl From<crate::publisher::PublishError> for Error {
    fn from(e: crate::publisher::PublishError) -> Self {
        Error::new(format!("publish error: {}", e))
    }
}

pub type WorkflowError = Error;

pub fn attach_node_span_attribute(node_id: &str) {
    eprintln!("[etdl.telemetry] span attribute etdl.node.id={}", node_id);
}

/// A Diagnostics Supplement (`etdl.diagnostics`) Correlation to surface
/// alongside an SLA anomaly, when the anomalous node/key matches a
/// declared Correlation's `spanValue` for `spanAttribute ==
/// "etdl.node.id"` (the only attribute `attach_node_span_attribute` ever
/// emits). Populated by generated code from compile-time-resolved
/// `x-diagnostics.correlations` data — this module never looks anything
/// up itself. Stays exactly as stub-like as `emit_anomaly_event` itself:
/// carries strings through the same `eprintln!`-based mechanism, no new
/// tracing/span infrastructure.
pub struct CorrelatedCause<'a> {
    pub cause_ref: &'a str,
    pub description: Option<&'a str>,
}

pub fn emit_anomaly_event(
    node_id: &str,
    outcome: &str,
    declared_probability: f64,
    observed_frequency: f64,
    cause: Option<CorrelatedCause>,
) {
    let cause_suffix = cause
        .map(|c| format!(" cause_ref={} description={}", c.cause_ref, c.description.unwrap_or("-")))
        .unwrap_or_default();
    eprintln!(
        "[etdl.telemetry] SLA ANOMALY | node={} outcome={} declared={:.6} observed={:.6} deviation={:.6}{}",
        node_id,
        outcome,
        declared_probability,
        observed_frequency,
        (observed_frequency - declared_probability).abs(),
        cause_suffix
    );
}

pub fn inject_traceparent(message_type: &str) -> String {
    let trace_id = generate_trace_id();
    let span_id = generate_span_id();
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    eprintln!(
        "[etdl.telemetry] inject traceparent into {} message: {}",
        message_type, traceparent
    );

    traceparent
}

fn generate_trace_id() -> String {
    let mut bytes = [0u8; 16];
    fill_random(&mut bytes);
    hex(&bytes)
}

fn generate_span_id() -> String {
    let mut bytes = [0u8; 8];
    fill_random(&mut bytes);
    hex(&bytes)
}

/// Fill `buf` with OS randomness; fall back to a time+counter mix if the OS
/// source is unavailable (rare; format remains valid and unique in-process).
fn fill_random(buf: &mut [u8]) {
    if getrandom::getrandom(buf).is_err() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut seed = nanos ^ counter.wrapping_mul(0x9E3779B97F4A7C15);
        for slot in buf.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *slot = seed as u8;
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traceparent_is_well_formed() {
        let tp = inject_traceparent("Test");
        // version-trace-span-flags
        let parts: Vec<&str> = tp.split('-').collect();
        assert_eq!(parts.len(), 4, "got {}", tp);
        assert_eq!(parts[0], "00");
        assert_eq!(parts[1].len(), 32, "trace-id must be 32 hex, got {}", tp);
        assert_eq!(parts[2].len(), 16, "span-id must be 16 hex, got {}", tp);
        assert_eq!(parts[3], "01");
        assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));
        assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn trace_ids_are_not_all_zero() {
        let a = generate_trace_id();
        let b = generate_span_id();
        assert_ne!(a, "00000000000000000000000000000000");
        assert_ne!(b, "0000000000000000");
    }

    #[test]
    fn hex_lengths() {
        assert_eq!(hex(&[0u8; 16]).len(), 32);
        assert_eq!(hex(&[0u8; 8]).len(), 16);
    }
}
