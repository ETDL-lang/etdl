//! Rust failure pattern definitions: classification, severity, ontology
//! mapping, and confidence.

use crate::candidate::{FailureClassification, Severity};
use crate::identity;
use crate::mapping::{MappingQuality, OntologyMapping};

/// A single detected source pattern: what to report and how to classify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustPattern {
    /// `?` on a `Result`/`Option` in a function that can propagate.
    ErrorPropagation,
    /// `return Err(...)`.
    ExplicitErrReturn,
    /// `unwrap()`.
    Unwrap,
    /// `expect(...)`.
    Expect,
    /// `panic!`.
    Panic,
    /// `unreachable!`.
    Unreachable,
    /// `todo!` / `unimplemented!`.
    Unimplemented,
    /// `assert!`, `assert_eq!`, `assert_ne!`.
    Assertion,
    /// Index expression `a[i]`.
    Indexing,
    /// `/` or `%` (potential divide-by-zero).
    Division,
    /// `.parse::<T>()`.
    Parsing,
    /// Filesystem operation.
    Filesystem,
    /// Network / client operation.
    Network,
    /// Serialization/deserialization.
    Serialization,
    /// Channel send/receive.
    Channel,
    /// Mutex / RwLock lock acquisition.
    Lock,
    /// Timeout API.
    Timeout,
    /// External dependency call (conservative).
    Dependency,
    /// Custom error type definition.
    CustomError,
}

impl RustPattern {
    /// The domain used in the stable candidate identity.
    pub fn domain(self) -> &'static str {
        match self {
            RustPattern::ErrorPropagation => "runtime",
            RustPattern::ExplicitErrReturn => "runtime",
            RustPattern::Unwrap => "runtime",
            RustPattern::Expect => "runtime",
            RustPattern::Panic => "runtime",
            RustPattern::Unreachable => "runtime",
            RustPattern::Unimplemented => "runtime",
            RustPattern::Assertion => "runtime",
            RustPattern::Indexing => "runtime",
            RustPattern::Division => "runtime",
            RustPattern::Parsing => "validation",
            RustPattern::Filesystem => "io",
            RustPattern::Network => "network",
            RustPattern::Serialization => "serialization",
            RustPattern::Channel => "messaging",
            RustPattern::Lock => "concurrency",
            RustPattern::Timeout => "network",
            RustPattern::Dependency => "dependency",
            RustPattern::CustomError => "application",
        }
    }

    /// The concept token used in the stable candidate identity.
    pub fn concept(self) -> &'static str {
        match self {
            RustPattern::ErrorPropagation => "error_propagation",
            RustPattern::ExplicitErrReturn => "explicit_err_return",
            RustPattern::Unwrap => "unwrap",
            RustPattern::Expect => "expect",
            RustPattern::Panic => "panic",
            RustPattern::Unreachable => "unreachable",
            RustPattern::Unimplemented => "unimplemented",
            RustPattern::Assertion => "assertion",
            RustPattern::Indexing => "index_out_of_bounds",
            RustPattern::Division => "division_by_zero",
            RustPattern::Parsing => "parse_failure",
            RustPattern::Filesystem => "io_failure",
            RustPattern::Network => "network_operation",
            RustPattern::Serialization => "serialization_failure",
            RustPattern::Channel => "channel_failure",
            RustPattern::Lock => "lock_poisoning",
            RustPattern::Timeout => "timeout",
            RustPattern::Dependency => "dependency_operation",
            RustPattern::CustomError => "custom_error",
        }
    }

    pub fn classification(self) -> FailureClassification {
        match self {
            RustPattern::ErrorPropagation
            | RustPattern::ExplicitErrReturn
            | RustPattern::CustomError => FailureClassification::ApplicationFailure,
            RustPattern::Unwrap
            | RustPattern::Expect
            | RustPattern::Panic
            | RustPattern::Unreachable
            | RustPattern::Unimplemented
            | RustPattern::Assertion
            | RustPattern::Indexing
            | RustPattern::Division => FailureClassification::ApplicationFailure,
            RustPattern::Parsing => FailureClassification::ValidationFailure,
            RustPattern::Filesystem => FailureClassification::IoFailure,
            RustPattern::Network => FailureClassification::DependencyFailure,
            RustPattern::Serialization => FailureClassification::SerializationFailure,
            RustPattern::Channel | RustPattern::Lock => FailureClassification::ConcurrencyFailure,
            RustPattern::Timeout => FailureClassification::TimeoutFailure,
            RustPattern::Dependency => FailureClassification::DependencyFailure,
        }
    }

    pub fn severity(self) -> Severity {
        match self {
            RustPattern::Unwrap
            | RustPattern::Expect
            | RustPattern::Panic
            | RustPattern::Indexing
            | RustPattern::Division => Severity::High,
            RustPattern::Unreachable | RustPattern::Unimplemented => Severity::Medium,
            RustPattern::Assertion
            | RustPattern::Network
            | RustPattern::Timeout
            | RustPattern::Dependency
            | RustPattern::Filesystem
            | RustPattern::Serialization
            | RustPattern::Channel
            | RustPattern::Lock => Severity::Medium,
            RustPattern::ErrorPropagation
            | RustPattern::ExplicitErrReturn
            | RustPattern::Parsing
            | RustPattern::CustomError => Severity::Low,
        }
    }

    /// Base discovery confidence: how sure we are the pattern is what it looks
    /// like. This is discovery confidence, never a failure probability.
    pub fn base_confidence(self) -> f64 {
        match self {
            RustPattern::Panic
            | RustPattern::Unreachable
            | RustPattern::Unimplemented
            | RustPattern::Assertion => 0.98,
            RustPattern::Unwrap | RustPattern::Expect => 0.95,
            RustPattern::Indexing | RustPattern::Division => 0.85,
            RustPattern::Parsing
            | RustPattern::Filesystem
            | RustPattern::Network
            | RustPattern::Serialization
            | RustPattern::Channel
            | RustPattern::Lock
            | RustPattern::Timeout
            | RustPattern::Dependency => 0.8,
            RustPattern::ErrorPropagation
            | RustPattern::ExplicitErrReturn
            | RustPattern::CustomError => 0.7,
        }
    }

    /// The canonical ontology id this pattern usually maps to, if one exists.
    pub fn default_ontology(self) -> Option<&'static str> {
        match self {
            RustPattern::ErrorPropagation | RustPattern::ExplicitErrReturn => {
                Some("failure.runtime.unhandled_error")
            }
            RustPattern::Unwrap | RustPattern::Expect => Some("failure.runtime.unhandled_error"),
            RustPattern::Panic
            | RustPattern::Unreachable
            | RustPattern::Unimplemented
            | RustPattern::Assertion => Some("failure.runtime.unhandled_error"),
            RustPattern::Indexing | RustPattern::Division => {
                Some("failure.runtime.unhandled_error")
            }
            RustPattern::Parsing => Some("failure.configuration.invalid"),
            RustPattern::Filesystem => Some("failure.storage.io_failure"),
            RustPattern::Network => Some("failure.network.unreachable"),
            RustPattern::Serialization => None,
            RustPattern::Channel => Some("failure.messaging.publish_failure"),
            RustPattern::Lock => None,
            RustPattern::Timeout => Some("failure.network.timeout"),
            RustPattern::Dependency => Some("failure.dependency.unavailable"),
            RustPattern::CustomError => None,
        }
    }

    /// A human label for evidence.
    pub fn evidence_label(self) -> &'static str {
        match self {
            RustPattern::ErrorPropagation => "Result/Option error propagation",
            RustPattern::ExplicitErrReturn => "explicit Err return",
            RustPattern::Unwrap => "unwrap()",
            RustPattern::Expect => "expect(...)",
            RustPattern::Panic => "panic!",
            RustPattern::Unreachable => "unreachable!",
            RustPattern::Unimplemented => "todo!/unimplemented!",
            RustPattern::Assertion => "assertion",
            RustPattern::Indexing => "index expression",
            RustPattern::Division => "division/remainder",
            RustPattern::Parsing => "parse operation",
            RustPattern::Filesystem => "filesystem operation",
            RustPattern::Network => "network operation",
            RustPattern::Serialization => "serialization operation",
            RustPattern::Channel => "channel send/receive",
            RustPattern::Lock => "lock acquisition",
            RustPattern::Timeout => "timeout API",
            RustPattern::Dependency => "external dependency call",
            RustPattern::CustomError => "custom error type",
        }
    }
}

/// Map a pattern (and optional symbol, e.g. a custom error name) into an
/// ontology mapping with a quality state.
pub fn pattern_mapping(
    pattern: RustPattern,
    ontology: &crate::ontology::OntologyView,
) -> OntologyMapping {
    let proposed = identity::candidate_id(pattern.domain(), pattern.concept());
    let Some(default) = pattern.default_ontology() else {
        return OntologyMapping::unmapped(proposed);
    };
    match ontology.resolve(default) {
        Some((alive, deprecated)) => {
            let alive_clone = alive.clone();
            let mut m = OntologyMapping {
                canonical_id: Some(alive),
                proposed_concept: None,
                quality: if deprecated {
                    MappingQuality::Deprecated
                } else {
                    MappingQuality::Exact
                },
                confidence: if deprecated { 0.6 } else { 0.95 },
                evidence: vec![format!("pattern maps to canonical id '{alive_clone}'")],
            };
            if deprecated {
                m.evidence.push(format!(
                    "ontology id '{default}' is deprecated/merged; resolved to '{alive_clone}'"
                ));
            }
            m
        }
        None => OntologyMapping::unmapped(proposed),
    }
}
