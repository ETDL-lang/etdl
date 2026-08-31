//! Compiled-in Prometheus scrape endpoint (PromQL dialect support).
//!
//! [`install`] sets up the `metrics` crate's process-global recorder and
//! starts an embedded HTTP listener serving Prometheus text exposition —
//! one call, no hand-rolled server code. `BranchMonitor` (this feature on)
//! reports every branch/success/failure through the same ambient global
//! recorder via the functions below; any future code can do the same
//! directly with the plain `metrics::counter!`/`gauge!`/`histogram!`
//! macros, with no coordination with this module required.

use super::Error;
use std::net::SocketAddr;

/// Starts the compiled-in Prometheus scrape endpoint at `bind_addr` (e.g.
/// `127.0.0.1:9464`, serving `/metrics`). Call once at startup, before
/// handling any messages. Safe to call from inside or outside an existing
/// Tokio runtime — `metrics-exporter-prometheus` manages its own
/// background task/thread either way.
pub fn install(bind_addr: SocketAddr) -> Result<(), Error> {
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(bind_addr)
        .install()
        .map_err(|e| Error(format!("prometheus exporter: {e}")))
}

pub(crate) fn record_branch(node_id: &str, outcome: &str) {
    metrics::counter!("etdl_branch_total", "event" => node_id.to_string(), "outcome" => outcome.to_string())
        .increment(1);
}

pub(crate) fn record_success(operation_id: &str) {
    metrics::counter!("etdl_operation_success_total", "operation" => operation_id.to_string())
        .increment(1);
}

pub(crate) fn record_failure(operation_id: &str) {
    metrics::counter!("etdl_operation_failure_total", "operation" => operation_id.to_string())
        .increment(1);
}

pub(crate) fn record_anomaly(node_id: &str, outcome: &str) {
    metrics::counter!("etdl_sla_anomaly_total", "event" => node_id.to_string(), "outcome" => outcome.to_string())
        .increment(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `metrics::set_global_recorder` can only succeed once per process, so
    /// this crate's test binary gets exactly one prometheus test — it uses
    /// `build()` (the same call `install()` makes internally) rather than
    /// `install()` itself so it can both bind a real listener *and* get a
    /// handle to assert the rendered text directly, without needing to
    /// discover the listener's ephemeral port for a real HTTP round-trip.
    #[tokio::test]
    async fn record_branch_is_scraped_as_real_prometheus_text() {
        let (recorder, exporter) = metrics_exporter_prometheus::PrometheusBuilder::new()
            .with_http_listener("127.0.0.1:0".parse::<SocketAddr>().unwrap())
            .build()
            .expect("build");
        let handle = recorder.handle();
        metrics::set_global_recorder(recorder).expect("set_global_recorder");
        tokio::spawn(exporter);

        record_branch("TestBarrier", "SUCCESS");
        record_success("checkout");
        record_failure("checkout");
        record_anomaly("TestBarrier", "SUCCESS");

        let rendered = handle.render();
        assert!(rendered.contains("etdl_branch_total"), "got:\n{rendered}");
        assert!(rendered.contains("event=\"TestBarrier\""), "got:\n{rendered}");
        assert!(rendered.contains("outcome=\"SUCCESS\""), "got:\n{rendered}");
        assert!(rendered.contains("etdl_operation_success_total"), "got:\n{rendered}");
        assert!(rendered.contains("etdl_operation_failure_total"), "got:\n{rendered}");
        assert!(rendered.contains("etdl_sla_anomaly_total"), "got:\n{rendered}");
        assert!(rendered.contains("operation=\"checkout\""), "got:\n{rendered}");
    }
}
