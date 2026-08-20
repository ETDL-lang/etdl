# ETDL Reliability Integration

This document describes the **core reliability integration layer** inside the ETDL
compiler: the generic extension mechanism, the probability-provider seam, local
artifact resolution, and how external probabilities feed the existing fault-tree
evaluator without changing ETDL Core semantics.

It is the stable center the future Reliability Supplement, reliability library,
ontology, failure-discovery, and analysis tools plug into. It deliberately does
**not** implement statistics, AI, Monte Carlo, Bayesian inference, or discovery.

## 1. Extension architecture

The compiler has a **generic semantic-extension mechanism** (not reliability-
specific). Any future supplement (safety, security, diagnostics, another
tree-event domain) uses the same seam.

```
                    ETDL
                     |
                     v
              +-------------+
              |  Compiler   |
              +------+------+
                     |
              Extension API (EtdlExtension)
                     |
         +-----------+-----------+
         |           |           |
         v           v           v
   Reliability   Future Tree  Future Domain
    Supplement    Supplement    Supplement
         |
         v
  Probability Provider (ProbabilityProvider)
         |
   +-----+-----+
   |           |
 literal    artifact
         |
         v
  Resolved Probability (with provenance)
         |
         v
     Existing FTA
         |
         v
   Code Generation
```

### `EtdlExtension` trait

Defined in `etdl-compiler/src/extension.rs`:

```rust
pub trait EtdlExtension: Send + Sync {
    fn id(&self) -> &str;          // namespaced, e.g. "etdl.reliability"
    fn version(&self) -> &str;     // semver
    fn validate(&self, doc, context, diagnostics);
    fn process(&self, doc, context, diagnostics) -> Box<dyn ExtensionResult>;
}
```

- `validate` runs after core validation.
- `process` runs before fault-tree evaluation and may resolve external values
  (e.g. probabilities), returning a typed `ExtensionResult`.

### `ExtensionRegistry`

```rust
pub struct ExtensionRegistry { /* ... */ }
// register / lookup / contains / list — deterministic (BTreeMap, sorted list)
```

- `builtin_registry()` returns the built-in extensions (currently the
  Reliability Extension when the `reliability` feature is enabled).
- Supplements declared in a document are validated against the registry:
  required-but-unsupported → E-108; optional-but-unsupported → W-407.
- No dynamic loading; extensions are compiled in.

## 2. Reliability extension

`etdl-compiler/src/reliability.rs` defines `ReliabilityExtension`, which
implements `EtdlExtension` and drives:

1. reading the document's `x-reliability` extension block (sources + unknown
   policy),
2. loading local reliability artifacts,
3. resolving each external-sourced basic event to a deterministic scalar,
4. producing a build manifest (provenance).

The parser does **not** contain reliability business logic — it only preserves
`x-*` fields generically; the reliability extension interprets them.

## 3. Provider interface

`etdl-reliability-core/src/artifact.rs` defines the provider seam:

```rust
pub trait ProbabilityProvider {
    fn resolve(
        &self,
        source: &ProbabilitySource,
        estimate: &ProbabilityEstimate,
    ) -> Result<f64, ReliabilityError>;
}
```

Initial providers:

- `LiteralProvider` — returns the estimate's declared scalar.
- `ArtifactResolver` — resolves a named estimate from a loaded artifact, with
  unknown-probability policy.

`MockProbabilityProvider` exists for tests, proving the compiler depends on the
trait, not on any concrete artifact format. Future providers (database, HTTP,
external calculation, organization-specific) can implement the same trait
without changing ETDL Core. Only compiled-in providers exist — no runtime
plugin downloading, no remote execution.

## 4. Resolution lifecycle

```
parse
  -> core validation
  -> extension discovery (supplement declarations -> registry)
  -> extension validation
  -> extension semantic processing (resolve external probabilities)
  -> core compilation (fault-tree evaluation uses resolved scalars)
  -> code generation
```

`Compiler::run_extensions` runs registered extensions' `process` step and feeds
the aggregated basic-event probability overrides into the existing
`fault_tree::resolve_fault_trees_with_overrides`. The **existing FTA evaluator
is authoritative** — gate mathematics are untouched; an externally supplied
probability arrives exactly as if it had been written in the document.

## 5. Probability representation

The built-in `etdl-reliability-core` provides:

- `Probability` semantics enforced via `ProbabilityEstimate::resolved_probability`
  (range `[0,1]`, metric must be probability-like, unknown is explicit).
- `ProbabilityState`: `declared, assumed, measured, estimated, predicted,
  inferred, imported, unknown`. `unknown` is never `0`.
- `ProbabilityMetric`: `probability, failureRate, eventFrequency, availability,
  MTBF, MTTF, MTTR`. Only `probability` is directly resolvable by the compiler;
  other metrics produce a clear unsupported-metric diagnostic.
- `ResolvedProbability { value, estimate_id, artifact_id, artifact_version }` —
  resolution returns value + provenance, never a bare `f64`.

## 6. Artifact interface

A reliability artifact is a versioned, non-executable file (JSON or YAML,
`.rprob`/`.json`/`.yaml`/`.etdl` data-only documents) with a schema identifier
`etdl.reliability.artifact/1.0`. It is **data, not code**; it is never executed.

The compiler isolates artifact parsing behind `ReliabilityArtifact::from_json /
from_yaml` and `ArtifactResolver`; the rest of the compiler depends on
`ResolvedProbability`, not on JSON/YAML fields. This keeps the artifact format
replaceable as the Reliability Supplement finalizes.

## 7. Configuration

Reliability artifact paths and the unknown-probability policy are read from the
document's `x-reliability` block:

```yaml
x-reliability:
  sources:
    - id: gw
      type: external
      file: "./reliability/gateway.rprob"
  unknownPolicy: "error"   # error | warning | allow (default warning)
```

Relative artifact paths resolve against the `.etdl` file's directory. Path
traversal (`..`) is rejected.

## 8. Compatibility

- Ordinary ETDL documents (no supplements) are unchanged: `probability: 0.008`,
  `failureRate`/`missionTime`, and `onFailureProbabilitySource` behave exactly
  as before.
- The `reliability` Cargo feature (default on) gates the reliability extension
  and its `etdl-reliability-core` dependency (the built-in layer). With
  `--no-default-features`, the compiler builds without it; the base stays
  lightweight. The rich `etdl-reliability` crate is never a compiler dependency.
- The worked example's top-event probability remains `0.012987` (golden test).

## 9. Security

- Artifacts are never executed.
- Artifact paths reject `..` traversal (mirrors the AsyncAPI import guard);
  a traversal attempt is an E-110 error, not a silent join.
- No network access required for resolution.
- Malformed artifacts, unknown schema versions, non-finite/out-of-range values,
  and unresolved required sources produce clear diagnostics (E-110/E-111/E-112
  and the reliability error variants).
- Artifacts are validated on load: id/version required, duplicate estimate ids
  detected, metric/time-basis rules enforced, uncertainty and distributions
  validated.

## 10. CLI

- `etdl reliability resolve <file>` — resolves external probabilities and prints
  value + provenance (artifact, version, estimate). `--json` for machine output.
- `etdl reliability validate <file>` — full document validation plus validation
  of every referenced reliability artifact. `--json` for machine output.
- `etdl reliability inspect <file.rprob>` — summarizes an artifact's structure,
  estimates, and any validation issues. `--json` for machine output.
- `etdl capabilities` — reports compiled-in capabilities and registered
  extensions (`--json` for machine output).
- `etdl compile` writes `etdl-build-manifest.json` when reliability is in use.

## 11. Example

See `examples/reliability-external/`:

- `service.etdl` declares the reliability supplement and a basic event whose
  probability comes from `./reliability/gateway.rprob`.
- `reliability/gateway.rprob` holds the estimate (value `0.003`).

```bash
etdl reliability resolve examples/reliability-external/service.etdl
etdl analyze examples/reliability-external/service.etdl
```

The top-event probability resolves to `0.003` through the existing fault-tree
evaluator, deterministically.

## 12. Deferred work (intentionally NOT in this layer)

- Monte Carlo, Bayesian inference, MLE, confidence intervals, uncertainty
  propagation, sensitivity/importance, FMEA — the reliability-analysis crate.
- Source-code failure discovery — `etdl-failure-discovery`.
- Canonical ontology storage — `etdl-reliability-ontology`.
- External/executable probability providers (database, HTTP, Python/R) — future
  provider implementations behind the same `ProbabilityProvider` trait.
