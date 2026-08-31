//! Push observations to a Loki-compatible push API (LogQL dialect
//! support).
//!
//! Two-tier, like [`super::otlp`]: [`layer`] returns a
//! `tracing_subscriber` layer to compose into an app's own subscriber
//! setup; [`install`] is a convenience that installs a fresh global
//! subscriber for an app with none yet. Either way, `BranchMonitor` (this
//! feature on) reports every branch/success/failure through `tracing`'s
//! ambient global subscriber via the functions below; any future code can
//! do the same directly with the plain `tracing::info!`/`event!` macros,
//! with no coordination with this module required.

use super::Error;
use std::collections::HashMap;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Builds a Loki layer and its background delivery task. Compose into the
/// app's own subscriber: `tracing_subscriber::registry().with(layer)...
/// .init()`, then `tokio::spawn(task)` to actually run delivery.
pub fn layer(
    loki_url: url::Url,
    labels: HashMap<String, String>,
) -> Result<(tracing_loki::Layer, tracing_loki::BackgroundTask), Error> {
    tracing_loki::layer(loki_url, labels, HashMap::new())
        .map_err(|e| Error(format!("loki exporter: {e}")))
}

/// Convenience: sets a fresh global `tracing` subscriber with the Loki
/// layer and spawns its background delivery task. Call once at startup,
/// before handling any messages. An app with its own `tracing` setup
/// already should compose [`layer`] into it instead of calling this —
/// installing a second global subscriber panics.
pub fn install(loki_url: url::Url, labels: HashMap<String, String>) -> Result<(), Error> {
    let (layer, task) = layer(loki_url, labels)?;
    tracing_subscriber::registry()
        .with(layer)
        .try_init()
        .map_err(|e| Error(format!("loki exporter: failed to install global subscriber: {e}")))?;
    spawn_background_task(task);
    Ok(())
}

/// Runs `task` on whatever Tokio runtime is already active on the calling
/// thread, or — mirroring `metrics-exporter-prometheus`'s own `install()`
/// — spins up a dedicated single-threaded runtime on a new OS thread when
/// there isn't one. Callers of `etdl_core` directly are always inside the
/// embedding app's own async runtime (generated handlers are `async fn`),
/// but a caller reaching this through `etdl-runtime-ffi` (a non-Rust host
/// language) has no Rust async runtime at all — this makes `install` work
/// either way rather than panicking with "no reactor running".
fn spawn_background_task(task: tracing_loki::BackgroundTask) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(task);
    } else {
        // Fire-and-forget, like `PrometheusBuilder::install`'s own
        // fallback thread: the dedicated thread now owns delivery for the
        // process's lifetime, so the join handle is intentionally dropped.
        let _ = std::thread::Builder::new()
            .name("etdl-loki-exporter".to_string())
            .spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    rt.block_on(task);
                }
            });
    }
}

pub(crate) fn record_branch(node_id: &str, outcome: &str, declared_probability: f64) {
    tracing::info!(
        etdl_kind = "branch",
        etdl_node_id = node_id,
        etdl_outcome = outcome,
        etdl_declared_probability = declared_probability
    );
}

pub(crate) fn record_success(operation_id: &str) {
    tracing::info!(etdl_kind = "success", etdl_operation_id = operation_id);
}

pub(crate) fn record_failure(operation_id: &str, error_message: &str) {
    tracing::info!(
        etdl_kind = "failure",
        etdl_operation_id = operation_id,
        etdl_error = error_message
    );
}

pub(crate) fn record_anomaly(node_id: &str, outcome: &str) {
    tracing::info!(etdl_kind = "sla_anomaly", etdl_node_id = node_id, etdl_outcome = outcome);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// No mocking crate needed: a raw `TcpListener` on an OS-assigned port
    /// captures the one HTTP request `tracing-loki`'s background task sends
    /// when its batch flushes, proving `record_branch` really reaches the
    /// push API — not just that the macro call compiles.
    #[tokio::test]
    async fn record_branch_is_pushed_to_the_loki_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let captured = std::thread::spawn(move || {
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
            let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n");
            (request_line, body)
        });

        let url = url::Url::parse(&format!("http://{addr}")).unwrap();
        let mut labels = HashMap::new();
        labels.insert("service".to_string(), "etdl-core-test".to_string());
        // Installed globally (`try_init`), matching tracing-loki's own
        // documented usage — a thread-scoped `tracing::subscriber::
        // with_default` around just the `record_branch` call below was
        // tried first and silently never delivered anything (tracing's
        // per-callsite interest caching does not treat a scoped default
        // the same as a global one; empirically confirmed, not just a
        // style preference).
        let (test_layer, task) = layer(url, labels).expect("layer");
        let _ = tracing_subscriber::registry().with(test_layer).try_init();
        let task_handle = tokio::spawn(task);

        record_branch("TestBarrier", "SUCCESS", 0.95);

        let (request_line, body) = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::task::spawn_blocking(move || captured.join().unwrap()),
        )
        .await
        .expect("loki push did not arrive within 10s")
        .unwrap();
        task_handle.abort();

        assert!(request_line.starts_with("POST"), "got: {request_line}");
        assert!(!body.is_empty(), "expected a non-empty pushed payload");
    }
}
