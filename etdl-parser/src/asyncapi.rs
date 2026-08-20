use crate::ast::{EtlDocument, ExternalRef, MessageRef};
use crate::jsonptr;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct AsyncApiRegistry {
    documents: BTreeMap<String, AsyncApiDocument>,
}

struct AsyncApiDocument {
    _path: PathBuf,
    root: Value,
}

impl Default for AsyncApiRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncApiRegistry {
    pub fn new() -> Self {
        AsyncApiRegistry {
            documents: BTreeMap::new(),
        }
    }

    pub fn load(&mut self, alias: &str, location: &str, base_dir: &Path) -> Result<(), String> {
        let resolved_path = resolve_location(location, base_dir)?;
        let content = fs::read_to_string(&resolved_path).map_err(|e| {
            format!(
                "cannot read AsyncAPI doc '{}': {}",
                resolved_path.display(),
                e
            )
        })?;

        let root: Value = if resolved_path.extension().is_some_and(|ext| ext == "json") {
            serde_json::from_str(&content).map_err(|e| {
                format!(
                    "invalid JSON in AsyncAPI doc '{}': {}",
                    resolved_path.display(),
                    e
                )
            })?
        } else {
            serde_yaml::from_str(&content).map_err(|e| {
                format!(
                    "invalid YAML in AsyncAPI doc '{}': {}",
                    resolved_path.display(),
                    e
                )
            })?
        };

        self.documents.insert(
            alias.to_string(),
            AsyncApiDocument {
                _path: resolved_path,
                root,
            },
        );
        Ok(())
    }

    pub fn load_from_content(
        &mut self,
        alias: &str,
        content: &str,
        is_json: bool,
    ) -> Result<(), String> {
        let root: Value = if is_json {
            serde_json::from_str(content)
                .map_err(|e| format!("invalid JSON in AsyncAPI doc '{}': {}", alias, e))?
        } else {
            serde_yaml::from_str(content)
                .map_err(|e| format!("invalid YAML in AsyncAPI doc '{}': {}", alias, e))?
        };

        self.documents.insert(
            alias.to_string(),
            AsyncApiDocument {
                _path: PathBuf::from(alias),
                root,
            },
        );
        Ok(())
    }

    pub fn resolve(&self, ext_ref: &ExternalRef) -> Result<&Value, String> {
        let doc = self.documents.get(&ext_ref.alias).ok_or_else(|| {
            format!(
                "import alias '{}' not found in loaded AsyncAPI documents",
                ext_ref.alias
            )
        })?;

        let pointer = &ext_ref.pointer;
        jsonptr::resolve_json_pointer(&doc.root, pointer).ok_or_else(|| {
            format!(
                "JSON Pointer '{}' does not resolve in AsyncAPI document '{}'",
                pointer, ext_ref.alias
            )
        })
    }

    pub fn resolve_ref(&self, alias: &str, pointer: &str) -> Result<&Value, String> {
        let doc = self.documents.get(alias).ok_or_else(|| {
            format!(
                "import alias '{}' not found in loaded AsyncAPI documents",
                alias
            )
        })?;

        jsonptr::resolve_json_pointer(&doc.root, pointer).ok_or_else(|| {
            format!(
                "JSON Pointer '{}' does not resolve in AsyncAPI document '{}'",
                pointer, alias
            )
        })
    }

    pub fn get_schema_for_path(
        &self,
        ext_ref: &ExternalRef,
        path_segments: &[crate::ecel::PathSegment],
    ) -> Result<Option<Value>, String> {
        let message_value = self.resolve(ext_ref)?;

        let payload_schema = message_value
            .get("payload")
            .or_else(|| message_value.get("schema"));

        let Some(schema) = payload_schema else {
            return Ok(None);
        };

        let resolved = resolve_schema_path(schema, path_segments);
        Ok(resolved)
    }

    /// Resolve a Message Reference (Section 5.3.4) to its AsyncAPI Message
    /// Object-shaped value, regardless of whether it's an External
    /// Reference (delegates to `resolve`) or an Internal Reference
    /// (`#/components/messages/<id>`, looked up on `doc` and re-shaped into
    /// the same `{name, payload, headers}` envelope an External Reference
    /// would already carry, so callers don't need to branch on the
    /// reference kind).
    pub fn resolve_message<'a>(
        &'a self,
        doc: &EtlDocument,
        msg_ref: &MessageRef,
    ) -> Result<Cow<'a, Value>, String> {
        match msg_ref {
            MessageRef::External(ext_ref) => self.resolve(ext_ref).map(Cow::Borrowed),
            MessageRef::Internal(int_ref) => {
                let id = internal_message_id(&int_ref.pointer).ok_or_else(|| {
                    format!(
                        "internal reference '{}' is not of the form #/components/messages/<id>",
                        int_ref.pointer
                    )
                })?;
                let message = doc
                    .components
                    .as_ref()
                    .and_then(|c| c.messages.as_ref())
                    .and_then(|m| m.get(id))
                    .ok_or_else(|| {
                        format!(
                            "internal reference '{}' does not resolve: no components.messages.{}",
                            int_ref.pointer, id
                        )
                    })?;
                let value = serde_json::to_value(message).map_err(|e| {
                    format!("cannot convert inline message '{}' to JSON: {}", id, e)
                })?;
                Ok(Cow::Owned(value))
            }
        }
    }

    /// Like `get_schema_for_path`, but accepts either kind of Message
    /// Reference (see `resolve_message`).
    pub fn get_schema_for_message_ref(
        &self,
        doc: &EtlDocument,
        msg_ref: &MessageRef,
        path_segments: &[crate::ecel::PathSegment],
    ) -> Result<Option<Value>, String> {
        let message_value = self.resolve_message(doc, msg_ref)?;

        let payload_schema = message_value
            .get("payload")
            .or_else(|| message_value.get("schema"));

        let Some(schema) = payload_schema else {
            return Ok(None);
        };

        Ok(resolve_schema_path(schema, path_segments))
    }
}

/// Extracts `<id>` from an internal message-reference pointer of the form
/// `#/components/messages/<id>`, or `None` if the pointer doesn't match
/// that shape (e.g. it's a fault-tree `probabilitySource` pointer instead).
fn internal_message_id(pointer: &str) -> Option<&str> {
    let id = pointer.strip_prefix("#/components/messages/")?;
    if id.is_empty() || id.contains('/') {
        None
    } else {
        Some(id)
    }
}

fn resolve_schema_path(schema: &Value, segments: &[crate::ecel::PathSegment]) -> Option<Value> {
    if segments.is_empty() {
        return Some(schema.clone());
    }

    let first = &segments[0];

    match first {
        crate::ecel::PathSegment::Field(name) => {
            // `message` and `payload` are root markers, not real fields:
            // `get_schema_for_path` (the only caller) already unwraps the
            // message envelope down to its payload schema before calling
            // this function, so `schema` here already *is* what
            // `message.payload` denotes. Without stripping `payload` too,
            // every `message.payload.<field>` path (the standard ECEL
            // root — see `docs/reference/cli.md`/§6.3) tried to resolve a
            // literal field named `payload` inside the payload schema,
            // which essentially never exists, so this always returned
            // `None` -> the caller treated the type as `Unknown` ->
            // V-204 type-checking silently never fired for any path
            // operand. `message.headers.<field>` is not fixed by this —
            // header schema introspection is a separate, already-known,
            // documented gap (`docs/SPEC_IMPLEMENTATION_MATRIX.md` §6.3).
            if (name == "message" || name == "payload") && segments.len() > 1 {
                return resolve_schema_path(schema, &segments[1..]);
            }

            let field_schema = resolve_field(schema, name)?;
            if segments.len() == 1 {
                Some(field_schema.clone())
            } else {
                resolve_schema_path(&field_schema, &segments[1..])
            }
        }
        crate::ecel::PathSegment::Wildcard => {
            let items_schema = resolve_array_items(schema)?;
            if segments.len() == 1 {
                Some(items_schema.clone())
            } else {
                resolve_schema_path(&items_schema, &segments[1..])
            }
        }
        crate::ecel::PathSegment::Index(_) => {
            let items_schema = resolve_array_items(schema)?;
            if segments.len() == 1 {
                Some(items_schema.clone())
            } else {
                resolve_schema_path(&items_schema, &segments[1..])
            }
        }
        crate::ecel::PathSegment::QuotedKey(name) => {
            let field_schema = resolve_field(schema, name)?;
            if segments.len() == 1 {
                Some(field_schema.clone())
            } else {
                resolve_schema_path(&field_schema, &segments[1..])
            }
        }
    }
}

fn resolve_field(schema: &Value, name: &str) -> Option<Value> {
    if let Some(properties) = schema.get("properties") {
        if let Some(field) = properties.get(name) {
            return Some(field.clone());
        }
    }

    if let Some(obj) = schema.as_object() {
        if let Some(field) = obj.get(name) {
            return Some(field.clone());
        }
    }

    None
}

fn resolve_array_items(schema: &Value) -> Option<Value> {
    if let Some(items) = schema.get("items") {
        return Some(items.clone());
    }

    if let Some(type_val) = schema.get("type") {
        if type_val.as_str() == Some("array") {
            if let Some(items) = schema.get("items") {
                return Some(items.clone());
            }
        }
    }

    None
}

fn resolve_location(location: &str, base_dir: &Path) -> Result<PathBuf, String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Err(format!(
            "remote AsyncAPI imports not supported in this version: '{}'",
            location
        ));
    }

    let path = Path::new(location);

    if path.is_absolute() {
        // Absolute paths are allowed as-is (caller-provided and trusted).
        return Ok(path.to_path_buf());
    }

    // Reject `..` escapes outside the project root (ETDL §12: local imports
    // MUST NOT escape the project root).
    if location.split('/').any(|seg| seg == "..") {
        return Err(format!(
            "AsyncAPI import '{}' must not contain '..' (path traversal outside the project root is forbidden)",
            location
        ));
    }

    Ok(base_dir.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rejects_path_traversal() {
        let base = Path::new("/proj");
        assert!(resolve_location("../../etc/passwd", base).is_err());
        assert!(resolve_location("./../secret.yaml", base).is_err());
        assert!(resolve_location("a/../b.yaml", base).is_err());
    }

    #[test]
    fn accepts_local_and_absolute() {
        let base = Path::new("/proj");
        assert_eq!(
            resolve_location("api.yaml", base).unwrap(),
            Path::new("/proj/api.yaml")
        );
        assert_eq!(
            resolve_location("/etc/api.yaml", base).unwrap(),
            Path::new("/etc/api.yaml")
        );
    }

    #[test]
    fn rejects_remote() {
        let base = Path::new("/proj");
        assert!(resolve_location("https://example.com/api.yaml", base).is_err());
    }

    #[test]
    fn load_from_content_roundtrip() {
        let mut registry = AsyncApiRegistry::new();
        let yaml =
            "asyncapi: '3.0.0'\ninfo:\n  title: t\n  version: '1'\nchannels: {}\ncomponents: {}\n";
        registry.load_from_content("api", yaml, false).unwrap();
        let ext = ExternalRef {
            alias: "api".to_string(),
            pointer: "/info/title".to_string(),
        };
        assert_eq!(registry.resolve(&ext).unwrap(), &serde_json::json!("t"));
    }
}
