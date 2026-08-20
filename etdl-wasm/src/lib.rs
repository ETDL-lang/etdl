use etdl_compiler::validate::DiagnosticSeverity;
use etdl_parser::spanned::SpanIndex;
use serde_json::{json, Map, Value};
use wasm_bindgen::prelude::*;

fn diagnostic_json(d: &etdl_compiler::validate::Diagnostic, index: Option<&SpanIndex>) -> Value {
    let severity = match d.severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
    };
    let (line, column, end_line, end_column) = resolve_position(d, index);
    let mut obj = Map::new();
    obj.insert("code".to_string(), Value::String(d.code.clone()));
    obj.insert("severity".to_string(), Value::String(severity.to_string()));
    obj.insert("message".to_string(), Value::String(d.message.clone()));
    obj.insert(
        "line".to_string(),
        line.map(Value::from).unwrap_or(Value::Null),
    );
    obj.insert(
        "column".to_string(),
        column.map(Value::from).unwrap_or(Value::Null),
    );
    obj.insert(
        "end_line".to_string(),
        end_line.map(Value::from).unwrap_or(Value::Null),
    );
    obj.insert(
        "end_column".to_string(),
        end_column.map(Value::from).unwrap_or(Value::Null),
    );
    Value::Object(obj)
}

/// Resolve a diagnostic's source position from its structured locator.
fn resolve_position(
    d: &etdl_compiler::validate::Diagnostic,
    index: Option<&SpanIndex>,
) -> (Option<u32>, Option<u32>, Option<u32>, Option<u32>) {
    if let (Some(line), Some(column)) = (d.line, d.column) {
        return (Some(line), Some(column), d.end_line, d.end_column);
    }
    if let (Some(key), Some(index)) = (&d.key, index) {
        if let Some(el) = index.resolve(key) {
            let span = el.key_span.unwrap_or(el.span);
            return (
                Some(span.line),
                Some(span.column),
                Some(span.end_line),
                Some(span.end_column),
            );
        }
    }
    (d.line, d.column, d.end_line, d.end_column)
}

fn build_registry(
    doc: &etdl_parser::ast::EtlDocument,
    asyncapi_files_json: &str,
) -> Result<etdl_parser::asyncapi::AsyncApiRegistry, String> {
    let mut registry = etdl_parser::asyncapi::AsyncApiRegistry::new();

    let files: Value = serde_json::from_str(asyncapi_files_json)
        .map_err(|e| format!("invalid asyncapi files payload: {}", e))?;

    let files_map = files
        .as_object()
        .ok_or("asyncapi files payload must be an object")?;

    let content_for = |location: &str| -> Option<&str> {
        let direct = files_map.get(location).and_then(|v| v.as_str());
        if direct.is_some() {
            return direct;
        }
        let normalized = location.trim_start_matches("./").to_string();
        files_map.iter().find_map(|(k, v)| {
            if k.trim_start_matches("./") == normalized {
                v.as_str()
            } else {
                None
            }
        })
    };

    for (alias, location) in &doc.asyncapi_imports {
        let content = content_for(location).ok_or_else(|| {
            format!(
                "AsyncAPI file '{}' (alias '{}') not provided; pass its contents to the validator",
                location, alias
            )
        })?;
        let is_json = location.ends_with(".json");
        registry
            .load_from_content(alias, content, is_json)
            .map_err(|e| e.to_string())?;
    }

    Ok(registry)
}

/// Best-effort source position from a `serde_yaml` error (1-based -> 0-based).
fn yaml_error_position(e: &serde_yaml::Error) -> (Option<u32>, Option<u32>) {
    e.location()
        .map(|loc| {
            (
                Some(loc.line().saturating_sub(1) as u32),
                Some(loc.column().saturating_sub(1) as u32),
            )
        })
        .unwrap_or((None, None))
}

#[wasm_bindgen]
pub fn validate_etdl(etdl_content: &str, asyncapi_files_json: &str) -> Result<JsValue, JsValue> {
    let span_index = etdl_parser::spanned::build_span_index(etdl_content).ok();

    let doc = match etdl_parser::parse_document_raw(etdl_content) {
        Ok(doc) => doc,
        Err(e) => {
            let (line, column) = yaml_error_position(&e);
            let message = format!("YAML parse error: {}", e);
            let result = json!({
                "valid": false,
                "diagnostics": [{
                    "code": "E-100",
                    "severity": "error",
                    "message": message,
                    "line": line,
                    "column": column,
                    "end_line": line,
                    "end_column": column
                }]
            });
            return Ok(JsValue::from_str(&result.to_string()));
        }
    };

    let registry = match build_registry(&doc, asyncapi_files_json) {
        Ok(r) => r,
        Err(e) => {
            let result = json!({
                "valid": false,
                "diagnostics": [{
                    "code": "E-101",
                    "severity": "error",
                    "message": e,
                    "line": null,
                    "column": null,
                    "end_line": null,
                    "end_column": null
                }]
            });
            return Ok(JsValue::from_str(&result.to_string()));
        }
    };

    let compiler = etdl_compiler::Compiler::new();
    let mut diagnostics = compiler.validate(&doc, &registry);

    // Duplicate-id detection happens at the YAML level (duplicate keys are
    // collapsed before the typed AST is built).
    if let Ok(duplicates) = etdl_parser::spanned::detect_duplicate_ids(etdl_content) {
        for dup in duplicates {
            let mut d = etdl_compiler::validate::Diagnostic::warning(
                "V-001",
                format!(
                    "duplicate {} id '{}' in tree '{}'",
                    dup.kind, dup.id, dup.tree
                ),
            )
            .with_position(dup.span.line, dup.span.column);
            d.end_line = Some(dup.span.end_line);
            d.end_column = Some(dup.span.end_column);
            diagnostics.push(d);
        }
    }

    let valid = !diagnostics.iter().any(|d| d.is_error());
    let diags: Vec<Value> = diagnostics
        .iter()
        .map(|d| diagnostic_json(d, span_index.as_ref()))
        .collect();

    let result = json!({
        "valid": valid,
        "diagnostics": diags
    });

    Ok(JsValue::from_str(&result.to_string()))
}

#[wasm_bindgen]
pub fn parse_for_diagram(etdl_content: &str) -> Result<JsValue, JsValue> {
    let doc = match etdl_parser::parse_document(etdl_content) {
        Ok(doc) => doc,
        Err(e) => return Err(JsValue::from_str(&e)),
    };

    let value = match serde_json::to_value(&doc) {
        Ok(v) => v,
        Err(e) => return Err(JsValue::from_str(&format!("serialization error: {}", e))),
    };

    Ok(JsValue::from_str(&value.to_string()))
}

#[wasm_bindgen]
pub fn parse_for_raaml(etdl_content: &str) -> Result<JsValue, JsValue> {
    let doc = match etdl_parser::parse_document(etdl_content) {
        Ok(doc) => doc,
        Err(e) => return Err(JsValue::from_str(&e)),
    };

    let mut value = match serde_json::to_value(&doc) {
        Ok(v) => v,
        Err(e) => return Err(JsValue::from_str(&format!("serialization error: {}", e))),
    };

    enrich_raaml(&mut value);

    Ok(JsValue::from_str(&value.to_string()))
}

fn enrich_raaml(value: &mut Value) {
    let fault_trees = match value.get_mut("fault_trees").and_then(|v| v.as_object_mut()) {
        Some(fts) => fts,
        None => return,
    };

    for ft in fault_trees.values_mut() {
        let gates = match ft.get_mut("gates").and_then(|v| v.as_object_mut()) {
            Some(g) => g,
            None => continue,
        };

        for gate in gates.values_mut() {
            let gate_type = gate
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or_default();

            let n = gate
                .get("inputs")
                .and_then(|i| i.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            if gate_type == "VOTING" {
                let k = gate.get("k").and_then(|k| k.as_u64()).unwrap_or(0);
                if let Some(obj) = gate.as_object_mut() {
                    obj.insert("voting_params".to_string(), json!({ "k": k, "n": n }));
                }
            }
        }
    }
}

/// Same semantic tree as `parse_for_raaml`, with a `span` attached to every
/// element. Scalar leaves with spans are wrapped as `{ "value", "span" }`.
#[wasm_bindgen]
pub fn parse_with_spans(etdl_content: &str) -> Result<JsValue, JsValue> {
    let (doc, index) = match etdl_parser::spanned::parse_document_with_spans(etdl_content) {
        Ok(v) => v,
        Err(e) => return Err(JsValue::from_str(&e)),
    };

    let mut value = match serde_json::to_value(&doc) {
        Ok(v) => v,
        Err(e) => return Err(JsValue::from_str(&format!("serialization error: {}", e))),
    };

    enrich_raaml(&mut value);
    etdl_parser::spanned::inject_spans(&mut value, &index);

    Ok(JsValue::from_str(&value.to_string()))
}

/// Return the deepest semantic element containing the given 0-based character
/// offset (or JSON `null` when nothing is found).
#[wasm_bindgen]
pub fn find_span(etdl_content: &str, offset: u32) -> Result<JsValue, JsValue> {
    let index = match etdl_parser::spanned::build_span_index(etdl_content) {
        Ok(index) => index,
        Err(e) => return Err(JsValue::from_str(&e)),
    };

    match index.find_deepest(offset) {
        Some(el) => match serde_json::to_value(el) {
            Ok(v) => Ok(JsValue::from_str(&v.to_string())),
            Err(e) => Err(JsValue::from_str(&format!("serialization error: {}", e))),
        },
        None => Ok(JsValue::NULL),
    }
}

// ---------------------------------------------------------------------------
// Request 4: LSP-style semantic endpoints
// ---------------------------------------------------------------------------

fn json_result(result: Result<Value, String>) -> Result<JsValue, JsValue> {
    match result {
        Ok(v) => Ok(JsValue::from_str(&v.to_string())),
        Err(e) => Err(JsValue::from_str(&e)),
    }
}

#[wasm_bindgen]
pub fn complete(etdl_content: &str, offset: u32) -> Result<JsValue, JsValue> {
    json_result(etdl_parser::semantic::complete(etdl_content, offset))
}

#[wasm_bindgen]
pub fn hover(etdl_content: &str, offset: u32) -> Result<JsValue, JsValue> {
    json_result(etdl_parser::semantic::hover(etdl_content, offset))
}

#[wasm_bindgen]
pub fn goto_definition(etdl_content: &str, offset: u32) -> Result<JsValue, JsValue> {
    json_result(etdl_parser::semantic::goto_definition(etdl_content, offset))
}

#[wasm_bindgen]
pub fn find_references(etdl_content: &str, offset: u32) -> Result<JsValue, JsValue> {
    json_result(etdl_parser::semantic::find_references(etdl_content, offset))
}

#[wasm_bindgen]
pub fn document_symbols(etdl_content: &str) -> Result<JsValue, JsValue> {
    json_result(etdl_parser::semantic::document_symbols(etdl_content))
}

#[wasm_bindgen]
pub fn format(etdl_content: &str) -> Result<JsValue, JsValue> {
    json_result(etdl_parser::semantic::format(etdl_content))
}

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
