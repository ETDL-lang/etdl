# ETDL Reliability Engineering

This document describes the reliability-engineering layer of the ETDL ecosystem:
the `etdl-reliability`, `etdl-reliability-ontology`, and `etdl-failure-discovery`
crates, their integration into the compiler, and the runtime evidence path.

## Concepts (distinguished precisely)

The reliability layer keeps these concepts separate (never collapsed):

| Concept | Meaning | Lifecycle |
|---|---|---|
| **Ontology** | *What is this?* — stable canonical failure-mode ids (`failure.network.timeout`) | Versioned; identity is stable |
| **Evidence** | *What happened?* — immutable observations | Append-only |
| **Probability estimate** | *How likely is it?* — a value with metric, population, provenance | Mutable, versioned |
| **Uncertainty** | *How well is that value known?* — an interval or distribution over the value | Travels with the estimate |
| **Sensitivity** | *If the input moved, how much would the answer move?* | Analysis output |
| **Importance** | *How much does the top event depend on this entity?* | Analysis output |
| **Probability model** | The statistical model that produces estimates from evidence | Versioned |
| **Probability artifact** | An external `.rprob`/`.etdl` file carrying estimates | Versioned |
| **Prediction** | A forward-looking estimate | Versioned |
| **Assumption** | A stated prior/condition | Reviewed |

Uncertainty, sensitivity and importance are three different numbers answering
three different questions, and no code path converts one into another. See
[`uncertainty-importance-sensitivity.md`](uncertainty-importance-sensitivity.md)
for the formulas, assumptions and limitations of every metric, and
[`dependency-analysis.md`](dependency-analysis.md) for the dependency and
common-cause model they are computed over.

Most importantly: **uncertainty is not probability.** `P = 1.6e-2` is a claim
about the system; `[1.3e-2, 1.9e-2]` is a claim about our knowledge of it.

A probability is **not** identified by its numeric value; it is identified by
(what event, under what conditions, for what population, using what
evidence/model). `failure.database.timeout` is the identity; `P = 0.0021` is
current knowledge about it.

## Architecture

```
ETDL Core (parser/compiler/runtime)
   |
   +-- etdl-reliability-core     BUILT-IN deterministic layer (compiler's only
   |      |-- estimate             reliability dependency; WASM-safe, no fs)
   |      |-- probability          metrics (probability/failureRate/...),
   |      |                          time basis (per-request/per-mission/...)
   |      |-- uncertainty          confidence/credible intervals, bounds
   |      |-- distribution         generic distribution representation
   |      |-- provenance           where an estimate came from
   |      |-- artifact             versioned .rprob artifacts + ArtifactResolver
   |      +-- validation           structural/semantic validation
   |
   +-- etdl-reliability           OPTIONAL richer library
   |      |-- analysis             empirical/Wilson, Beta-Binomial, exponential,
   |      |                          sensitivity/importance
   |      |-- observation          immutable runtime observations (evidence)
   |      |-- evidence             aggregated evidence
   |      |-- observations         AggregateObservation (counted evidence)
   |      |-- dataset               versioned, immutable ObservationDataset +
   |      |                          compatibility-checked aggregation
   |      |-- calibration          predicted vs. observed, drift reporting
   |      |-- failure/dependency   failure modes, common-cause
   |      +-- (re-exports core)
   |
   +-- etdl-reliability-ontology  canonical taxonomy + versioning + mappings
   |
   +-- etdl-failure-discovery     source-code discovery -> candidate failure modes

etdl-core (runtime)
   +-- observation               lightweight ReliabilityObservation + sinks
```

Dependencies are one-directional:

```
etdl-reliability-core
        ^   ^
        |   |
etdl-reliability   (optional rich layer builds on the built-in core)
etdl-compiler      (built-in: depends ONLY on etdl-reliability-core)

etdl-reliability-ontology
        ^
        |
etdl-failure-discovery

etdl-cli  ->  etdl-compiler + etdl-reliability-core
              (+ optional etdl-failure-discovery, etdl-reliability-ontology via the
               `discovery` feature)
```

`etdl-core` does **not** depend on the reliability crates; it only collects
lightweight observations. The compiler never depends on the rich
`etdl-reliability` crate.

## Two paths (never mixed)

1. **Deterministic compilation**: `ETDL -> resolve (artifacts) -> fault-tree ->
   scalar -> generated code`. No statistics at build time beyond the existing
   fault-tree evaluator; no runtime service.
2. **Analysis**: `ETDL -> reliability model -> distributions -> estimation/
   sensitivity -> report`. Consumes the model; never mutates source.

Analysis never silently rewrites `probability: 0.01` in a user's document; it
produces a **new versioned artifact** the engineer must explicitly accept.

## Dependency-aware analysis

Dependent events, common-cause failures (CCF), conditional probabilities,
uncertainty propagation, importance, and sensitivity are handled by an
optional, explicit dependency model — the classic independent fault-tree
mathematics are preserved for ordinary documents. See
[dependency-analysis.md](dependency-analysis.md).

```bash
etdl analyze service.etdl --dependencies deps.yaml --monte-carlo 20000
```

## Runtime feedback and calibration

A compiled, deployed service can be compared against the reliability
artifact that predicted its behavior — without that comparison ever
mutating the artifact automatically. See
[runtime-feedback-calibration.md](runtime-feedback-calibration.md) for the
full pipeline (runtime observations → observation dataset → predicted vs.
observed → calibration status) and
[../../examples/reliability-runtime-feedback](../../examples/reliability-runtime-feedback)
for a worked example.

```bash
etdl reliability calibrate gw.rprob failure.gateway.timeout \
  --dataset prod-week-1.yaml --dataset prod-week-2.yaml
```

## Evidence → estimate → artifact

The full engineering workflow (discovery → review → observation → estimation →
artifact → compilation), including how every numerical value is traced back to
its origin and method, is documented in
[evidence-to-estimate.md](evidence-to-estimate.md).

Key commands:

```bash
etdl reliability estimate observations.yaml --method empirical --output gw.rprob
etdl reliability trace gw.rprob failure.gateway.timeout
```

## Probability vs failure rate vs frequency vs availability

These are distinct `ProbabilityMetric` values and are **not** implicitly
converted. A failure rate `λ` is per unit time; a probability-per-request is a
probability over that population. The supplement's conversion
`P = 1 − e^(−λt)` (exponential model) is the only cross-metric conversion, and
only where the metric is a failure rate.

## Unknown probability

`unknown` is an explicit state. It is **never** translated to `0`. The compiler
policy is configurable and governs only *unknown-valued* estimates (an estimate
that exists in the artifact but carries no value):

- `error` — fail the build (safety-critical)
- `warning` — warn and fall back to the document's declared probability (W-408)
- `allow` — silently treat the estimate as unresolved

Default: `warning`. A **missing** estimate id (the document references an
estimate the artifact does not contain) is always an error regardless of
policy — that is a reference error, not a data-quality question. An unknown
value is never turned into a scalar under any policy.

## Artifact validation

Artifacts are validated on load (build path) and on demand (CLI):

- schema version must match `etdl.reliability.artifact/1.0`;
- id and version are required (version is needed for provenance);
- duplicate estimate ids are detected;
- values must be finite (NaN/∞ rejected) and in range for the metric;
- rate/time-based metrics (`failureRate`, `frequency`, MTBF/MTTF/MTTR) require
  a `time_basis`;
- `unknown` state must not carry a value;
- uncertainty (confidence/credible intervals, bounds) and distributions are
  validated (finite bounds, parameter counts, shape constraints);
- non-literal provenance must carry at least one identifying attribute
  (`dataset`, `model`, or `model_version`).

Malformed artifacts fail the build with E-110 rather than silently missing
estimates.

## External artifacts and security

A reliability artifact (`.rprob`, `.rjson`, or a data-only `.etdl`) is
**data, not code**. It is never executed. Resolution is deterministic and
offline. The compiler:

- validates the artifact schema version;
- validates probability ranges, metric compatibility, uncertainty, and
  distributions;
- prevents path traversal in artifact paths (`..` is rejected, mirroring the
  AsyncAPI import guard);
- emits a **build manifest** (`etdl-build-manifest.json`) with full provenance:
  ETDL version, compiler version, supplement versions, artifact ids/versions,
  and each resolved probability keyed by `fault_tree :: basic_event`.

The generated service code remains independent of any centralized reliability
service.

## CLI

`etdl reliability` provides three subcommands (built-in with the default
`reliability` feature):

- `etdl reliability resolve <file.etdl>` — resolve external probability sources
  and print each resolved value with provenance (artifact, version, estimate);
- `etdl reliability validate <file.etdl>` — run full document validation and
  validate every referenced reliability artifact;
- `etdl reliability inspect <file.rprob>` — summarize an artifact's structure
  and estimates and report any validation issues.
- `etdl reliability calibrate <artifact> <event> --dataset <ds>...` — compare
  an artifact's prediction against one or more observation datasets and
  report a calibration status (`consistent`, `potential_deviation`,
  `significant_deviation`, `insufficient_data`, `unsupported_comparison`).
  Read-only: writes a report, never modifies the artifact.

`etdl capabilities` reports which capabilities are compiled into the binary
(core, reliability, failure discovery, ontology, registered extensions) —
useful for reproducible builds. All subcommands support `--json` for
machine-readable output.

## Ontology governance

- Ontology entries have lifecycle status: `candidate`, `reviewed`, `accepted`,
  `rejected`, `merged`, `deprecated`.
- A discovery engine never moves a candidate to `accepted`; engineering review
  is required.
- Deprecation/merge is traceable (`replaced_by`) and versioned; cycles are
  detected.
- Ontology identity and reliability knowledge are versioned independently.

## Discovery

`etdl discover <file|dir>` analyzes source for language-independent conceptual
failure indicators (timeouts, retries, external/database/network/storage calls,
serialization, resource allocation, configuration reads, process spawn,
dependency boundaries) and produces **candidates** mapped to the ontology with
confidence and evidence. Candidates establish **possibility**, never
probability; probability estimation is a separate stage backed by
evidence/statistics.

## Compatibility

- Ordinary ETDL documents (no supplements) are unchanged: `probability: 0.008`,
  `failureRate`/`missionTime`, and `onFailureProbabilitySource` behave exactly
  as before.
- The reliability supplement is opt-in via `supplements: [etdl.reliability]`.
- A required-but-unsupported supplement is an error (E-108); an optional
  unsupported supplement warns (W-407).
- Existing fault-tree mathematics are preserved; external resolution feeds the
  existing evaluator with deterministic scalars.

## Security considerations

- Artifacts are never executed.
- Artifact paths reject `..` traversal.
- No network access required for compilation.
- Optional checksum/signing is future work.
- Observations omit sensitive payload data by default.
