# etdl-reliability

[![Crates.io](https://img.shields.io/crates/v/etdl-reliability.svg)](https://crates.io/crates/etdl-reliability)
[![Docs.rs](https://img.shields.io/docsrs/etdl-reliability)](https://docs.rs/etdl-reliability)

**The optional, richer reliability engineering layer for [ETDL](https://github.com/ETDL-lang/etdl)'s Reliability Supplement** (`etdl.reliability`) — statistical estimation, uncertainty propagation, evidence/observations, and sensitivity analysis, built on top of the built-in [`etdl-reliability-core`](https://crates.io/crates/etdl-reliability-core) layer the compiler itself depends on.

## What it provides

- **`analysis`** — statistical estimation: empirical/Wilson interval, Beta-Binomial Bayesian, exponential failure model; sensitivity and importance analysis.
- **`evidence`, `observation`, `observations`** — immutable runtime observations feeding an estimate's provenance.
- **`failure`, `dependency`** — failure-mode and dependency modeling.
- **`calibration`, `predictive`, `review`, `selection`, `trace`, `dataset`, `probability_adapter`, `tree_adapter`** — the supporting machinery for turning raw evidence into a reviewed, versioned reliability artifact.

## Where this sits — the compiler does *not* depend on this crate

[`etdl-compiler`](https://crates.io/crates/etdl-compiler) depends only on `etdl-reliability-core` (the small, deterministic, WASM-compatible built-in layer). This crate is for reliability engineers performing analysis and producing the `.rprob` artifacts the ordinary compiler then consumes — a user who only wants to compile ETDL documents never pulls this crate in.

The invariant maintained across the reliability ecosystem:

- **Ontology** — what is this? (canonical IDs — [`etdl-reliability-ontology`](https://crates.io/crates/etdl-reliability-ontology))
- **Evidence** — what happened? (`observation`, `evidence`, here)
- **Reliability model** — how likely is it? (built-in estimates — `etdl-reliability-core`)
- **Analysis** — what does the model imply? (`analysis`, here)
- **ETDL** — how does the system behave? ([`etdl-compiler`](https://crates.io/crates/etdl-compiler))

Full reliability-layer architecture: [`docs/architecture/reliability-layers.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/reliability-layers.md) and [`docs/reliability/README.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/reliability/README.md).

## License

Apache-2.0
