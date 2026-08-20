//! Statistical and structural diagnostics emitted by reliability analysis.
//!
//! Diagnostics are how the analysis says "this number exists but here is what
//! you must know about it" — or "this analysis is not supported for this
//! model". They are part of the result artifact and part of the JSON schema:
//! codes are stable, so tooling can gate on them.
//!
//! Diagnostics never replace an error. When an analysis cannot be performed
//! correctly it fails; a diagnostic is not an excuse to return a number
//! computed under assumptions the model contradicts.

use serde::{Deserialize, Serialize};

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// The requested analysis was not performed, or its result must not be
    /// used as reported.
    Error,
    /// The result is usable but a stated caveat materially affects
    /// interpretation.
    Warning,
    /// Contextual information about how the result was produced.
    Info,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// Stable diagnostic codes. Adding a code is a minor change; changing the
/// meaning of an existing code is not permitted.
pub mod code {
    /// A declared dependency structure is not supported by the selected
    /// analysis method, and independence was NOT substituted.
    pub const UNSUPPORTED_DEPENDENCY: &str = "RA001";
    /// A declared uncertainty could not be turned into a sampling law.
    pub const UNSUPPORTED_UNCERTAINTY: &str = "RA002";
    /// Sample count is small enough that quantile estimates are unreliable.
    pub const INSUFFICIENT_SAMPLES: &str = "RA003";
    /// Convergence criteria were not met at the sample count used.
    pub const NOT_CONVERGED: &str = "RA004";
    /// Declared uncertainty bounds are invalid.
    pub const INVALID_UNCERTAINTY_BOUNDS: &str = "RA005";
    /// The baseline value is at or near zero, so a relative (normalised)
    /// measure is undefined and was not reported.
    pub const NEAR_ZERO_BASELINE: &str = "RA006";
    /// Parameter uncertainties were sampled independently while the model
    /// declares dependence between the underlying events.
    pub const CORRELATED_UNCERTAINTY_NOT_MODELLED: &str = "RA007";
    /// The fault tree is non-coherent (contains NOT or XOR), so measures whose
    /// definition assumes coherence were not computed.
    pub const NON_COHERENT_TREE: &str = "RA008";
    /// The top-event probability is zero, making relative importance measures
    /// degenerate.
    pub const ZERO_TOP_EVENT: &str = "RA009";
    /// A distribution sample was clamped to [0, 1].
    pub const SAMPLE_TRUNCATION: &str = "RA010";
    /// A default was applied because the caller supplied none; the value used
    /// is reported.
    pub const DEFAULT_APPLIED: &str = "RA011";
    /// A probability is small enough that reported precision is limited.
    pub const RARE_EVENT_PRECISION: &str = "RA012";
    /// An input declared no uncertainty, so it contributes none to the output
    /// interval. This is a modelling gap, not a finding of certainty.
    pub const NO_DECLARED_UNCERTAINTY: &str = "RA013";
}

/// One diagnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisDiagnostic {
    /// A stable code from [`code`].
    pub code: String,
    pub severity: Severity,
    /// The model entity the diagnostic is about (a basic event, a common
    /// cause, the top event), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// A message written for an engineer, not for a log parser.
    pub message: String,
}

impl AnalysisDiagnostic {
    pub fn new(code: &str, severity: Severity, message: impl Into<String>) -> Self {
        AnalysisDiagnostic {
            code: code.to_string(),
            severity,
            subject: None,
            message: message.into(),
        }
    }

    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Error, message)
    }

    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Warning, message)
    }

    pub fn info(code: &str, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Info, message)
    }

    pub fn about(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// A collection of diagnostics with deterministic ordering.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagnostics {
    #[serde(default)]
    pub items: Vec<AnalysisDiagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Diagnostics::default()
    }

    pub fn push(&mut self, d: AnalysisDiagnostic) {
        self.items.push(d);
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = AnalysisDiagnostic>) {
        self.items.extend(other);
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.is_error())
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Sort by (severity, code, subject) so output is stable across runs.
    pub fn sorted(mut self) -> Self {
        self.items.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then_with(|| a.code.cmp(&b.code))
                .then_with(|| a.subject.cmp(&b.subject))
                .then_with(|| a.message.cmp(&b.message))
        });
        self
    }

    pub fn iter(&self) -> std::slice::Iter<'_, AnalysisDiagnostic> {
        self.items.iter()
    }
}

impl IntoIterator for Diagnostics {
    type Item = AnalysisDiagnostic;
    type IntoIter = std::vec::IntoIter<AnalysisDiagnostic>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_orders_errors_first() {
        let d = Diagnostics {
            items: vec![
                AnalysisDiagnostic::info(code::DEFAULT_APPLIED, "default"),
                AnalysisDiagnostic::error(code::UNSUPPORTED_DEPENDENCY, "unsupported"),
                AnalysisDiagnostic::warning(code::NOT_CONVERGED, "not converged"),
            ],
        }
        .sorted();
        assert_eq!(d.items[0].severity, Severity::Error);
        assert_eq!(d.items[1].severity, Severity::Warning);
        assert_eq!(d.items[2].severity, Severity::Info);
        assert!(d.has_errors());
    }

    #[test]
    fn subject_is_preserved() {
        let d = AnalysisDiagnostic::warning(code::NEAR_ZERO_BASELINE, "baseline near zero")
            .about("GatewayTimeout");
        assert_eq!(d.subject.as_deref(), Some("GatewayTimeout"));
        assert_eq!(d.code, "RA006");
    }
}
