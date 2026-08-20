//! Dependency and common-cause relationships.

use serde::{Deserialize, Serialize};

/// Kind of dependency between two elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyKind {
    Dependency,
    Causal,
    Statistical,
    CommonCause,
}

/// A directed dependency relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
}

/// A single common cause producing multiple failures. Common-cause
/// relationships invalidate naive independence assumptions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommonCause {
    pub id: String,
    pub cause: FailureModeIdOrName,
    pub failures: Vec<String>,
}

pub type FailureModeIdOrName = String;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_cause_requires_multiple_failures() {
        let cc = CommonCause {
            id: "ccf.region-outage".into(),
            cause: "RegionOutage".into(),
            failures: vec![
                "failure.database.unavailable".into(),
                "failure.network.unreachable".into(),
            ],
        };
        assert_eq!(cc.failures.len(), 2);
    }
}
