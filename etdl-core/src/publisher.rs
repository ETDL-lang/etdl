//! Publisher abstraction for generated ETDL handlers.
//!
//! Generated code emits Consequence `send` operations as calls on a
//! [`Publisher`] supplied by the caller. This keeps generated handlers free of
//! any concrete transport (Kafka, NATS, HTTP, in-memory, ...) so they remain
//! pure, deterministic, and testable — while the application wires a real
//! transport at the boundary.
//!
//! The reference implementation ships [`NoopPublisher`] (discards with a log) and
//! [`ChannelCapturingPublisher`] (records `(channel, payload)` pairs for tests).
//! Applications implement [`Publisher`] for their own infrastructure and, per
//! ETDL §9.2, SHOULD inject the W3C `traceparent` (see
//! [`crate::telemetry::inject_traceparent`]) into every outbound message.

use std::sync::{Arc, Mutex};

/// An error produced while publishing a message to a channel.
#[derive(Debug, Clone)]
pub struct PublishError(pub String);

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PublishError {}

impl From<String> for PublishError {
    fn from(s: String) -> Self {
        PublishError(s)
    }
}

/// A transport-agnostic channel publisher.
///
/// `payload` is the message serialized to a JSON [`serde_json::Value`]. The
/// concrete AsyncAPI message type is serialized by generated code before the
/// call so the trait stays free of generics and remains object-safe.
pub trait Publisher: Send + Sync {
    /// Publish `payload` to `channel`.
    fn publish(&self, channel: &str, payload: &serde_json::Value) -> Result<(), PublishError>;
}

/// A [`Publisher`] that logs and discards every message.
///
/// Useful as a default in tests or during bring-up when no transport exists yet.
#[derive(Debug, Default, Clone)]
pub struct NoopPublisher;

impl Publisher for NoopPublisher {
    fn publish(&self, channel: &str, payload: &serde_json::Value) -> Result<(), PublishError> {
        eprintln!(
            "[etdl.publisher] noop: channel={} payload={}",
            channel, payload
        );
        Ok(())
    }
}

/// A [`Publisher`] that records `(channel, payload)` pairs for assertions.
#[derive(Debug, Default, Clone)]
pub struct ChannelCapturingPublisher {
    sent: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
}

impl ChannelCapturingPublisher {
    /// Create a new empty capturing publisher.
    pub fn new() -> Self {
        Self::default()
    }

    /// The recorded `(channel, payload)` pairs in publish order.
    pub fn sent(&self) -> Vec<(String, serde_json::Value)> {
        self.sent.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// True if any message was published to `channel`.
    pub fn published_to(&self, channel: &str) -> bool {
        self.sent().iter().any(|(c, _)| c == channel)
    }
}

impl Publisher for ChannelCapturingPublisher {
    fn publish(&self, channel: &str, payload: &serde_json::Value) -> Result<(), PublishError> {
        if let Ok(mut g) = self.sent.lock() {
            g.push((channel.to_string(), payload.clone()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_publisher_accepts_all() {
        let p = NoopPublisher;
        assert!(p.publish("ch", &serde_json::json!({"a": 1})).is_ok());
    }

    #[test]
    fn capturing_publisher_records_ordered() {
        let p = ChannelCapturingPublisher::new();
        p.publish("a", &serde_json::json!(1)).unwrap();
        p.publish("b", &serde_json::json!({"k": "v"})).unwrap();
        let sent = p.sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, "a");
        assert_eq!(sent[1].0, "b");
        assert!(p.published_to("b"));
        assert!(!p.published_to("c"));
    }
}
