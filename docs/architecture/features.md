# ETDL Feature & Packaging Architecture

This document describes how the ETDL workspace is packaged: which crates exist,
which Cargo features gate optional capability, and how the layers are kept
separate so that ordinary ETDL users never pay for advanced reliability
engineering they do not use.

## Packages

| Package | Role | Required for `etdl compile`? |
|---|---|---|
| `etdl-core` | Runtime types (branch monitors, retry, SLA, telemetry) | No (generated code targets it) |
| `etdl-parser` | Parse `.etdl`, AsyncAPI imports, ECEL | Yes (compiler dependency) |
| `etdl-compiler` | Validate, resolve fault trees, generate code | Yes |
| `etdl-cli` | The `etdl` binary | No (consumes the compiler) |
| `etdl-wasm` | Browser/Node bindings | No |
| `etdl-reliability-core` | **Built-in** reliability: artifacts, resolution, validation | Only if the document declares `etdl.reliability` |
| `etdl-reliability` | Optional richer reliability domain + analysis | No |
| `etdl-reliability-ontology` | Canonical reliability concepts | No |
| `etdl-failure-discovery` | Source analysis producing failure candidates | No |

## Dependency direction

```
etdl-parser
etdl-reliability-ontology          etdl-reliability-core
      ^                                  ^
      |                                  |
etdl-failure-discovery             etdl-reliability
      ^                                  ^
      |                                  |
      +------------ etdl-compiler -------+
                         |
                     etdl-cli / etdl-wasm
```

Rules:

- Domain crates (`etdl-reliability-core`, `etdl-reliability`,
  `etdl-reliability-ontology`, `etdl-failure-discovery`) do **not** depend on
  the compiler or parser.
- `etdl-compiler` depends only on the **built-in** `etdl-reliability-core` for
  reliability; it never depends on the rich `etdl-reliability` crate.
- The CLI composes the compiler + optional discovery/ontology crates.

## Analysis capabilities

Advanced analysis is compiled in with the rich `etdl-reliability` crate and is
reported by `etdl capabilities`. Nothing here is required to compile an ETDL
document.

| Capability | Status | Notes |
|---|---|---|
| Reliability (built-in) | available | `etdl-reliability-core`, WASM-safe |
| Statistical estimation | optional | Wilson, Beta-Binomial, exponential |
| Uncertainty analysis | optional | representation is built-in; propagation is optional |
| Monte Carlo | optional | `monte-carlo-propagation/1`, sampler `xorshift64star/1` |
| Importance | optional | Birnbaum, Fussell-Vesely, criticality, RAW, RRW |
| Sensitivity | optional | `finite-perturbation/absolute/two-sided` |
| Uncertainty ranking | optional | variance-freeze with common random numbers |
| Analysis comparison | optional | before/after, without causal attribution |
| Correlated parameter uncertainty | **unsupported** | reported as unavailable, never as available |
| Conditional probability evaluation | **unsupported** | declaring one makes analysis refuse (RA001) |

The last two rows matter: `etdl capabilities` reports them as unsupported in
every build. A capability that is not implemented is never reported as
available.

## Cargo features

### `etdl-compiler`

```toml
[features]
default = ["reliability"]
reliability = ["dep:etdl-reliability-core"]
```

- **Default** build includes built-in reliability: the Reliability Supplement
  (`etdl.reliability`) can be parsed, validated, and compiled to deterministic
  probabilities. This pulls only the small, WASM-safe `etdl-reliability-core`.
- **`--no-default-features`** builds a compiler without any reliability
  support. A document declaring the reliability supplement then fails with a
  clear diagnostic (E-108 required / W-407 optional) instead of silently
  ignoring it.

### `etdl-cli`

```toml
[features]
default = ["reliability", "discovery"]
reliability = ["etdl-compiler/reliability", "dep:etdl-reliability-core"]
discovery = ["dep:etdl-failure-discovery", "dep:etdl-reliability-ontology"]
```

- **Default** CLI: reliability compilation built in (the normal experience) and
  `etdl discover` available.
- **`--no-default-features`**: minimal CLI — no reliability, no discovery.
- **`--features discovery`**: add source-code failure discovery and the
  ontology crate.

Requests for a capability that is not compiled in produce a clear message (for
example `etdl discover` without the `discovery` feature) and exit non-zero.
Nothing is downloaded at runtime.

### `etdl-wasm`

`etdl-wasm` depends on `etdl-compiler` with its default features, so built-in
reliability is available in the browser/Node. The built-in layer has no
filesystem or OS dependencies, so the WASM target stays clean.

## Feature matrix

`scripts/feature-matrix.sh` verifies every documented combination:

| Case | Command | Purpose |
|---|---|---|
| A | `cargo check -p etdl-compiler --no-default-features` | minimal ETDL build |
| B | `cargo check -p etdl-compiler` | ETDL + built-in reliability |
| C | `cargo check -p etdl-reliability` | optional reliability library |
| D | `cargo check -p etdl-cli --no-default-features --features discovery` | ETDL + failure discovery |
| E | `cargo check -p etdl-reliability-ontology` | ontology |
| F | `cargo check --workspace --all-features` | everything |
| G | `cargo check -p etdl-wasm --target wasm32-unknown-unknown` | WASM-compatible set |

## Reproducibility

- Two builds with the same ETDL source, compiler version, `Cargo.lock`, enabled
  features, and artifacts use the same reliability implementation — runtime
  discovery never changes semantics.
- `etdl capabilities` reports exactly what a binary can do.
- The build manifest (`etdl-build-manifest.json`) records the enabled features,
  implementation versions, and artifact schema version.

## Three users, three dependency costs

- **USER A — CLI engineer**: `cargo install etdl-cli` → `etdl compile model.etdl`.
  Reliability is built in; no feature knowledge needed.
- **USER B — Rust application**: depends on `etdl-compiler` and enables only the
  features it needs (`default` includes reliability; disable with
  `default-features = false`).
- **USER C — Reliability engineer**: depends on `etdl-reliability` (analysis),
  `etdl-reliability-ontology`, and `etdl-failure-discovery` to produce
  artifacts the ordinary compiler then consumes.

## Versioning

Versions are independent per package (ETDL Core vs Compiler vs Reliability
Supplement vs artifact schema). The workspace currently uses a single
`workspace.package.version`; individual crates may diverge when published
separately. The **artifact schema** (`etdl.reliability.artifact/1.0`) and the
**supplement version** (`etdl.reliability` `1.0`) are versioned independently
of crate versions.
