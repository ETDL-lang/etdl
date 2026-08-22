//! `--target` resolution: the registry mapping a CLI target name to a
//! [`etdl_compiler::CodeGenerator`] implementation.
//!
//! Lives here, not in `etdl-compiler`, so the compiler crate itself stays
//! target-agnostic — adding a target to the CLI never touches the parsing,
//! validation, or fault-tree-resolution pipeline. Every entry below is
//! actually wired to a real generator producing a thin, language-native
//! binding to the compiled `etdl-runtime-ffi` Rust runtime — none of them
//! reimplement ETDL semantics in their own language (see
//! `docs/architecture/targets.md`). `javascript`/`typescript` are designed
//! for but intentionally not listed here yet — advertising a target string
//! here is a promise it produces real, usable output, which is only true
//! for the targets actually compiled into this binary via their Cargo
//! feature.
//!
//! `java`/`python`/`go`/`dotnet` do not need their language's toolchain
//! *installed on this machine* to be listed here — this crate only emits
//! source text for them (via `etdl-runtime-ffi`'s C ABI + a
//! language-specific binding). Building/running the *generated* Java,
//! Python, Go, or C# code is where that target's own toolchain (and a
//! built `etdl-runtime-ffi`) becomes necessary — see each target's tests
//! for how that's gated.

use etdl_compiler::{CodeGenerator, RustCodeGenerator};

/// Every target compiled into this binary, in a stable display order
/// (`rust` first, then everything else alphabetically) — used for both
/// dispatch and the `--target` help/error text.
pub fn available_targets() -> Vec<Box<dyn CodeGenerator>> {
    #[allow(unused_mut)]
    let mut targets: Vec<Box<dyn CodeGenerator>> = vec![Box::new(RustCodeGenerator::new())];

    #[cfg(feature = "target-dotnet")]
    targets.push(Box::new(etdl_target_dotnet::DotnetCodeGenerator::new()));

    #[cfg(feature = "target-go")]
    targets.push(Box::new(etdl_target_go::GoCodeGenerator::new()));

    #[cfg(feature = "target-java")]
    targets.push(Box::new(etdl_target_java::JavaCodeGenerator::new()));

    #[cfg(feature = "target-python")]
    targets.push(Box::new(etdl_target_python::PythonCodeGenerator::new()));

    targets
}

/// Names of every target compiled into this binary, `rust` first. Used in
/// `--help` and in the unknown-target error so both always reflect exactly
/// what this build can actually generate.
pub fn available_target_names() -> Vec<&'static str> {
    available_targets().iter().map(|t| t.target_name()).collect()
}

/// Resolve one `--target` name to its generator, or `None` if this build
/// doesn't have that target compiled in.
pub fn resolve_target(name: &str) -> Option<Box<dyn CodeGenerator>> {
    available_targets().into_iter().find(|t| t.target_name() == name)
}

/// Splits a `--target` value on commas (spec: "If practical, support
/// specifying multiple targets in one invocation, e.g. `--target
/// rust,java,python`"), trimming whitespace and dropping empty segments
/// (so a trailing comma or repeated commas don't produce a confusing empty
/// target name in error messages).
pub fn split_target_names(target: &str) -> Vec<&str> {
    target.split(',').map(str::trim).filter(|s| !s.is_empty()).collect()
}
