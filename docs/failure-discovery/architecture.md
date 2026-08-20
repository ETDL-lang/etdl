# Failure Discovery Architecture

## Discovery ≠ estimation ≠ observation

These three concepts are separate and must never be collapsed:

| Concept | Question | Producer | Example |
|---|---|---|---|
| **Discovery** | "What failures are *possible*?" | Static source analysis | `unwrap()` at `src/lib.rs:17` |
| **Estimation** | "How *likely*?" | Reliability model + evidence | `P = 0.0015` (observed 1/667) |
| **Observation** | "What *happened*?" | Runtime telemetry | 1 timeout recorded yesterday |

```
                  OBSERVATION (runtime)
                         │
                         ▼
   SOURCE ──► DISCOVERY ──► CANDIDATE ──► HUMAN/ENGINEERING REVIEW
     │                                          │
     │                                          ▼
     │                                    ONTOLOGY MAPPING
     │                                          │
     │                                          ▼
     │                                      EVIDENCE
     │                                          │
     │                                          ▼
     │                                     ESTIMATION
     │                                          │
     │                                          ▼
     │                               RELIABILITY ARTIFACT (.rprob)
     │                                          │
     ▼                                          ▼
        ───────────────────► ETDL COMPILATION
                                     │
                                     ▼
                            DETERMINISTIC BUILD
```

## Crate layout

```
etdl-failure-discovery
├── analyzer.rs      SourceAnalyzer trait + AnalyzerRegistry
├── candidate.rs     DiscoveryCandidate, classification, severity, evidence
├── config.rs        DiscoveryConfig (language, min-confidence, paths, policy)
├── error.rs         structured DiscoveryError
├── identity.rs      stable candidate identity scheme
├── location.rs      SourceLocation (file/line/col/byte span), FunctionContext
├── mapping.rs       MappingQuality (exact/probable/ambiguous/unmapped/deprecated)
├── ontology.rs      read-only OntologyView (never writes)
├── report.rs        DiscoveryReport + ReportStatistics + schema version
├── source.rs        project walking, ignore rules, hashing, project identity
├── bridge.rs        discovery → reliability artifact bridge (external values only)
└── rust/
    ├── mod.rs       RustAnalyzer entry
    ├── patterns.rs  RustPattern definitions + ontology mapping
    └── visitor.rs   syn-based AST visitor
```

Dependency direction (respecting the established architecture):

```
etdl-reliability-ontology  ───►  etdl-failure-discovery  ───►  etdl-reliability-core (bridge)
etdl-failure-discovery is OPTIONAL: the compiler and ordinary builds do not
depend on it. The CLI enables it via the `discovery` feature.
```

## Analyzer abstraction

```rust
pub trait SourceAnalyzer: Send + Sync {
    fn language(&self) -> &str;
    fn version(&self) -> &str;
    fn analyze_file(&self, path, config) -> Result<DiscoveryReport, DiscoveryError>;
    fn analyze_project(&self, root, config) -> Result<DiscoveryReport, DiscoveryError>;
}
```

`AnalyzerRegistry::new()` registers the built-in `RustAnalyzer`. Analyzers are
deterministic and compiled in — no dynamic loading.

## Rust analyzer

Uses `syn` to walk the AST and record hits with precise `proc_macro2` spans
(line, column, byte offsets). Detected patterns:

- `?` propagation, `return Err(...)`, `unwrap()`, `expect(...)`
- `panic!`, `unreachable!`, `todo!`, `unimplemented!`
- `assert!`, `assert_eq!`, `assert_ne!`
- index expressions, `/` and `%` (divide-by-zero potential)
- `.parse::<T>()` / `FromStr`
- filesystem, network/client, serialization operations
- channel send/receive, mutex/RwLock lock acquisition
- timeout APIs, external dependency calls (conservative)
- custom error types (`enum XxxError`, `#[derive(Error)]`)

## Determinism & provenance

- No randomness, LLM calls, network, or current time.
- `SourceIdentity.content_hash`: deterministic FNV-1a over sorted file
  contents.
- Candidates sorted by (file, line, column, id).
- Report schema is versioned (`etdl.failure-discovery.report/1.0`).
- Analyzer name + version recorded in every report.

## Bridge (discovery → reliability)

`etdl-failure-discovery::bridge` converts **accepted, human-reviewed**
candidates into a reliability artifact **only when the caller supplies an
explicit estimate value**. Without a value it emits a candidate-only artifact
clearly marked as discovery output. It never derives a probability from
discovery confidence.

## Security

- Local-only; never executes analyzed code; never invokes binaries; never
  downloads; never transmits source.
- Ontology is read-only during discovery; new concepts are proposals.
