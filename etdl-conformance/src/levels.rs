//! Objective conformance status — no marketing claims (per this task's own
//! instruction: "Do not invent marketing claims. Instead expose objective
//! states").

use serde::{Deserialize, Serialize};

use crate::vector::Level;

/// The four states a conformance area can be in. `Pass` requires every
/// vector for that area to have run and succeeded; `Failed` means at least
/// one ran and did not; `Unsupported` means the capability is not compiled
/// into this build (e.g. the `reliability` feature is off); `Partial`
/// means some vectors exist and pass but the area's own documentation
/// states known gaps (see the traceability matrix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceStatus {
    Pass,
    Partial,
    Unsupported,
    Failed,
}

impl std::fmt::Display for ConformanceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConformanceStatus::Pass => "PASS",
            ConformanceStatus::Partial => "PARTIAL",
            ConformanceStatus::Unsupported => "UNSUPPORTED",
            ConformanceStatus::Failed => "FAILED",
        };
        write!(f, "{s}")
    }
}

/// One area's conformance status, as reported by `etdl conformance
/// status`. An "area" is coarser than a [`Level`] — e.g. Level 3
/// (Supplement) has one area per supplement, because each supplement can
/// be independently present or absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaStatus {
    pub area: String,
    pub level: Level,
    pub status: ConformanceStatus,
    /// Human-readable detail, e.g. which cargo feature gates this area, or
    /// which known gap makes it `Partial` rather than `Pass`.
    pub detail: String,
}
