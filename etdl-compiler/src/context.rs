//! `etdl context`'s compiler-side support: turning a parsed [`EtlDocument`]
//! into representations suited for feeding LLM pipelines rather than for
//! compiling — a unified `{nodes, edges}` graph ([`build_graph`]) and a
//! flat list of retrieval-ready chunks ([`build_chunks`]), one per
//! semantically meaningful unit (an event tree, a node, a fault tree, a
//! gate, a basic event, and — when declared — a Hazard/Safety Barrier,
//! Budget/Barrier Check, or generic Tree/TreeNode).
//!
//! Deliberately **parse-only**: neither function runs `Compiler::validate`
//! or resolves fault-tree probabilities — mirrors `etdl-wasm`'s existing
//! `parse_for_diagram`/`parse_for_raaml` precedent (`etdl-wasm/src/lib.rs`),
//! which also works directly off a freshly parsed [`EtlDocument`]. A
//! corpus-quality filter based on `etdl validate`'s diagnostics is a
//! reasonable follow-up, not built here.
//!
//! [`build_chunks`] calls every existing supplement parser
//! (`safety::parse_and_validate_safety`,
//! `performance::parse_and_validate_performance`,
//! `live_reliability::parse_and_validate_live_reliability`,
//! `tree_event::parse_and_validate_trees`) unconditionally — each already
//! internally checks whether its own supplement is declared and returns
//! empty data otherwise, so no extra gating is needed here. Their
//! diagnostics are intentionally discarded (this module is a read-only
//! export, not a validator; a caller wanting validation should run `etdl
//! validate` separately).
//!
//! Event/fault trees and `etdl-tree-core::Tree` share no common traversal
//! trait, so [`build_graph`] walks each of the three tree-like AST shapes
//! by hand rather than through one generic abstraction.

use etdl_parser::ast::{
    Barrier, Consequence, ConsequenceOperation, EtlDocument, EventTree, FaultTree, Gate,
    Node, Operation,
};
use etdl_tree_core::{NodeKind, Tree as GenericTree, TreeNode as GenericTreeNode};
use serde::Serialize;
use serde_json::json;

use crate::{live_reliability, performance, safety, tree_event};

/// One node in a [`DocumentGraph`]. `id` is unique within one document
/// (qualified by tree/fault-tree id), never by array position.
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub tree_id: String,
    pub label: String,
    pub attributes: serde_json::Value,
}

/// One directed edge in a [`DocumentGraph`]. `label` names the relationship
/// (a branch outcome, `"rootCause"`, `"input"`, `"onFailure"`, ...) — absent
/// when the edge shape alone (e.g. an initiating event's `next`) already
/// says everything.
#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A whole document projected into a unified `{nodes, edges}` graph —
/// event trees, fault trees, and (when declared) `etdl.tree-event` generic
/// trees, all in one node/edge namespace.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DocumentGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub fn build_graph(doc: &EtlDocument) -> DocumentGraph {
    let mut graph = DocumentGraph::default();

    for (tree_id, tree) in &doc.event_trees {
        add_event_tree(&mut graph, tree_id, tree);
    }

    if let Some(fault_trees) = &doc.fault_trees {
        for (ft_id, ft) in fault_trees {
            add_fault_tree(&mut graph, ft_id, ft);
        }
    }

    let (generic_trees, _diagnostics) = tree_event::parse_and_validate_trees(doc);
    for tree in &generic_trees {
        add_generic_tree(&mut graph, tree);
    }

    graph
}

fn add_event_tree(graph: &mut DocumentGraph, tree_id: &str, tree: &EventTree) {
    let ie = &tree.initiating_event;
    let ie_id = format!("{tree_id}/{}", ie.id);
    graph.nodes.push(GraphNode {
        id: ie_id.clone(),
        kind: "initiating_event".to_string(),
        tree_id: tree_id.to_string(),
        label: ie.id.clone(),
        attributes: json!({ "message": ie.message.as_string() }),
    });
    graph.edges.push(GraphEdge {
        from: ie_id,
        to: format!("{tree_id}/{}", ie.next),
        label: None,
    });

    for (node_id, node) in &tree.nodes {
        let id = format!("{tree_id}/{node_id}");
        match node {
            Node::Barrier(b) => add_barrier_node(graph, tree_id, &id, node_id, b),
            Node::Operation(op) => add_operation_node(graph, tree_id, &id, node_id, op),
            Node::Consequence(c) => add_consequence_node(graph, tree_id, &id, node_id, c),
        }
    }
}

fn add_barrier_node(graph: &mut DocumentGraph, tree_id: &str, id: &str, node_id: &str, b: &Barrier) {
    let branches: Vec<_> = b
        .branches
        .iter()
        .map(|br| {
            json!({
                "outcome": br.outcome,
                "probability": br.effective_probability(),
                "probability_source": br.probability_source.as_ref().map(|r| r.as_string()),
                "next": br.next,
            })
        })
        .collect();
    graph.nodes.push(GraphNode {
        id: id.to_string(),
        kind: "barrier".to_string(),
        tree_id: tree_id.to_string(),
        label: node_id.to_string(),
        attributes: json!({ "branches": branches }),
    });
    for br in &b.branches {
        graph.edges.push(GraphEdge {
            from: id.to_string(),
            to: format!("{tree_id}/{}", br.next),
            label: Some(br.outcome.clone()),
        });
    }
}

fn add_operation_node(graph: &mut DocumentGraph, tree_id: &str, id: &str, node_id: &str, op: &Operation) {
    graph.nodes.push(GraphNode {
        id: id.to_string(),
        kind: "operation".to_string(),
        tree_id: tree_id.to_string(),
        label: node_id.to_string(),
        attributes: json!({
            "handler": op.handler,
            "timeout_ms": op.timeout_ms,
        }),
    });
    graph.edges.push(GraphEdge {
        from: id.to_string(),
        to: format!("{tree_id}/{}", op.next),
        label: Some("success".to_string()),
    });
    if let Some(on_failure) = &op.on_failure {
        graph.edges.push(GraphEdge {
            from: id.to_string(),
            to: format!("{tree_id}/{on_failure}"),
            label: Some("failure".to_string()),
        });
    }
}

fn add_consequence_node(graph: &mut DocumentGraph, tree_id: &str, id: &str, node_id: &str, c: &Consequence) {
    let op_label = consequence_operation_label(&c.consequence_operation);
    graph.nodes.push(GraphNode {
        id: id.to_string(),
        kind: "consequence".to_string(),
        tree_id: tree_id.to_string(),
        label: node_id.to_string(),
        attributes: json!({
            "operation": op_label,
            "channel": c.channel.as_ref().map(|ch| ch.as_string()),
        }),
    });
    // Terminal by construction (core Section 5.10): no outgoing edges.
}

fn consequence_operation_label(op: &ConsequenceOperation) -> &'static str {
    match op {
        ConsequenceOperation::Send => "send",
        ConsequenceOperation::Terminate => "terminate",
    }
}

fn add_fault_tree(graph: &mut DocumentGraph, ft_id: &str, ft: &FaultTree) {
    let top_id = format!("{ft_id}/topEvent");
    graph.nodes.push(GraphNode {
        id: top_id.clone(),
        kind: "fault_tree_top_event".to_string(),
        tree_id: ft_id.to_string(),
        label: ft.top_event.id.clone(),
        attributes: json!({ "description": ft.top_event.description }),
    });
    graph.edges.push(GraphEdge {
        from: top_id,
        to: format!("{ft_id}/{}", ft.top_event.root_cause),
        label: Some("rootCause".to_string()),
    });

    if let Some(gates) = &ft.gates {
        for (gate_id, gate) in gates {
            add_gate_node(graph, ft_id, gate_id, gate);
        }
    }

    for (be_id, be) in &ft.basic_events {
        let id = format!("{ft_id}/{be_id}");
        graph.nodes.push(GraphNode {
            id,
            kind: "basic_event".to_string(),
            tree_id: ft_id.to_string(),
            label: be_id.clone(),
            attributes: json!({
                "probability": be.probability,
                "failure_rate": be.failure_rate,
                "description": be.description,
            }),
        });
    }

    if let Some(transfers) = &ft.transfers {
        for (t_id, transfer) in transfers {
            let id = format!("{ft_id}/{t_id}");
            graph.nodes.push(GraphNode {
                id: id.clone(),
                kind: "transfer".to_string(),
                tree_id: ft_id.to_string(),
                label: t_id.clone(),
                attributes: json!({ "target": transfer.target, "label": transfer.label }),
            });
            // `transfer.target` may name a node outside this fault tree's
            // own id namespace (a cross-tree transfer) — recorded verbatim
            // rather than qualified, since this module has no way to know
            // which fault tree it belongs to.
            graph.edges.push(GraphEdge {
                from: id,
                to: transfer.target.clone(),
                label: Some("transfersTo".to_string()),
            });
        }
    }
}

fn add_gate_node(graph: &mut DocumentGraph, ft_id: &str, gate_id: &str, gate: &Gate) {
    let id = format!("{ft_id}/{gate_id}");
    graph.nodes.push(GraphNode {
        id: id.clone(),
        kind: "gate".to_string(),
        tree_id: ft_id.to_string(),
        label: gate_id.to_string(),
        attributes: json!({ "type": gate.gate_type, "k": gate.k }),
    });
    for input in &gate.inputs {
        graph.edges.push(GraphEdge {
            from: id.clone(),
            to: format!("{ft_id}/{input}"),
            label: Some("input".to_string()),
        });
    }
}

fn add_generic_tree(graph: &mut DocumentGraph, tree: &GenericTree) {
    for (node_id, node) in &tree.nodes {
        let id = format!("{}/{node_id}", tree.id);
        let kind = if node.is_leaf() { "generic_tree_leaf" } else { "generic_tree_gate" };
        let event_ref = match &node.kind {
            NodeKind::Leaf { event_ref } => event_ref.clone(),
            NodeKind::Gate { .. } => None,
        };
        graph.nodes.push(GraphNode {
            id: id.clone(),
            kind: kind.to_string(),
            tree_id: tree.id.clone(),
            label: node_id.clone(),
            attributes: json!({
                "description": node.description,
                "event_ref": event_ref,
            }),
        });
        for child in node.children() {
            graph.edges.push(GraphEdge {
                from: id.clone(),
                to: format!("{}/{child}", tree.id),
                label: None,
            });
        }
    }
}

/// One retrieval-ready unit for a RAG pipeline: a stable id, a coarse
/// `kind` tag, an auto-generated natural-language `text` summary (embed
/// this), and structured `metadata` (filter/display on this).
#[derive(Debug, Clone, Serialize)]
pub struct Chunk {
    pub chunk_id: String,
    pub kind: String,
    pub text: String,
    pub metadata: serde_json::Value,
}

pub fn build_chunks(doc: &EtlDocument) -> Vec<Chunk> {
    let mut chunks = Vec::new();

    chunks.push(document_chunk(doc));

    for (tree_id, tree) in &doc.event_trees {
        chunks.push(event_tree_chunk(tree_id, tree));
        for (node_id, node) in &tree.nodes {
            chunks.push(node_chunk(tree_id, node_id, node));
        }
    }

    if let Some(fault_trees) = &doc.fault_trees {
        let (live_data, _diagnostics) = live_reliability::parse_and_validate_live_reliability(doc);
        for (ft_id, ft) in fault_trees {
            let live_decl = live_data.fault_trees.iter().find(|d| &d.id == ft_id);
            chunks.push(fault_tree_chunk(ft_id, ft, live_decl));
            if let Some(gates) = &ft.gates {
                for (gate_id, gate) in gates {
                    chunks.push(gate_chunk(ft_id, gate_id, gate));
                }
            }
            for (be_id, be) in &ft.basic_events {
                chunks.push(basic_event_chunk(ft_id, be_id, be));
            }
        }
    }

    let (safety_data, _diagnostics) = safety::parse_and_validate_safety(doc);
    for hazard in &safety_data.hazards {
        chunks.push(hazard_chunk(hazard));
    }
    for barrier in &safety_data.barriers {
        chunks.push(safety_barrier_chunk(barrier));
    }

    let (perf_data, _diagnostics) = performance::parse_and_validate_performance(doc);
    for budget in &perf_data.budgets {
        chunks.push(budget_chunk(budget));
    }
    for bc in &perf_data.barrier_checks {
        chunks.push(barrier_check_chunk(bc));
    }

    let (generic_trees, _diagnostics) = tree_event::parse_and_validate_trees(doc);
    for tree in &generic_trees {
        chunks.push(generic_tree_chunk(tree));
        for (node_id, node) in &tree.nodes {
            chunks.push(generic_tree_node_chunk(tree, node_id, node));
        }
    }

    chunks
}

fn document_chunk(doc: &EtlDocument) -> Chunk {
    let supplement_ids: Vec<&str> = doc.supplements.iter().map(|s| s.id.as_str()).collect();
    let fault_tree_count = doc.fault_trees.as_ref().map(|m| m.len()).unwrap_or(0);
    let text = format!(
        "Document '{}' (v{}, domain {}){}. Declares {} event tree(s) and {} fault tree(s){}.",
        doc.info.title,
        doc.info.version,
        doc.info.domain,
        doc.info
            .description
            .as_ref()
            .map(|d| format!(": {d}"))
            .unwrap_or_default(),
        doc.event_trees.len(),
        fault_tree_count,
        if supplement_ids.is_empty() {
            String::new()
        } else {
            format!("; supplements: {}", supplement_ids.join(", "))
        },
    );
    Chunk {
        chunk_id: "document".to_string(),
        kind: "document".to_string(),
        text,
        metadata: json!({
            "title": doc.info.title,
            "version": doc.info.version,
            "domain": doc.info.domain,
            "supplements": supplement_ids,
            "event_tree_ids": doc.event_trees.keys().collect::<Vec<_>>(),
            "fault_tree_ids": doc.fault_trees.as_ref().map(|m| m.keys().collect::<Vec<_>>()).unwrap_or_default(),
        }),
    }
}

fn event_tree_chunk(tree_id: &str, tree: &EventTree) -> Chunk {
    let node_ids: Vec<&str> = tree.nodes.keys().map(String::as_str).collect();
    let text = format!(
        "Event tree '{tree_id}'{} starts at initiating event '{}' (message {}) leading to node '{}'. Contains {} node(s): {}.",
        tree.description.as_ref().map(|d| format!(" ({d})")).unwrap_or_default(),
        tree.initiating_event.id,
        tree.initiating_event.message.as_string(),
        tree.initiating_event.next,
        tree.nodes.len(),
        node_ids.join(", "),
    );
    Chunk {
        chunk_id: tree_id.to_string(),
        kind: "event_tree".to_string(),
        text,
        metadata: json!({
            "initiating_event": tree.initiating_event.id,
            "first_node": tree.initiating_event.next,
            "node_ids": node_ids,
        }),
    }
}

fn node_chunk(tree_id: &str, node_id: &str, node: &Node) -> Chunk {
    let chunk_id = format!("{tree_id}/{node_id}");
    match node {
        Node::Barrier(b) => {
            let branch_descriptions: Vec<String> = b
                .branches
                .iter()
                .map(|br| {
                    let prob = match (br.effective_probability(), &br.probability_source) {
                        (Some(p), _) => format!("p={p}"),
                        (None, Some(src)) => format!("probability from {}", src.as_string()),
                        (None, None) => "probability unspecified".to_string(),
                    };
                    format!("{} ({prob}) -> {}", br.outcome, br.next)
                })
                .collect();
            let text = format!(
                "Barrier '{node_id}' in event tree '{tree_id}' has {} branch(es): {}.",
                b.branches.len(),
                branch_descriptions.join("; "),
            );
            Chunk {
                chunk_id,
                kind: "node.barrier".to_string(),
                text,
                metadata: json!({
                    "tree_id": tree_id,
                    "node_id": node_id,
                    "branches": b.branches.iter().map(|br| json!({
                        "outcome": br.outcome,
                        "probability": br.effective_probability(),
                        "probability_source": br.probability_source.as_ref().map(|r| r.as_string()),
                        "next": br.next,
                    })).collect::<Vec<_>>(),
                }),
            }
        }
        Node::Operation(op) => {
            let text = format!(
                "Operation '{node_id}' in event tree '{tree_id}' invokes handler '{}'{}. On success -> '{}'{}.",
                op.handler,
                op.timeout_ms.map(|t| format!(" (timeout {t}ms)")).unwrap_or_default(),
                op.next,
                op.on_failure.as_ref().map(|f| format!("; on failure -> '{f}'")).unwrap_or_default(),
            );
            Chunk {
                chunk_id,
                kind: "node.operation".to_string(),
                text,
                metadata: json!({
                    "tree_id": tree_id,
                    "node_id": node_id,
                    "handler": op.handler,
                    "next": op.next,
                    "on_failure": op.on_failure,
                    "timeout_ms": op.timeout_ms,
                }),
            }
        }
        Node::Consequence(c) => {
            let op_label = consequence_operation_label(&c.consequence_operation);
            let text = format!(
                "Consequence '{node_id}' in event tree '{tree_id}' performs '{op_label}'{}.",
                c.channel
                    .as_ref()
                    .map(|ch| format!(" on channel '{}'", ch.as_string()))
                    .unwrap_or_default(),
            );
            Chunk {
                chunk_id,
                kind: "node.consequence".to_string(),
                text,
                metadata: json!({
                    "tree_id": tree_id,
                    "node_id": node_id,
                    "operation": op_label,
                    "channel": c.channel.as_ref().map(|ch| ch.as_string()),
                }),
            }
        }
    }
}

fn fault_tree_chunk(ft_id: &str, ft: &FaultTree, live: Option<&live_reliability::LiveFaultTreeDecl>) -> Chunk {
    let gate_count = ft.gates.as_ref().map(|g| g.len()).unwrap_or(0);
    let text = format!(
        "Fault tree '{ft_id}': top event '{}' ({}) rooted at '{}'. Has {gate_count} gate(s) and {} basic event(s).{}",
        ft.top_event.id,
        ft.top_event.description,
        ft.top_event.root_cause,
        ft.basic_events.len(),
        if live.is_some() { " Live-tracked via etdl.live-reliability." } else { "" },
    );
    Chunk {
        chunk_id: ft_id.to_string(),
        kind: "fault_tree".to_string(),
        text,
        metadata: json!({
            "top_event_id": ft.top_event.id,
            "root_cause": ft.top_event.root_cause,
            "gate_ids": ft.gates.as_ref().map(|g| g.keys().collect::<Vec<_>>()).unwrap_or_default(),
            "basic_event_ids": ft.basic_events.keys().collect::<Vec<_>>(),
            "live_tracked": live.is_some(),
            "live_threshold": live.map(|d| d.threshold),
        }),
    }
}

fn gate_chunk(ft_id: &str, gate_id: &str, gate: &Gate) -> Chunk {
    let type_label = serde_json::to_value(&gate.gate_type)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let text = format!(
        "Gate '{gate_id}' in fault tree '{ft_id}' is a {type_label} gate over input(s): {}.",
        gate.inputs.join(", "),
    );
    Chunk {
        chunk_id: format!("{ft_id}/{gate_id}"),
        kind: "gate".to_string(),
        text,
        metadata: json!({
            "fault_tree_id": ft_id,
            "gate_id": gate_id,
            "type": gate.gate_type,
            "inputs": gate.inputs,
            "k": gate.k,
        }),
    }
}

fn basic_event_chunk(ft_id: &str, be_id: &str, be: &etdl_parser::ast::BasicEvent) -> Chunk {
    let quantifier = match (be.probability, be.failure_rate) {
        (Some(p), _) => format!(" (probability {p})"),
        (None, Some(r)) => format!(" (failure rate {r})"),
        (None, None) => String::new(),
    };
    let text = format!("Basic event '{be_id}' in fault tree '{ft_id}': {}{quantifier}.", be.description);
    Chunk {
        chunk_id: format!("{ft_id}/{be_id}"),
        kind: "basic_event".to_string(),
        text,
        metadata: json!({
            "fault_tree_id": ft_id,
            "basic_event_id": be_id,
            "probability": be.probability,
            "failure_rate": be.failure_rate,
        }),
    }
}

fn hazard_chunk(hazard: &safety::Hazard) -> Chunk {
    let text = format!(
        "Hazard '{}': {} (severity {}, likelihood {}, risk index {}), consequence at {}.",
        hazard.id, hazard.description, hazard.severity, hazard.likelihood, hazard.risk_index, hazard.consequence_ref,
    );
    Chunk {
        chunk_id: format!("hazard/{}", hazard.id),
        kind: "hazard".to_string(),
        text,
        metadata: json!({
            "id": hazard.id,
            "severity": hazard.severity,
            "likelihood": hazard.likelihood,
            "risk_index": hazard.risk_index,
            "consequence_ref": hazard.consequence_ref,
        }),
    }
}

fn safety_barrier_chunk(barrier: &safety::SafetyBarrier) -> Chunk {
    let independence = if barrier.independent_of.is_empty() {
        String::new()
    } else {
        format!(", declared independent of: {}", barrier.independent_of.join(", "))
    };
    let text = format!(
        "Safety barrier '{}' (SIL {}) at {}, failure outcome '{}'{independence}.",
        barrier.id, barrier.sil, barrier.node_ref, barrier.failure_outcome,
    );
    Chunk {
        chunk_id: format!("safety-barrier/{}", barrier.id),
        kind: "safety_barrier".to_string(),
        text,
        metadata: json!({
            "id": barrier.id,
            "node_ref": barrier.node_ref,
            "sil": barrier.sil,
            "failure_outcome": barrier.failure_outcome,
            "independent_of": barrier.independent_of,
            "common_cause_group": barrier.common_cause_group,
        }),
    }
}

fn budget_chunk(budget: &performance::Budget) -> Chunk {
    let text = format!(
        "Budget '{}' at {}: p50={}ms, p95={}ms, p99={}ms{}{}.",
        budget.id,
        budget.node_ref,
        budget.p50_ms,
        budget.p95_ms,
        budget.p99_ms,
        budget.max_concurrency.map(|c| format!(", max concurrency {c}")).unwrap_or_default(),
        budget.expected_rate_per_second.map(|r| format!(", expected rate {r}/s")).unwrap_or_default(),
    );
    Chunk {
        chunk_id: format!("budget/{}", budget.id),
        kind: "budget".to_string(),
        text,
        metadata: json!({
            "id": budget.id,
            "node_ref": budget.node_ref,
            "p50_ms": budget.p50_ms,
            "p95_ms": budget.p95_ms,
            "p99_ms": budget.p99_ms,
            "max_concurrency": budget.max_concurrency,
            "expected_rate_per_second": budget.expected_rate_per_second,
        }),
    }
}

fn barrier_check_chunk(bc: &performance::BarrierCheck) -> Chunk {
    let text = format!("Barrier check '{}' links barrier {} to budget '{}'.", bc.id, bc.node_ref, bc.budget_ref);
    Chunk {
        chunk_id: format!("barrier-check/{}", bc.id),
        kind: "barrier_check".to_string(),
        text,
        metadata: json!({
            "id": bc.id,
            "node_ref": bc.node_ref,
            "budget_ref": bc.budget_ref,
        }),
    }
}

fn generic_tree_chunk(tree: &GenericTree) -> Chunk {
    let text = format!(
        "Generic tree '{}' (v{}){}: rooted at '{}', {} node(s).",
        tree.id,
        tree.version,
        tree.description.as_ref().map(|d| format!(" - {d}")).unwrap_or_default(),
        tree.root,
        tree.nodes.len(),
    );
    Chunk {
        chunk_id: format!("tree-event/{}", tree.id),
        kind: "generic_tree".to_string(),
        text,
        metadata: json!({
            "id": tree.id,
            "version": tree.version,
            "root": tree.root,
            "node_ids": tree.nodes.keys().collect::<Vec<_>>(),
        }),
    }
}

fn generic_tree_node_chunk(tree: &GenericTree, node_id: &str, node: &GenericTreeNode) -> Chunk {
    let text = if node.is_leaf() {
        let event_ref = match &node.kind {
            NodeKind::Leaf { event_ref: Some(r) } => format!(" referencing event '{r}'"),
            _ => String::new(),
        };
        format!("Node '{node_id}' in generic tree '{}' is a leaf{event_ref}.", tree.id)
    } else {
        let gate_label = match &node.kind {
            NodeKind::Gate { gate, .. } => gate.label(),
            NodeKind::Leaf { .. } => "",
        };
        format!(
            "Node '{node_id}' in generic tree '{}' is a {gate_label} gate over: {}.",
            tree.id,
            node.children().join(", "),
        )
    };
    Chunk {
        chunk_id: format!("tree-event/{}/{node_id}", tree.id),
        kind: "generic_tree_node".to_string(),
        text,
        metadata: json!({
            "tree_id": tree.id,
            "node_id": node_id,
            "is_leaf": node.is_leaf(),
            "children": node.children(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r##"
etdl: "1.0.0"
info: { title: "T", version: "1.0.0", domain: "D" }
supplements:
  - id: etdl.safety
    version: "1.0"
  - id: etdl.performance
    version: "1.0"
  - id: etdl.tree-event
    version: "1.0"
components:
  messages:
    M:
      payload: { type: object }
faultTrees:
  FT1:
    topEvent: { id: Top, description: "d", rootCause: Gate1 }
    gates:
      Gate1: { type: OR, inputs: [BE1, BE2] }
    basicEvents:
      BE1: { description: "d1", probability: 0.01 }
      BE2: { description: "d2", probability: 0.02 }
eventTrees:
  T1:
    initiatingEvent: { id: I1, message: "#/components/messages/M", next: Barrier1 }
    nodes:
      Barrier1:
        type: barrier
        branches:
          - outcome: OK
            condition: default
            probabilitySource: "#/faultTrees/FT1/topEvent"
            next: Op1
      Op1:
        type: operation
        action: execute
        handler: "h"
        next: C1
        onFailure: C1
      C1: { type: consequence, operation: terminate }
x-safety:
  hazards:
    - id: h1
      description: "d"
      severity: critical
      likelihood: remote
      riskIndex: 2
      consequenceRef: "#/eventTrees/T1/nodes/C1"
  barriers:
    - id: b1
      nodeRef: "#/eventTrees/T1/nodes/Barrier1"
      sil: 2
      failureOutcome: OK
x-performance:
  budgets:
    - id: budget1
      nodeRef: "#/eventTrees/T1/nodes/Op1"
      p50Ms: 10
      p95Ms: 20
      p99Ms: 30
x-tree-event:
  trees:
    - id: GT1
      version: "1.0"
      root: root
      nodes:
        root: { kind: gate, gate: OR, children: [leaf1, leaf2] }
        leaf1: { kind: leaf }
        leaf2: { kind: leaf }
"##;

    fn doc() -> EtlDocument {
        serde_yaml::from_str(DOC).expect("fixture doc parses")
    }

    #[test]
    fn graph_has_expected_nodes_and_edges() {
        let doc = doc();
        let graph = build_graph(&doc);

        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"T1/I1"), "{ids:?}");
        assert!(ids.contains(&"T1/Barrier1"), "{ids:?}");
        assert!(ids.contains(&"T1/Op1"), "{ids:?}");
        assert!(ids.contains(&"T1/C1"), "{ids:?}");
        assert!(ids.contains(&"FT1/topEvent"), "{ids:?}");
        assert!(ids.contains(&"FT1/Gate1"), "{ids:?}");
        assert!(ids.contains(&"FT1/BE1"), "{ids:?}");
        assert!(ids.contains(&"GT1/root"), "{ids:?}");
        assert!(ids.contains(&"GT1/leaf1"), "{ids:?}");

        let branch_edge = graph
            .edges
            .iter()
            .find(|e| e.from == "T1/Barrier1" && e.to == "T1/Op1")
            .expect("branch edge exists");
        assert_eq!(branch_edge.label.as_deref(), Some("OK"));

        let failure_edge = graph
            .edges
            .iter()
            .find(|e| e.from == "T1/Op1" && e.to == "T1/C1" && e.label.as_deref() == Some("failure"));
        assert!(failure_edge.is_some());

        let root_cause_edge = graph
            .edges
            .iter()
            .find(|e| e.from == "FT1/topEvent" && e.to == "FT1/Gate1");
        assert!(root_cause_edge.is_some());

        let gate_input_edge = graph
            .edges
            .iter()
            .any(|e| e.from == "FT1/Gate1" && e.to == "FT1/BE1" && e.label.as_deref() == Some("input"));
        assert!(gate_input_edge);

        let generic_edge = graph
            .edges
            .iter()
            .any(|e| e.from == "GT1/root" && e.to == "GT1/leaf1");
        assert!(generic_edge);
    }

    #[test]
    fn chunks_cover_every_declared_supplement() {
        let doc = doc();
        let chunks = build_chunks(&doc);
        let kinds: Vec<&str> = chunks.iter().map(|c| c.kind.as_str()).collect();

        for expected in [
            "document",
            "event_tree",
            "node.barrier",
            "node.operation",
            "node.consequence",
            "fault_tree",
            "gate",
            "basic_event",
            "hazard",
            "safety_barrier",
            "budget",
            "generic_tree",
            "generic_tree_node",
        ] {
            assert!(kinds.contains(&expected), "missing chunk kind '{expected}' in {kinds:?}");
        }

        let barrier_chunk = chunks
            .iter()
            .find(|c| c.chunk_id == "T1/Barrier1")
            .expect("barrier chunk exists");
        assert!(barrier_chunk.text.contains("OK"));
        assert!(barrier_chunk.text.contains("Op1"));
    }

    #[test]
    fn document_without_any_supplements_still_produces_core_chunks_and_graph() {
        let minimal = r##"
etdl: "1.0.0"
info: { title: "T", version: "1.0.0", domain: "D" }
components:
  messages:
    M:
      payload: { type: object }
eventTrees:
  T1:
    initiatingEvent: { id: I1, message: "#/components/messages/M", next: C1 }
    nodes:
      C1: { type: consequence, operation: terminate }
"##;
        let doc: EtlDocument = serde_yaml::from_str(minimal).expect("fixture doc parses");

        let graph = build_graph(&doc);
        assert!(graph.nodes.iter().any(|n| n.id == "T1/C1"));

        let chunks = build_chunks(&doc);
        let kinds: Vec<&str> = chunks.iter().map(|c| c.kind.as_str()).collect();
        assert!(kinds.contains(&"document"));
        assert!(kinds.contains(&"event_tree"));
        assert!(kinds.contains(&"node.consequence"));
        assert!(!kinds.contains(&"hazard"));
        assert!(!kinds.contains(&"budget"));
    }
}
