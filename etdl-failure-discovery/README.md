# etdl-failure-discovery

[![Crates.io](https://img.shields.io/crates/v/etdl-failure-discovery.svg)](https://crates.io/crates/etdl-failure-discovery)
[![Docs.rs](https://img.shields.io/docsrs/etdl-failure-discovery)](https://docs.rs/etdl-failure-discovery)

**Source-code failure discovery for [ETDL](https://github.com/ETDL-lang/etdl)** — answers *"what failure modes are possible?"* by statically analyzing source code, producing **candidate** failure modes with evidence, source locations, and reliability-ontology mapping. Never produces authoritative probabilities; never silently modifies the ontology.

## The pipeline

```
source code
    |
    v
discovery (deterministic, local, read-only)
    |
    v
candidate failure modes
    |
    v
ontology mapping (reviewed, not authoritative)
    |
    v
engineering review -> accepted failure mode -> reliability model
```

## Core semantic distinction

- **Discovered candidate** — static analysis suggests a failure is possible.
- **Estimated failure** — a statistical/reliability model assigns a probability or rate ([`etdl-reliability`](https://crates.io/crates/etdl-reliability)).
- **Observed failure** — something actually happened at runtime.

Discovery confidence is **not** failure probability — a candidate with `confidence = 0.92` is not claiming `P(failure) = 0.92`.

## What it provides

- **`candidate::DiscoveryCandidate`** — a discovered candidate, its classification, severity, and evidence.
- **`location::{SourceLocation, FunctionContext}`** — exactly where in the source the candidate came from.
- **`mapping::MappingQuality`** — how well a candidate maps onto an [`etdl-reliability-ontology`](https://crates.io/crates/etdl-reliability-ontology) entry.
- **`report::DiscoveryReport`** — the machine-readable output [`etdl-cli`](https://crates.io/crates/etdl-cli)'s `etdl discover` command produces, with schema, statistics, and provenance.
- **`config::DiscoveryConfig`** — what to scan and how.

Powers `etdl discover` (the `discovery` feature, default-on in `etdl-cli`). Full reliability-layer architecture: [`docs/failure-discovery/README.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/failure-discovery/README.md).

## License

Apache-2.0
