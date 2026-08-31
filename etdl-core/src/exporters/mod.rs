//! Optional observability exporters for [`crate::BranchMonitor`]'s runtime
//! observations — each an independent, off-by-default Cargo feature. See
//! `docs/reference/observability-exporters.md`.
//!
//! Each exporter installs a process-global recorder/subscriber/provider
//! from an established third-party crate (`metrics`, `tracing`,
//! `opentelemetry`) once, at startup. [`crate::monitor::BranchMonitor`]
//! then reports through the same ambient global backend from
//! `record_branch`/`record_success`/`record_failure`, with no handle
//! threaded through call sites — and so can any future code (e.g. a later
//! safety/security/diagnostics runtime hook), by calling the same
//! `metrics::*!`/`tracing::*!`/`opentelemetry::global::*` APIs directly,
//! with no coordination with this module required.

use std::fmt;

#[cfg(feature = "exporter-loki")]
pub mod loki;
#[cfg(feature = "exporter-otlp")]
pub mod otlp;
#[cfg(feature = "exporter-prometheus")]
pub mod prometheus;

/// An error setting up an exporter (install/build failed). Setup-time
/// only — never returned from a handler.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}
