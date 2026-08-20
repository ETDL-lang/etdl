//! Runs the ETDL conformance suite (declarative cases in `conformance/`).
//! Kept separate so the suite can grow without touching compiler internals.

#[path = "../../conformance/conformance.rs"]
mod conformance;

#[test]
fn etdl_conformance_suite() {
    // The suite defines its own #[test]s; this wrapper just ensures the module
    // compiles and is linked.
    let _ = conformance::run_conformance_main();
}
