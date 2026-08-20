# ETDL Documentation

Welcome to the ETDL (Event Tree Definition Language) documentation.

ETDL is a declarative, design-time domain-specific language (DSL) for **reliability-aware, event-driven business processes**. It models causal sequences as **event trees** (IEC 62502) and failure probability as **fault trees** (IEC 61025), resolved against **AsyncAPI 3.0** contracts — then compiles the whole model to Rust code.

## Getting started

- [Getting Started](getting-started.md) — install, write your first `.etdl` document, compile, run
- [Quick Start example](examples/order-fulfillment.md) — annotated walkthrough of the spec's worked example
- [Developer Guide](developer/README.md) — "how do I use ETDL?"

## Concepts

- [Event Trees](concepts/event-trees.md) — barriers, operations, consequences (IEC 62502)
- [Fault Trees](concepts/fault-trees.md) — gates, basic events, probability math (IEC 61025)
- [ECEL](ECEL.md) — the Event-tree Condition Expression Language
- [Probability Linking](concepts/probability-linking.md) — connecting operations to fault trees
- [Probability Semantics](PROBABILITY_SEMANTICS.md) — exact formulas and numerical rules
- [Fault Tree Analysis](FAULT_TREE_ANALYSIS.md) — engine + worked examples
- [Event Tree Analysis](EVENT_TREE_ANALYSIS.md) — engine + validation rules
- [AsyncAPI Integration](ASYNCAPI_INTEGRATION.md) — imports, resolution, security

## Reference

- [CLI](CLI.md) — commands, exit codes, `--json`
- [Crates](reference/crates.md) — the five crates
- [API Stability](API_STABILITY.md) — public vs internal API, compatibility
- [Diagnostics](DIAGNOSTICS.md) — every code, severity, suggestion
- [Runtime](RUNTIME.md) — guarantees and configuration
- [Conformance](CONFORMANCE.md) — what conformance means + the suite

## Deep dives

- [Architecture](architecture.md) — compiler pipeline and the codegen contract
- [Readiness Audit](READINESS_AUDIT.md) — P0–P3 inventory
- [Current Readiness Audit](CURRENT_READINESS_AUDIT.md) — fresh evidence-based audit (post-0.2.0)
- [Readiness Backlog](READINESS_BACKLOG.md) — prioritized P0–P3 findings
- [Readiness Scorecard](READINESS_SCORECARD.md) — current scores with evidence
- [Spec × Implementation Matrix](SPEC_IMPLEMENTATION_MATRIX.md) — normative mapping
- [Specification](https://github.com/usamassem/etdl-specification) — the formal spec (v1.0.0, CC BY 4.0)

## Getting help

- Open an issue on [github.com/usamassem/etdl](https://github.com/usamassem/etdl)
- Chat about the specification at [github.com/usamassem/etdl-specification](https://github.com/usamassem/etdl-specification)
- Report a vulnerability per [SECURITY.md](../SECURITY.md)
