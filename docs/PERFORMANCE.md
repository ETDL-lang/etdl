# ETDL Performance Baselines

Informational baselines from `cargo bench --bench etdl_bench` (reference
implementation). Not a performance claim; treat as a point-in-time measurement.

## Method

- Benchmarks: `etdl-compiler/benches/etdl_bench.rs` (criterion).
- Input: the order-fulfillment fixture (~3.5 KB).
- Machine: developer workstation (Linux x86_64).
- Sample size: 20 (informational).

## Baselines

| Operation | Time |
|---|---|
| `parse_document` (YAML → AST) | ~77 µs |
| `validate_document` | ~5.8 µs |
| `compile_rust` (full pipeline) | ~5.6 µs |
| `parse_ecel_condition` | ~380 ns |

## Notes

- Parsing dominates (YAML deserialization); validation and codegen are
  sub-millisecond for typical documents.
- `compile_rust` ≈ `validate_document` + codegen; both are fast relative to
  parse because they operate on the already-parsed AST.
- These benches exercise a single small document. For large models (thousands
  of nodes), validation is dominated by DAG traversals; see the unbounded
  recursion note in `READINESS_AUDIT.md` (P0-3b) — bounded now by document size
  but not yet by explicit depth limits.

## Reliability analysis (optional path)

Measured with `cargo run --release` on the sandbox toolchain (rustc 1.75,
x86_64). Absolute numbers are machine-dependent; the ratios are the point.

| Operation | 10 basic events | 50 basic events |
|---|---|---|
| Point estimate, independent | 0.002 ms | 0.008 ms |
| Point estimate, 1 common cause | 0.004 ms | 0.018 ms |
| Importance (all entities, all measures) | 0.045 ms | 0.879 ms |
| Sensitivity, two-sided (all entities) | 0.047 ms | 0.901 ms |
| `analyze_with`, no propagation | 0.117 ms | 3.567 ms |
| Monte Carlo, 10,000 samples | 42.9 ms | 216.9 ms |

Reading these:

- **Ordinary compilation is unaffected.** None of this runs during
  `etdl compile`. The compiler depends on `etdl-reliability-core`, which
  contains no statistical algorithms and gained none in this change.
- Importance and sensitivity are `O(entities x evaluation)` — each entity is
  conditioned or perturbed and the tree re-evaluated. At 50 events that is
  ~100 full evaluations for two-sided sensitivity, and the cost shows.
- Monte Carlo dominates by three orders of magnitude, which is why it is
  opt-in and never implied. `analyze_with` with `monte_carlo: None` performs no
  sampling at all.
- Common-cause conditioning costs `2^k` evaluations for `k` declared causes and
  is refused above `k = 20`.
- The uncertainty contribution ranking costs one extra propagation run **per
  uncertain input**, so a 50-event model with 50 uncertain inputs at 10k samples
  is ~11 s. It is opt-in for that reason.

## Large-document benchmark (future)

Add bench fixtures at 1k / 10k / 100k nodes to establish scaling behavior
before optimizing. Do not optimize prematurely — measure first.
