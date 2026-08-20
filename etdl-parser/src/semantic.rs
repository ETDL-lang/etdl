//! LSP-style semantic endpoints over an ETDL document (completions, hover,
//! go-to-definition, find-references, document symbols, formatting).
//!
//! These are structural services built on the typed AST
//! ([`crate::parse_document`]) and the position index
//! ([`crate::spanned::SpanIndex`]). Line/column numbers are 0-based; offsets are
//! character offsets, matching LSP conventions.

use serde_json::{json, Value};

use crate::ast::{BasicEventType, EtlDocument, Node};
use crate::spanned::{
    parse_document_with_spans, ElementKind, IndexedElement, PathKey, PathPart, Span, SpanIndex,
    SpanKey,
};

// LSP `SymbolKind` / `CompletionItemKind` constants.
const SYMBOL_NAMESPACE: u32 = 3;
const SYMBOL_OBJECT: u32 = 19;
const SYMBOL_FIELD: u32 = 8;
const SYMBOL_EVENT: u32 = 24;
const SYMBOL_METHOD: u32 = 6;

const COMPLETION_KEYWORD: u32 = 14;
const COMPLETION_FIELD: u32 = 5;
const COMPLETION_REFERENCE: u32 = 18;

/// Convert a character offset to a byte offset (clamped). `content[..byte]`
/// contains exactly `offset` characters.
fn char_to_byte(content: &str, offset: u32) -> usize {
    content
        .char_indices()
        .nth(offset as usize)
        .map(|(i, _)| i)
        .unwrap_or(content.len())
}

fn range(span: &Span) -> Value {
    json!({
        "start": { "line": span.line, "character": span.column },
        "end": { "line": span.end_line, "character": span.end_column }
    })
}

fn location(el: &IndexedElement) -> Value {
    let span = el.key_span.unwrap_or(el.span);
    json!({ "range": range(&span) })
}

// ---------------------------------------------------------------------------
// document_symbols
// ---------------------------------------------------------------------------

pub fn document_symbols(content: &str) -> Result<Value, String> {
    let (doc, index) = parse_document_with_spans(content)?;
    let mut symbols: Vec<Value> = Vec::new();

    if let Some(section_el) = index.resolve(&SpanKey::Section("event_trees")) {
        let mut children = Vec::new();
        for (tree_name, tree) in &doc.event_trees {
            let mut tree_children = Vec::new();
            if let Some(ie) = index.resolve(&SpanKey::InitiatingEvent {
                tree: tree_name.clone(),
                field: "id",
            }) {
                tree_children.push(symbol("initiatingEvent", None, SYMBOL_EVENT, ie, vec![]));
            }
            let mut node_children = Vec::new();
            for (node_id, node) in &tree.nodes {
                let Some(el) = index.resolve(&SpanKey::Node {
                    tree: tree_name.clone(),
                    id: node_id.clone(),
                }) else {
                    continue;
                };
                let detail = match node {
                    Node::Barrier(b) => Some(format!("barrier · {} branch(es)", b.branches.len())),
                    Node::Operation(op) => Some(format!("operation · handler {}", op.handler)),
                    Node::Consequence(c) => {
                        let op = match c.consequence_operation {
                            crate::ast::ConsequenceOperation::Send => "send",
                            crate::ast::ConsequenceOperation::Terminate => "terminate",
                        };
                        Some(format!("consequence · {}", op))
                    }
                };
                node_children.push(symbol(node_id, detail, SYMBOL_OBJECT, el, vec![]));
            }
            let nodes_el = index
                .resolve(&SpanKey::NodeField {
                    tree: tree_name.clone(),
                    id: String::new(),
                    field: "branches",
                })
                .or_else(|| {
                    index.resolve(&SpanKey::Tree {
                        tree: tree_name.clone(),
                    })
                });
            if !node_children.is_empty() {
                let group_el = nodes_el.unwrap_or(section_el);
                tree_children.push(symbol("nodes", None, SYMBOL_FIELD, group_el, node_children));
            }
            let tree_el = index
                .resolve(&SpanKey::Tree {
                    tree: tree_name.clone(),
                })
                .unwrap_or(section_el);
            children.push(symbol(
                tree_name,
                None,
                SYMBOL_NAMESPACE,
                tree_el,
                tree_children,
            ));
        }
        symbols.push(symbol(
            "eventTrees",
            None,
            SYMBOL_NAMESPACE,
            section_el,
            children,
        ));
    }

    if let Some(section_el) = index.resolve(&SpanKey::Section("fault_trees")) {
        let mut children = Vec::new();
        if let Some(ftrees) = &doc.fault_trees {
            for (ft_name, ft) in ftrees {
                let mut ft_children = Vec::new();
                if let Some(te) = index.resolve(&SpanKey::TopEvent {
                    tree: ft_name.clone(),
                    field: "id",
                }) {
                    ft_children.push(symbol("topEvent", None, SYMBOL_EVENT, te, vec![]));
                }
                let mut gate_children = Vec::new();
                if let Some(gates) = &ft.gates {
                    for (gate_id, gate) in gates {
                        let Some(el) = index.resolve(&SpanKey::Gate {
                            tree: ft_name.clone(),
                            id: gate_id.clone(),
                        }) else {
                            continue;
                        };
                        let detail =
                            format!("{:?} gate · {} input(s)", gate.gate_type, gate.inputs.len());
                        gate_children.push(symbol(
                            gate_id,
                            Some(detail),
                            SYMBOL_METHOD,
                            el,
                            vec![],
                        ));
                    }
                }
                if !gate_children.is_empty() {
                    let group_el = index
                        .resolve(&SpanKey::Gate {
                            tree: ft_name.clone(),
                            id: String::new(),
                        })
                        .or_else(|| {
                            index.resolve(&SpanKey::FaultTree {
                                tree: ft_name.clone(),
                            })
                        });
                    let group_el = group_el.unwrap_or(section_el);
                    ft_children.push(symbol("gates", None, SYMBOL_FIELD, group_el, gate_children));
                }
                let mut be_children = Vec::new();
                for (be_id, be) in &ft.basic_events {
                    let Some(el) = index.resolve(&SpanKey::BasicEvent {
                        tree: ft_name.clone(),
                        id: be_id.clone(),
                    }) else {
                        continue;
                    };
                    let detail = match be.event_type {
                        Some(BasicEventType::House) => "house event".to_string(),
                        Some(BasicEventType::Undeveloped) => "undeveloped event".to_string(),
                        Some(BasicEventType::Conditional) => "conditional event".to_string(),
                        _ => "basic event".to_string(),
                    };
                    be_children.push(symbol(be_id, Some(detail), SYMBOL_EVENT, el, vec![]));
                }
                if !be_children.is_empty() {
                    let group_el = index
                        .resolve(&SpanKey::BasicEvent {
                            tree: ft_name.clone(),
                            id: String::new(),
                        })
                        .or_else(|| {
                            index.resolve(&SpanKey::FaultTree {
                                tree: ft_name.clone(),
                            })
                        });
                    let group_el = group_el.unwrap_or(section_el);
                    ft_children.push(symbol(
                        "basicEvents",
                        None,
                        SYMBOL_FIELD,
                        group_el,
                        be_children,
                    ));
                }
                let ft_el = index
                    .resolve(&SpanKey::FaultTree {
                        tree: ft_name.clone(),
                    })
                    .unwrap_or(section_el);
                children.push(symbol(ft_name, None, SYMBOL_NAMESPACE, ft_el, ft_children));
            }
        }
        symbols.push(symbol(
            "faultTrees",
            None,
            SYMBOL_NAMESPACE,
            section_el,
            children,
        ));
    }

    Ok(json!({
        "symbols": symbols
    }))
}

fn symbol(
    name: &str,
    detail: Option<String>,
    kind: u32,
    el: &IndexedElement,
    children: Vec<Value>,
) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("name".to_string(), Value::String(name.to_string()));
    if let Some(d) = detail {
        obj.insert("detail".to_string(), Value::String(d));
    }
    obj.insert("kind".to_string(), Value::from(kind));
    obj.insert("range".to_string(), range(&el.span));
    let selection = el.key_span.unwrap_or(el.span);
    obj.insert("selectionRange".to_string(), range(&selection));
    if !children.is_empty() {
        obj.insert("children".to_string(), Value::Array(children));
    }
    Value::Object(obj)
}

// ---------------------------------------------------------------------------
// hover
// ---------------------------------------------------------------------------

pub fn hover(content: &str, offset: u32) -> Result<Value, String> {
    let (doc, index) = parse_document_with_spans(content)?;
    let Some(el) = index.find_deepest(offset) else {
        return Ok(json!(null));
    };
    let text = hover_text(&doc, &index, el);
    let span = el.key_span.unwrap_or(el.span);
    Ok(json!({
        "contents": { "kind": "markdown", "value": text },
        "range": range(&span)
    }))
}

fn hover_text(doc: &EtlDocument, index: &SpanIndex, el: &IndexedElement) -> String {
    let tree = el.tree.as_deref().unwrap_or("");
    match el.kind {
        ElementKind::Reference => {
            let field = el.field.as_deref().unwrap_or("");
            if matches!(field, "message" | "emits" | "channel") {
                format!("**AsyncAPI reference** `{}`\n\nField: `{}`", el.name, field)
            } else {
                let mut text = format!("**References** `{}`", el.name);
                if let Some(def) = index.definition(tree, &el.name) {
                    let span = def.key_span.unwrap_or(def.span);
                    text.push_str(&format!("\n\nDefined at line {}", span.line + 1));
                }
                text
            }
        }
        ElementKind::Definition => {
            if let Some((detail, description)) = definition_detail(doc, el) {
                let mut text = format!("**{}**", el.name);
                if !detail.is_empty() {
                    text.push_str(&format!(" — {}", detail));
                }
                if let Some(d) = description {
                    if !d.is_empty() {
                        text.push_str(&format!("\n\n{}", d));
                    }
                }
                text
            } else {
                format!("**{}**", el.name)
            }
        }
        ElementKind::Field => {
            let field = el.field.as_deref().unwrap_or("");
            if !el.name.is_empty() {
                format!("`{}` = `{}`", field, el.name)
            } else {
                format!("`{}`", field)
            }
        }
        ElementKind::Section => format!("**Section** `{}`", el.name),
    }
}

/// Best-effort human description for a definition element.
fn definition_detail(doc: &EtlDocument, el: &IndexedElement) -> Option<(String, Option<String>)> {
    let tree = el.tree.as_deref().unwrap_or("");
    if let Some(node) = doc
        .event_trees
        .get(tree)
        .and_then(|t| t.nodes.get(&el.name))
    {
        match node {
            Node::Barrier(b) => Some((
                format!("barrier · {} branch(es)", b.branches.len()),
                b.description.clone(),
            )),
            Node::Operation(op) => Some((
                format!("operation · handler `{}`", op.handler),
                op.description.clone(),
            )),
            Node::Consequence(c) => {
                let op = match c.consequence_operation {
                    crate::ast::ConsequenceOperation::Send => "send",
                    crate::ast::ConsequenceOperation::Terminate => "terminate",
                };
                Some((format!("consequence · `{}`", op), c.description.clone()))
            }
        }
    } else if doc.event_trees.contains_key(tree) && el.field.is_none() {
        Some((
            "event tree".to_string(),
            doc.event_trees[tree].description.clone(),
        ))
    } else if let Some(ft) = doc.fault_trees.as_ref().and_then(|f| f.get(tree)) {
        if let Some(gate) = ft.gates.as_ref().and_then(|g| g.get(&el.name)) {
            Some((
                format!("{:?} gate", gate.gate_type),
                gate.description.clone(),
            ))
        } else if let Some(be) = ft.basic_events.get(&el.name) {
            let detail = if be.failure_rate.is_some() {
                "basic event (failure rate model)".to_string()
            } else {
                "basic event".to_string()
            };
            Some((detail, Some(be.description.clone())))
        } else if el.name == tree {
            Some(("fault tree".to_string(), ft.description.clone()))
        } else {
            None
        }
    } else if el.name == tree {
        Some((
            "event tree".to_string(),
            doc.event_trees
                .get(tree)
                .and_then(|t| t.description.clone()),
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// goto_definition / find_references
// ---------------------------------------------------------------------------

pub fn goto_definition(content: &str, offset: u32) -> Result<Value, String> {
    let (_doc, index) = parse_document_with_spans(content)?;
    let Some(el) = index.find_deepest(offset) else {
        return Ok(json!(null));
    };
    match el.kind {
        ElementKind::Reference => {
            let field = el.field.as_deref().unwrap_or("");
            let tree = el.tree.as_deref().unwrap_or("");
            if matches!(field, "message" | "emits" | "channel") {
                return Ok(json!(null)); // AsyncAPI refs resolve in another document
            }
            // References that point at a fault tree via an internal pointer.
            if matches!(
                field,
                "on_failure_probability_source" | "probability_source"
            ) {
                let ft_id = el
                    .name
                    .trim_start_matches("#/faultTrees/")
                    .split('/')
                    .next()
                    .unwrap_or_default();
                if let Some(def) = index.definition(ft_id, ft_id) {
                    return Ok(location(def));
                }
                return Ok(json!(null));
            }
            if let Some(def) = index.definition(tree, &el.name) {
                return Ok(location(def));
            }
            Ok(json!(null))
        }
        ElementKind::Definition => Ok(location(el)),
        _ => Ok(json!(null)),
    }
}

pub fn find_references(content: &str, offset: u32) -> Result<Value, String> {
    let (_doc, index) = parse_document_with_spans(content)?;
    let Some(el) = index.find_deepest(offset) else {
        return Ok(json!([]));
    };
    let (tree, id) = match el.kind {
        ElementKind::Reference | ElementKind::Definition => (
            el.tree.as_deref().unwrap_or("").to_string(),
            el.name.clone(),
        ),
        _ => return Ok(json!([])),
    };
    let locations: Vec<Value> = index
        .by_identity(&tree, &id)
        .iter()
        .map(|r| location(r))
        .collect();
    Ok(Value::Array(locations))
}

// ---------------------------------------------------------------------------
// complete
// ---------------------------------------------------------------------------

pub fn complete(content: &str, offset: u32) -> Result<Value, String> {
    let (doc, index) = parse_document_with_spans(content)?;
    let items = completion_items(&doc, &index, content, offset);
    Ok(json!({ "isIncomplete": false, "items": items }))
}

fn completion_items(
    doc: &EtlDocument,
    index: &SpanIndex,
    content: &str,
    offset: u32,
) -> Vec<Value> {
    let byte = char_to_byte(content, offset);
    let line_start = content[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &content[line_start..byte];

    let is_value_position = line.contains(':');
    let field = if is_value_position {
        line.split(':').next().unwrap_or("").trim().to_string()
    } else {
        String::new()
    };
    let prefix = if is_value_position {
        line.split(':').nth(1).unwrap_or("").trim()
    } else {
        line.trim()
    };

    let map_path = enclosing_map_path(index, offset);
    let mut items: Vec<Value> = Vec::new();

    if is_value_position {
        for (label, kind, detail) in value_completions(doc, &map_path, &field) {
            if label.starts_with(prefix) {
                items.push(completion_item(&label, kind, detail));
            }
        }
    } else if let Some(path) = map_path {
        for (label, kind) in key_completions(doc, &path) {
            if label.starts_with(prefix) {
                items.push(completion_item(&label, kind, None));
            }
        }
    }

    items
}

fn enclosing_map_path(index: &SpanIndex, offset: u32) -> Option<PathKey> {
    let el = index.find_deepest(offset)?;
    match el.kind {
        ElementKind::Definition => Some(el.path.clone()),
        _ => {
            let mut p = el.path.clone();
            p.pop();
            Some(p)
        }
    }
}

fn completion_item(label: &str, kind: u32, detail: Option<String>) -> Value {
    json!({ "label": label, "kind": kind, "detail": detail })
}

/// Allowed keys for a map at the given path, with LSP completion kinds.
fn key_completions(doc: &EtlDocument, path: &PathKey) -> Vec<(String, u32)> {
    let last = path.last().map(|p| match p {
        PathPart::Key(k) => k.as_str(),
        PathPart::Index(_) => "",
    });

    let field = |s: &'static str| (s.to_string(), COMPLETION_FIELD);
    let node = |s: &'static str| (s.to_string(), COMPLETION_KEYWORD);

    match last {
        None => vec![
            node("etdl"),
            node("info"),
            node("asyncapi_imports"),
            node("components"),
            node("eventTrees"),
            node("faultTrees"),
        ],
        Some("info") => vec![
            field("title"),
            field("version"),
            field("domain"),
            field("description"),
        ],
        Some("asyncapi_imports") => vec![],
        Some("event_trees") => vec![],
        Some("fault_trees") => vec![],
        Some("initiating_event") => vec![field("id"), field("message"), field("next")],
        Some("top_event") => vec![
            field("id"),
            field("description"),
            field("message"),
            field("rootCause"),
        ],
        Some("nodes") => vec![],
        Some("gates") => vec![],
        Some("basic_events") => vec![],
        Some("transfers") => vec![],
        Some("branches") => vec![
            field("outcome"),
            field("condition"),
            field("probability"),
            field("probabilityOfSuccess"),
            field("probabilityOfFailure"),
            field("probabilitySource"),
            field("next"),
            field("description"),
        ],
        Some("components") => vec![
            node("barriers"),
            node("operations"),
            node("gates"),
            node("basicEvents"),
        ],
        _ => {
            // Path-based node/gate/basic-event field maps.
            if let Some((section, tree, sub)) = tree_sub_context(path) {
                match sub {
                    TreeSub::Node(id) => node_field_keys(doc, section, &tree, &id),
                    TreeSub::Gate => vec![
                        node("type"),
                        field("inputs"),
                        field("k"),
                        field("inhibitCondition"),
                        field("description"),
                    ],
                    TreeSub::BasicEvent => vec![
                        field("description"),
                        field("probability"),
                        field("failureRate"),
                        field("missionTime"),
                        field("undeveloped"),
                        field("eventType"),
                        field("message"),
                    ],
                    TreeSub::None => {
                        if section == "event_trees" {
                            vec![node("initiatingEvent"), node("nodes"), field("description")]
                        } else {
                            vec![
                                node("topEvent"),
                                node("gates"),
                                node("basicEvents"),
                                node("transfers"),
                                field("description"),
                            ]
                        }
                    }
                }
            } else {
                vec![]
            }
        }
    }
    .into_iter()
    .collect()
}

enum TreeSub {
    None,
    Node(String),
    Gate,
    BasicEvent,
}

/// Interpret a path as `section > tree > sub (node/gate/basic-event id)`.
fn tree_sub_context(path: &PathKey) -> Option<(&str, String, TreeSub)> {
    let section_idx = path
        .iter()
        .position(|p| matches!(p, PathPart::Key(k) if k == "event_trees" || k == "fault_trees"))?;
    let PathPart::Key(section) = &path[section_idx] else {
        return None;
    };
    let PathPart::Key(tree) = &path[section_idx + 1] else {
        return None;
    };

    let after = &path[section_idx + 2..];
    match after {
        [] => Some((section, tree.clone(), TreeSub::None)),
        [PathPart::Key(k)] => match k.as_str() {
            "nodes" => Some((section, tree.clone(), TreeSub::Node(String::new()))),
            "gates" => Some((section, tree.clone(), TreeSub::Gate)),
            "basic_events" => Some((section, tree.clone(), TreeSub::BasicEvent)),
            _ => Some((section, tree.clone(), TreeSub::None)),
        },
        [PathPart::Key(k), PathPart::Key(id)] => match k.as_str() {
            "nodes" => Some((section, tree.clone(), TreeSub::Node(id.clone()))),
            "gates" => Some((section, tree.clone(), TreeSub::Gate)),
            "basic_events" => Some((section, tree.clone(), TreeSub::BasicEvent)),
            _ => Some((section, tree.clone(), TreeSub::None)),
        },
        _ => Some((section, tree.clone(), TreeSub::None)),
    }
}

fn node_field_keys(doc: &EtlDocument, section: &str, tree: &str, id: &str) -> Vec<(String, u32)> {
    let field = |s: &'static str| (s.to_string(), COMPLETION_FIELD);
    let kw = |s: &'static str| (s.to_string(), COMPLETION_KEYWORD);
    let tree_map = if section == "event_trees" {
        doc.event_trees.get(tree)
    } else {
        None
    };
    if let Some(node) = tree_map.and_then(|t| t.nodes.get(id)) {
        match node {
            Node::Barrier(_) => vec![kw("type"), kw("branches"), field("description")],
            Node::Operation(_) => vec![
                kw("type"),
                kw("action"),
                field("handler"),
                field("emits"),
                field("next"),
                field("onFailure"),
                field("onFailureProbabilitySource"),
                kw("retryPolicy"),
                field("timeoutMs"),
                field("description"),
            ],
            Node::Consequence(_) => vec![
                kw("type"),
                kw("operation"),
                field("channel"),
                field("message"),
                field("description"),
            ],
        }
    } else if id.is_empty() {
        // Just entered a new node; suggest `type` first.
        vec![kw("type")]
    } else {
        vec![]
    }
}

/// Value completions for a field within a given map path.
fn value_completions(
    doc: &EtlDocument,
    map_path: &Option<PathKey>,
    field: &str,
) -> Vec<(String, u32, Option<String>)> {
    let mut out = Vec::new();
    let Some(path) = map_path else { return out };

    let tree = tree_from_path(path);
    let section = tree.as_ref().map(|(s, _)| *s).unwrap_or("");

    let mut push = |label: String, detail: Option<String>| {
        out.push((label, COMPLETION_REFERENCE, detail));
    };

    match field {
        "type" => {
            if section == "fault_trees" {
                for t in [
                    "AND",
                    "OR",
                    "NOT",
                    "XOR",
                    "VOTING",
                    "INHIBIT",
                    "PRIORITY_AND",
                ] {
                    push(t.to_string(), Some("gate type".to_string()));
                }
            } else {
                for t in ["barrier", "operation", "consequence"] {
                    push(t.to_string(), Some("node type".to_string()));
                }
            }
        }
        "operation" => {
            for t in ["send", "terminate"] {
                push(t.to_string(), Some("consequence operation".to_string()));
            }
        }
        "action" => push("execute".to_string(), Some("operation action".to_string())),
        "eventType" => {
            for t in ["basic", "house", "undeveloped", "conditional"] {
                push(t.to_string(), Some("basic event type".to_string()));
            }
        }
        "backoffStrategy" => {
            for t in ["fixed", "exponential"] {
                push(t.to_string(), Some("backoff strategy".to_string()));
            }
        }
        "condition" => push(
            "default".to_string(),
            Some("default branch condition".to_string()),
        ),
        "next" | "onFailure" => {
            if let Some((_, tree_name)) = tree {
                if let Some(t) = doc.event_trees.get(&tree_name) {
                    for nid in t.nodes.keys() {
                        push(nid.clone(), Some("node".to_string()));
                    }
                }
            }
        }
        "inputs" | "rootCause" => {
            if let Some((_, tree_name)) = tree {
                if let Some(ft) = doc.fault_trees.as_ref().and_then(|f| f.get(&tree_name)) {
                    if let Some(gates) = &ft.gates {
                        for gid in gates.keys() {
                            push(gid.clone(), Some("gate".to_string()));
                        }
                    }
                    for be_id in ft.basic_events.keys() {
                        push(be_id.clone(), Some("basic event".to_string()));
                    }
                }
            }
        }
        "message" | "emits" | "channel" => {
            for alias in doc.asyncapi_imports.keys() {
                push(
                    format!("{}#/", alias),
                    Some(format!("import alias `{}`", alias)),
                );
            }
        }
        "onFailureProbabilitySource" | "probabilitySource" => {
            if let Some(ftrees) = &doc.fault_trees {
                for ft_id in ftrees.keys() {
                    push(
                        format!("#/faultTrees/{}/topEvent", ft_id),
                        Some("fault tree top event".to_string()),
                    );
                }
            }
        }
        "target" => {
            if let Some(ftrees) = &doc.fault_trees {
                for ft_id in ftrees.keys() {
                    push(
                        format!("#/faultTrees/{}/topEvent", ft_id),
                        Some("fault tree top event".to_string()),
                    );
                }
            }
        }
        _ => {}
    }
    out
}

fn tree_from_path(path: &PathKey) -> Option<(&str, String)> {
    for (i, p) in path.iter().enumerate() {
        if let PathPart::Key(k) = p {
            if k == "event_trees" || k == "fault_trees" {
                if let Some(PathPart::Key(t)) = path.get(i + 1) {
                    return Some((k.as_str(), t.clone()));
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// format
// ---------------------------------------------------------------------------

pub fn format(content: &str) -> Result<Value, String> {
    use saphyr::{LoadableYamlNode, Yaml, YamlEmitter};

    let docs = Yaml::load_from_str(content).map_err(|e| e.to_string())?;
    let doc = docs.first().ok_or("empty ETDL document")?;
    let mut out = String::new();
    let mut emitter = YamlEmitter::new(&mut out);
    emitter.dump(doc).map_err(|e| e.to_string())?;
    Ok(json!({ "text": out }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/order-fulfillment.etdl");

    fn char_offset(content: &str, line: usize, col: usize) -> u32 {
        let line_start = content
            .lines()
            .take(line)
            .map(|l| l.len() + 1)
            .sum::<usize>();
        (line_start + col) as u32
    }

    #[test]
    fn goto_definition_follows_references() {
        // "next: FulfillmentConsequence" is 0-based line 39, value at column 15.
        let offset = char_offset(FIXTURE, 39, 15);
        let loc = goto_definition(FIXTURE, offset).unwrap();
        assert!(!loc.is_null(), "expected a definition location");
        let range = &loc["range"];
        assert_eq!(
            range["start"]["line"], 48,
            "target is the node definition line"
        );
        assert_eq!(range["start"]["character"], 6);
    }

    #[test]
    fn goto_definition_null_for_external_ref() {
        // "message" line 16, value column 12.
        let offset = char_offset(FIXTURE, 16, 12);
        let loc = goto_definition(FIXTURE, offset).unwrap();
        assert!(loc.is_null(), "asyncapi refs have no local definition");
    }

    #[test]
    fn find_references_returns_all() {
        // Offset on the FulfillmentConsequence definition name (0-based line 49, col 7).
        let offset = char_offset(FIXTURE, 49, 7);
        let refs = find_references(FIXTURE, offset).unwrap();
        let array = refs.as_array().expect("array of locations");
        assert!(
            array.len() >= 2,
            "definition plus at least one reference: {:?}",
            array
        );
    }

    #[test]
    fn hover_renders_markdown() {
        let offset = char_offset(FIXTURE, 39, 15);
        let hover = hover(FIXTURE, offset).unwrap();
        let value = hover["contents"]["value"].as_str().unwrap();
        assert!(value.contains("FulfillmentConsequence"));
        assert!(hover.get("range").is_some());
    }

    #[test]
    fn document_symbols_has_trees_and_nodes() {
        let symbols = document_symbols(FIXTURE).unwrap();
        let event = &symbols["symbols"][0];
        assert_eq!(event["name"], "eventTrees");
        let tree = &event["children"][0];
        assert_eq!(tree["name"], "OrderFulfillment");
        let names: Vec<&str> = tree["children"][1]["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"InventoryCheckBarrier"));
    }

    #[test]
    fn complete_suggests_node_fields() {
        // Offset after the node name line (0-based line 21, col 30) — key position.
        let offset = char_offset(FIXTURE, 21, 30);
        let result = complete(FIXTURE, offset).unwrap();
        let items = result["items"].as_array().unwrap();
        let labels: Vec<&str> = items.iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert!(labels.contains(&"branches"));
        assert!(labels.contains(&"type"));
    }

    #[test]
    fn complete_suggests_next_values() {
        // Value position for `next:` inside ProcessPaymentOperation (line 39).
        let line = "        next: ";
        let byte_start = FIXTURE.lines().take(39).map(|l| l.len() + 1).sum::<usize>();
        let offset = (byte_start + line.find(':').unwrap() + 1) as u32;
        let result = complete(FIXTURE, offset).unwrap();
        let items = result["items"].as_array().unwrap();
        let labels: Vec<&str> = items.iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert!(labels.contains(&"FulfillmentConsequence"));
        assert!(labels.contains(&"PaymentFailedConsequence"));
    }

    #[test]
    fn format_round_trips() {
        let result = format(FIXTURE).unwrap();
        let text = result["text"].as_str().unwrap();
        assert!(text.contains("eventTrees:"));
        assert!(text.contains("OrderFulfillment:"));
    }
}
