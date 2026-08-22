# etdl-reliability-ontology

[![Crates.io](https://img.shields.io/crates/v/etdl-reliability-ontology.svg)](https://crates.io/crates/etdl-reliability-ontology)
[![Docs.rs](https://img.shields.io/docsrs/etdl-reliability-ontology)](https://docs.rs/etdl-reliability-ontology)

**The canonical failure taxonomy for [ETDL](https://github.com/ETDL-lang/etdl)'s Reliability Supplement** — stable identifiers answering *"what is this?"*, deliberately separate from reliability *knowledge* ("how likely is it?") and from *evidence* ("what happened?").

## The three-way split this crate enforces

- **Ontology identity** — `failure.network.timeout` — stable. (**this crate**)
- **Reliability knowledge** — `P = 0.0031` — mutable, versioned. ([`etdl-reliability-core`](https://crates.io/crates/etdl-reliability-core))
- **Observations** — immutable evidence. ([`etdl-reliability`](https://crates.io/crates/etdl-reliability))

A new observation **never** creates a new ontology identifier — it only updates knowledge. Ontology refinement (e.g. splitting `failure.database.timeout` into connection/query/lock timeouts) is versioned and traceable, and a discovery engine ([`etdl-failure-discovery`](https://crates.io/crates/etdl-failure-discovery)) never silently modifies the authoritative ontology.

## What it provides

- **`taxonomy::Taxonomy`** — the canonical failure-mode hierarchy.
- **`mapping::{OntologyMapping, MappingRule, MappingStatus}`** — how a discovered or externally-named failure maps onto a canonical ontology entry, with an explicit lifecycle.
- **`FailureStatus`** — `Candidate → Reviewed → Accepted / Rejected / Merged / Deprecated`. A discovery engine never silently moves an entry to `Accepted`; engineering review is required.

Full reliability-layer architecture: [`docs/architecture/reliability-layers.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/architecture/reliability-layers.md).

## License

Apache-2.0
