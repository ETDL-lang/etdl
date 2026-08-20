//! ARCH-* vectors: workspace dependency-graph / architecture invariants.
//!
//! Covers task §25 (dependency graph: cycles, forbidden dependencies,
//! unexpected compiler dependencies) and §51 (supplement dependency
//! declarations: Generic Tree must NOT require Reliability; Predictive
//! Reliability requires Reliability + Probability). These are exactly the
//! invariants the crates' own `Cargo.toml` files and module docs already
//! claim in prose throughout this workspace's history — this file makes
//! them fail CI if a future change silently violates them.

use etdl_conformance::depgraph::DependencyGraph;
use etdl_conformance::vector::{ConformanceVector, Level};

const WORKSPACE_MEMBERS: &[&str] = &[
    "etdl-parser",
    "etdl-compiler",
    "etdl-core",
    "etdl-cli",
    "etdl-wasm",
    "etdl-probability-core",
    "etdl-tree-core",
    "etdl-reliability-core",
    "etdl-reliability",
    "etdl-reliability-ontology",
    "etdl-failure-discovery",
    "etdl-conformance",
];

fn graph() -> DependencyGraph {
    // The workspace root is two levels up from `etdl-conformance/tests/`.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().expect("workspace root");
    DependencyGraph::from_cargo_metadata(workspace_root).expect("cargo metadata succeeds")
}

#[test]
fn arch_001_probability_core_has_zero_reliability_dependency() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ARCH-001",
        Level::StandardLibrary,
        "docs/reference/standard-probability-library.md#where-this-sits",
        "etdl-probability-core must have zero dependency on any reliability crate",
    );
    let g = graph();
    for forbidden in [
        "etdl-reliability",
        "etdl-reliability-core",
        "etdl-reliability-ontology",
    ] {
        assert!(
            !g.transitively_depends_on("etdl-probability-core", forbidden),
            "{}: etdl-probability-core must not depend on {forbidden} ({})",
            VECTOR.id,
            VECTOR.requirement
        );
    }
}

#[test]
fn arch_002_tree_core_has_zero_reliability_or_probability_dependency() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ARCH-002",
        Level::Supplement,
        "docs/reference/generic-tree-event-supplement.md#where-this-sits",
        "Generic Tree Event (etdl-tree-core) must NOT require Reliability or Probability",
    );
    let g = graph();
    for forbidden in [
        "etdl-reliability",
        "etdl-reliability-core",
        "etdl-reliability-ontology",
        "etdl-probability-core",
    ] {
        assert!(
            !g.transitively_depends_on("etdl-tree-core", forbidden),
            "{}: etdl-tree-core must not depend on {forbidden} ({})",
            VECTOR.id,
            VECTOR.requirement
        );
    }
}

#[test]
fn arch_003_reliability_may_depend_on_probability_and_tree_but_not_vice_versa() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ARCH-003",
        Level::Supplement,
        "docs/reference/predictive-reliability-supplement.md#where-this-sits",
        "Predictive Reliability (etdl-reliability) requires Reliability + Probability; \
         the dependency is one-directional",
    );
    let g = graph();
    assert!(
        g.direct_dependencies_of("etdl-reliability")
            .contains("etdl-probability-core"),
        "{}: etdl-reliability must depend on etdl-probability-core",
        VECTOR.id
    );
    assert!(
        g.direct_dependencies_of("etdl-reliability")
            .contains("etdl-tree-core"),
        "{}: etdl-reliability must depend on etdl-tree-core (tree_adapter)",
        VECTOR.id
    );
    assert!(
        !g.transitively_depends_on("etdl-probability-core", "etdl-reliability"),
        "{}: the dependency must not be reversed",
        VECTOR.id
    );
    assert!(
        !g.transitively_depends_on("etdl-tree-core", "etdl-reliability"),
        "{}: the dependency must not be reversed",
        VECTOR.id
    );
}

#[test]
fn arch_004_compiler_reliability_dependency_is_optional() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ARCH-004",
        Level::Semantic,
        "docs/reference/crates.md#etdl-compiler",
        "etdl-compiler's dependency on etdl-reliability-core is optional (behind the \
         `reliability` cargo feature), so a lean build excludes it entirely",
    );
    let g = graph();
    let edge = g
        .edges
        .iter()
        .find(|e| e.from == "etdl-compiler" && e.to == "etdl-reliability-core")
        .unwrap_or_else(|| {
            panic!(
                "{}: expected an etdl-compiler -> etdl-reliability-core edge",
                VECTOR.id
            )
        });
    assert!(
        edge.optional,
        "{}: etdl-compiler -> etdl-reliability-core must be optional",
        VECTOR.id
    );
}

/// **Resolution note (found by this vector, not assumed going in):** an
/// earlier draft of this vector asserted `etdl-wasm` has zero dependency on
/// *any* reliability crate, by analogy with `etdl-probability-core` and
/// `etdl-tree-core`. That assertion was false: `etdl-wasm` depends on
/// `etdl-compiler` with default features, and `etdl-compiler`'s
/// `reliability` feature (which pulls in `etdl-reliability-core`, a pure
/// serde-typed crate with no filesystem/thread/IO code — confirmed
/// WASM-safe by the existing `wasm` CI job) is on by default. Per this
/// task's own §4 ("determine whether implementation is wrong / behavior is
/// intentionally implementation-defined"): `etdl-wasm`'s own source
/// (`etdl-wasm/src/*.rs`) never references reliability at all, so this is
/// not a deliberate feature of the WASM bindings — but `etdl-reliability-
/// core` is lightweight and WASM-safe, and disabling it would silently
/// stop the WASM validator from surfacing E-110/111/112 reliability
/// diagnostics for documents that declare `x-reliability`, which is
/// plausibly desirable for the VS Code extension. Absent an explicit
/// decision either way, this vector asserts the invariant that actually
/// matters for bundle size and architectural intent — no dependency on the
/// *heavy* reliability engine (`etdl-reliability`), the ontology crate, or
/// failure discovery — and documents the `etdl-reliability-core` finding
/// in `docs/reference/crates.md` instead of silently asserting it away.
#[test]
fn arch_005_wasm_has_zero_dependency_on_the_heavy_reliability_engine() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ARCH-005",
        Level::Wasm,
        "docs/reference/crates.md#etdl-wasm",
        "etdl-wasm must have zero dependency on the rich reliability engine, the \
         ontology crate, or failure discovery (etdl-reliability-core alone is a \
         documented, WASM-safe exception — see the resolution note above)",
    );
    let g = graph();
    for forbidden in [
        "etdl-reliability",
        "etdl-reliability-ontology",
        "etdl-failure-discovery",
    ] {
        assert!(
            !g.transitively_depends_on("etdl-wasm", forbidden),
            "{}: etdl-wasm must not depend on {forbidden}",
            VECTOR.id
        );
    }
}

#[test]
fn arch_006_workspace_dependency_graph_has_no_cycles() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ARCH-006",
        Level::Full,
        "docs/architecture.md",
        "the workspace-internal dependency graph must be acyclic",
    );
    let g = graph();
    if let Some(cycle) = g.find_cycle(WORKSPACE_MEMBERS) {
        panic!("{}: cycle detected: {}", VECTOR.id, cycle.join(" -> "));
    }
}

#[test]
fn arch_007_cli_reliability_family_dependencies_are_all_optional() {
    const VECTOR: ConformanceVector = ConformanceVector::new(
        "ARCH-007",
        Level::Full,
        "docs/reference/cli.md",
        "etdl-cli's reliability-family dependencies (etdl-reliability, \
         etdl-reliability-core, etdl-reliability-ontology, etdl-failure-discovery) \
         are all optional, so `--no-default-features` genuinely excludes them",
    );
    let g = graph();
    for dep_name in [
        "etdl-reliability",
        "etdl-reliability-core",
        "etdl-reliability-ontology",
        "etdl-failure-discovery",
    ] {
        let edge = g
            .edges
            .iter()
            .find(|e| e.from == "etdl-cli" && e.to == dep_name)
            .unwrap_or_else(|| panic!("{}: expected an etdl-cli -> {dep_name} edge", VECTOR.id));
        assert!(
            edge.optional,
            "{}: etdl-cli -> {dep_name} must be optional",
            VECTOR.id
        );
    }
}
