//! Runtime library for code generated from Event Tree Definition Language
//! (ETDL) documents — the reliability-aware, event-driven DSL based on
//! [IEC 62502](https://github.com/ETDL-lang/etdl-specification) event tree and
//! [IEC 61025](https://github.com/ETDL-lang/etdl-specification) fault tree analysis.
//!
//! Generated handlers use these components to record branches and failures
//! against build-time-resolved probabilities, enforce declared retry policies,
//! detect SLA anomalies, and inject chaos in controlled environments.
//!
//! # Components
//!
//! - [`BranchMonitor`] — records taken branches and failures per node with their
//!   declared probabilities
//! - [`RetryPolicy`] / [`BackoffStrategy`] — async retry with exponential or fixed
//!   backoff and a total time budget
//! - [`SlaTracker`] — rolling-window anomaly detection
//!   (`ETDL_SLA_WINDOW`, `ETDL_SLA_THRESHOLD`)
//! - [`ChaosController`] — seeded, node-scoped failure injection, guarded off in
//!   production via `ETDL_ENV`
//! - [`inject_traceparent`] — W3C trace context propagation
//!
//! # Example
//!
//! ```
//! use etdl_core::{BranchMonitor, BackoffStrategy, RetryPolicy};
//! use std::time::Duration;
//!
//! let mut monitor = BranchMonitor::new("InventoryCheckBarrier");
//! monitor.record_branch("SUCCESS", 0.95);
//!
//! let retry = RetryPolicy {
//!     max_attempts: 3,
//!     backoff_ms: 250,
//!     strategy: BackoffStrategy::Exponential,
//! };
//! ```

pub mod chaos;
pub mod condition;
pub mod monitor;
pub mod observation;
pub mod publisher;
pub mod retry;
pub mod sla;
pub mod telemetry;

pub use chaos::ChaosController;
pub use monitor::BranchMonitor;
pub use observation::{
    generate_observation_id, now_rfc3339, CapturingSink, JsonlSink, NoopSink, ObservationSink,
    ReliabilityObservation, SharedSink,
};
pub use publisher::{ChannelCapturingPublisher, NoopPublisher, PublishError, Publisher};
pub use retry::{BackoffStrategy, RetryPolicy};
pub use sla::SlaTracker;
pub use telemetry::{inject_traceparent, Error as WorkflowError};

/// Re-export so generated code can call `etdl_core::serde_json::to_value(...)`
/// without the consuming crate declaring `serde_json` directly.
pub use serde_json;
