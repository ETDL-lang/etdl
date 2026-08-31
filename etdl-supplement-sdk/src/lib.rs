//! SDK for writing ETDL supplement plugins.
//!
//! A supplement plugin is a `wasm32-unknown-unknown` module `etdl-cli`
//! loads dynamically (`etdl install`) and runs sandboxed — no ambient
//! filesystem, network, or clock access — via `wasmtime` in
//! `etdl-compiler`'s `WasmExtension` host adapter.
//!
//! This crate hides the wire format (a small alloc/dealloc + JSON-over-
//! linear-memory ABI, documented in full in
//! `docs/reference/supplement-plugins.md` for non-Rust plugin authors) so
//! a Rust plugin author only writes ordinary Rust:
//!
//! ```ignore
//! use etdl_supplement_sdk::{Supplement, SupplementContext, SupplementDiagnostic, Severity};
//!
//! #[derive(Default)]
//! struct MyAudit;
//!
//! impl Supplement for MyAudit {
//!     fn id(&self) -> &str { "etdl.mycompany-audit" }
//!     fn version(&self) -> &str { "1.0" }
//!     fn validate(&self, _doc: &serde_json::Value, _ctx: &SupplementContext) -> Vec<SupplementDiagnostic> {
//!         Vec::new()
//!     }
//! }
//!
//! etdl_supplement_sdk::etdl_supplement!(MyAudit);
//! ```
//!
//! `cargo build --target wasm32-unknown-unknown` then produces a
//! conforming module.
//!
//! **Why `serde_json::Value`, not `etdl_parser::ast::EtlDocument`, for the
//! document a plugin sees**: the wire contract is JSON either way (the
//! host serializes `EtlDocument` before crossing the WASM boundary), and
//! keeping the plugin-facing type a plain `Value` means a plugin author's
//! `Cargo.toml` never depends on `etdl-parser` at all, and this SDK's ABI
//! stays decoupled from `etdl-parser`'s internal AST shape evolving —
//! only this crate's own `Supplement` trait is a compatibility surface a
//! plugin author needs to track.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Mirrors `etdl_compiler::extension::ExtensionContext`, minus the
/// borrowed `&EtlDocument` (the plugin receives that separately, as JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplementContext {
    pub base_dir: String,
    #[serde(default)]
    pub config: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// Mirrors `etdl_compiler::validate::Diagnostic`'s essential fields — a
/// plugin reports what's wrong; the host attaches source-position info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplementDiagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
}

impl SupplementDiagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        SupplementDiagnostic {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        SupplementDiagnostic {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

/// A supplement plugin. Implementations SHOULD be pure and deterministic —
/// the host runs each call under a fuel limit and with no ambient I/O
/// capability granted, so anything requiring the filesystem, network, or
/// wall-clock time will not work regardless.
pub trait Supplement: Default {
    /// The namespaced extension id, e.g. `etdl.mycompany-audit`.
    fn id(&self) -> &str;

    /// The extension version.
    fn version(&self) -> &str;

    /// Validate the document. `doc` is the parsed `EtlDocument`,
    /// serialized to JSON by the host.
    fn validate(&self, doc: &serde_json::Value, ctx: &SupplementContext) -> Vec<SupplementDiagnostic>;

    /// Optional semantic processing step. Returns basic-event probability
    /// overrides as `(override_key, value)` pairs — the same shape
    /// `etdl_compiler::extension::ExtensionResult::basic_event_overrides`
    /// already uses on the host side. Default: none.
    fn process(&self, _doc: &serde_json::Value, _ctx: &SupplementContext) -> Vec<(String, f64)> {
        Vec::new()
    }
}

// --- Guest-side ABI plumbing (called by the generated `extern "C"` shims;
// not meant to be called directly by plugin authors). ---

#[doc(hidden)]
pub fn __alloc(len: u32) -> u32 {
    let mut buf = Vec::<u8>::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr() as u32;
    std::mem::forget(buf);
    ptr
}

#[doc(hidden)]
pub fn __dealloc(ptr: u32, len: u32) {
    unsafe {
        let _ = Vec::from_raw_parts(ptr as *mut u8, 0, len as usize);
    }
}

#[doc(hidden)]
/// Reads `len` bytes starting at `ptr` out of this module's own linear
/// memory. Safe only because the host is required to have written exactly
/// that many bytes there via `__alloc` before calling in — the same
/// contract every WASM string-passing plugin ABI of this shape relies on.
pub unsafe fn __read_bytes(ptr: u32, len: u32) -> Vec<u8> {
    std::slice::from_raw_parts(ptr as *const u8, len as usize).to_vec()
}

#[doc(hidden)]
/// Copies `s` into freshly `__alloc`'d guest memory and packs
/// `(ptr << 32) | len` into a single `u64` — the whole point of the
/// packing is that a WASM function can return exactly one `i64` without
/// needing multi-value-return support on the host.
pub fn __ret_bytes(bytes: Vec<u8>) -> u64 {
    let len = bytes.len() as u32;
    let ptr = __alloc(len);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, len as usize);
    }
    std::mem::forget(bytes);
    ((ptr as u64) << 32) | (len as u64)
}

#[doc(hidden)]
pub fn __ret_str(s: &str) -> u64 {
    __ret_bytes(s.as_bytes().to_vec())
}

/// Generates the four `extern "C"` exports (`etdl_alloc`, `etdl_dealloc`,
/// `etdl_supplement_id`, `etdl_supplement_version`,
/// `etdl_supplement_validate`, `etdl_supplement_process`) `etdl-cli`'s
/// `WasmExtension` host expects, wrapping a `$ty: Supplement + Default`.
#[macro_export]
macro_rules! etdl_supplement {
    ($ty:ty) => {
        #[no_mangle]
        pub extern "C" fn etdl_alloc(len: u32) -> u32 {
            $crate::__alloc(len)
        }

        #[no_mangle]
        pub extern "C" fn etdl_dealloc(ptr: u32, len: u32) {
            $crate::__dealloc(ptr, len)
        }

        #[no_mangle]
        pub extern "C" fn etdl_supplement_id() -> u64 {
            $crate::__ret_str(<$ty as ::std::default::Default>::default().id())
        }

        #[no_mangle]
        pub extern "C" fn etdl_supplement_version() -> u64 {
            $crate::__ret_str(<$ty as ::std::default::Default>::default().version())
        }

        #[no_mangle]
        pub extern "C" fn etdl_supplement_validate(
            doc_ptr: u32,
            doc_len: u32,
            ctx_ptr: u32,
            ctx_len: u32,
        ) -> u64 {
            let doc_bytes = unsafe { $crate::__read_bytes(doc_ptr, doc_len) };
            let ctx_bytes = unsafe { $crate::__read_bytes(ctx_ptr, ctx_len) };
            let result = (|| -> Result<Vec<$crate::SupplementDiagnostic>, String> {
                let doc: ::serde_json::Value =
                    ::serde_json::from_slice(&doc_bytes).map_err(|e| e.to_string())?;
                let ctx: $crate::SupplementContext =
                    ::serde_json::from_slice(&ctx_bytes).map_err(|e| e.to_string())?;
                Ok(<$ty as ::std::default::Default>::default().validate(&doc, &ctx))
            })();
            let json = match result {
                Ok(diags) => ::serde_json::to_vec(&diags).unwrap_or_default(),
                Err(e) => format!("{{\"__plugin_error\":{:?}}}", e).into_bytes(),
            };
            $crate::__ret_bytes(json)
        }

        #[no_mangle]
        pub extern "C" fn etdl_supplement_process(
            doc_ptr: u32,
            doc_len: u32,
            ctx_ptr: u32,
            ctx_len: u32,
        ) -> u64 {
            let doc_bytes = unsafe { $crate::__read_bytes(doc_ptr, doc_len) };
            let ctx_bytes = unsafe { $crate::__read_bytes(ctx_ptr, ctx_len) };
            let result = (|| -> Result<Vec<(String, f64)>, String> {
                let doc: ::serde_json::Value =
                    ::serde_json::from_slice(&doc_bytes).map_err(|e| e.to_string())?;
                let ctx: $crate::SupplementContext =
                    ::serde_json::from_slice(&ctx_bytes).map_err(|e| e.to_string())?;
                Ok(<$ty as ::std::default::Default>::default().process(&doc, &ctx))
            })();
            let json = match result {
                Ok(overrides) => {
                    ::serde_json::to_vec(&::serde_json::json!({ "overrides": overrides }))
                        .unwrap_or_default()
                }
                Err(e) => format!("{{\"__plugin_error\":{:?}}}", e).into_bytes(),
            };
            $crate::__ret_bytes(json)
        }
    };
}
