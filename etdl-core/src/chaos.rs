use std::collections::HashSet;

pub struct ChaosController {
    enabled: bool,
    seed: Option<u64>,
    scope: Option<HashSet<String>>,
    production_detected: bool,
    counter: u64,
}

impl ChaosController {
    pub fn new() -> Self {
        let production = ChaosController::detect_production();

        ChaosController {
            enabled: !production && ChaosController::env_chaos_enabled(),
            seed: ChaosController::env_chaos_seed(),
            scope: ChaosController::env_chaos_scope(),
            production_detected: production,
            counter: 0,
        }
    }

    pub fn should_inject_chaos(&mut self, node_id: &str) -> bool {
        if !self.enabled || self.production_detected {
            return false;
        }

        if let Some(ref scope) = self.scope {
            if !scope.contains(node_id) && !scope.contains("*") {
                return false;
            }
        }

        self.counter += 1;

        if let Some(seed) = self.seed {
            let hash = simple_hash(seed, self.counter, node_id);
            hash.is_multiple_of(2)
        } else {
            self.counter.is_multiple_of(2)
        }
    }

    fn detect_production() -> bool {
        // Probe a set of conventionally-set environment variables. The first
        // non-empty one wins (ETDL_ENV is authoritative).
        let env = std::env::var("ETDL_ENV")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| std::env::var("DEPLOYMENT_ENVIRONMENT").ok())
            .or_else(|| std::env::var("ENVIRONMENT").ok())
            .or_else(|| std::env::var("ENV").ok())
            .unwrap_or_default();

        is_production_value(&env)
    }

    fn env_chaos_enabled() -> bool {
        std::env::var("ETDL_CHAOS")
            .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
            .unwrap_or(false)
    }

    fn env_chaos_seed() -> Option<u64> {
        std::env::var("ETDL_CHAOS_SEED")
            .ok()
            .and_then(|v| v.parse().ok())
    }

    fn env_chaos_scope() -> Option<HashSet<String>> {
        std::env::var("ETDL_CHAOS_SCOPE")
            .ok()
            .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
    }
}

fn simple_hash(seed: u64, counter: u64, node_id: &str) -> u64 {
    let mut hash = seed.wrapping_mul(0x9E3779B9);
    hash = hash.wrapping_add(counter);
    for byte in node_id.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    hash
}

/// Robust production-environment detection. Normalizes the value (lowercase,
/// non-alphanumeric collapsed to `-`) and returns true when it is an exact
/// production token or begins with one followed by a separator or suffix, e.g.
/// `production`, `prod`, `prd`, `live`, `production-us-east`,
/// `prod_eu_1`, `live-eu-west`.
fn is_production_value(raw: &str) -> bool {
    let normalized: String = raw
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    let tokens = ["production", "prod", "prd", "live"];
    tokens.iter().any(|t| {
        normalized == *t
            || normalized.starts_with(&format!("{}-", t))
            || normalized.starts_with(&t.to_string()) && {
                // "prd2", "prod1" — a token immediately followed by digits
                let rest = normalized.trim_start_matches(t);
                !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
            }
            || normalized.ends_with(&format!("-{}", t))
            || normalized.contains(&format!("-{}-", t))
    })
}

impl Default for ChaosController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chaos_disabled_in_production() {
        std::env::set_var("ETDL_ENV", "production");
        std::env::set_var("ETDL_CHAOS", "true");

        let mut controller = ChaosController::new();
        assert!(!controller.should_inject_chaos("test_node"));

        std::env::remove_var("ETDL_ENV");
        std::env::remove_var("ETDL_CHAOS");
    }

    #[test]
    fn test_chaos_scope_filtering() {
        std::env::set_var("ETDL_CHAOS", "true");
        std::env::set_var("ETDL_CHAOS_SCOPE", "node_a,node_b");

        let controller = ChaosController::new();

        assert!(controller.scope.as_ref().unwrap().contains("node_a"));
        assert!(!controller.scope.as_ref().unwrap().contains("node_c"));

        std::env::remove_var("ETDL_CHAOS");
        std::env::remove_var("ETDL_CHAOS_SCOPE");
    }

    #[test]
    fn test_chaos_deterministic_with_seed() {
        std::env::set_var("ETDL_CHAOS", "true");
        std::env::set_var("ETDL_CHAOS_SEED", "42");

        let mut c1 = ChaosController::new();
        let mut c2 = ChaosController::new();

        let r1 = c1.should_inject_chaos("test");
        let r2 = c2.should_inject_chaos("test");
        assert_eq!(r1, r2);

        std::env::remove_var("ETDL_CHAOS");
        std::env::remove_var("ETDL_CHAOS_SEED");
    }

    #[test]
    fn production_detection_handles_suffixes() {
        for v in [
            "production",
            "prod",
            "prd",
            "live",
            "production-us-east",
            "prod_eu_1",
            "live-eu-west",
            "Production_1",
            "PRD2",
        ] {
            assert!(is_production_value(v), "should detect '{}'", v);
        }
        for v in ["development", "staging", "test", "dev", "qa", "local", ""] {
            assert!(!is_production_value(v), "should NOT detect '{}'", v);
        }
    }

    #[test]
    fn chaos_disabled_for_qualified_prod_env() {
        std::env::set_var("ETDL_ENV", "production-us-east-1");
        std::env::set_var("ETDL_CHAOS", "true");
        let mut controller = ChaosController::new();
        assert!(
            !controller.should_inject_chaos("n"),
            "chaos must not activate in a qualified production env"
        );
        std::env::remove_var("ETDL_ENV");
        std::env::remove_var("ETDL_CHAOS");
    }
}
