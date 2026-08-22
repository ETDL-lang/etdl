# etdl-reliability-core

[![Crates.io](https://img.shields.io/crates/v/etdl-reliability-core.svg)](https://crates.io/crates/etdl-reliability-core)
[![Docs.rs](https://img.shields.io/docsrs/etdl-reliability-core)](https://docs.rs/etdl-reliability-core)

**The built-in layer for [ETDL](https://github.com/ETDL-lang/etdl)'s Reliability Supplement** (`etdl.reliability`) — the small, deterministic subset the ETDL compiler depends on directly to understand and compile a reliability reference. WASM-compatible and dependency-light on purpose, so a normal ETDL install never needs more than this.

## What it provides

- **`probability`** — metrics (probability / failure rate / ...), time bases, sources.
- **`estimate`** — probability estimates, an explicit `unknown` state (never silently coerced to zero).
- **`provenance`** — where an estimate came from.
- **`uncertainty`, `distribution`** — representation (not statistical computation) of uncertainty.
- **`artifact`** — versioned `.rprob` artifacts, deterministic resolution, the provider seam (`artifact::ProbabilityProvider`).
- **`validation`** — structural and semantic validation of estimates/artifacts.

Deliberately **no statistical algorithms**, no numerical simulation, no network clients, no filesystem access, and no runtime service coupling — that richer engineering lives one layer up, in optional crates this one never depends on.

## Where this sits

The invariant maintained across the whole reliability ecosystem:

- **Ontology** — what is this? (canonical IDs — [`etdl-reliability-ontology`](https://crates.io/crates/etdl-reliability-ontology))
- **Evidence** — what happened? (observations — [`etdl-reliability`](https://crates.io/crates/etdl-reliability))
- **Reliability model** — how likely is it? (estimates — **this crate**)
- **Analysis** — what does the model imply? (optional — [`etdl-reliability`](https://crates.io/crates/etdl-reliability))
- **ETDL** — how does the system behave? ([`etdl-compiler`](https://crates.io/crates/etdl-compiler))

The richer reliability engineering (Bayesian/empirical estimation, distribution algorithms, Monte Carlo, evidence/observations, ontology, source-code failure discovery) lives in separate, optional crates:

- [`etdl-reliability`](https://crates.io/crates/etdl-reliability) — richer domain model + analysis
- [`etdl-reliability-ontology`](https://crates.io/crates/etdl-reliability-ontology) — canonical reliability concepts
- [`etdl-failure-discovery`](https://crates.io/crates/etdl-failure-discovery) — source analysis producing failure candidates

[`etdl-compiler`](https://crates.io/crates/etdl-compiler)'s `reliability` feature (default-on) depends only on this crate — never on the richer, optional ones — so a user who only wants basic ETDL compilation never pulls in statistical/analysis tooling they didn't ask for.

Full reliability-layer architecture: [`docs/architecture/reliability-layers.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/reliability-layers.md) and [`docs/reliability/README.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/reliability/README.md).

## License

Apache-2.0
