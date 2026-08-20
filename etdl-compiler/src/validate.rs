use etdl_parser::ast::{BasicEventType, EtlDocument, EventTree, FaultTree, Gate, GateType, Node};
use etdl_parser::asyncapi::AsyncApiRegistry;
use etdl_parser::ecel::Condition;
use etdl_parser::spanned::SpanKey;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
    /// Structured locator used to resolve source positions in the WASM layer.
    pub key: Option<SpanKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

impl Diagnostic {
    pub fn error(code: &str, message: String) -> Self {
        Diagnostic {
            code: code.to_string(),
            severity: DiagnosticSeverity::Error,
            message,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            key: None,
        }
    }

    pub fn warning(code: &str, message: String) -> Self {
        Diagnostic {
            code: code.to_string(),
            severity: DiagnosticSeverity::Warning,
            message,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            key: None,
        }
    }

    pub fn with_position(mut self, line: u32, column: u32) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    /// Attach a structured source locator so the caller can resolve the span.
    pub fn at(mut self, key: SpanKey) -> Self {
        self.key = Some(key);
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == DiagnosticSeverity::Error
    }
}

pub fn validate_document(
    doc: &EtlDocument,
    registry: &AsyncApiRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    validate_document_with_extensions(doc, registry, &[], diagnostics);
}

/// Like [`validate_document`], additionally treating every id in
/// `registered_extensions` as supported for the purposes of E-108/W-407
/// (core Section 5.1.1) — the ids [`crate::Compiler::with_extension`]-
/// registered extensions declare via their `EtdlExtension::id()`. Without
/// this, a document declaring a supplement a caller registered through
/// `Compiler::with_extension` would be incorrectly reported as
/// unimplemented: [`supplement_is_supported`] only ever knew about the two
/// built-in extensions (`crate::extension::builtin_registry`), since this
/// function has no `Compiler` instance to consult otherwise.
pub fn validate_document_with_extensions(
    doc: &EtlDocument,
    registry: &AsyncApiRegistry,
    registered_extensions: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) {
    validate_language_version(doc, diagnostics);
    validate_supplements(doc, registered_extensions, diagnostics);
    validate_references(doc, registry, diagnostics);
    validate_event_trees(doc, registry, diagnostics);
    validate_fault_trees(doc, diagnostics);
}

/// The set of supplements this compiler implements. Kept for consumers that
/// inspect supported supplements directly; the authoritative check is the
/// extension registry (`crate::extension::builtin_registry`).
pub const SUPPORTED_SUPPLEMENTS: &[&str] = &["etdl.reliability"];

/// Whether a supplement id is implemented by this compiler build, either
/// built in or registered by the caller (`registered_extensions`).
fn supplement_is_supported(id: &str, registered_extensions: &[&str]) -> bool {
    crate::extension::builtin_registry().contains(id) || registered_extensions.contains(&id)
}

/// Validate supplement declarations (core Section 5.1.2-5.1.3, 5.1.1):
/// E-106 invalid id, E-107 invalid/future version, E-108 required-but-unsupported.
fn validate_supplements(doc: &EtlDocument, registered_extensions: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    for sup in &doc.supplements {
        // E-106: id must match `etdl.<domain>`.
        let valid_id = sup
            .id
            .strip_prefix("etdl.")
            .map(|domain| {
                !domain.is_empty()
                    && domain
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            })
            .unwrap_or(false);
        if !valid_id {
            diagnostics.push(Diagnostic::error(
                "E-106",
                format!(
                    "supplement id '{}' is not a valid supplement identifier (must be 'etdl.<domain>')",
                    sup.id
                ),
            ));
        }

        let supported = supplement_is_supported(&sup.id, registered_extensions);

        // E-107: version must be valid SemVer (MAJOR.MINOR[.PATCH]) and MAJOR must be supported.
        let major = parse_supplement_major(&sup.version);
        match major {
            None => {
                diagnostics.push(Diagnostic::error(
                    "E-107",
                    format!(
                        "supplement '{}' version '{}' is not valid SemVer",
                        sup.id, sup.version
                    ),
                ));
            }
            Some(m) => {
                if supported {
                    const SUPPORTED_RELIABILITY_MAJOR: u64 = 1;
                    if m > SUPPORTED_RELIABILITY_MAJOR {
                        diagnostics.push(Diagnostic::error(
                            "E-107",
                            format!(
                                "supplement '{}' version '{}' uses future major {} (supports major {})",
                                sup.id, sup.version, m, SUPPORTED_RELIABILITY_MAJOR
                            ),
                        ));
                    }
                }
            }
        }

        // E-108: required but unsupported.
        if sup.required && !supported {
            diagnostics.push(Diagnostic::error(
                "E-108",
                format!(
                    "supplement '{}' is required: true but is not implemented by this compiler",
                    sup.id
                ),
            ));
        }

        // W-407: optional but unsupported.
        if !sup.required && !supported {
            diagnostics.push(Diagnostic::warning(
                "W-407",
                format!(
                    "supplement '{}' is not implemented by this compiler; its semantics will not be applied",
                    sup.id
                ),
            ));
        }
    }
}

/// Validate declared library imports (`libraries:`) against the resolution
/// errors [`crate::stdlib::expand_libraries`] already produced:
/// E-113 invalid name, E-114 incompatible/unparseable version,
/// E-115 invalid library manifest, E-116 required-but-unresolvable
/// (including an attempt to resolve a reserved `std.*` name from a
/// non-built-in source), E-117 cyclic dependency, W-409
/// optional-but-unresolvable.
pub fn validate_libraries(
    doc: &EtlDocument,
    lib_errors: &[crate::stdlib::LibraryError],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for import in &doc.libraries {
        let valid_name = !import.name.is_empty()
            && !import.name.starts_with('.')
            && !import.name.ends_with('.')
            && !import.name.contains("..")
            && import
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        if !valid_name {
            diagnostics.push(Diagnostic::error(
                "E-113",
                format!(
                    "library name '{}' is not a valid library identifier (dotted lowercase \
                     segments, e.g. 'std.events')",
                    import.name
                ),
            ));
        }
    }

    for err in lib_errors {
        use crate::stdlib::LibraryError;
        match err {
            LibraryError::Cyclic { .. } => {
                diagnostics.push(Diagnostic::error("E-117", err.to_string()));
            }
            LibraryError::IncompatibleVersion { .. } => {
                diagnostics.push(Diagnostic::error("E-114", err.to_string()));
            }
            LibraryError::InvalidManifest { .. } => {
                diagnostics.push(Diagnostic::error("E-115", err.to_string()));
            }
            LibraryError::NotFound { name, .. } | LibraryError::Shadowing { name, .. } => {
                let required = doc
                    .libraries
                    .iter()
                    .find(|l| &l.name == name)
                    .map(|l| l.required)
                    .unwrap_or(true);
                if required {
                    diagnostics.push(Diagnostic::error("E-116", err.to_string()));
                } else {
                    diagnostics.push(Diagnostic::warning("W-409", err.to_string()));
                }
            }
        }
    }
}

fn parse_supplement_major(version: &str) -> Option<u64> {
    let trimmed = version.trim();
    if trimmed.is_empty() {
        return None;
    }
    let major_part = trimmed.split(['.', '+']).next()?;
    major_part.trim().parse::<u64>().ok()
}

/// Whether the document declares (and the compiler supports) the reliability
/// supplement.
pub fn declares_supplement(doc: &EtlDocument, id: &str) -> bool {
    doc.supplements.iter().any(|s| s.id == id)
}

/// Validate the document's `etdl` language version against the compiler's
/// supported version. Per spec §10.1 the compiler MUST accept any document whose
/// MAJOR matches its supported MAJOR and MUST reject unimplemented future MAJORs.
fn validate_language_version(doc: &EtlDocument, diagnostics: &mut Vec<Diagnostic>) {
    const SUPPORTED_MAJOR: u64 = 1;

    let parse_major = |v: &str| -> Option<u64> {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            return None;
        }
        let major_part = trimmed.split(['.', '+']).next()?;
        major_part.trim().parse::<u64>().ok()
    };

    match parse_major(&doc.etdl) {
        None => {
            diagnostics.push(Diagnostic::error(
                "E-100",
                format!(
                    "document 'etdl' version '{}' is not a valid semantic version",
                    doc.etdl
                ),
            ));
        }
        Some(major) if major > SUPPORTED_MAJOR => {
            diagnostics.push(Diagnostic::error(
                "E-100",
                format!(
                    "document 'etdl' version '{}' has major version {} which is not supported by this compiler (supports major {})",
                    doc.etdl, major, SUPPORTED_MAJOR
                ),
            ));
        }
        Some(_) => {
            // Same MAJOR (or lower MAJOR) is accepted.
        }
    }
}

fn validate_references(
    doc: &EtlDocument,
    registry: &AsyncApiRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for alias in doc.asyncapi_imports.keys() {
        if alias
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '_')
        {
            diagnostics.push(
                Diagnostic::error(
                    "E-103",
                    format!("import alias '{}' contains invalid characters", alias),
                )
                .at(SpanKey::ImportAlias {
                    alias: alias.clone(),
                }),
            );
        }
    }

    for (tree_name, tree) in &doc.event_trees {
        validate_message_ref(
            &tree.initiating_event.message,
            doc,
            registry,
            diagnostics,
            "initiatingEvent.message",
            SpanKey::InitiatingEvent {
                tree: tree_name.clone(),
                field: "message",
            },
        );

        for (node_id, node) in &tree.nodes {
            match node {
                Node::Operation(op) => {
                    if let Some(ref emits_ref) = op.emits {
                        validate_message_ref(
                            emits_ref,
                            doc,
                            registry,
                            diagnostics,
                            &format!("nodes.{}.emits", node_id),
                            SpanKey::NodeField {
                                tree: tree_name.clone(),
                                id: node_id.clone(),
                                field: "emits",
                            },
                        );
                    }
                }
                Node::Consequence(cons) => {
                    if let Some(ref channel_ref) = cons.channel {
                        validate_channel_ref(
                            channel_ref,
                            doc,
                            registry,
                            diagnostics,
                            &format!("nodes.{}.channel", node_id),
                            SpanKey::NodeField {
                                tree: tree_name.clone(),
                                id: node_id.clone(),
                                field: "channel",
                            },
                        );
                    }
                    if let Some(ref message_ref) = cons.message {
                        validate_message_ref(
                            message_ref,
                            doc,
                            registry,
                            diagnostics,
                            &format!("nodes.{}.message", node_id),
                            SpanKey::NodeField {
                                tree: tree_name.clone(),
                                id: node_id.clone(),
                                field: "message",
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(ref fault_trees) = doc.fault_trees {
        for (ft_name, ft) in fault_trees {
            if let Some(ref msg_ref) = ft.top_event.message {
                validate_message_ref(
                    msg_ref,
                    doc,
                    registry,
                    diagnostics,
                    "topEvent.message",
                    SpanKey::TopEvent {
                        tree: ft_name.clone(),
                        field: "message",
                    },
                );
            }
            for (be_name, be) in &ft.basic_events {
                if let Some(ref msg_ref) = be.message {
                    validate_message_ref(
                        msg_ref,
                        doc,
                        registry,
                        diagnostics,
                        "basicEvent.message",
                        SpanKey::BasicEventField {
                            tree: ft_name.clone(),
                            id: be_name.clone(),
                            field: "message",
                        },
                    );
                }
            }
        }
    }
}

fn validate_external_ref(
    ext_ref: &etdl_parser::ast::ExternalRef,
    doc: &EtlDocument,
    registry: &AsyncApiRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    context: &str,
    key: SpanKey,
) {
    if !doc.asyncapi_imports.contains_key(&ext_ref.alias) {
        diagnostics.push(
            Diagnostic::error(
                "E-103",
                format!(
                    "{}: import alias '{}' is not a key in asyncapi_imports",
                    context, ext_ref.alias
                ),
            )
            .at(key),
        );
        return;
    }

    if registry.resolve(ext_ref).is_err() {
        diagnostics.push(
            Diagnostic::error(
                "E-104",
                format!(
                    "{}: JSON Pointer '{}' does not resolve in AsyncAPI document '{}'",
                    context, ext_ref.pointer, ext_ref.alias
                ),
            )
            .at(key),
        );
    }
}

/// Validates a Message Reference (Section 5.3.4): an External Reference is
/// checked exactly as before (E-103/E-104); an Internal Reference
/// (`#/components/messages/<id>`) is checked against the document's own
/// inline `components.messages` — an unresolved `#/components/<kind>/<id>`
/// pointer is already E-105's defined shape (Section 5.3.2/7).
fn validate_message_ref(
    msg_ref: &etdl_parser::ast::MessageRef,
    doc: &EtlDocument,
    registry: &AsyncApiRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    context: &str,
    key: SpanKey,
) {
    match msg_ref {
        etdl_parser::ast::MessageRef::External(ext_ref) => {
            validate_external_ref(ext_ref, doc, registry, diagnostics, context, key);
        }
        etdl_parser::ast::MessageRef::Internal(int_ref) => {
            if registry.resolve_message(doc, msg_ref).is_err() {
                diagnostics.push(
                    Diagnostic::error(
                        "E-105",
                        format!(
                            "{}: internal reference '{}' does not resolve to an entry under components.messages",
                            context, int_ref.pointer
                        ),
                    )
                    .at(key),
                );
            }
        }
    }
}

/// Validates a Channel Reference (Section 5.3.5): a bare channel-name
/// string is only permitted when the document declares no
/// `asyncapi_imports` at all; otherwise it MUST be an External Reference,
/// so a bare string is a reference matching neither required form — E-101
/// (Section 5.3.3 rule 3 / Section 7), the same code a malformed reference
/// string of any other kind gets.
fn validate_channel_ref(
    channel_ref: &etdl_parser::ast::ChannelRef,
    doc: &EtlDocument,
    registry: &AsyncApiRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    context: &str,
    key: SpanKey,
) {
    match channel_ref {
        etdl_parser::ast::ChannelRef::External(ext_ref) => {
            validate_external_ref(ext_ref, doc, registry, diagnostics, context, key);
        }
        etdl_parser::ast::ChannelRef::Bare(name) => {
            if !doc.asyncapi_imports.is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        "E-101",
                        format!(
                            "{}: bare channel name '{}' is only permitted when the document declares no asyncapi_imports (Section 5.3.5); use an External Reference",
                            context, name
                        ),
                    )
                    .at(key),
                );
            }
        }
    }
}

fn validate_event_trees(
    doc: &EtlDocument,
    _registry: &AsyncApiRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (tree_name, tree) in &doc.event_trees {
        validate_tree_structure(tree_name, tree, diagnostics);
    }
}

fn validate_tree_structure(tree_name: &str, tree: &EventTree, diagnostics: &mut Vec<Diagnostic>) {
    check_node_references(tree_name, tree, diagnostics);
    check_dag(tree_name, tree, diagnostics);
    check_reachability(tree_name, tree, diagnostics);
    check_terminal_paths(tree_name, tree, diagnostics);
    check_barrier_rules(tree_name, tree, diagnostics);
    check_operation_rules(tree_name, tree, diagnostics);
    check_consequence_rules(tree_name, tree, diagnostics);
}

fn check_node_references(tree_name: &str, tree: &EventTree, diagnostics: &mut Vec<Diagnostic>) {
    if !tree.nodes.contains_key(&tree.initiating_event.next) {
        diagnostics.push(
            Diagnostic::error(
                "V-101",
                format!(
                    "tree '{}': initiatingEvent.next '{}' does not resolve to a node in this tree",
                    tree_name, tree.initiating_event.next
                ),
            )
            .at(SpanKey::InitiatingEvent {
                tree: tree_name.to_string(),
                field: "next",
            }),
        );
    }

    for (node_id, node) in &tree.nodes {
        let next_targets: Vec<(&str, SpanKey)> = match node {
            Node::Barrier(barrier) => barrier
                .branches
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    (
                        b.next.as_str(),
                        SpanKey::BranchField {
                            tree: tree_name.to_string(),
                            id: node_id.clone(),
                            branch: i,
                            field: "next",
                        },
                    )
                })
                .collect(),
            Node::Operation(op) => {
                let mut targets = vec![(
                    op.next.as_str(),
                    SpanKey::NodeField {
                        tree: tree_name.to_string(),
                        id: node_id.clone(),
                        field: "next",
                    },
                )];
                if let Some(ref on_fail) = op.on_failure {
                    targets.push((
                        on_fail.as_str(),
                        SpanKey::NodeField {
                            tree: tree_name.to_string(),
                            id: node_id.clone(),
                            field: "on_failure",
                        },
                    ));
                }
                targets
            }
            Node::Consequence(_) => continue,
        };

        for (target, key) in next_targets {
            if !tree.nodes.contains_key(target) {
                diagnostics.push(
                    Diagnostic::error(
                        "V-101",
                        format!(
                            "tree '{}': node '{}' references '{}' which does not exist in this tree",
                            tree_name, node_id, target
                        ),
                    )
                    .at(key),
                );
            }
        }
    }
}

fn check_dag(tree_name: &str, tree: &EventTree, diagnostics: &mut Vec<Diagnostic>) {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let mut colors: HashMap<&str, Color> = HashMap::new();
    for node_id in tree.nodes.keys() {
        colors.insert(node_id.as_str(), Color::White);
    }

    fn dfs<'a>(
        node: &'a str,
        tree: &'a EventTree,
        colors: &mut HashMap<&'a str, Color>,
        diagnostics: &mut Vec<Diagnostic>,
        tree_name: &str,
    ) {
        colors.insert(node, Color::Gray);

        let next_nodes: Vec<&str> = match tree.nodes.get(node) {
            Some(Node::Barrier(barrier)) => {
                barrier.branches.iter().map(|b| b.next.as_str()).collect()
            }
            Some(Node::Operation(op)) => {
                let mut targets = vec![op.next.as_str()];
                if let Some(ref on_fail) = op.on_failure {
                    targets.push(on_fail.as_str());
                }
                targets
            }
            Some(Node::Consequence(_)) => {
                // Consequences are terminal: mark black so that re-visiting a
                // consequence from another branch is not mis-flagged as a cycle.
                colors.insert(node, Color::Black);
                return;
            }
            None => return,
        };

        for next in next_nodes {
            match colors.get(next) {
                Some(Color::Gray) => {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-102",
                            format!(
                                "tree '{}': cycle detected involving node '{}' -> '{}'",
                                tree_name, node, next
                            ),
                        )
                        .at(SpanKey::Node {
                            tree: tree_name.to_string(),
                            id: node.to_string(),
                        }),
                    );
                }
                Some(Color::White) => {
                    dfs(next, tree, colors, diagnostics, tree_name);
                }
                _ => {}
            }
        }

        colors.insert(node, Color::Black);
    }

    let start_id = tree.initiating_event.next.as_str();
    if tree.nodes.contains_key(start_id) {
        dfs(start_id, tree, &mut colors, diagnostics, tree_name);
    }
}

fn check_reachability(tree_name: &str, tree: &EventTree, diagnostics: &mut Vec<Diagnostic>) {
    let mut reachable: HashMap<&str, bool> = HashMap::new();
    for node_id in tree.nodes.keys() {
        reachable.insert(node_id.as_str(), false);
    }

    let start_id = tree.initiating_event.next.as_str();
    if tree.nodes.contains_key(start_id) {
        reachable.insert(start_id, true);
        propagate_reachability(start_id, tree, &mut reachable);
    }

    for (node_id, &is_reachable) in &reachable {
        if !is_reachable {
            diagnostics.push(
                Diagnostic::error(
                    "V-103",
                    format!(
                        "tree '{}': node '{}' is unreachable from initiatingEvent",
                        tree_name, node_id
                    ),
                )
                .at(SpanKey::Node {
                    tree: tree_name.to_string(),
                    id: node_id.to_string(),
                }),
            );
        }
    }
}

fn propagate_reachability<'a>(
    node_id: &'a str,
    tree: &'a EventTree,
    reachable: &mut HashMap<&'a str, bool>,
) {
    let next_nodes: Vec<&str> = match tree.nodes.get(node_id) {
        Some(Node::Barrier(barrier)) => barrier.branches.iter().map(|b| b.next.as_str()).collect(),
        Some(Node::Operation(op)) => {
            let mut targets = vec![op.next.as_str()];
            if let Some(ref on_fail) = op.on_failure {
                targets.push(on_fail.as_str());
            }
            targets
        }
        Some(Node::Consequence(_)) => return,
        None => return,
    };

    for next in next_nodes {
        if let Some(was_reachable) = reachable.get_mut(next) {
            if !*was_reachable {
                *was_reachable = true;
                propagate_reachability(next, tree, reachable);
            }
        }
    }
}

fn check_terminal_paths(tree_name: &str, tree: &EventTree, diagnostics: &mut Vec<Diagnostic>) {
    fn check_termination<'a>(
        node_id: &'a str,
        tree: &'a EventTree,
        visited: &mut Vec<&'a str>,
        tree_name: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> bool {
        if visited.contains(&node_id) {
            return false;
        }
        visited.push(node_id);

        match tree.nodes.get(node_id) {
            Some(Node::Consequence(_)) => {
                visited.pop();
                true
            }
            Some(Node::Barrier(barrier)) => {
                let mut all_terminal = true;
                for branch in &barrier.branches {
                    if !check_termination(&branch.next, tree, visited, tree_name, diagnostics) {
                        all_terminal = false;
                    }
                }
                visited.pop();
                all_terminal
            }
            Some(Node::Operation(op)) => {
                let mut all_terminal = true;
                if !check_termination(&op.next, tree, visited, tree_name, diagnostics) {
                    all_terminal = false;
                }
                if let Some(ref on_fail) = op.on_failure {
                    if !check_termination(on_fail, tree, visited, tree_name, diagnostics) {
                        all_terminal = false;
                    }
                }
                visited.pop();
                all_terminal
            }
            None => {
                visited.pop();
                false
            }
        }
    }

    let start_id = tree.initiating_event.next.as_str();
    if tree.nodes.contains_key(start_id) {
        let mut visited = Vec::new();
        let terminates = check_termination(start_id, tree, &mut visited, tree_name, diagnostics);
        if !terminates {
            diagnostics.push(
                Diagnostic::error(
                    "V-104",
                    format!(
                        "tree '{}': a path from initiatingEvent '{}' does not terminate in a consequence node (every path must end in a Consequence)",
                        tree_name, tree.initiating_event.id
                    ),
                )
                .at(SpanKey::InitiatingEvent {
                    tree: tree_name.to_string(),
                    field: "next",
                }),
            );
        }
    }
}

fn check_barrier_rules(tree_name: &str, tree: &EventTree, diagnostics: &mut Vec<Diagnostic>) {
    for (node_id, node) in &tree.nodes {
        if let Node::Barrier(barrier) = node {
            if barrier.branches.len() < 2 {
                diagnostics.push(
                    Diagnostic::error(
                        "V-201",
                        format!(
                            "tree '{}': barrier '{}' has fewer than 2 branches",
                            tree_name, node_id
                        ),
                    )
                    .at(SpanKey::Node {
                        tree: tree_name.to_string(),
                        id: node_id.clone(),
                    }),
                );
            }

            let mut default_count = 0;
            let mut last_is_default = false;
            for (i, branch) in barrier.branches.iter().enumerate() {
                if branch.condition == Condition::Default {
                    default_count += 1;
                    if i == barrier.branches.len() - 1 {
                        last_is_default = true;
                    }
                }
            }
            if default_count > 1 {
                diagnostics.push(
                    Diagnostic::error(
                        "V-202",
                        format!(
                            "tree '{}': barrier '{}' has more than one default branch",
                            tree_name, node_id
                        ),
                    )
                    .at(SpanKey::Node {
                        tree: tree_name.to_string(),
                        id: node_id.clone(),
                    }),
                );
            } else if default_count == 1 && !last_is_default {
                diagnostics.push(
                    Diagnostic::error(
                        "V-202",
                        format!(
                            "tree '{}': barrier '{}' default branch is not the last branch",
                            tree_name, node_id
                        ),
                    )
                    .at(SpanKey::Node {
                        tree: tree_name.to_string(),
                        id: node_id.clone(),
                    }),
                );
            }

            for (i, branch) in barrier.branches.iter().enumerate() {
                if branch.condition == Condition::Default {
                    continue;
                }
                let has_prob =
                    branch.effective_probability().is_some() || branch.probability_source.is_some();
                if !has_prob {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-203",
                            format!(
                                "tree '{}': barrier '{}' branch {} has no probability or probabilitySource",
                                tree_name, node_id, i
                            ),
                        )
                        .at(SpanKey::BranchField {
                            tree: tree_name.to_string(),
                            id: node_id.clone(),
                            branch: i,
                            field: "probability",
                        }),
                    );
                }
            }
        }
    }
}

fn check_operation_rules(tree_name: &str, tree: &EventTree, diagnostics: &mut Vec<Diagnostic>) {
    for (node_id, node) in &tree.nodes {
        if let Node::Operation(op) = node {
            if op.on_failure.is_none() {
                diagnostics.push(
                    Diagnostic::warning(
                        "W-401",
                        format!(
                            "tree '{}': operation '{}' has no onFailure path",
                            tree_name, node_id
                        ),
                    )
                    .at(SpanKey::Node {
                        tree: tree_name.to_string(),
                        id: node_id.clone(),
                    }),
                );
            }

            // V-301: operation handler must be a syntactically valid identifier
            // in at least one configured target language (spec §7.4). The Rust
            // backend is the configured reference target; we validate the Rust
            // identifier form (letter or underscore first, then word chars).
            if !is_rust_ident(&op.handler) {
                diagnostics.push(
                    Diagnostic::error(
                        "V-301",
                        format!(
                            "tree '{}': operation '{}' handler '{}' is not a valid identifier (must start with a letter or underscore and contain only letters, digits, and underscores)",
                            tree_name, node_id, op.handler
                        ),
                    )
                    .at(SpanKey::Node {
                        tree: tree_name.to_string(),
                        id: node_id.clone(),
                    }),
                );
            }
        }
    }
}

fn is_rust_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn check_consequence_rules(tree_name: &str, tree: &EventTree, diagnostics: &mut Vec<Diagnostic>) {
    for (node_id, node) in &tree.nodes {
        if let Node::Consequence(cons) = node {
            match cons.consequence_operation {
                etdl_parser::ast::ConsequenceOperation::Send => {
                    if cons.channel.is_none() || cons.message.is_none() {
                        diagnostics.push(
                            Diagnostic::error(
                                "V-302",
                                format!(
                                    "tree '{}': consequence '{}' has operation: send but omits channel or message",
                                    tree_name, node_id
                                ),
                            )
                            .at(SpanKey::Node {
                                tree: tree_name.to_string(),
                                id: node_id.clone(),
                            }),
                        );
                    }
                }
                etdl_parser::ast::ConsequenceOperation::Terminate => {}
            }
        }
    }
}

fn validate_fault_trees(doc: &EtlDocument, diagnostics: &mut Vec<Diagnostic>) {
    let fault_trees = match &doc.fault_trees {
        Some(fts) => fts,
        None => return,
    };

    for (ft_name, ft) in fault_trees {
        check_fault_tree_structure(doc, ft_name, ft, diagnostics);
        check_gate_rules(ft_name, ft, diagnostics);
        check_basic_event_rules(ft_name, ft, diagnostics);
    }
}

fn check_fault_tree_structure(
    doc: &EtlDocument,
    ft_name: &str,
    ft: &FaultTree,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut known_ids: HashMap<&str, bool> = HashMap::new();

    if let Some(ref gates) = ft.gates {
        for gate_id in gates.keys() {
            known_ids.insert(gate_id.as_str(), false);
        }
    }
    for be_id in ft.basic_events.keys() {
        if known_ids.contains_key(be_id.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "V-402",
                    format!(
                        "fault tree '{}': gate and basic event share ID '{}'",
                        ft_name, be_id
                    ),
                )
                .at(SpanKey::BasicEvent {
                    tree: ft_name.to_string(),
                    id: be_id.clone(),
                }),
            );
        }
        known_ids.insert(be_id.as_str(), false);
    }

    for (be_id, be) in &ft.basic_events {
        if let Some(BasicEventType::House) = be.event_type {
            if be.probability.is_some() || be.failure_rate.is_some() {
                diagnostics.push(
                    Diagnostic::warning(
                        "W-406",
                        format!(
                            "fault tree '{}': house event '{}' declares a probability/failureRate; house events are boundary conditions and their value is not a computed leaf probability",
                            ft_name, be_id
                        ),
                    )
                    .at(SpanKey::BasicEvent {
                        tree: ft_name.to_string(),
                        id: be_id.clone(),
                    }),
                );
            }
        }
    }

    let root_id = ft.top_event.root_cause.as_str();
    match known_ids.get(root_id) {
        None => {
            diagnostics.push(
                Diagnostic::error(
                    "V-401",
                    format!(
                        "fault tree '{}': topEvent.rootCause '{}' does not resolve to a gate or basic event",
                        ft_name, root_id
                    ),
                )
                .at(SpanKey::TopEvent {
                    tree: ft_name.to_string(),
                    field: "root_cause",
                }),
            );
        }
        Some(_) => {
            known_ids.insert(root_id, true);
        }
    }

    if let Some(ref gates) = ft.gates {
        for (gate_id, gate) in gates {
            for (idx, input) in gate.inputs.iter().enumerate() {
                match known_ids.get(input.as_str()) {
                    None => {
                        diagnostics.push(
                            Diagnostic::error(
                                "V-401",
                                format!(
                                    "fault tree '{}': gate '{}' input '{}' does not resolve",
                                    ft_name, gate_id, input
                                ),
                            )
                            .at(SpanKey::GateInput {
                                tree: ft_name.to_string(),
                                id: gate_id.clone(),
                                idx,
                            }),
                        );
                    }
                    Some(_) => {
                        known_ids.insert(input.as_str(), true);
                    }
                }
            }
        }
    }

    check_fault_tree_dag(ft_name, ft, diagnostics);

    // Emit V-404 in deterministic (sorted) id order.
    let mut unreachable: Vec<&str> = known_ids
        .iter()
        .filter(|(id, &is_reachable)| !is_reachable && **id != root_id)
        .map(|(id, _)| *id)
        .collect();
    unreachable.sort_unstable();

    for id in unreachable {
        let key = if ft.gates.as_ref().is_some_and(|g| g.contains_key(id)) {
            SpanKey::Gate {
                tree: ft_name.to_string(),
                id: id.to_string(),
            }
        } else {
            SpanKey::BasicEvent {
                tree: ft_name.to_string(),
                id: id.to_string(),
            }
        };
        diagnostics.push(
            Diagnostic::error(
                "V-404",
                format!(
                    "fault tree '{}': '{}' is not reachable from topEvent.rootCause",
                    ft_name, id
                ),
            )
            .at(key),
        );
    }

    check_transfers(doc, ft_name, ft, diagnostics);
}

fn check_transfers(
    doc: &EtlDocument,
    ft_name: &str,
    ft: &FaultTree,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let transfers = match &ft.transfers {
        Some(t) => t,
        None => return,
    };

    for (transfer_id, transfer) in transfers {
        let target = transfer.target.trim_start_matches("#");
        if !target.starts_with("/faultTrees/") {
            diagnostics.push(
                Diagnostic::error(
                    "V-506",
                    format!(
                        "fault tree '{}': transfer '{}' target '{}' must be an Internal Reference of the form '#/faultTrees/<id>/...'",
                        ft_name, transfer_id, transfer.target
                    ),
                )
                .at(SpanKey::Transfer {
                    tree: ft_name.to_string(),
                    id: transfer_id.clone(),
                    field: "target",
                }),
            );
        } else {
            // The target must resolve to an existing fault tree.
            let tree_id = target.trim_start_matches("/faultTrees/").split('/').next();
            let tree_exists = match tree_id {
                Some(id) => doc
                    .fault_trees
                    .as_ref()
                    .is_some_and(|fts| fts.contains_key(id)),
                None => false,
            };
            if !tree_exists {
                diagnostics.push(
                    Diagnostic::error(
                        "V-506",
                        format!(
                            "fault tree '{}': transfer '{}' target '{}' references fault tree '{}' which does not exist in this document",
                            ft_name,
                            transfer_id,
                            transfer.target,
                            tree_id.unwrap_or("")
                        ),
                    )
                    .at(SpanKey::Transfer {
                        tree: ft_name.to_string(),
                        id: transfer_id.clone(),
                        field: "target",
                    }),
                );
            }
        }
        if let Some(label) = &transfer.label {
            if label.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::warning(
                        "W-405",
                        format!(
                            "fault tree '{}': transfer '{}' has an empty label",
                            ft_name, transfer_id
                        ),
                    )
                    .at(SpanKey::Transfer {
                        tree: ft_name.to_string(),
                        id: transfer_id.clone(),
                        field: "label",
                    }),
                );
            }
        }
    }
}

fn check_fault_tree_dag(ft_name: &str, ft: &FaultTree, diagnostics: &mut Vec<Diagnostic>) {
    let gates = match &ft.gates {
        Some(g) => g,
        None => return,
    };

    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let mut colors: HashMap<&str, Color> = HashMap::new();
    for gate_id in gates.keys() {
        colors.insert(gate_id.as_str(), Color::White);
    }

    fn dfs_gate<'a>(
        gate_id: &'a str,
        gates: &'a BTreeMap<String, Gate>,
        colors: &mut HashMap<&'a str, Color>,
        diagnostics: &mut Vec<Diagnostic>,
        ft_name: &str,
    ) {
        if let Some(Color::Black) = colors.get(gate_id) {
            return;
        }
        if let Some(Color::Gray) = colors.get(gate_id) {
            return;
        }

        colors.insert(gate_id, Color::Gray);

        if let Some(gate) = gates.get(gate_id) {
            for input in &gate.inputs {
                if gates.contains_key(input.as_str()) {
                    match colors.get(input.as_str()) {
                        Some(Color::Gray) => {
                            diagnostics.push(
                                Diagnostic::error(
                                    "V-403",
                                    format!(
                                        "fault tree '{}': cycle detected involving gate '{}' -> '{}'",
                                        ft_name, gate_id, input
                                    ),
                                )
                                .at(SpanKey::Gate {
                                    tree: ft_name.to_string(),
                                    id: gate_id.to_string(),
                                }),
                            );
                        }
                        Some(Color::White) => {
                            dfs_gate(input, gates, colors, diagnostics, ft_name);
                        }
                        _ => {}
                    }
                }
            }
        }

        colors.insert(gate_id, Color::Black);
    }

    let root_id = ft.top_event.root_cause.as_str();
    if gates.contains_key(root_id) {
        dfs_gate(root_id, gates, &mut colors, diagnostics, ft_name);
    }
}

fn check_gate_rules(ft_name: &str, ft: &FaultTree, diagnostics: &mut Vec<Diagnostic>) {
    let gates = match &ft.gates {
        Some(g) => g,
        None => return,
    };

    for (gate_id, gate) in gates {
        let n = gate.inputs.len();

        match gate.gate_type {
            GateType::And | GateType::Or => {
                if n < 2 {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-501",
                            format!(
                                "fault tree '{}': {:?} gate '{}' has {} input(s), minimum 2 required",
                                ft_name, gate.gate_type, gate_id, n
                            ),
                        )
                        .at(SpanKey::Gate {
                            tree: ft_name.to_string(),
                            id: gate_id.clone(),
                        }),
                    );
                }
            }
            GateType::Not => {
                if n != 1 {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-501",
                            format!(
                                "fault tree '{}': NOT gate '{}' has {} input(s), exactly 1 required",
                                ft_name, gate_id, n
                            ),
                        )
                        .at(SpanKey::Gate {
                            tree: ft_name.to_string(),
                            id: gate_id.clone(),
                        }),
                    );
                }
            }
            GateType::Xor => {
                if n != 2 {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-501",
                            format!(
                                "fault tree '{}': XOR gate '{}' has {} input(s), exactly 2 required",
                                ft_name, gate_id, n
                            ),
                        )
                        .at(SpanKey::Gate {
                            tree: ft_name.to_string(),
                            id: gate_id.clone(),
                        }),
                    );
                }
            }
            GateType::Voting => {
                if n < 2 {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-501",
                            format!(
                                "fault tree '{}': VOTING gate '{}' has {} input(s), minimum 2 required",
                                ft_name, gate_id, n
                            ),
                        )
                        .at(SpanKey::Gate {
                            tree: ft_name.to_string(),
                            id: gate_id.clone(),
                        }),
                    );
                }
                if let Some(k) = gate.k {
                    if k < 1 || k as usize > n {
                        diagnostics.push(
                            Diagnostic::error(
                                "V-502",
                                format!(
                                    "fault tree '{}': VOTING gate '{}' k={} must satisfy 1 <= k <= n={}",
                                    ft_name, gate_id, k, n
                                ),
                            )
                            .at(SpanKey::GateField {
                                tree: ft_name.to_string(),
                                id: gate_id.clone(),
                                field: "k",
                            }),
                        );
                    }
                } else {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-502",
                            format!(
                                "fault tree '{}': VOTING gate '{}' missing required 'k' field",
                                ft_name, gate_id
                            ),
                        )
                        .at(SpanKey::GateField {
                            tree: ft_name.to_string(),
                            id: gate_id.clone(),
                            field: "k",
                        }),
                    );
                }
            }
            GateType::Inhibit => {
                if n != 2 {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-501",
                            format!(
                                "fault tree '{}': INHIBIT gate '{}' has {} input(s), exactly 2 required",
                                ft_name, gate_id, n
                            ),
                        )
                        .at(SpanKey::Gate {
                            tree: ft_name.to_string(),
                            id: gate_id.clone(),
                        }),
                    );
                }
                if gate.inhibit_condition.is_none() {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-505",
                            format!(
                                "fault tree '{}': INHIBIT gate '{}' missing required 'inhibitCondition' field",
                                ft_name, gate_id
                            ),
                        )
                        .at(SpanKey::GateField {
                            tree: ft_name.to_string(),
                            id: gate_id.clone(),
                            field: "inhibit_condition",
                        }),
                    );
                }
            }
            GateType::PriorityAnd => {
                if n < 2 {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-501",
                            format!(
                                "fault tree '{}': PRIORITY_AND gate '{}' has {} input(s), minimum 2 required",
                                ft_name, gate_id, n
                            ),
                        )
                        .at(SpanKey::Gate {
                            tree: ft_name.to_string(),
                            id: gate_id.clone(),
                        }),
                    );
                }
            }
        }
    }
}

fn basic_event_numeric_error(ft_name: &str, be_id: &str, detail: String) -> Diagnostic {
    Diagnostic::error(
        "V-507",
        format!("fault tree '{}': basic event '{}': {}", ft_name, be_id, detail),
    )
    .at(SpanKey::BasicEvent {
        tree: ft_name.to_string(),
        id: be_id.to_string(),
    })
}

fn check_basic_event_rules(ft_name: &str, ft: &FaultTree, diagnostics: &mut Vec<Diagnostic>) {
    for (be_id, be) in &ft.basic_events {
        let has_prob = be.probability.is_some();
        let has_rate = be.failure_rate.is_some();
        let has_time = be.mission_time.is_some();
        // A basic event may obtain its probability from an external reliability
        // source via the `x-reliability.source` extension (Reliability
        // Supplement §13.2), which is an explicit extension of the probability
        // semantics. In that case neither `probability` nor `failureRate` is
        // required in the document.
        let external_source = be
            .extensions
            .get("x-reliability")
            .and_then(|v| v.get("source"))
            .is_some();

        if has_prob && has_rate {
            diagnostics.push(
                Diagnostic::error(
                    "V-503",
                    format!(
                        "fault tree '{}': basic event '{}' supplies both probability and failureRate",
                        ft_name, be_id
                    ),
                )
                .at(SpanKey::BasicEvent {
                    tree: ft_name.to_string(),
                    id: be_id.clone(),
                }),
            );
        } else if !has_prob && !has_rate && !external_source {
            diagnostics.push(
                Diagnostic::error(
                    "V-503",
                    format!(
                        "fault tree '{}': basic event '{}' supplies neither probability nor failureRate",
                        ft_name, be_id
                    ),
                )
                .at(SpanKey::BasicEvent {
                    tree: ft_name.to_string(),
                    id: be_id.clone(),
                }),
            );
        }

        if has_rate && !has_time {
            diagnostics.push(
                Diagnostic::error(
                    "V-504",
                    format!(
                        "fault tree '{}': basic event '{}' has failureRate but no missionTime",
                        ft_name, be_id
                    ),
                )
                .at(SpanKey::BasicEvent {
                    tree: ft_name.to_string(),
                    id: be_id.clone(),
                }),
            );
        }

        // V-507: numeric validity. Without this, a negative failureRate or
        // missionTime flows unvalidated into `1.0 - exp(-rate * time)`,
        // producing a negative "probability" that then propagates through
        // AND/OR/XOR gate math into emitted constants — silently wrong
        // reliability output rather than a caught authoring error. A
        // directly declared `probability` outside [0,1] is the same class
        // of error one level up. NaN/infinite values are always invalid,
        // for any of the three fields.
        if let Some(p) = be.probability {
            if !p.is_finite() {
                diagnostics.push(basic_event_numeric_error(
                    ft_name,
                    be_id,
                    format!("probability {} is not finite (NaN or infinity)", p),
                ));
            } else if !(0.0..=1.0).contains(&p) {
                diagnostics.push(basic_event_numeric_error(
                    ft_name,
                    be_id,
                    format!("probability {} is outside the valid range [0, 1]", p),
                ));
            }
        }
        if let Some(r) = be.failure_rate {
            if !r.is_finite() {
                diagnostics.push(basic_event_numeric_error(
                    ft_name,
                    be_id,
                    format!("failureRate {} is not finite (NaN or infinity)", r),
                ));
            } else if r < 0.0 {
                diagnostics.push(basic_event_numeric_error(
                    ft_name,
                    be_id,
                    format!("failureRate {} is negative; a rate must be >= 0", r),
                ));
            }
        }
        if let Some(t) = be.mission_time {
            if !t.is_finite() {
                diagnostics.push(basic_event_numeric_error(
                    ft_name,
                    be_id,
                    format!("missionTime {} is not finite (NaN or infinity)", t),
                ));
            } else if t < 0.0 {
                diagnostics.push(basic_event_numeric_error(
                    ft_name,
                    be_id,
                    format!("missionTime {} is negative; a duration must be >= 0", t),
                ));
            }
        }
    }
}

pub type FaultTreeProbabilities = BTreeMap<String, f64>;

pub fn resolve_probability_links(
    doc: &EtlDocument,
    fault_tree_probs: &FaultTreeProbabilities,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, f64> {
    let mut branch_probs: BTreeMap<String, f64> = BTreeMap::new();

    for (tree_name, tree) in &doc.event_trees {
        for (node_id, node) in &tree.nodes {
            match node {
                Node::Barrier(barrier) => {
                    for (i, branch) in barrier.branches.iter().enumerate() {
                        let key = format!("{}.branch.{}", node_id, i);

                        if let Some(ref ps) = branch.probability_source {
                            let ft_id = extract_fault_tree_id(&ps.pointer);
                            if let Some(&prob) = fault_tree_probs.get(&ft_id) {
                                if let Some(cached) = branch.effective_probability() {
                                    if (cached - prob).abs() > 0.001 {
                                        diagnostics.push(
                                            Diagnostic::warning(
                                                "W-402",
                                                format!(
                                                    "branch '{}[{}]' cached probability {} drifted from fault tree computed {}",
                                                    node_id, i, cached, prob
                                                ),
                                            )
                                            .at(SpanKey::BranchField {
                                                tree: tree_name.clone(),
                                                id: node_id.clone(),
                                                branch: i,
                                                field: "probability",
                                            }),
                                        );
                                    }
                                }
                                branch_probs.insert(key, prob);
                            } else {
                                diagnostics.push(
                                    Diagnostic::error(
                                        "E-105",
                                        format!(
                                            "branch '{}[{}]' probabilitySource references unknown fault tree",
                                            node_id, i
                                        ),
                                    )
                                    .at(SpanKey::BranchField {
                                        tree: tree_name.clone(),
                                        id: node_id.clone(),
                                        branch: i,
                                        field: "probability_source",
                                    }),
                                );
                            }
                        } else if let Some(prob) = branch.effective_probability() {
                            branch_probs.insert(key, prob);
                        }
                    }
                }
                Node::Operation(op) => {
                    if let Some(ref ps) = op.on_failure_probability_source {
                        let ft_id = extract_fault_tree_id(&ps.pointer);
                        if let Some(&prob) = fault_tree_probs.get(&ft_id) {
                            branch_probs.insert(format!("{}.onFailure", node_id), prob);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    branch_probs
}

fn extract_fault_tree_id(pointer: &str) -> String {
    let parts: Vec<&str> = pointer
        .trim_start_matches("#/faultTrees/")
        .split('/')
        .collect();
    parts[0].to_string()
}

pub fn validate_probability_sums(
    doc: &EtlDocument,
    resolved_probs: &BTreeMap<String, f64>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (tree_name, tree) in &doc.event_trees {
        for (node_id, node) in &tree.nodes {
            if let Node::Barrier(barrier) = node {
                let mut sum: f64 = 0.0;
                let mut all_declared = true;

                for (i, b) in barrier.branches.iter().enumerate() {
                    let prob = if let Some(_ps) = &b.probability_source {
                        resolved_probs
                            .get(&format!("{}.branch.{}", node_id, i))
                            .copied()
                    } else {
                        b.effective_probability()
                    };

                    match prob {
                        Some(p) => {
                            // Per spec §5.8.1, a branch probability must be in [0,1].
                            if !(0.0..=1.0).contains(&p) {
                                diagnostics.push(
                                    Diagnostic::error(
                                        "V-203",
                                        format!(
                                            "tree '{}': barrier '{}' branch '{}' probability {} is outside [0,1]",
                                            tree_name, node_id, b.outcome, p
                                        ),
                                    )
                                    .at(SpanKey::BranchField {
                                        tree: tree_name.clone(),
                                        id: node_id.clone(),
                                        branch: i,
                                        field: "probability",
                                    }),
                                );
                            }
                            sum += p;
                        }
                        None => {
                            all_declared = false;
                        }
                    }
                }

                if !barrier.branches.is_empty() && all_declared && (sum - 1.0).abs() > 0.0001 {
                    diagnostics.push(
                            Diagnostic::error(
                                "V-203",
                                format!(
                                    "tree '{}': barrier '{}' branch probabilities sum to {:.4} (must be 1.0 within ±0.0001)",
                                    tree_name, node_id, sum
                                ),
                            )
                            .at(SpanKey::Node {
                                tree: tree_name.clone(),
                                id: node_id.clone(),
                            }),
                        );
                }
            }
        }
    }
}
