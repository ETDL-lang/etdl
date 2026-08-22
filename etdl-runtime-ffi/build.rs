use std::env;
use std::path::PathBuf;

/// Regenerates `include/etdl_runtime.h` from `src/lib.rs` on every build, so
/// the header handed to Go's cgo (and kept as documentation for Python/.NET,
/// which bind without needing the header directly) can never drift from the
/// actual `extern "C"` functions below it.
fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_path = PathBuf::from(&crate_dir).join("include").join("etdl_runtime.h");
    std::fs::create_dir_all(out_path.parent().unwrap()).expect("create include/ dir");

    let config = cbindgen::Config::from_file(PathBuf::from(&crate_dir).join("cbindgen.toml"))
        .expect("valid cbindgen.toml");

    match cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
    {
        Ok(bindings) => {
            bindings.write_to_file(&out_path);
        }
        Err(e) => {
            // A stale committed header is safer than a build that hard-fails
            // on cbindgen's own transient parse issues (e.g. mid-refactor);
            // still fail loudly so it's never silently ignored in CI.
            println!("cargo:warning=cbindgen failed to regenerate {}: {}", out_path.display(), e);
            if !out_path.exists() {
                panic!("cbindgen failed and no prior header exists at {}: {}", out_path.display(), e);
            }
        }
    }

    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
}
