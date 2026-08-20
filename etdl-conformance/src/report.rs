//! Objective per-area conformance status (task §48/§49): what `etdl
//! conformance status` reports.
//!
//! Status here reflects **compiled-in capability**, not "did every test
//! pass in this run" — this crate's own `tests/` directory is the thing
//! that actually exercises the vectors; this module reports what a given
//! binary *claims*, from compile-time feature flags alone, mirroring
//! `etdl-cli`'s existing `capabilities` command exactly (never probing
//! anything at runtime, never duplicating that command's own diagnostic
//! plumbing).

use crate::levels::{AreaStatus, ConformanceStatus};
use crate::vector::Level;

/// Builds the full area-by-area status report for a compiled binary.
/// `reliability_available` is the same `cfg!(feature = "reliability")`
/// boolean `etdl-cli`'s `cmd_capabilities` already computes — passed in
/// rather than recomputed, so this function has no feature-flag logic of
/// its own to drift out of sync.
pub fn area_statuses(reliability_available: bool) -> Vec<AreaStatus> {
    let mut areas = vec![
        AreaStatus {
            area: "core-syntax".to_string(),
            level: Level::Syntax,
            status: ConformanceStatus::Pass,
            detail: "etdl-parser; conformance/conformance.rs (existing suite)".to_string(),
        },
        AreaStatus {
            area: "core-semantic".to_string(),
            level: Level::Semantic,
            status: ConformanceStatus::Pass,
            detail: "etdl-compiler; conformance/conformance.rs (existing suite)".to_string(),
        },
        AreaStatus {
            area: "standard-library".to_string(),
            level: Level::StandardLibrary,
            status: ConformanceStatus::Pass,
            detail: "std.events, std.logic, std.probability; std.units/std.collections deferred (documented)".to_string(),
        },
        AreaStatus {
            area: "supplement.generic-tree-event".to_string(),
            level: Level::Supplement,
            status: ConformanceStatus::Pass,
            detail: "etdl-tree-core; domain-neutral, always available".to_string(),
        },
    ];

    let reliability_status = if reliability_available {
        ConformanceStatus::Pass
    } else {
        ConformanceStatus::Unsupported
    };
    let reliability_detail = if reliability_available {
        "etdl-reliability; compiled in".to_string()
    } else {
        "requires the `reliability` cargo feature".to_string()
    };

    areas.push(AreaStatus {
        area: "supplement.reliability".to_string(),
        level: Level::Supplement,
        status: reliability_status,
        detail: reliability_detail.clone(),
    });
    areas.push(AreaStatus {
        area: "supplement.predictive-reliability".to_string(),
        level: Level::Supplement,
        status: if reliability_available {
            ConformanceStatus::Partial
        } else {
            ConformanceStatus::Unsupported
        },
        detail: if reliability_available {
            "exponential + weibull models tested; no std.reliability ETDL-source facade yet (documented gap)".to_string()
        } else {
            reliability_detail.clone()
        },
    });
    areas.push(AreaStatus {
        area: "supplement.runtime-feedback-calibration".to_string(),
        level: Level::Supplement,
        status: reliability_status,
        detail: reliability_detail.clone(),
    });
    areas.push(AreaStatus {
        area: "artifact".to_string(),
        level: Level::Artifact,
        status: reliability_status,
        detail: reliability_detail.clone(),
    });
    areas.push(AreaStatus {
        area: "runtime".to_string(),
        level: Level::Runtime,
        status: ConformanceStatus::Partial,
        detail: "BranchMonitor/observation emission tested; no dedicated conformance-suite runtime harness yet".to_string(),
    });
    areas.push(AreaStatus {
        area: "wasm".to_string(),
        level: Level::Wasm,
        status: ConformanceStatus::Partial,
        detail: "etdl-wasm builds for wasm32-unknown-unknown and is checked in CI; reliability/predictive/tree-event supplements are native-only by design (etdl-wasm has zero dependency on them)".to_string(),
    });

    areas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_area_has_a_level_and_a_detail() {
        for area in area_statuses(true) {
            assert!(!area.area.is_empty());
            assert!(!area.detail.is_empty());
        }
    }

    #[test]
    fn reliability_areas_are_unsupported_when_the_feature_is_off() {
        let areas = area_statuses(false);
        let reliability = areas
            .iter()
            .find(|a| a.area == "supplement.reliability")
            .unwrap();
        assert_eq!(reliability.status, ConformanceStatus::Unsupported);
    }
}
