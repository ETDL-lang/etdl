# ETDL Reliability Layers

This document explains the three reliability layers and the two lifecycle
dimensions (compile-time vs analysis-time). The central property:

> **Advanced reliability engineering must be optional, but basic reliability
> compilation must be a first-class ETDL capability.**

## The three layers

| Layer | What it is | Where it lives | Required? |
|---|---|---|---|
| **Built-in** | Deterministic understanding of a reliability *reference*: parse the `x-reliability` extension, validate basic references, read a versioned artifact, resolve a deterministic probability, pass it into the fault-tree evaluator, preserve provenance, generate code | `etdl-reliability-core` + `etdl-compiler` | Yes, when the Reliability Supplement is used; lightweight |
| **Optional reliability library** | Richer reliability-domain functionality: distributions algorithms, uncertainty, evidence/observations, failure modes, statistical estimation (empirical, Wilson, Beta-Binomial, exponential) | `etdl-reliability` | No |
| **Optional analysis / discovery** | Heavy functionality: advanced statistical analysis, Bayesian, Monte Carlo, source-code failure discovery, ontology analysis, external data integration | `etdl-reliability` (future analysis crates), `etdl-failure-discovery`, `etdl-reliability-ontology` | No |

## Two dimensions, kept separate

- **Compile-time**: resolving an external deterministic value into the build.
  This is what `etdl compile` does. No statistics, no simulation, no scanning.
- **Analysis-time**: producing the value in the first place (from observations,
  evidence, models). This happens in optional libraries *before* compilation.

```
Observations / Data / Models
         |
         v  (optional analysis, e.g. etdl-reliability)
ReliabilityArtifact (.rprob)     <-- file-first interoperability boundary
         |
         v  (built-in compile-time resolution)
Deterministic ETDL compilation
         |
         v
Generated Rust with a constant
```

Compilation never invokes Monte Carlo, Bayesian inference, source scanning, or
external statistical analysis. If an analysis mode is explicitly requested
(separate, optional tooling), that is different.

## Built-in layer — exact scope

`etdl-reliability-core` contains only what the compiler needs:

- parse/retain reliability extension data (`x-reliability`);
- validate basic reliability references (supplement ids/versions, artifact
  schema, ranges);
- read a versioned reliability artifact (`.rprob` / `.rjson` / data-only
  `.etdl`);
- resolve a deterministic probability (literal or external reference);
- pass that probability into existing fault/event-tree processing;
- preserve build provenance (build manifest);
- generate deterministic code.

It does **not** contain: statistical engines, numerical simulation,
source-code parsers, machine learning, network clients, databases, or
filesystem access. It is WASM-compatible.

## Optional reliability crate — `etdl-reliability`

The richer domain library. It re-exports the built-in types and adds:

- `analysis` — empirical / Wilson, Beta-Binomial (Bayesian), exponential
  failure model, sensitivity / importance;
- `evidence`, `observation` — immutable runtime observations;
- `failure`, `dependency` — failure modes and dependencies.

It may grow: Monte Carlo, uncertainty propagation, reliability growth models,
advanced Bayesian inference, external data integration — always behind the
same crate boundary. The compiler does not depend on it.

## Provider seam

The built-in layer defines `etdl_reliability_core::artifact::ProbabilityProvider`
with `LiteralProvider` (declared values) and, for the build path, a
file-based reader for `.rprob` artifacts. Future optional libraries may
implement `DatabaseProvider`, `GeneratedProvider`, `PythonProvider`, etc.
**Only compiled-in providers exist** — no runtime plugin downloading, no remote
execution. A provider is data + deterministic resolution; an artifact is DATA
and is never executed.

## Extension registry: registered / available / enabled

- **Registered**: an extension is present in `ExtensionRegistry` (e.g.
  `etdl.reliability`).
- **Available**: the implementation is compiled into the binary (feature
  enabled). `etdl-compiler::extension::builtin_registry()` registers the
  reliability extension only when the `reliability` feature is on.
- **Enabled**: the document declares the supplement *and* it is available.

When a document declares `etdl.reliability` but the binary was built without
the feature, the compiler emits a clear diagnostic (E-108 for required, W-407
for optional) rather than silently ignoring it. `etdl capabilities` reports
availability explicitly.

## Build manifest

`etdl-build-manifest.json` records:

- ETDL document version and compiler version;
- enabled features;
- implementation versions (which reliability implementation produced this
  build);
- artifact schema version;
- supplements used;
- reliability artifact files;
- each resolved probability with provenance.

This answers "which reliability implementation produced this binary?" for a
given build.

## Future supplements

The extension registry and `CompilerConfig` are supplement-agnostic. A future
supplement (another domain using tree-event modeling) registers through the
same `EtdlExtension` trait and `ExtensionRegistry::with_builtins()`, without
modifying ETDL core fault-tree mathematics.
