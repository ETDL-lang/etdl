pub fn resolve_json_pointer<'a>(
    root: &'a serde_json::Value,
    pointer: &str,
) -> Option<&'a serde_json::Value> {
    if pointer.is_empty() || pointer == "#" {
        return Some(root);
    }

    let path = if pointer.starts_with("#/") {
        &pointer[2..]
    } else if pointer.starts_with('/') {
        &pointer[1..]
    } else {
        return None;
    };

    if path.is_empty() {
        return Some(root);
    }

    let segments = parse_pointer_segments(path);
    let mut current = root;

    for segment in segments {
        match current {
            serde_json::Value::Object(map) => {
                current = map.get(&segment)?;
            }
            serde_json::Value::Array(arr) => {
                let index: usize = segment.parse().ok()?;
                current = arr.get(index)?;
            }
            _ => return None,
        }
    }

    Some(current)
}

fn parse_pointer_segments(path: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars();

    while let Some(c) = chars.next() {
        if c == '/' {
            segments.push(std::mem::take(&mut current));
        } else if c == '~' {
            match chars.next() {
                Some('0') => current.push('~'),
                Some('1') => current.push('/'),
                _ => {}
            }
        } else {
            current.push(c);
        }
    }
    segments.push(current);

    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_resolve_root() {
        let doc = json!({"a": 1});
        assert_eq!(resolve_json_pointer(&doc, ""), Some(&doc));
        assert_eq!(resolve_json_pointer(&doc, "#"), Some(&doc));
        assert_eq!(resolve_json_pointer(&doc, "#/"), Some(&doc));
    }

    #[test]
    fn test_resolve_nested() {
        let doc = json!({
            "components": {
                "messages": {
                    "OrderPlaced": {
                        "payload": {"type": "object"}
                    }
                }
            }
        });

        let resolved = resolve_json_pointer(&doc, "#/components/messages/OrderPlaced");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), &json!({"payload": {"type": "object"}}));
    }

    #[test]
    fn test_resolve_array_index() {
        let doc = json!({
            "items": [{"name": "a"}, {"name": "b"}]
        });
        let resolved = resolve_json_pointer(&doc, "#/items/1/name");
        assert_eq!(resolved, Some(&json!("b")));
    }

    #[test]
    fn test_escape_sequences() {
        let doc = json!({
            "a~b/c": "value"
        });
        let resolved = resolve_json_pointer(&doc, "#/a~0b~1c");
        assert_eq!(resolved, Some(&json!("value")));
    }
}
