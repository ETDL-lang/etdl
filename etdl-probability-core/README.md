# etdl-probability-core

[![Crates.io](https://img.shields.io/crates/v/etdl-probability-core.svg)](https://crates.io/crates/etdl-probability-core)
[![Docs.rs](https://img.shields.io/docsrs/etdl-probability-core)](https://docs.rs/etdl-probability-core)

**The [ETDL](https://github.com/ETDL-lang/etdl) Standard Probability Library native layer** (`std.probability`'s Rust API) — domain-neutral probability math with **zero dependency on any reliability crate**, enforced by this crate's own `Cargo.toml`, not merely convention.

## What it provides

- **`Probability`, `Rate`** — validated probability/rate value types.
- **`distribution`** — foundational distributions: Bernoulli, Binomial, Beta, Exponential, Normal.
- **Composition math** — complement, independent AND/OR, conditional probability, Bayes' rule — explicit operations, not ad-hoc arithmetic scattered across call sites.

## Where this sits

```
ETDL Core
   |
ETDL Standard Library
   |
std.probability          <- this crate is the native layer beneath it
   |
Domain Libraries          (reliability, safety, security, risk, ...)
   |
User Models
```

`etdl-reliability`/`etdl-reliability-core` may (and, via a small adapter, do) depend on this crate; this crate never depends on them.

## Why this is a Rust crate, not ETDL source

ETDL is a declarative YAML document format with no general expression or function-call syntax — ECEL, the one embedded mini-language, exists solely to parse barrier-branch conditions and has no arithmetic or function definitions. There's no honest way to express "compute the complement of a referenced probability" as ETDL YAML, so the actual math is a compiled, native layer instead. What *is* expressible in pure ETDL — reusable, named probability **constants** — lives in `std.probability`'s `.etdl` standard-library document, resolved at compile time by [`etdl-compiler`](https://crates.io/crates/etdl-compiler).

Used directly by [`etdl-cli`](https://crates.io/crates/etdl-cli)'s Monte Carlo/sensitivity/uncertainty analysis commands.

Full standard-library architecture: [`docs/reference/standard-probability-library.md`](https://github.com/ETDL-lang/etdl/blob/main/docs/reference/standard-probability-library.md) in the main repo.

## License

Apache-2.0
