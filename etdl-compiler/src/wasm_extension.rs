//! Dynamic supplement plugins: a sandboxed `wasmtime` host adapter
//! implementing [`EtdlExtension`] over a loaded `.wasm` module.
//!
//! A plugin is untrusted third-party code (`etdl install <path-or-url>`),
//! so every call into it is bounded: no WASI is linked
//! (no ambient filesystem, network, clock, or environment access — the
//! module's only capability is the JSON-in/JSON-out ABI below), and every
//! call runs under a fuel limit so a plugin that loops forever gets
//! trapped rather than hanging the host indefinitely. A plugin that
//! panics, traps, or returns malformed data becomes an ordinary
//! `Diagnostic::error`, never a host crash — the same "untrusted input
//! must never crash the compiler" bar `asyncapi_imports` resolution is
//! already held to.
//!
//! ## Wire ABI
//!
//! A conforming module exports:
//!
//! ```text
//! etdl_alloc(len: u32) -> u32                                    (ptr)
//! etdl_dealloc(ptr: u32, len: u32)
//! etdl_supplement_id() -> u64                                    (ptr<<32 | len)
//! etdl_supplement_version() -> u64                               (ptr<<32 | len)
//! etdl_supplement_validate(doc_ptr, doc_len, ctx_ptr, ctx_len) -> u64
//! etdl_supplement_process(doc_ptr, doc_len, ctx_ptr, ctx_len) -> u64
//! memory                                                          (exported linear memory)
//! ```
//!
//! `doc`/`ctx` are JSON (`serde_json::to_vec(&EtlDocument)` and
//! [`etdl_supplement_sdk::SupplementContext`] respectively); the two
//! `_validate`/`_process` calls return JSON too (`Vec<SupplementDiagnostic>`
//! and `{"overrides": [[key, value], ...]}`). See
//! `docs/reference/supplement-plugins.md` for the full contract (written
//! for non-Rust plugin authors); Rust authors use `etdl-supplement-sdk`
//! instead of hand-rolling this.

use etdl_parser::ast::EtlDocument;
use etdl_supplement_sdk::{Severity as SdkSeverity, SupplementContext, SupplementDiagnostic};
use wasmtime::{Config, Engine, Instance, Linker, Memory, Module, Store};

use crate::extension::{EtdlExtension, ExtensionContext, ExtensionResult};
use crate::validate::{Diagnostic, DiagnosticSeverity};

/// Fuel budget per call into a plugin (`validate`/`process`, and the two
/// `id`/`version` calls made once at load time). Generous for real work,
/// bounded so a runaway loop traps instead of hanging `etdl`. `wasmtime`
/// charges roughly one unit of fuel per executed instruction, so this is
/// on the order of tens of millions of instructions — comfortably enough
/// for JSON encode/decode plus real validation logic over a document,
/// nowhere near enough to let an infinite loop run unbounded.
const FUEL_PER_CALL: u64 = 50_000_000;

pub struct WasmExtension {
    id: String,
    version: String,
    engine: Engine,
    module: Module,
}

impl WasmExtension {
    /// Loads and validates a candidate plugin module: it must instantiate
    /// with no host-provided imports (no WASI — a conforming module needs
    /// none) and export the id/version functions we call once here to
    /// populate this adapter's cached identity. Returns an error rather
    /// than panicking on anything malformed — this is the same function
    /// `etdl install` calls to reject a broken module before ever
    /// installing it.
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(|e| format!("wasmtime engine init failed: {e}"))?;
        let module = Module::new(&engine, bytes)
            .map_err(|e| format!("not a valid WebAssembly module: {e}"))?;

        let (mut store, instance) = instantiate(&engine, &module)?;
        let id = call_str(&mut store, &instance, "etdl_supplement_id")
            .map_err(|e| format!("etdl_supplement_id: {e}"))?;
        let version = call_str(&mut store, &instance, "etdl_supplement_version")
            .map_err(|e| format!("etdl_supplement_version: {e}"))?;

        Ok(WasmExtension {
            id,
            version,
            engine,
            module,
        })
    }

    /// Re-instantiates fresh per call rather than reusing one instance
    /// across the plugin's lifetime — `etdl-cli` is a short-lived
    /// process, so the extra instantiation cost is negligible, and a
    /// fresh instance per call means a plugin author's own bug (a
    /// forgotten `etdl_dealloc`, say) can never accumulate across many
    /// calls within one run.
    fn call_json(&self, export: &str, doc_json: &[u8], ctx_json: &[u8]) -> Result<Vec<u8>, String> {
        let (mut store, instance) = instantiate(&self.engine, &self.module)?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or("module has no exported linear memory named \"memory\"")?;

        let (doc_ptr, doc_len) = write_bytes(&mut store, &instance, doc_json)?;
        let (ctx_ptr, ctx_len) = write_bytes(&mut store, &instance, ctx_json)?;

        let f = instance
            .get_typed_func::<(u32, u32, u32, u32), u64>(&mut store, export)
            .map_err(|e| format!("plugin does not export {export}: {e}"))?;
        let packed = f
            .call(&mut store, (doc_ptr, doc_len, ctx_ptr, ctx_len))
            .map_err(|e| format!("plugin trapped in {export}: {e}"))?;

        let (result_ptr, result_len) = unpack(packed);
        let result = read_bytes(&mut store, &memory, result_ptr, result_len)?;

        // Best-effort cleanup: a plugin that never implemented
        // `etdl_dealloc` correctly just leaks within its own (about to be
        // dropped) instance's memory, never the host's.
        let _ = call_dealloc(&mut store, &instance, doc_ptr, doc_len);
        let _ = call_dealloc(&mut store, &instance, ctx_ptr, ctx_len);
        let _ = call_dealloc(&mut store, &instance, result_ptr, result_len);

        Ok(result)
    }
}

fn instantiate(engine: &Engine, module: &Module) -> Result<(Store<()>, Instance), String> {
    let mut store = Store::new(engine, ());
    store
        .set_fuel(FUEL_PER_CALL)
        .map_err(|e| format!("failed to set fuel budget: {e}"))?;
    // No imports linked, deliberately: a conforming plugin needs no
    // ambient capability, and an empty Linker makes that a hard
    // guarantee rather than a convention — a module that (wrongly)
    // imports anything fails to instantiate here, loudly, at load time.
    let linker: Linker<()> = Linker::new(engine);
    let instance = linker
        .instantiate(&mut store, module)
        .map_err(|e| format!("failed to instantiate (unexpected import, or trapped during start): {e}"))?;
    Ok((store, instance))
}

fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, (packed & 0xFFFF_FFFF) as u32)
}

fn read_bytes(store: &mut Store<()>, memory: &Memory, ptr: u32, len: u32) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; len as usize];
    memory
        .read(&mut *store, ptr as usize, &mut buf)
        .map_err(|e| format!("out-of-bounds read from plugin memory: {e}"))?;
    Ok(buf)
}

fn write_bytes(store: &mut Store<()>, instance: &Instance, bytes: &[u8]) -> Result<(u32, u32), String> {
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or("module has no exported linear memory named \"memory\"")?;
    let alloc = instance
        .get_typed_func::<u32, u32>(&mut *store, "etdl_alloc")
        .map_err(|e| format!("plugin does not export etdl_alloc: {e}"))?;
    let ptr = alloc
        .call(&mut *store, bytes.len() as u32)
        .map_err(|e| format!("plugin trapped in etdl_alloc: {e}"))?;
    memory
        .write(&mut *store, ptr as usize, bytes)
        .map_err(|e| format!("out-of-bounds write into plugin memory: {e}"))?;
    Ok((ptr, bytes.len() as u32))
}

fn call_dealloc(store: &mut Store<()>, instance: &Instance, ptr: u32, len: u32) -> Result<(), String> {
    let f = instance
        .get_typed_func::<(u32, u32), ()>(&mut *store, "etdl_dealloc")
        .map_err(|e| format!("plugin does not export etdl_dealloc: {e}"))?;
    f.call(&mut *store, (ptr, len))
        .map_err(|e| format!("plugin trapped in etdl_dealloc: {e}"))
}

/// Calls a zero-argument, packed-`u64`-returning export (`etdl_supplement_id`/
/// `_version`) and decodes the result as UTF-8.
fn call_str(store: &mut Store<()>, instance: &Instance, export: &str) -> Result<String, String> {
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or("module has no exported linear memory named \"memory\"")?;
    let f = instance
        .get_typed_func::<(), u64>(&mut *store, export)
        .map_err(|e| format!("plugin does not export {export}: {e}"))?;
    let packed = f
        .call(&mut *store, ())
        .map_err(|e| format!("plugin trapped in {export}: {e}"))?;
    let (ptr, len) = unpack(packed);
    let bytes = read_bytes(store, &memory, ptr, len)?;
    String::from_utf8(bytes).map_err(|e| format!("{export} did not return valid UTF-8: {e}"))
}

fn plugin_error(id: &str, message: impl std::fmt::Display) -> Diagnostic {
    // Not one of the spec's normative E-/V-/W- codes deliberately: dynamic
    // plugin hosting is an `etdl-cli`-specific capability, not new ETDL
    // language semantics every Conforming Compiler must implement, so
    // this stays outside the spec's registered diagnostic namespace.
    Diagnostic::error(
        "PLUGIN-ERROR",
        format!("supplement plugin '{id}' failed: {message}"),
    )
}

fn context_json(context: &ExtensionContext<'_>) -> Result<Vec<u8>, String> {
    let ctx = SupplementContext {
        base_dir: context.base_dir.display().to_string(),
        config: context
            .config
            .iter()
            .filter_map(|(k, v)| serde_json::to_value(v).ok().map(|v| (k.clone(), v)))
            .collect(),
    };
    serde_json::to_vec(&ctx).map_err(|e| format!("failed to serialize plugin context: {e}"))
}

impl EtdlExtension for WasmExtension {
    fn id(&self) -> &str {
        &self.id
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn validate(
        &self,
        doc: &EtlDocument,
        context: &ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let doc_json = match serde_json::to_vec(doc) {
            Ok(v) => v,
            Err(e) => {
                diagnostics.push(plugin_error(&self.id, format!("failed to serialize document: {e}")));
                return;
            }
        };
        let ctx_json = match context_json(context) {
            Ok(v) => v,
            Err(e) => {
                diagnostics.push(plugin_error(&self.id, e));
                return;
            }
        };

        match self.call_json("etdl_supplement_validate", &doc_json, &ctx_json) {
            Ok(bytes) => match serde_json::from_slice::<Vec<SupplementDiagnostic>>(&bytes) {
                Ok(plugin_diags) => {
                    for d in plugin_diags {
                        let severity = match d.severity {
                            SdkSeverity::Error => DiagnosticSeverity::Error,
                            SdkSeverity::Warning => DiagnosticSeverity::Warning,
                        };
                        let mut diag = match severity {
                            DiagnosticSeverity::Error => Diagnostic::error(&d.code, d.message),
                            DiagnosticSeverity::Warning => Diagnostic::warning(&d.code, d.message),
                        };
                        diag.message = format!("[{}] {}", self.id, diag.message);
                        diagnostics.push(diag);
                    }
                }
                Err(_) => diagnostics.push(plugin_error(
                    &self.id,
                    "validate returned malformed JSON (expected an array of diagnostics)",
                )),
            },
            Err(e) => diagnostics.push(plugin_error(&self.id, e)),
        }
    }

    fn process(
        &self,
        doc: &EtlDocument,
        context: &ExtensionContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Box<dyn ExtensionResult + '_> {
        let overrides = (|| -> Result<Vec<(String, f64)>, String> {
            let doc_json = serde_json::to_vec(doc).map_err(|e| format!("failed to serialize document: {e}"))?;
            let ctx_json = context_json(context)?;
            let bytes = self.call_json("etdl_supplement_process", &doc_json, &ctx_json)?;
            #[derive(serde::Deserialize)]
            struct ProcessResult {
                overrides: Vec<(String, f64)>,
            }
            let result: ProcessResult = serde_json::from_slice(&bytes)
                .map_err(|_| "process returned malformed JSON (expected {\"overrides\": [...]})".to_string())?;
            Ok(result.overrides)
        })();

        match overrides {
            Ok(overrides) => Box::new(WasmExtensionResult {
                id: self.id.clone(),
                overrides,
            }),
            Err(e) => {
                diagnostics.push(plugin_error(&self.id, e));
                Box::new(WasmExtensionResult {
                    id: self.id.clone(),
                    overrides: Vec::new(),
                })
            }
        }
    }
}

struct WasmExtensionResult {
    id: String,
    overrides: Vec<(String, f64)>,
}

impl ExtensionResult for WasmExtensionResult {
    fn extension_id(&self) -> &str {
        &self.id
    }

    fn basic_event_overrides(&self) -> Vec<(String, f64)> {
        self.overrides.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/wasm-plugins")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {}", path.display(), e))
    }

    fn minimal_doc() -> EtlDocument {
        let yaml = r#"
etdl: "1.0.0"
info: { title: "T", version: "1.0.0", domain: "D" }
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent: { id: I, message: "a#/m", next: C }
    nodes:
      C: { type: consequence, operation: terminate }
"#;
        serde_yaml::from_str(yaml).expect("minimal doc parses")
    }

    #[test]
    fn valid_plugin_loads_and_reports_id_version() {
        let ext = WasmExtension::load(&fixture("valid.wasm")).expect("loads");
        assert_eq!(ext.id(), "etdl.fixture-valid");
        assert_eq!(ext.version(), "1.0");
    }

    #[test]
    fn valid_plugin_validate_propagates_real_diagnostic() {
        let ext = WasmExtension::load(&fixture("valid.wasm")).expect("loads");
        let doc = minimal_doc();
        let base_dir = std::path::PathBuf::from(".");
        let ctx = ExtensionContext {
            doc: &doc,
            base_dir: &base_dir,
            config: BTreeMap::new(),
        };
        let mut diagnostics = Vec::new();
        ext.validate(&doc, &ctx, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "FIXTURE-001");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);
        assert!(diagnostics[0].message.contains("etdl.fixture-valid"));
        assert!(diagnostics[0].message.contains("fixture plugin ran successfully"));
    }

    #[test]
    fn valid_plugin_process_propagates_overrides() {
        let ext = WasmExtension::load(&fixture("valid.wasm")).expect("loads");
        let doc = minimal_doc();
        let base_dir = std::path::PathBuf::from(".");
        let ctx = ExtensionContext {
            doc: &doc,
            base_dir: &base_dir,
            config: BTreeMap::new(),
        };
        let mut diagnostics = Vec::new();
        let result = ext.process(&doc, &ctx, &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert_eq!(result.extension_id(), "etdl.fixture-valid");
        assert_eq!(
            result.basic_event_overrides(),
            vec![("FT.SomeEvent".to_string(), 0.042)]
        );
    }

    #[test]
    fn looping_plugin_traps_on_fuel_exhaustion_not_a_hang() {
        // Loading only calls id()/version(), which don't loop -- the trap
        // happens on the actual `validate` call.
        let ext = WasmExtension::load(&fixture("looping.wasm")).expect("loads");
        assert_eq!(ext.id(), "etdl.fixture-looping");
        let doc = minimal_doc();
        let base_dir = std::path::PathBuf::from(".");
        let ctx = ExtensionContext {
            doc: &doc,
            base_dir: &base_dir,
            config: BTreeMap::new(),
        };
        let mut diagnostics = Vec::new();
        // Must return promptly (the test itself would hang otherwise) and
        // produce a clean diagnostic, never panic the host process.
        ext.validate(&doc, &ctx, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "PLUGIN-ERROR");
        assert!(diagnostics[0].message.contains("etdl.fixture-looping"));
    }

    #[test]
    fn malformed_output_plugin_reports_clean_diagnostic_not_a_panic() {
        let ext = WasmExtension::load(&fixture("malformed.wasm")).expect("loads");
        assert_eq!(ext.id(), "etdl.fixture-malformed");
        let doc = minimal_doc();
        let base_dir = std::path::PathBuf::from(".");
        let ctx = ExtensionContext {
            doc: &doc,
            base_dir: &base_dir,
            config: BTreeMap::new(),
        };
        let mut diagnostics = Vec::new();
        ext.validate(&doc, &ctx, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "PLUGIN-ERROR");
        assert!(diagnostics[0].message.contains("malformed JSON"));
    }

    #[test]
    fn not_a_wasm_module_fails_to_load_cleanly() {
        let result = WasmExtension::load(b"not a wasm module");
        assert!(result.is_err());
    }
}
