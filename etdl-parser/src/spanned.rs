//! Source-position tracking for ETDL documents.
//!
//! The typed AST produced by [`crate::parse_document`] carries no source
//! positions. This module parses the same document a second time with `saphyr`
//! (a position-aware YAML 1.2 parser) and builds a [`SpanIndex`] that records
//! the location of every semantic element — sections, tree/node/gate/basic-event
//! definitions, fields, and identifier reference value tokens.
//!
//! The index is keyed by the *serde output JSON path* (e.g.
//! `event_trees.OrderFulfillment.nodes.InventoryCheckBarrier.branches[0].next`)
//! so it can be injected directly into the AST serialization produced by
//! [`crate::parse_document`].
//!
//! All line/column numbers are **0-based** (LSP convention). `start`/`end` are
//! **character offsets** into the original document (not UTF-16 code units).

use saphyr::LoadableYamlNode;
use saphyr::MarkedYaml;
use serde::Serialize;
use serde_json::Value;

use crate::ast::EtlDocument;

/// A half-open `[start, end)` span into the source document.
///
/// Offsets are 0-based character offsets; `line`/`column`/`end_line`/`end_column`
/// are 0-based line/column numbers (LSP convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// The kind of a recorded element, matching the `kind` field of `find_span`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ElementKind {
    Section,
    Definition,
    Field,
    Reference,
}

impl ElementKind {
    fn rank(self) -> u8 {
        match self {
            ElementKind::Reference => 4,
            ElementKind::Field => 3,
            ElementKind::Definition => 2,
            ElementKind::Section => 1,
        }
    }
}

/// A component of an index path: either a map key or a sequence index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathPart {
    Key(String),
    Index(usize),
}

/// A path into the serde output JSON (see module docs).
pub type PathKey = Vec<PathPart>;

/// One recorded element in the [`SpanIndex`].
#[derive(Debug, Clone, Serialize)]
pub struct IndexedElement {
    #[serde(rename = "kind")]
    pub kind: ElementKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<String>,
    pub span: Span,
    /// For definitions: the span of the name token itself (used to anchor
    /// go-to-definition and to hit-test the identifier).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_span: Option<Span>,
    /// Structural depth in the document (used for find_span tie-breaking).
    #[serde(skip)]
    pub depth: usize,
    /// The index path of this element (not serialized).
    #[serde(skip)]
    pub path: PathKey,
}

/// A structured locator for a semantic element, used by the validator to attach
/// positions to diagnostics. Field names use the *serde output* naming
/// (e.g. `on_failure`, `root_cause`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanKey {
    Section(&'static str),
    Tree {
        tree: String,
    },
    FaultTree {
        tree: String,
    },
    InitiatingEvent {
        tree: String,
        field: &'static str,
    },
    TopEvent {
        tree: String,
        field: &'static str,
    },
    Node {
        tree: String,
        id: String,
    },
    NodeField {
        tree: String,
        id: String,
        field: &'static str,
    },
    BranchField {
        tree: String,
        id: String,
        branch: usize,
        field: &'static str,
    },
    Gate {
        tree: String,
        id: String,
    },
    GateField {
        tree: String,
        id: String,
        field: &'static str,
    },
    GateInput {
        tree: String,
        id: String,
        idx: usize,
    },
    BasicEvent {
        tree: String,
        id: String,
    },
    BasicEventField {
        tree: String,
        id: String,
        field: &'static str,
    },
    Transfer {
        tree: String,
        id: String,
        field: &'static str,
    },
    ImportAlias {
        alias: String,
    },
}

impl SpanKey {
    fn path(&self) -> PathKey {
        let key = |s: &str| PathPart::Key(s.to_string());
        match self {
            SpanKey::Section(s) => vec![key(s)],
            SpanKey::Tree { tree } => vec![key("event_trees"), key(tree)],
            SpanKey::FaultTree { tree } => vec![key("fault_trees"), key(tree)],
            SpanKey::InitiatingEvent { tree, field } => vec![
                key("event_trees"),
                key(tree),
                key("initiating_event"),
                key(field),
            ],
            SpanKey::TopEvent { tree, field } => {
                vec![key("fault_trees"), key(tree), key("top_event"), key(field)]
            }
            SpanKey::Node { tree, id } => {
                vec![key("event_trees"), key(tree), key("nodes"), key(id)]
            }
            SpanKey::NodeField { tree, id, field } => {
                let mut p = SpanKey::Node {
                    tree: tree.clone(),
                    id: id.clone(),
                }
                .path();
                p.push(key(field));
                p
            }
            SpanKey::BranchField {
                tree,
                id,
                branch,
                field,
            } => {
                let mut p = SpanKey::Node {
                    tree: tree.clone(),
                    id: id.clone(),
                }
                .path();
                p.push(key("branches"));
                p.push(PathPart::Index(*branch));
                p.push(key(field));
                p
            }
            SpanKey::Gate { tree, id } => {
                vec![key("fault_trees"), key(tree), key("gates"), key(id)]
            }
            SpanKey::GateField { tree, id, field } => {
                let mut p = SpanKey::Gate {
                    tree: tree.clone(),
                    id: id.clone(),
                }
                .path();
                p.push(key(field));
                p
            }
            SpanKey::GateInput { tree, id, idx } => {
                let mut p = SpanKey::Gate {
                    tree: tree.clone(),
                    id: id.clone(),
                }
                .path();
                p.push(key("inputs"));
                p.push(PathPart::Index(*idx));
                p
            }
            SpanKey::BasicEvent { tree, id } => {
                vec![key("fault_trees"), key(tree), key("basic_events"), key(id)]
            }
            SpanKey::BasicEventField { tree, id, field } => {
                let mut p = SpanKey::BasicEvent {
                    tree: tree.clone(),
                    id: id.clone(),
                }
                .path();
                p.push(key(field));
                p
            }
            SpanKey::Transfer { tree, id, field } => vec![
                key("fault_trees"),
                key(tree),
                key("transfers"),
                key(id),
                key(field),
            ],
            SpanKey::ImportAlias { alias } => vec![key("asyncapi_imports"), key(alias)],
        }
    }
}

/// A source-position index over an ETDL document.
#[derive(Debug, Default, Clone)]
pub struct SpanIndex {
    pub elements: Vec<IndexedElement>,
    by_path: std::collections::HashMap<PathKey, usize>,
    /// `(tree, id)` -> indices of all definition + reference elements sharing
    /// that identity (used for go-to-definition / find-references).
    by_identity: std::collections::HashMap<(String, String), Vec<usize>>,
}

impl SpanIndex {
    /// Resolve a [`SpanKey`] to the recorded element.
    pub fn resolve(&self, key: &SpanKey) -> Option<&IndexedElement> {
        self.by_path.get(&key.path()).map(|&i| &self.elements[i])
    }

    /// Return the deepest element whose span (or name-token span) contains the
    /// given 0-based character offset. "Deepest" = the smallest containing span,
    /// preferring reference > field > definition > section on ties.
    pub fn find_deepest(&self, offset: u32) -> Option<&IndexedElement> {
        let mut best: Option<&IndexedElement> = None;
        let mut best_size: u64 = u64::MAX;
        let mut best_rank: u8 = 0;
        let mut best_depth: usize = 0;
        for el in &self.elements {
            let mut size: Option<u64> = None;
            if el.span.start <= offset && offset < el.span.end {
                size = Some((el.span.end - el.span.start) as u64);
            }
            if let Some(ks) = &el.key_span {
                if ks.start <= offset && offset < ks.end {
                    let s = (ks.end - ks.start) as u64;
                    if size.is_none_or(|cur| s < cur) {
                        size = Some(s);
                    }
                }
            }
            if let Some(size) = size {
                let better = size < best_size
                    || (size == best_size
                        && (el.kind.rank() > best_rank
                            || (el.kind.rank() == best_rank && el.depth > best_depth)));
                if better {
                    best = Some(el);
                    best_size = size;
                    best_rank = el.kind.rank();
                    best_depth = el.depth;
                }
            }
        }
        best
    }

    /// All elements sharing the given identity `(tree, id)`.
    pub fn by_identity(&self, tree: &str, id: &str) -> Vec<&IndexedElement> {
        self.by_identity
            .get(&(tree.to_string(), id.to_string()))
            .map(|v| v.iter().map(|&i| &self.elements[i]).collect())
            .unwrap_or_default()
    }

    /// The single definition element for `(tree, id)`, if any.
    pub fn definition(&self, tree: &str, id: &str) -> Option<&IndexedElement> {
        self.by_identity
            .get(&(tree.to_string(), id.to_string()))
            .and_then(|v| {
                v.iter()
                    .find(|&&i| self.elements[i].kind == ElementKind::Definition)
                    .map(|&i| &self.elements[i])
            })
    }
}

/// A detected duplicate identifier under a `nodes`/`gates`/`basicEvents` map.
#[derive(Debug, Clone)]
pub struct DuplicateId {
    pub tree: String,
    pub kind: String,
    pub id: String,
    pub span: Span,
}

/// Parse the document with `serde_yaml` (producing the typed AST) and build a
/// [`SpanIndex`] over the same content.
pub fn parse_document_with_spans(content: &str) -> Result<(EtlDocument, SpanIndex), String> {
    let doc = crate::parse_document(content)?;
    let index = build_span_index(content)?;
    Ok((doc, index))
}

/// Build a [`SpanIndex`] over an ETDL document.
pub fn build_span_index(content: &str) -> Result<SpanIndex, String> {
    // `MarkedYaml::load_from_str` drives saphyr's char-iterator `BufferedInput`,
    // whose end-of-stream sentinel (`'\0'`) is misclassified as ordinary content
    // by saphyr-parser's `is_yaml_non_break` (checks `is_break`, not `is_breakz`).
    // A token left unterminated at true EOF (e.g. a bare `%` directive with
    // nothing after it) then loops forever appending `'\0'` to an unbounded
    // buffer. `Parser::new_from_str`'s `&str`-backed `StrInput` doesn't share
    // that bug (bounds are checked against the real buffer length), so we go
    // through it instead — same as `detect_duplicate_ids` below already does.
    let mut parser = saphyr_parser::Parser::new_from_str(content);
    let docs = MarkedYaml::load_from_parser(&mut parser).map_err(|e| e.to_string())?;
    let root = docs.first().ok_or("empty ETDL document")?;
    let mut builder = Builder::new(content);
    builder.walk_root(root);
    Ok(builder.index)
}

/// Inject `span` objects into a serialized AST, wrapping scalar leaves that have
/// spans as `{ "value": ..., "span": ... }`.
pub fn inject_spans(value: &mut Value, index: &SpanIndex) {
    let mut path: PathKey = Vec::new();
    walk_inject(value, index, &mut path);
}

fn walk_inject(value: &mut Value, index: &SpanIndex, path: &mut PathKey) {
    if let Some(&idx) = index.by_path.get(path) {
        let el = &index.elements[idx];
        let span = serde_json::to_value(el.span).unwrap_or_default();
        match value {
            Value::Object(map) => {
                map.insert("span".to_string(), span);
            }
            Value::Array(_) => {
                // Collection spans attach to a wrapping object only.
            }
            _ => {
                let inner = std::mem::replace(value, Value::Null);
                let mut map = serde_json::Map::new();
                map.insert("value".to_string(), inner);
                map.insert("span".to_string(), span);
                *value = Value::Object(map);
            }
        }
    }
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                path.push(PathPart::Key(k.clone()));
                walk_inject(v, index, path);
                path.pop();
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter_mut().enumerate() {
                path.push(PathPart::Index(i));
                walk_inject(v, index, path);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Detect duplicate ids under `nodes`/`gates`/`basicEvents` maps using saphyr's
/// low-level event stream (duplicate YAML keys are collapsed before the typed
/// AST is built, so they must be caught here).
pub fn detect_duplicate_ids(content: &str) -> Result<Vec<DuplicateId>, String> {
    use saphyr_parser::{Event, Parser};

    #[derive(Clone, Copy, PartialEq)]
    enum Container {
        Mapping,
        Sequence,
    }

    #[derive(Default)]
    struct MapCtx {
        expect_key: bool,
        seen: std::collections::BTreeMap<String, usize>,
        path: Vec<String>,
        kind: Option<String>,
        tree: Option<String>,
    }

    let mut parser = Parser::new_from_str(content);
    let mut duplicates = Vec::new();
    let line_map = LineMap::new(content);
    let mut maps: Vec<MapCtx> = Vec::new();
    let mut containers: Vec<Container> = Vec::new();

    while let Some(res) = parser.next_event() {
        let (ev, span) = res.map_err(|e| e.to_string())?;
        match ev {
            Event::MappingStart(..) => {
                if let Some(parent) = maps.last_mut() {
                    // A mapping value consumes the key it follows.
                    parent.expect_key = true;
                }
                let parent_path = maps.last().map(|c| c.path.clone()).unwrap_or_default();
                let mut ctx = MapCtx {
                    expect_key: true,
                    path: parent_path,
                    ..Default::default()
                };
                if let Some(last) = ctx.path.last() {
                    let section_idx = ctx
                        .path
                        .iter()
                        .position(|p| p == "eventTrees" || p == "faultTrees");
                    if let Some(section_idx) = section_idx {
                        let section = ctx.path[section_idx].as_str();
                        let tree = ctx.path.get(section_idx + 1).cloned();
                        match (section, last.as_str()) {
                            ("eventTrees", "nodes") => {
                                ctx.kind = Some("node".to_string());
                                ctx.tree = tree;
                            }
                            ("faultTrees", "gates") => {
                                ctx.kind = Some("gate".to_string());
                                ctx.tree = tree;
                            }
                            ("faultTrees", "basicEvents") => {
                                ctx.kind = Some("basicEvent".to_string());
                                ctx.tree = tree;
                            }
                            _ => {}
                        }
                    }
                }
                maps.push(ctx);
                containers.push(Container::Mapping);
            }
            Event::MappingEnd => {
                maps.pop();
                containers.pop();
            }
            Event::SequenceStart(..) => {
                if let Some(parent) = maps.last_mut() {
                    parent.expect_key = true;
                }
                containers.push(Container::Sequence);
            }
            Event::SequenceEnd => {
                containers.pop();
            }
            Event::Scalar(v, ..) => {
                if containers.last() != Some(&Container::Mapping) {
                    continue;
                }
                let Some(ctx) = maps.last_mut() else { continue };
                if ctx.expect_key {
                    let key = v.to_string();
                    if let (Some(kind), Some(tree)) = (ctx.kind.clone(), ctx.tree.clone()) {
                        if ctx.seen.contains_key(&key) {
                            duplicates.push(DuplicateId {
                                tree,
                                kind,
                                id: key.clone(),
                                span: line_map.span_of(span.start.index(), span.end.index()),
                            });
                        } else {
                            ctx.seen.insert(key.clone(), span.start.index());
                        }
                    }
                    ctx.path.push(key);
                    ctx.expect_key = false;
                } else {
                    ctx.path.pop();
                    ctx.expect_key = true;
                }
            }
            _ => {}
        }
    }

    Ok(duplicates)
}

// ---------------------------------------------------------------------------
// Index builder
// ---------------------------------------------------------------------------

struct Builder<'a> {
    content: &'a str,
    index: SpanIndex,
    line_map: LineMap<'a>,
}

impl<'a> Builder<'a> {
    fn new(content: &'a str) -> Self {
        Builder {
            content,
            index: SpanIndex::default(),
            line_map: LineMap::new(content),
        }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        self.line_map.span_of(start, end)
    }

    /// Span covering the token of a scalar value (strips surrounding quotes).
    fn value_span(&self, node: &MarkedYaml) -> Span {
        let (s, e) = byte_range(node);
        let (ts, te) = token_span(self.content, s, e, &node_str(node));
        self.span(ts, te)
    }

    fn add(&mut self, el: IndexedElement) {
        let identity = match el.kind {
            ElementKind::Definition | ElementKind::Reference => {
                Some((el.tree.clone().unwrap_or_default(), el.name.clone()))
            }
            _ => None,
        };
        let idx = if let Some(&existing) = self.index.by_path.get(&el.path) {
            self.index.elements[existing] = el;
            existing
        } else {
            let idx = self.index.elements.len();
            self.index.by_path.insert(el.path.clone(), idx);
            self.index.elements.push(el);
            idx
        };
        if let Some(id) = identity {
            self.index.by_identity.entry(id).or_default().push(idx);
        }
    }
}

// --- span / token helpers -------------------------------------------------

fn node_str(n: &MarkedYaml) -> String {
    n.data.as_str().map(|s| s.to_string()).unwrap_or_default()
}

fn byte_range(n: &MarkedYaml) -> (usize, usize) {
    (n.span.start.index(), n.span.end.index())
}

/// Compute the exact token span for a scalar value, stripping surrounding
/// quotes/whitespace by locating the decoded value inside the reported region.
fn token_span(content: &str, start: usize, end: usize, value: &str) -> (usize, usize) {
    let lo = start.min(content.len());
    let hi = end.min(content.len());
    let hay = &content[lo..hi];
    if !value.is_empty() {
        if let Some(rel) = hay.find(value) {
            return (lo + rel, lo + rel + value.len());
        }
    }
    (lo, hi)
}

/// Translate an input key to its serde output name (kept unchanged when unknown).
fn out_name(key: &str) -> String {
    match key {
        "eventTrees" => "event_trees",
        "faultTrees" => "fault_trees",
        "basicEvents" => "basic_events",
        "initiatingEvent" => "initiating_event",
        "topEvent" => "top_event",
        "onFailure" => "on_failure",
        "onFailureProbabilitySource" => "on_failure_probability_source",
        "probabilityOfSuccess" => "probability_of_success",
        "probabilityOfFailure" => "probability_of_failure",
        "probabilitySource" => "probability_source",
        "rootCause" => "root_cause",
        "inhibitCondition" => "inhibit_condition",
        "retryPolicy" => "retry_policy",
        "timeoutMs" => "timeout_ms",
        "maxAttempts" => "max_attempts",
        "backoffMs" => "backoff_ms",
        "backoffStrategy" => "backoff_strategy",
        "failureRate" => "failure_rate",
        "missionTime" => "mission_time",
        "eventType" => "event_type",
        other => other,
    }
    .to_string()
}

// --- schema walker ---------------------------------------------------------

impl<'a> Builder<'a> {
    fn walk_root(&mut self, root: &MarkedYaml) {
        let Some(map) = root.data.as_mapping() else {
            return;
        };
        for (k, v) in map {
            let key = node_str(k);
            let out = out_name(&key);
            let (ks, _ke) = byte_range(k);
            let (_vs, ve) = byte_range(v);
            let path = vec![PathPart::Key(out.clone())];
            self.add(IndexedElement {
                kind: ElementKind::Section,
                name: out.clone(),
                field: None,
                tree: None,
                span: self.span(ks, ve),
                key_span: Some(self.span(ks, ks + key.len())),
                path: path.clone(),
                depth: 1,
            });
            match key.as_str() {
                "info" => self.walk_info(v, &out),
                "asyncapi_imports" => self.walk_imports(v, &out),
                "components" => self.walk_components(v, &out),
                "eventTrees" => self.walk_event_trees(v, &out),
                "faultTrees" => self.walk_fault_trees(v, &out),
                _ => {}
            }
        }
    }

    fn walk_info(&mut self, node: &MarkedYaml, base: &str) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (k, v) in map {
            let key = node_str(k);
            let out = out_name(&key);
            let (ks, _ke) = byte_range(k);
            let path = vec![PathPart::Key(base.to_string()), PathPart::Key(out.clone())];
            self.add(IndexedElement {
                kind: ElementKind::Field,
                name: node_str(v),
                field: Some(out.clone()),
                tree: None,
                span: self.span(ks, byte_range(v).1),
                key_span: Some(self.span(ks, ks + key.len())),
                path,
                depth: 2,
            });
        }
    }

    fn walk_imports(&mut self, node: &MarkedYaml, base: &str) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (k, v) in map {
            let alias = node_str(k);
            let (ks, _ke) = byte_range(k);
            let path = vec![
                PathPart::Key(base.to_string()),
                PathPart::Key(alias.clone()),
            ];
            self.add(IndexedElement {
                kind: ElementKind::Field,
                name: alias.clone(),
                field: Some(alias.clone()),
                tree: None,
                span: self.span(ks, byte_range(v).1),
                key_span: Some(self.span(ks, ks + alias.len())),
                path,
                depth: 2,
            });
        }
    }

    fn walk_event_trees(&mut self, node: &MarkedYaml, base: &str) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (tk, tv) in map {
            let tree = node_str(tk);
            let (ks, _ke) = byte_range(tk);
            let (_, ve) = byte_range(tv);
            let path = vec![PathPart::Key(base.to_string()), PathPart::Key(tree.clone())];
            self.add(IndexedElement {
                kind: ElementKind::Definition,
                name: tree.clone(),
                field: None,
                tree: Some(tree.clone()),
                span: self.span(ks, ve),
                key_span: Some(self.span(ks, ks + tree.len())),
                path: path.clone(),
                depth: 2,
            });
            self.walk_event_tree_fields(tv, base, &tree, &path);
        }
    }

    fn walk_event_tree_fields(
        &mut self,
        node: &MarkedYaml,
        base: &str,
        tree: &str,
        tree_path: &PathKey,
    ) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (k, v) in map {
            let key = node_str(k);
            let (ks, _ke) = byte_range(k);
            let (_, ve) = byte_range(v);
            let mut path = tree_path.clone();
            match key.as_str() {
                "initiatingEvent" => {
                    path.push(PathPart::Key("initiating_event".to_string()));
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: "initiatingEvent".to_string(),
                        field: Some("initiating_event".to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(ks, ve),
                        key_span: Some(self.span(ks, ks + key.len())),
                        path: path.clone(),
                        depth: 3,
                    });
                    self.walk_initiating_event(v, tree, &path);
                }
                "nodes" => {
                    path.push(PathPart::Key("nodes".to_string()));
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: "nodes".to_string(),
                        field: Some("nodes".to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(ks, ve),
                        key_span: Some(self.span(ks, ks + key.len())),
                        path: path.clone(),
                        depth: 3,
                    });
                    self.walk_nodes(v, base, tree, &path);
                }
                "description" => {
                    path.push(PathPart::Key("description".to_string()));
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: node_str(v),
                        field: Some("description".to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(ks, ve),
                        key_span: Some(self.span(ks, ks + key.len())),
                        path,
                        depth: 3,
                    });
                }
                _ => {}
            }
        }
    }

    fn walk_initiating_event(&mut self, node: &MarkedYaml, tree: &str, base_path: &PathKey) {
        self.walk_scalar_map(
            node,
            tree,
            base_path,
            &[("id", None), ("message", Some(true)), ("next", Some(true))],
        );
    }

    fn walk_nodes(&mut self, node: &MarkedYaml, base: &str, tree: &str, nodes_path: &PathKey) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (nk, nv) in map {
            let nid = node_str(nk);
            let (ks, _ke) = byte_range(nk);
            let (_, ve) = byte_range(nv);
            let mut path = nodes_path.clone();
            path.push(PathPart::Key(nid.clone()));
            self.add(IndexedElement {
                kind: ElementKind::Definition,
                name: nid.clone(),
                field: None,
                tree: Some(tree.to_string()),
                span: self.span(ks, ve),
                key_span: Some(self.span(ks, ks + nid.len())),
                path: path.clone(),
                depth: 4,
            });
            let Some(fields) = nv.data.as_mapping() else {
                continue;
            };
            for (k, v) in fields {
                let key = node_str(k);
                let (fks, _fke) = byte_range(k);
                let mut fpath = path.clone();
                let field = out_name(&key);
                fpath.push(PathPart::Key(field.clone()));
                let is_ref = matches!(
                    key.as_str(),
                    "next"
                        | "onFailure"
                        | "onFailureProbabilitySource"
                        | "emits"
                        | "channel"
                        | "message"
                );
                if is_ref {
                    self.add(IndexedElement {
                        kind: ElementKind::Reference,
                        name: node_str(v),
                        field: Some(field.clone()),
                        tree: Some(tree.to_string()),
                        span: self.value_span(v),
                        key_span: None,
                        path: fpath,
                        depth: 5,
                    });
                } else if key == "branches" {
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: "branches".to_string(),
                        field: Some("branches".to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(fks, byte_range(v).1),
                        key_span: Some(self.span(fks, fks + key.len())),
                        path: fpath.clone(),
                        depth: 5,
                    });
                    self.walk_branches(v, tree, &fpath);
                } else {
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: node_str(v),
                        field: Some(field.clone()),
                        tree: Some(tree.to_string()),
                        span: self.span(fks, byte_range(v).1),
                        key_span: Some(self.span(fks, fks + key.len())),
                        path: fpath,
                        depth: 5,
                    });
                }
            }
            let _ = base;
        }
    }

    fn walk_branches(&mut self, node: &MarkedYaml, tree: &str, branches_path: &PathKey) {
        let Some(seq) = node.data.as_vec() else {
            return;
        };
        for (i, bv) in seq.iter().enumerate() {
            let (bs, be) = byte_range(bv);
            let mut bpath = branches_path.clone();
            bpath.push(PathPart::Index(i));
            self.add(IndexedElement {
                kind: ElementKind::Field,
                name: format!("branches[{}]", i),
                field: Some("branches".to_string()),
                tree: Some(tree.to_string()),
                span: self.span(bs, be),
                key_span: None,
                path: bpath.clone(),
                depth: 6,
            });
            let Some(map) = bv.data.as_mapping() else {
                continue;
            };
            for (k, v) in map {
                let key = node_str(k);
                let (ks, _ke) = byte_range(k);
                let mut fpath = bpath.clone();
                let field = out_name(&key);
                fpath.push(PathPart::Key(field.clone()));
                let is_ref = matches!(key.as_str(), "next" | "probabilitySource");
                let kind = if is_ref {
                    ElementKind::Reference
                } else {
                    ElementKind::Field
                };
                self.add(IndexedElement {
                    kind,
                    name: node_str(v),
                    field: Some(field.clone()),
                    tree: Some(tree.to_string()),
                    span: if is_ref {
                        self.value_span(v)
                    } else {
                        self.span(ks, byte_range(v).1)
                    },
                    key_span: None,
                    path: fpath,
                    depth: 7,
                });
            }
        }
    }

    /// Generic walker for a mapping whose values are scalars, with a list of
    /// fields and whether each is a reference / definition.
    fn walk_scalar_map(
        &mut self,
        node: &MarkedYaml,
        tree: &str,
        base_path: &PathKey,
        spec: &[(&str, Option<bool>)],
    ) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (k, v) in map {
            let key = node_str(k);
            let field = out_name(&key);
            if let Some((_, is_ref)) = spec.iter().find(|(f, _)| *f == key.as_str()) {
                let (ks, _ke) = byte_range(k);
                let mut path = base_path.clone();
                path.push(PathPart::Key(field.clone()));
                let kind = if is_ref.unwrap_or(false) {
                    ElementKind::Reference
                } else {
                    ElementKind::Definition
                };
                self.add(IndexedElement {
                    kind,
                    name: node_str(v),
                    field: Some(field.clone()),
                    tree: Some(tree.to_string()),
                    span: if kind == ElementKind::Reference {
                        self.value_span(v)
                    } else {
                        self.span(ks, byte_range(v).1)
                    },
                    key_span: None,
                    path,
                    depth: 4,
                });
            }
        }
    }

    fn walk_fault_trees(&mut self, node: &MarkedYaml, base: &str) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (tk, tv) in map {
            let tree = node_str(tk);
            let (ks, _ke) = byte_range(tk);
            let (_, ve) = byte_range(tv);
            let path = vec![PathPart::Key(base.to_string()), PathPart::Key(tree.clone())];
            self.add(IndexedElement {
                kind: ElementKind::Definition,
                name: tree.clone(),
                field: None,
                tree: Some(tree.clone()),
                span: self.span(ks, ve),
                key_span: Some(self.span(ks, ks + tree.len())),
                path: path.clone(),
                depth: 2,
            });
            self.walk_fault_tree_fields(tv, base, &tree, &path);
        }
    }

    fn walk_fault_tree_fields(
        &mut self,
        node: &MarkedYaml,
        base: &str,
        tree: &str,
        tree_path: &PathKey,
    ) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (k, v) in map {
            let key = node_str(k);
            let (ks, _ke) = byte_range(k);
            let (_, ve) = byte_range(v);
            let mut path = tree_path.clone();
            match key.as_str() {
                "topEvent" => {
                    path.push(PathPart::Key("top_event".to_string()));
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: "topEvent".to_string(),
                        field: Some("top_event".to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(ks, ve),
                        key_span: Some(self.span(ks, ks + key.len())),
                        path: path.clone(),
                        depth: 3,
                    });
                    self.walk_top_event(v, tree, &path);
                }
                "gates" => {
                    path.push(PathPart::Key("gates".to_string()));
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: "gates".to_string(),
                        field: Some("gates".to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(ks, ve),
                        key_span: Some(self.span(ks, ks + key.len())),
                        path: path.clone(),
                        depth: 3,
                    });
                    self.walk_gates(v, base, tree, &path);
                }
                "basicEvents" => {
                    let field = "basic_events";
                    path.push(PathPart::Key(field.to_string()));
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: "basicEvents".to_string(),
                        field: Some(field.to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(ks, ve),
                        key_span: Some(self.span(ks, ks + key.len())),
                        path: path.clone(),
                        depth: 3,
                    });
                    self.walk_basic_events(v, base, tree, &path);
                }
                "transfers" => {
                    path.push(PathPart::Key("transfers".to_string()));
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: "transfers".to_string(),
                        field: Some("transfers".to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(ks, ve),
                        key_span: Some(self.span(ks, ks + key.len())),
                        path: path.clone(),
                        depth: 3,
                    });
                    self.walk_transfers(v, base, tree, &path);
                }
                "description" => {
                    path.push(PathPart::Key("description".to_string()));
                    self.add(IndexedElement {
                        kind: ElementKind::Field,
                        name: node_str(v),
                        field: Some("description".to_string()),
                        tree: Some(tree.to_string()),
                        span: self.span(ks, ve),
                        key_span: Some(self.span(ks, ks + key.len())),
                        path,
                        depth: 3,
                    });
                }
                _ => {}
            }
        }
    }

    fn walk_top_event(&mut self, node: &MarkedYaml, tree: &str, base_path: &PathKey) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (k, v) in map {
            let key = node_str(k);
            let (ks, _ke) = byte_range(k);
            let field = out_name(&key);
            let mut path = base_path.clone();
            path.push(PathPart::Key(field.clone()));
            let is_ref = matches!(key.as_str(), "message" | "rootCause");
            let kind = if is_ref {
                ElementKind::Reference
            } else {
                ElementKind::Field
            };
            self.add(IndexedElement {
                kind,
                name: node_str(v),
                field: Some(field.clone()),
                tree: Some(tree.to_string()),
                span: if is_ref {
                    self.value_span(v)
                } else {
                    self.span(ks, byte_range(v).1)
                },
                key_span: None,
                path,
                depth: 4,
            });
        }
    }

    fn walk_gates(&mut self, node: &MarkedYaml, base: &str, tree: &str, gates_path: &PathKey) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (gk, gv) in map {
            let gid = node_str(gk);
            let (ks, _ke) = byte_range(gk);
            let (_, ve) = byte_range(gv);
            let mut path = gates_path.clone();
            path.push(PathPart::Key(gid.clone()));
            self.add(IndexedElement {
                kind: ElementKind::Definition,
                name: gid.clone(),
                field: None,
                tree: Some(tree.to_string()),
                span: self.span(ks, ve),
                key_span: Some(self.span(ks, ks + gid.len())),
                path: path.clone(),
                depth: 4,
            });
            let Some(fields) = gv.data.as_mapping() else {
                continue;
            };
            for (k, v) in fields {
                let key = node_str(k);
                let (fks, _fke) = byte_range(k);
                let field = out_name(&key);
                let mut fpath = path.clone();
                fpath.push(PathPart::Key(field.clone()));
                match key.as_str() {
                    "inputs" => {
                        self.add(IndexedElement {
                            kind: ElementKind::Field,
                            name: node_str(v),
                            field: Some("inputs".to_string()),
                            tree: Some(tree.to_string()),
                            span: self.span(fks, byte_range(v).1),
                            key_span: Some(self.span(fks, fks + key.len())),
                            path: fpath.clone(),
                            depth: 5,
                        });
                        if let Some(seq) = v.data.as_vec() {
                            for (i, item) in seq.iter().enumerate() {
                                let mut ipath = fpath.clone();
                                ipath.push(PathPart::Index(i));
                                self.add(IndexedElement {
                                    kind: ElementKind::Reference,
                                    name: node_str(item),
                                    field: Some("inputs".to_string()),
                                    tree: Some(tree.to_string()),
                                    span: self.value_span(item),
                                    key_span: None,
                                    path: ipath,
                                    depth: 6,
                                });
                            }
                        }
                    }
                    _ => {
                        self.add(IndexedElement {
                            kind: ElementKind::Field,
                            name: node_str(v),
                            field: Some(field.clone()),
                            tree: Some(tree.to_string()),
                            span: self.span(fks, byte_range(v).1),
                            key_span: Some(self.span(fks, fks + key.len())),
                            path: fpath,
                            depth: 5,
                        });
                    }
                }
            }
            let _ = base;
        }
    }

    fn walk_basic_events(
        &mut self,
        node: &MarkedYaml,
        base: &str,
        tree: &str,
        events_path: &PathKey,
    ) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (ek, ev) in map {
            let eid = node_str(ek);
            let (ks, _ke) = byte_range(ek);
            let (_, ve) = byte_range(ev);
            let mut path = events_path.clone();
            path.push(PathPart::Key(eid.clone()));
            self.add(IndexedElement {
                kind: ElementKind::Definition,
                name: eid.clone(),
                field: None,
                tree: Some(tree.to_string()),
                span: self.span(ks, ve),
                key_span: Some(self.span(ks, ks + eid.len())),
                path: path.clone(),
                depth: 4,
            });
            let Some(fields) = ev.data.as_mapping() else {
                continue;
            };
            for (k, v) in fields {
                let key = node_str(k);
                let (fks, _fke) = byte_range(k);
                let field = out_name(&key);
                let mut fpath = path.clone();
                fpath.push(PathPart::Key(field.clone()));
                let is_ref = key == "message";
                let kind = if is_ref {
                    ElementKind::Reference
                } else {
                    ElementKind::Field
                };
                self.add(IndexedElement {
                    kind,
                    name: node_str(v),
                    field: Some(field.clone()),
                    tree: Some(tree.to_string()),
                    span: if is_ref {
                        self.value_span(v)
                    } else {
                        self.span(fks, byte_range(v).1)
                    },
                    key_span: None,
                    path: fpath,
                    depth: 5,
                });
            }
            let _ = base;
        }
    }

    fn walk_transfers(
        &mut self,
        node: &MarkedYaml,
        base: &str,
        tree: &str,
        transfers_path: &PathKey,
    ) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (tk, tv) in map {
            let tid = node_str(tk);
            let (ks, _ke) = byte_range(tk);
            let (_, ve) = byte_range(tv);
            let mut path = transfers_path.clone();
            path.push(PathPart::Key(tid.clone()));
            self.add(IndexedElement {
                kind: ElementKind::Definition,
                name: tid.clone(),
                field: None,
                tree: Some(tree.to_string()),
                span: self.span(ks, ve),
                key_span: Some(self.span(ks, ks + tid.len())),
                path: path.clone(),
                depth: 4,
            });
            let Some(fields) = tv.data.as_mapping() else {
                continue;
            };
            for (k, v) in fields {
                let key = node_str(k);
                let (fks, _fke) = byte_range(k);
                let field = out_name(&key);
                let mut fpath = path.clone();
                fpath.push(PathPart::Key(field.clone()));
                self.add(IndexedElement {
                    kind: ElementKind::Field,
                    name: node_str(v),
                    field: Some(field.clone()),
                    tree: Some(tree.to_string()),
                    span: self.span(fks, byte_range(v).1),
                    key_span: None,
                    path: fpath,
                    depth: 5,
                });
            }
            let _ = base;
        }
    }

    fn walk_components(&mut self, node: &MarkedYaml, base: &str) {
        let Some(map) = node.data.as_mapping() else {
            return;
        };
        for (k, v) in map {
            let key = node_str(k);
            let (ks, _ke) = byte_range(k);
            let field = out_name(&key);
            let path = vec![
                PathPart::Key(base.to_string()),
                PathPart::Key(field.clone()),
            ];
            self.add(IndexedElement {
                kind: ElementKind::Field,
                name: key.clone(),
                field: Some(field.clone()),
                tree: None,
                span: self.span(ks, byte_range(v).1),
                key_span: Some(self.span(ks, ks + key.len())),
                path: path.clone(),
                depth: 2,
            });
            let Some(items) = v.data.as_mapping() else {
                continue;
            };
            for (ik, iv) in items {
                let iid = node_str(ik);
                let (iks, _ike) = byte_range(ik);
                let mut ipath = path.clone();
                ipath.push(PathPart::Key(iid.clone()));
                self.add(IndexedElement {
                    kind: ElementKind::Definition,
                    name: iid.clone(),
                    field: None,
                    tree: Some(iid.clone()),
                    span: self.span(iks, byte_range(iv).1),
                    key_span: Some(self.span(iks, iks + iid.len())),
                    path: ipath.clone(),
                    depth: 3,
                });
                let Some(fields) = iv.data.as_mapping() else {
                    continue;
                };
                for (fk, fv) in fields {
                    let fkey = node_str(fk);
                    let (fks, _fke) = byte_range(fk);
                    let fname = out_name(&fkey);
                    let mut fpath = ipath.clone();
                    fpath.push(PathPart::Key(fname.clone()));
                    let is_ref = matches!(
                        fkey.as_str(),
                        "next" | "onFailure" | "message" | "channel" | "emits" | "inputs"
                    );
                    let kind = if is_ref {
                        ElementKind::Reference
                    } else {
                        ElementKind::Field
                    };
                    self.add(IndexedElement {
                        kind,
                        name: node_str(fv),
                        field: Some(fname.clone()),
                        tree: Some(iid.clone()),
                        span: if is_ref {
                            self.value_span(fv)
                        } else {
                            self.span(fks, byte_range(fv).1)
                        },
                        key_span: None,
                        path: fpath,
                        depth: 4,
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Position helpers
// ---------------------------------------------------------------------------

struct LineMap<'a> {
    content: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> LineMap<'a> {
    fn new(content: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in content.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineMap {
            content,
            line_starts,
        }
    }

    fn line_of(&self, byte: usize) -> usize {
        match self.line_starts.binary_search(&byte) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
        .min(self.line_starts.len().saturating_sub(1))
    }

    fn char_offset(&self, byte: usize) -> usize {
        let byte = byte.min(self.content.len());
        // Only count complete chars; a mid-codepoint byte boundary is clamped
        // back to the nearest char boundary so slicing never panics.
        let byte = self.nearest_char_boundary(byte);
        self.content[..byte].chars().count()
    }

    /// Clamp `byte` to the nearest preceding UTF-8 char boundary.
    fn nearest_char_boundary(&self, byte: usize) -> usize {
        let byte = byte.min(self.content.len());
        let bytes = self.content.as_bytes();
        let mut b = byte;
        // A position is a char boundary unless it lands on a continuation byte
        // (0x80..=0xBF). Walk back while `b` itself is a continuation byte.
        while b > 0 && b < bytes.len() && (bytes[b] & 0xC0) == 0x80 {
            b -= 1;
        }
        b
    }

    fn span_of(&self, start: usize, end: usize) -> Span {
        let start = self.nearest_char_boundary(start);
        let end = self.nearest_char_boundary(end);
        let line = self.line_of(start);
        let line_start = self.nearest_char_boundary(self.line_starts[line]);
        let line_start = line_start.min(start);
        let column = self.content[line_start..start].chars().count();
        let end_line = self.line_of(end);
        let end_line_start = self.nearest_char_boundary(self.line_starts[end_line]);
        let end_line_start = end_line_start.min(end);
        let end_column = self.content[end_line_start..end].chars().count();
        Span {
            start: self.char_offset(start) as u32,
            end: self.char_offset(end) as u32,
            line: line as u32,
            column: column as u32,
            end_line: end_line as u32,
            end_column: end_column as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE: &str = include_str!("../tests/fixtures/order-fulfillment.etdl");

    #[test]
    fn index_covers_sections_and_definitions() {
        let index = build_span_index(FIXTURE).unwrap();
        let event_trees = index
            .resolve(&SpanKey::Section("event_trees"))
            .expect("event_trees section");
        assert_eq!(event_trees.kind, ElementKind::Section);
        assert_eq!(event_trees.span.line, 11); // "eventTrees:" is 0-based line 11

        let tree = index
            .resolve(&SpanKey::Tree {
                tree: "OrderFulfillment".to_string(),
            })
            .expect("tree definition");
        assert_eq!(tree.kind, ElementKind::Definition);
        assert_eq!(tree.span.line, 12);

        let node = index
            .resolve(&SpanKey::Node {
                tree: "OrderFulfillment".to_string(),
                id: "InventoryCheckBarrier".to_string(),
            })
            .expect("node definition");
        assert_eq!(node.span.line, 20);
        let key_span = node.key_span.expect("key span");
        assert_eq!(key_span.line, 20);
        assert_eq!(key_span.column, 6); // 0-based column of the node name
    }

    #[test]
    fn index_covers_references() {
        let index = build_span_index(FIXTURE).unwrap();
        let next = index
            .resolve(&SpanKey::NodeField {
                tree: "OrderFulfillment".to_string(),
                id: "ProcessPaymentOperation".to_string(),
                field: "next",
            })
            .expect("next reference");
        assert_eq!(next.kind, ElementKind::Reference);
        assert_eq!(next.name, "FulfillmentConsequence");
        assert_eq!(next.span.line, 39);

        let message = index
            .resolve(&SpanKey::InitiatingEvent {
                tree: "OrderFulfillment".to_string(),
                field: "message",
            })
            .expect("initiatingEvent.message reference");
        assert_eq!(message.kind, ElementKind::Reference);
        assert_eq!(message.name, "orders_api#/components/messages/OrderPlaced");

        let gate_input = index
            .resolve(&SpanKey::GateInput {
                tree: "PaymentGatewayFailure".to_string(),
                id: "GatewayUnavailableOrRejected".to_string(),
                idx: 1,
            })
            .expect("gate input reference");
        assert_eq!(gate_input.name, "ChargeRejected");

        let root_cause = index
            .resolve(&SpanKey::TopEvent {
                tree: "PaymentGatewayFailure".to_string(),
                field: "root_cause",
            })
            .expect("rootCause reference");
        assert_eq!(root_cause.name, "GatewayUnavailableOrRejected");
    }

    #[test]
    fn find_deepest_resolves_reference_tokens() {
        let index = build_span_index(FIXTURE).unwrap();
        // Line "        next: FulfillmentConsequence" is 0-based line 39.
        let line = FIXTURE.lines().nth(39).unwrap();
        let byte_offset = FIXTURE.lines().take(39).map(|l| l.len() + 1).sum::<usize>()
            + line.find("FulfillmentConsequence").unwrap();
        let char_offset = FIXTURE[..byte_offset].chars().count() as u32;

        let el = index.find_deepest(char_offset).expect("found");
        assert_eq!(el.kind, ElementKind::Reference);
        assert_eq!(el.name, "FulfillmentConsequence");
        assert_eq!(el.field.as_deref(), Some("next"));
        assert_eq!(el.tree.as_deref(), Some("OrderFulfillment"));
        assert!(el.span.start <= char_offset && char_offset < el.span.end);
    }

    #[test]
    fn inject_spans_wraps_scalars() {
        let (doc, index) = parse_document_with_spans(FIXTURE).unwrap();
        let mut value = serde_json::to_value(&doc).unwrap();
        inject_spans(&mut value, &index);

        let event_trees = value.get("event_trees").expect("event_trees");
        assert!(event_trees.get("span").is_some(), "section span attached");
        let tree = &event_trees["OrderFulfillment"];
        assert!(tree.get("span").is_some());
        let node = &tree["nodes"]["InventoryCheckBarrier"];
        assert!(node.get("span").is_some(), "node block span attached");

        // Scalar reference wrapped as { value, span }.
        let op = &tree["nodes"]["ProcessPaymentOperation"];
        let next = &op["next"];
        assert!(next.is_object());
        assert_eq!(next["value"], "FulfillmentConsequence");
        assert!(next.get("span").is_some());

        // A scalar without a recorded span stays plain (retryPolicy internals).
        assert_eq!(op["retry_policy"]["max_attempts"], 3);
        assert_eq!(op["action"]["value"], "execute");
        assert!(op["action"].get("span").is_some());
    }

    #[test]
    fn duplicate_ids_are_detected() {
        let yaml = r#"
etdl: "1.0.0"
info:
  title: "T"
  version: "1.0.0"
  domain: "D"
asyncapi_imports: {}
eventTrees:
  T:
    initiatingEvent:
      id: I
      message: "a#/m"
      next: N
    nodes:
      N:
        type: barrier
        branches:
          - outcome: ok
            condition: "default"
            next: M
      M:
        type: consequence
        operation: terminate
      N:
        type: consequence
        operation: terminate
"#;
        let dups = detect_duplicate_ids(yaml).unwrap();
        assert_eq!(dups.len(), 1, "expected one duplicate node id");
        assert_eq!(dups[0].kind, "node");
        assert_eq!(dups[0].id, "N");
        assert_eq!(dups[0].tree, "T");
    }

    #[test]
    fn span_key_resolves_to_path() {
        let index = build_span_index(FIXTURE).unwrap();
        let branch = index
            .resolve(&SpanKey::BranchField {
                tree: "OrderFulfillment".to_string(),
                id: "InventoryCheckBarrier".to_string(),
                branch: 1,
                field: "next",
            })
            .expect("branch next reference");
        assert_eq!(branch.name, "OutOfStockConsequence");
        let _ = json!({});
    }
}
