//! Publisher abstraction for generated ETDL handlers.
//!
//! Generated code emits Consequence `send` operations as calls on a
//! [`Publisher`] supplied by the caller. This keeps generated handlers free of
//! any concrete transport (Kafka, NATS, HTTP, in-memory, ...) so they remain
//! pure, deterministic, and testable — while the application wires a real
//! transport at the boundary.
//!
//! The reference implementation ships [`NoopPublisher`] (discards with a log) and
//! [`ChannelCapturingPublisher`] (records `(channel, payload, headers)` triples
//! for tests).
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

    /// Publish `payload` to `channel` with additional `headers` attached —
    /// used by generated code when a document declares
    /// `etdl.live-reliability` (see `etdl-core::live` and
    /// `docs/reference/live-reliability.md`) to carry a fault tree's
    /// current values to the next service. **Additive**: the default
    /// implementation ignores `headers` and delegates to [`Publisher::publish`],
    /// so every existing implementor (this crate's own, and any
    /// application's) keeps compiling and behaving identically without
    /// changes. An implementor that doesn't override this silently drops
    /// the carried values — the receiving service's affected node just
    /// falls back to its own prior/cold-start estimate, never a hard
    /// failure, matching this whole feature's best-effort character.
    fn publish_with_headers(
        &self,
        channel: &str,
        payload: &serde_json::Value,
        headers: &serde_json::Value,
    ) -> Result<(), PublishError> {
        let _ = headers;
        self.publish(channel, payload)
    }
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

/// A `(channel, payload, headers)` triple recorded by [`ChannelCapturingPublisher`].
/// `headers` is `None` for a plain [`Publisher::publish`] call and `Some(...)`
/// for a [`Publisher::publish_with_headers`] call.
pub type CapturedPublish = (String, serde_json::Value, Option<serde_json::Value>);

/// A [`Publisher`] that records `(channel, payload, headers)` triples for
/// assertions. `headers` is `None` for plain [`Publisher::publish`] calls and
/// `Some(...)` for [`Publisher::publish_with_headers`] calls — the latter is
/// how a document declaring `etdl.live-reliability` attaches a fault tree's
/// current values to an outgoing message (see [`crate::live`]), so tests
/// exercising that supplement need a way to recover what was attached.
#[derive(Debug, Default, Clone)]
pub struct ChannelCapturingPublisher {
    sent: Arc<Mutex<Vec<CapturedPublish>>>,
}

impl ChannelCapturingPublisher {
    /// Create a new empty capturing publisher.
    pub fn new() -> Self {
        Self::default()
    }

    /// The recorded `(channel, payload)` pairs in publish order (headers, if
    /// any were attached, are dropped — see [`Self::sent_with_headers`]).
    pub fn sent(&self) -> Vec<(String, serde_json::Value)> {
        self.sent_with_headers()
            .into_iter()
            .map(|(c, p, _)| (c, p))
            .collect()
    }

    /// The recorded `(channel, payload, headers)` triples in publish order.
    pub fn sent_with_headers(&self) -> Vec<CapturedPublish> {
        self.sent.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// True if any message was published to `channel`.
    pub fn published_to(&self, channel: &str) -> bool {
        self.sent().iter().any(|(c, _)| c == channel)
    }

    /// The headers attached to the most recent [`Publisher::publish_with_headers`]
    /// call for `channel`, or `None` if no such call happened (either nothing
    /// was published to `channel`, or it was published via plain
    /// [`Publisher::publish`] with no headers attached).
    pub fn headers_sent_to(&self, channel: &str) -> Option<serde_json::Value> {
        self.sent_with_headers()
            .into_iter()
            .rev()
            .find(|(c, _, _)| c == channel)
            .and_then(|(_, _, h)| h)
    }
}

impl Publisher for ChannelCapturingPublisher {
    fn publish(&self, channel: &str, payload: &serde_json::Value) -> Result<(), PublishError> {
        if let Ok(mut g) = self.sent.lock() {
            g.push((channel.to_string(), payload.clone(), None));
        }
        Ok(())
    }

    fn publish_with_headers(
        &self,
        channel: &str,
        payload: &serde_json::Value,
        headers: &serde_json::Value,
    ) -> Result<(), PublishError> {
        if let Ok(mut g) = self.sent.lock() {
            g.push((channel.to_string(), payload.clone(), Some(headers.clone())));
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

    #[test]
    fn capturing_publisher_records_headers_separately_from_plain_publish() {
        let p = ChannelCapturingPublisher::new();
        p.publish("plain", &serde_json::json!(1)).unwrap();
        p.publish_with_headers("with-headers", &serde_json::json!(2), &serde_json::json!({"k": "v"}))
            .unwrap();

        assert_eq!(p.headers_sent_to("plain"), None);
        assert_eq!(
            p.headers_sent_to("with-headers"),
            Some(serde_json::json!({"k": "v"}))
        );
        assert_eq!(p.headers_sent_to("never-published"), None);

        // `sent()` still reports both, headers-less, for callers that only
        // care about channel/payload (unchanged behavior).
        let sent = p.sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].0, "with-headers");
    }
}
