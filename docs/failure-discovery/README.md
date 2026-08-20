# ETDL Failure Discovery

Failure discovery analyzes source code and produces **candidate** failure
modes with evidence, source locations, and ontology mapping. It is a
deterministic, local, read-only static-analysis capability.

> **The core semantic rule:** discovery finds *possible* failures. It never
> claims a failure will occur, and it never invents a probability.

```
SOURCE CODE
    ↓
CODE ANALYSIS (deterministic, local, read-only)
    ↓
POSSIBLE FAILURE / EXCEPTION CANDIDATES
    ↓
NORMALIZATION
    ↓
ONTOLOGY MAPPING
    ↓
EVIDENCE + CONFIDENCE
    ↓
DISCOVERY REPORT
    ↓
OPTIONAL RELIABILITY ARTIFACT / ETDL INPUT
```

## Purpose

Answer: **"what failure modes are possible in this code, and where?"**

Discovery is the upstream stage of the reliability pipeline. It is followed by
engineering review, ontology mapping, and — separately — reliability
estimation.

## Scope

- **Supported language:** Rust (a `syn`-based analyzer). No other language has
  a real analyzer; requesting one is an explicit error.
- **Extensibility:** `SourceAnalyzer` trait + `AnalyzerRegistry`. Future
  analyzers (Python, Go, TypeScript, ...) plug in without touching the
  compiler.

## Candidate semantics

A `DiscoveryCandidate` has:

| Field | Meaning |
|---|---|
| `id` | Stable concept identity (`failure.<domain>.<concept>`) |
| `classification` | Coarse class (application, dependency, data, validation, timeout, resource, concurrency, configuration, serialization, io, unknown) |
| `severity` | Engineering severity (info..critical) — NOT probability |
| `location` | File, line, column, end line/column, byte span |
| `context` | Crate, module, function, impl type |
| `evidence` | Structured evidence: kind, source pattern, detail, line text |
| `ontology` | Mapping quality (exact/probable/ambiguous/unmapped/deprecated) |
| `confidence` | Confidence the discovery/classification is correct — **NOT failure probability** |
| `possible` | Always `true` — a candidate is possible, never proven |
| `status` | `candidate` until engineering review |

## Confidence ≠ probability

`confidence = 0.92` means "we are fairly confident this is a genuine potential
failure mechanism at this location." It does **not** mean `P(failure) = 0.92`.
Discovery never emits a probability. A later, separate reliability engineering
process may assign a probability from observations/evidence/model.

## Ontology mapping

Discovery maps candidates into the canonical
`etdl-reliability-ontology` taxonomy:

- `exact` — the concept is a canonical ontology id;
- `probable` — a confident heuristic mapping;
- `ambiguous` — several concepts could match;
- `unmapped` — the candidate proposes a new concept (human approval required);
- `deprecated` — the mapped id is deprecated/merged; resolved to the alive id.

**Discovery never modifies the ontology.** It may *propose* new concepts, but a
human must approve ontology changes.

## CLI

```bash
# Analyze a Rust file, directory, or workspace
etdl discover ./service

# Options
etdl discover ./service --language rust            # only 'rust' is implemented
etdl discover ./service --format text              # text | json | yaml
etdl discover ./service --format json              # stable, versioned schema
etdl discover ./service --min-confidence 0.7       # filter candidates
etdl discover ./service --output failures.json     # write report to file
etdl discover ./service --exclude tests            # exclude paths (repeatable)
etdl discover ./service --ontology-policy auto     # auto | conservative | off
```

Default output is a concise human-readable summary followed by each candidate
with ID, classification, location, evidence, ontology mapping, and confidence.

JSON/YAML output uses the versioned report schema:

```
etdl.failure-discovery.report/1.0
```

This schema is **distinct** from `etdl.reliability.artifact/1.0` — a discovery
report is not a reliability probability artifact.

## Report format

A `DiscoveryReport` contains:

- `schema` — `etdl.failure-discovery.report/1.0`;
- `analyzer` — name, version, language;
- `source` — path, deterministic content hash, file count, package name;
- `config` — the configuration snapshot (no machine-specific paths);
- `candidates` — sorted deterministically by (file, line, column, id);
- `diagnostics` — non-fatal issues;
- `statistics` — totals by classification/severity, high-confidence count,
  mapped/unmapped counts, potential-panic count.

## Determinism

Given the same source, analyzer version, and configuration, discovery produces
identical output. No randomness, no LLM calls, no network, no current time.
Content hashing is a deterministic FNV-1a over sorted file contents.

## WASM

Discovery requires local filesystem access (`std::fs`) and `git` for project
identity, so it is a host-side capability. It is **not** linked into the WASM
build, and the ordinary compiler/WASM path does not depend on it. If discovery
were to run in a browser environment in the future, the file-walking layer
would need an alternative source provider; the analyzer core itself
(`analyze_source`) is pure and environment-independent.

## Security

- Analysis is fully local; no network.
- Analyzed code is never executed; no tests are run; no binaries invoked.
- No source is transmitted anywhere.
- No dynamic plugin downloading — analyzers are compiled in.

## Limitations (false negatives)

The analyzer does **not** detect:

- complex runtime reflection or dynamically generated code;
- semantic business failures with no static trace;
- distributed race conditions (no actual race analysis);
- failures requiring runtime environment knowledge;
- anything in non-Rust languages (no analyzer exists).

## Relationship to reliability estimation

```
DISCOVERY        ESTIMATION             COMPILATION
possible?   →   how likely?  →          resolved value
(failure.    (P(failure) from   →       (ETDL uses the
 network.     observations/               declared artifact
 timeout)     evidence/model)             deterministically)
```

Discovery produces candidates. A separate reliability engineering process
assigns probabilities. ETDL compiles the declared artifact. These three
statements remain separate — discovery confidence is never fed into a fault
tree as a probability.

## Future (Failure Discovery 2.0)

- Additional language analyzers (Python, Go, TypeScript).
- Propagation path analysis (`origin → path → destination`).
- Config-file driven ignore rules and custom pattern tables.
- IDE integration surfacing candidates inline.
