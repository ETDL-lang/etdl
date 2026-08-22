# ETDL CLI Reference

`etdl` is the command-line interface to the ETDL compiler. Install with
`cargo install etdl-cli`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success (valid, compiled, version) |
| 1 | validation/compile/analysis failure, or I/O error |
| 2 | usage error (unknown flag/command) |

## Global flags

| Flag | Effect |
|---|---|
| `--json` | machine-readable JSON output (where supported) |
| `--quiet` | suppress non-error output (exit code preserved) |
| `--verbose` | extra detail (reserved for future use) |

## Commands

### etdl validate <FILE|DIR>...

Validate one or more documents (files or directories; directories are scanned
recursively for `*.etdl`).

```bash
etdl validate order-fulfillment.etdl
etdl validate --json models/
etdl validate --quiet models/
```

Exit 0 when all documents validate; 1 otherwise. Diagnostics print to stdout.

JSON output shape:

```json
{
  "results": [
    { "file": "order-fulfillment.etdl", "valid": true, "diagnostics": [] }
  ]
}
```

### etdl compile <FILE> [--target <TARGET>] --out-dir <DIR>

Compile a single `.etdl` document to one or more target languages.
`--target` defaults to `rust` and accepts a comma-separated list
(`--target rust,java`) to generate more than one target in a single
invocation. Every non-`rust` target generates a thin binding to
`etdl-runtime-ffi` (the compiled Rust runtime) in that language's own
idiomatic way — none of them reimplement ETDL semantics. See
[Target Architecture](architecture/targets.md) for the full `--target`
mechanism, how each target's binding works, and how future targets
(JavaScript, TypeScript) are expected to be added.

```bash
etdl compile order-fulfillment.etdl --out-dir ./generated               # rust (default)
etdl compile order-fulfillment.etdl --target rust --out-dir ./generated # same as above, explicit
etdl compile order-fulfillment.etdl --target java --out-dir ./generated
etdl compile order-fulfillment.etdl --target python --out-dir ./generated
etdl compile order-fulfillment.etdl --target go --out-dir ./generated
etdl compile order-fulfillment.etdl --target dotnet --out-dir ./generated
etdl compile order-fulfillment.etdl --target rust,java,python,go,dotnet --out-dir ./generated
```

Running anything generated for `java`/`python`/`go`/`dotnet` also needs a
built `etdl-runtime-ffi`:

```bash
cargo build -p etdl-runtime-ffi --release
```

An unrecognized target name fails immediately (before reading the input
file) with an error listing every target actually available in this build:

```
error: unsupported target 'cobol'; available targets: rust, dotnet, go, java, python
```

Diagnostics print to stdout; the success/failure summary prints to stderr.
The `rust` target writes a single `<stem>.rs`; other targets follow their
own ecosystem's output layout (e.g. `java` writes a package/directory tree
under `--out-dir`) — every target's `--help` text and error messages
reflect only what the running binary actually has compiled in. Exit 0 on
success (all requested targets), 1 if any requested target fails to
generate.

### etdl analyze <FILE> [OPTIONS]

Print a reliability summary without generating code: event-tree count,
fault-tree count, and each fault tree's resolved top-event probability.

```bash
etdl analyze order-fulfillment.etdl
etdl analyze --json order-fulfillment.etdl
```

Options:

- `--dependencies <FILE>` — run **dependency-aware analysis** with a declared
  dependency model (YAML/JSON). Enables common-cause (CCF) handling, conditional
  probability validation, importance, and sensitivity. See
  [docs/reliability/dependency-analysis.md](reliability/dependency-analysis.md).
- `--monte-carlo <N>` — run seeded Monte Carlo uncertainty propagation with N
  samples. Must be greater than zero.
- `--seed <N>` — explicit seed for Monte Carlo (default `42`).
- `--uncertainty <FILE>` — declared uncertainty per basic event (YAML/JSON), as
  a map of event id to sampling law. Without this, propagation has nothing to
  propagate and says so rather than reporting a zero-width interval as
  certainty.
- `--level <L>` — central interval level for the propagated result
  (default `0.95`).
- `--perturbation <D>` — absolute perturbation size for sensitivity
  (default `1e-3`).
- `--uncertainty-ranking` — rank inputs by how much of the output variance each
  accounts for. Requires propagation; costs one extra run per uncertain input.
- `--no-importance`, `--no-sensitivity` — skip those analyses.
- `--output <FILE>` — write the analysis-result artifact as JSON.

```bash
etdl analyze service.etdl --dependencies deps.yaml
etdl analyze service.etdl --dependencies deps.yaml --monte-carlo 20000 --seed 7
etdl analyze service.etdl --dependencies deps.yaml \
    --uncertainty uncertainty.yaml --monte-carlo 20000 --seed 7 \
    --uncertainty-ranking --output before.json
etdl analyze service.etdl --dependencies deps.yaml --json
```

Passing `--uncertainty` without `--monte-carlo` applies the documented default
sample count and prints a note; the count used is always reported, never hidden.

Without `--dependencies`, `etdl analyze` uses the classic independence-based
fault-tree mathematics exactly as before. Independence is then recorded in the
result as an explicit assumption rather than left implicit.

A model that declares a conditional probability or a `depends-on` edge is
**refused** with diagnostic `RA001`, because the conditioning evaluator cannot
represent those structures and will not substitute an independence answer for
them.

The uncertainty input file maps each basic-event id to a sampling law:

```yaml
GatewayTimeout:
  law: beta
  alpha: 10000.0
  beta: 990000.0
DatabaseUnavailable:
  law: normal-from-interval
  lower: 0.0035
  upper: 0.0065
  level: 0.95
  meaning: confidence      # confidence | credible | plausible-range
```

Supported laws: `deterministic`, `uniform`, `beta`, `normal`, `lognormal`,
`normal-from-interval`. A one-sided bound is not a distribution and is refused
rather than guessed at. See
[docs/reliability/uncertainty-importance-sensitivity.md](reliability/uncertainty-importance-sensitivity.md).

### etdl reliability compare <BEFORE> <AFTER>

Compare two analysis-result artifacts produced by `etdl analyze --output`, for
example before and after a mitigation.

```bash
etdl reliability compare before.json after.json
etdl reliability compare before.json after.json --json
```

Reports the top-event change, the input changes, the assumption and method
changes, and the importance rank changes. It does **not** attribute the outcome
to any one modification unless exactly one input changed in exactly one way.

### etdl capabilities

Report which capabilities are compiled into this binary (for reproducible
build environments). Emits Core/Reliability/Failure-Discovery/Ontology status,
the individual analysis capabilities (statistical estimation, uncertainty
analysis, Monte Carlo with its sampler identity, importance with its measure
list, sensitivity with its method), and the list of registered extensions.

Capabilities that are not implemented — correlated parameter uncertainty and
conditional probability evaluation — are reported as unsupported in every build.
Unsupported functionality is never reported as available.

```bash
etdl capabilities
etdl capabilities --json
```

Example output:

```
Core: yes
Reliability: built-in
Reliability Analysis: unavailable (optional etdl-reliability library)
Failure Discovery: available
Ontology: available
Registered extensions:
  etdl.reliability @1.0
```

### etdl reliability resolve|validate|inspect|estimate|trace <FILE>

Reliability subcommands (built-in when compiled with the `reliability` feature,
which is the default):

- `etdl reliability resolve <file.etdl>` — resolve external probability sources
  and print each resolved value with provenance.
- `etdl reliability validate <file.etdl>` — validate the document and every
  referenced reliability artifact.
- `etdl reliability inspect <file.rprob>` — summarize an artifact's structure
  and validation issues.
- `etdl reliability estimate <observations.yaml> [OPTIONS]` — estimate
  probabilities from observations and write a reliability artifact.
  Options: `--method empirical|beta-binomial|exponential`,
  `--level 0.95`, `--prior-alpha 1.0`, `--prior-beta 1.0`,
  `--mission-time <t>` (exponential), `--output gw.rprob`.
- `etdl reliability trace <file.rprob> <estimate-id>` — print the backward
  trace (failure mode → ontology → evidence → observations → method →
  artifact) of one estimate.

When the `reliability` feature is not compiled in, these report a clear
"support is not enabled in this build" message and exit non-zero.

### etdl discover <FILE|DIR> [OPTIONS]

Run failure discovery on source code, producing candidate failure modes
mapped to the reliability ontology (compiled in only with the `discovery`
feature; it is a default feature). Discovery is deterministic, local, and
read-only; it establishes *possible* failures and never invents probabilities.

```bash
etdl discover ./service
etdl discover ./service --language rust
etdl discover ./service --format json
etdl discover ./service --format yaml --output failures.yaml
etdl discover ./service --min-confidence 0.7
etdl discover ./service --exclude tests
etdl discover ./service --ontology-policy auto    # auto | conservative | off
```

Options:

- `--language <LANG>` — source language (only `rust` is implemented; others
  produce an explicit error).
- `--format <text|json|yaml>` — output format (default `text`).
- `--min-confidence <F>` — drop candidates below this confidence (0.0-1.0).
- `--output <FILE>` — write the report to a file instead of stdout.
- `--exclude <PATH>` — exclude a path (repeatable).
- `--ontology-policy <auto|conservative|off>` — ontology mapping policy.

JSON/YAML output uses the versioned schema `etdl.failure-discovery.report/1.0`,
distinct from `etdl.reliability.artifact/1.0`. See
[docs/failure-discovery/README.md](failure-discovery/README.md).

### etdl version / etdl --version

Print the CLI version (`--json` emits `{"name":"etdl","version":"..."}`).

## Output conventions

- Human diagnostics: `[ERROR] E-103 (10:17): message ...` (1-based positions).
- JSON diagnostics: `{ code, severity, message, line, column }` (0-based).
- Deterministic output: same input + same flags → identical stdout/stderr
  (no timestamps).
