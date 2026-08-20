//! Rust source analyzer.
//!
//! A deterministic, `syn`-based analyzer that walks the Rust AST and emits
//! failure **candidates**. It detects *possible* failure mechanisms; it never
//! proves they will occur and never invents probabilities.
//!
//! ## Supported detection
//!
//! - `Result`/`Option` error propagation (`?`)
//! - `unwrap()` / `expect(...)`
//! - `panic!`, `unreachable!`, `todo!`, `unimplemented!`
//! - `assert!`, `assert_eq!`, `assert_ne!`
//! - index expressions (potential out-of-bounds)
//! - division/remainder by zero potential
//! - `.parse::<T>()` / `FromStr`
//! - filesystem operations (`std::fs`, `File::open`, ...)
//! - network/client operations (TCP, HTTP, reqwest, tokio net)
//! - serialization/deserialization (`serde_json`, `serde_yaml`, ...)
//! - channel send/receive (`mpsc`, `tokio::sync`, crossbeam)
//! - mutex / RwLock lock acquisition (poisoning potential)
//! - timeout APIs
//! - external dependency calls (conservative heuristics)
//! - custom error types (`enum XxxError`, `#[derive(thiserror::Error)]`)
//! - `return Err(...)` explicit returns
//!
//! ## Not detected (documented limitations)
//!
//! - Complex runtime reflection, dynamically generated code
//! - Semantic business failures that leave no static trace
//! - Distributed race conditions
//! - Failures requiring runtime environment knowledge

pub mod patterns;
mod visitor;

pub use patterns::{pattern_mapping, RustPattern};
pub use visitor::{RustAnalyzer, RustAnalyzerConfig};
