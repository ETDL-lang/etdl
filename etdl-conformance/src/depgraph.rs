//! The workspace dependency graph as ground truth, not convention.
//!
//! Every architectural boundary this workspace has documented across its
//! reference docs — "`etdl-probability-core` has zero dependency on any
//! reliability crate," "`etdl-tree-core` must NOT require Reliability,"
//! "the compiler does not depend on `etdl-reliability`" — is a claim about
//! `Cargo.toml`, not about intent. This module makes that claim checkable:
//! it shells out to `cargo metadata` (already installed; no new
//! dependency) and parses the **normal** (non-dev, non-build) dependency
//! edges only, since a dev-dependency (e.g. `etdl-failure-discovery`'s
//! test-only use of `etdl-compiler`/`etdl-reliability`) is not a runtime
//! architectural coupling.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

#[derive(Debug, Clone, thiserror::Error)]
pub enum DepGraphError {
    #[error("failed to run `cargo metadata`: {0}")]
    Spawn(String),
    #[error("`cargo metadata` exited with a non-zero status")]
    NonZeroExit,
    #[error("could not parse `cargo metadata` output: {0}")]
    Parse(String),
}

/// A normal (non-dev, non-build) dependency edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub optional: bool,
}

/// The full normal-dependency edge set for every workspace package,
/// keyed by package name for convenience.
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub edges: Vec<Edge>,
}

impl DependencyGraph {
    /// Runs `cargo metadata --format-version=1 --no-deps` in `manifest_dir`
    /// (the workspace root) and extracts normal dependency edges among
    /// workspace member packages (external crates like `serde` are kept
    /// too, so `depends_on` can also assert "X depends on no external
    /// crate beyond this explicit list" if ever needed, though today's
    /// vectors only check workspace-internal edges).
    pub fn from_cargo_metadata(manifest_dir: &std::path::Path) -> Result<Self, DepGraphError> {
        let output = Command::new("cargo")
            .args(["metadata", "--format-version=1", "--no-deps"])
            .current_dir(manifest_dir)
            .output()
            .map_err(|e| DepGraphError::Spawn(e.to_string()))?;
        if !output.status.success() {
            return Err(DepGraphError::NonZeroExit);
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| DepGraphError::Parse(e.to_string()))?;

        let packages = json
            .get("packages")
            .and_then(|p| p.as_array())
            .ok_or_else(|| DepGraphError::Parse("missing `packages` array".to_string()))?;

        let mut edges = Vec::new();
        for pkg in packages {
            let from = pkg
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| DepGraphError::Parse("package missing `name`".to_string()))?
                .to_string();
            let deps = pkg
                .get("dependencies")
                .and_then(|d| d.as_array())
                .ok_or_else(|| {
                    DepGraphError::Parse("package missing `dependencies`".to_string())
                })?;
            for dep in deps {
                // `kind` is `null` for a normal dependency, `"dev"` or
                // `"build"` otherwise — only normal edges are an
                // architectural runtime coupling.
                let is_normal = dep.get("kind").map(|k| k.is_null()).unwrap_or(false);
                if !is_normal {
                    continue;
                }
                let to = dep
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| DepGraphError::Parse("dependency missing `name`".to_string()))?
                    .to_string();
                let optional = dep
                    .get("optional")
                    .and_then(|o| o.as_bool())
                    .unwrap_or(false);
                edges.push(Edge {
                    from: from.clone(),
                    to,
                    optional,
                });
            }
        }
        Ok(DependencyGraph { edges })
    }

    pub fn direct_dependencies_of(&self, package: &str) -> BTreeSet<&str> {
        self.edges
            .iter()
            .filter(|e| e.from == package)
            .map(|e| e.to.as_str())
            .collect()
    }

    /// `true` if `package` transitively depends on `target` (workspace
    /// packages only — external crates are leaves with no outgoing edges
    /// in this graph, which is fine: we only ever ask about workspace
    /// members here).
    pub fn transitively_depends_on(&self, package: &str, target: &str) -> bool {
        let mut visited = BTreeSet::new();
        let mut stack = vec![package.to_string()];
        while let Some(current) = stack.pop() {
            if current == target && current != package {
                return true;
            }
            if !visited.insert(current.clone()) {
                continue;
            }
            for dep in self.direct_dependencies_of(&current) {
                stack.push(dep.to_string());
            }
        }
        false
    }

    /// Finds a cycle among workspace-internal packages, if one exists.
    /// External crates are excluded from consideration since a cycle
    /// there would mean `cargo metadata` itself failed to resolve the
    /// workspace, which this function would already have errored on.
    pub fn find_cycle(&self, workspace_packages: &[&str]) -> Option<Vec<String>> {
        let members: BTreeSet<&str> = workspace_packages.iter().copied().collect();
        let mut color: BTreeMap<&str, u8> = BTreeMap::new(); // 0=white,1=gray,2=black
        let mut path: Vec<String> = Vec::new();

        fn visit<'a>(
            node: &'a str,
            graph: &'a DependencyGraph,
            members: &BTreeSet<&'a str>,
            color: &mut BTreeMap<&'a str, u8>,
            path: &mut Vec<String>,
        ) -> Option<Vec<String>> {
            color.insert(node, 1);
            path.push(node.to_string());
            for dep in graph.direct_dependencies_of(node) {
                if !members.contains(dep) {
                    continue;
                }
                match color.get(dep).copied().unwrap_or(0) {
                    0 => {
                        if let Some(cycle) = visit(dep, graph, members, color, path) {
                            return Some(cycle);
                        }
                    }
                    1 => {
                        let start = path.iter().position(|n| n == dep).unwrap_or(0);
                        let mut cycle = path[start..].to_vec();
                        cycle.push(dep.to_string());
                        return Some(cycle);
                    }
                    _ => {}
                }
            }
            path.pop();
            color.insert(node, 2);
            None
        }

        for &pkg in workspace_packages {
            if color.get(pkg).copied().unwrap_or(0) == 0 {
                if let Some(cycle) = visit(pkg, self, &members, &mut color, &mut path) {
                    return Some(cycle);
                }
            }
        }
        None
    }
}
