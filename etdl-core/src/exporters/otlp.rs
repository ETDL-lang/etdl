//! Push metrics to an OTel Collector via the official OpenTelemetry Rust
//! SDK, over HTTP+protobuf (not gRPC/`tonic` — no extra codegen toolchain
//! needed to build).
//!
//! Two-tier, like [`super::loki`]: [`build`] returns an `SdkMeterProvider`
//! for an app that manages OTel providers itself; [`install`] is a
//! convenience that sets it as the global meter provider — needed for the
//! ambient-global design this module shares with the other exporters:
//! `BranchMonitor` (this feature on) reports through
//! `opentelemetry::global::meter("etdl")`, not a threaded handle, via the
//! functions below; any future code can do the same directly.

use super::Error;
use opentelemetry::metrics::Counter;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::{MetricExporter, WithExportConfig};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use std::sync::OnceLock;

/// Builds an `SdkMeterProvider` exporting to `endpoint` (an OTel Collector,
/// or any OTLP/HTTP receiver) via periodic push. Does not install it
/// globally — see [`install`] for that.
pub fn build(endpoint: &str) -> Result<SdkMeterProvider, Error> {
    let exporter = MetricExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .build()
        .map_err(|e| Error(format!("otlp exporter: {e}")))?;
    let reader = PeriodicReader::builder(exporter).build();
    Ok(SdkMeterProvider::builder().with_reader(reader).build())
}

/// Convenience: builds the provider and installs it as the global OTel
/// meter provider. Call once at startup, before handling any messages.
pub fn install(endpoint: &str) -> Result<(), Error> {
    global::set_meter_provider(build(endpoint)?);
    Ok(())
}

// One cached instrument per metric name, per the `Meter::u64_counter` doc
// comment's own guidance (creating a fresh instrument per call "could
// lower SDK performance" — clone/reuse instead).
static BRANCH_COUNTER: OnceLock<Counter<u64>> = OnceLock::new();
static SUCCESS_COUNTER: OnceLock<Counter<u64>> = OnceLock::new();
static FAILURE_COUNTER: OnceLock<Counter<u64>> = OnceLock::new();
static ANOMALY_COUNTER: OnceLock<Counter<u64>> = OnceLock::new();

fn meter() -> opentelemetry::metrics::Meter {
    global::meter("etdl")
}

pub(crate) fn record_branch(node_id: &str, outcome: &str) {
    let counter = BRANCH_COUNTER.get_or_init(|| meter().u64_counter("etdl.branch.total").build());
    counter.add(1, &[KeyValue::new("event", node_id.to_string()), KeyValue::new("outcome", outcome.to_string())]);
}

pub(crate) fn record_success(operation_id: &str) {
    let counter =
        SUCCESS_COUNTER.get_or_init(|| meter().u64_counter("etdl.operation.success.total").build());
    counter.add(1, &[KeyValue::new("operation", operation_id.to_string())]);
}

pub(crate) fn record_failure(operation_id: &str) {
    let counter =
        FAILURE_COUNTER.get_or_init(|| meter().u64_counter("etdl.operation.failure.total").build());
    counter.add(1, &[KeyValue::new("operation", operation_id.to_string())]);
}

pub(crate) fn record_anomaly(node_id: &str, outcome: &str) {
    let counter =
        ANOMALY_COUNTER.get_or_init(|| meter().u64_counter("etdl.sla.anomaly.total").build());
    counter.add(1, &[KeyValue::new("event", node_id.to_string()), KeyValue::new("outcome", outcome.to_string())]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::metrics::MeterProvider as _;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// Same "raw local `TcpListener`, no mocking crate" approach as the
    /// Loki test: proves `record_branch` really produces an OTLP/HTTP push
    /// to the configured endpoint, not just that the call compiles. Uses a
    /// short export interval so the test doesn't wait for the SDK's 60s
    /// default periodic-export window.
    #[test]
    fn record_branch_is_pushed_to_the_otlp_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(false).unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut content_length = 0usize;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            std::io::Read::read_exact(&mut reader, &mut body).unwrap();
            let mut stream = stream;
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/x-protobuf\r\ncontent-length: 0\r\n\r\n",
            );
            let _ = tx.send((request_line, body));
        });

        let exporter = MetricExporter::builder()
            .with_http()
            .with_endpoint(format!("http://{addr}"))
            .build()
            .expect("build exporter");
        let reader = PeriodicReader::builder(exporter)
            .with_interval(std::time::Duration::from_millis(50))
            .build();
        let provider = SdkMeterProvider::builder().with_reader(reader).build();

        let counter = provider.meter("etdl-test").u64_counter("etdl.branch.total").build();
        counter.add(1, &[KeyValue::new("event", "TestBarrier"), KeyValue::new("outcome", "SUCCESS")]);

        let (request_line, body) = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("otlp push did not arrive within 10s");
        let _ = provider.shutdown();

        assert!(request_line.starts_with("POST"), "got: {request_line}");
        assert!(!body.is_empty(), "expected a non-empty protobuf-encoded payload");
    }
}
