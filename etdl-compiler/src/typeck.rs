use etdl_parser::ast::{EtlDocument, Node};
use etdl_parser::asyncapi::AsyncApiRegistry;
use etdl_parser::ecel::*;
use etdl_parser::spanned::SpanKey;

use crate::validate::Diagnostic;

pub fn type_check_conditions(
    doc: &EtlDocument,
    registry: &AsyncApiRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (tree_name, tree) in &doc.event_trees {
        let init_msg_ref = &tree.initiating_event.message;

        if registry.resolve_message(doc, init_msg_ref).is_err() {
            continue;
        }

        for (node_id, node) in &tree.nodes {
            if let Node::Barrier(barrier) = node {
                for (i, branch) in barrier.branches.iter().enumerate() {
                    if branch.condition == Condition::Default {
                        continue;
                    }

                    if let Condition::Comparison(ref cmp) = branch.condition {
                        let ctx = MessageContext {
                            doc,
                            message_ref: init_msg_ref,
                            registry,
                        };
                        check_comparison_type(&ctx, cmp, tree_name, node_id, i, diagnostics);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum EcelType {
    Number,
    String,
    Bool,
    Null,
    Array(Box<EcelType>),
    Object,
    Unknown,
}

/// The document/message-reference/registry triple every schema-lookup in
/// this module needs; bundled to keep `check_comparison_type` and
/// `resolve_operand_type` under clippy's argument-count lint.
struct MessageContext<'a> {
    doc: &'a EtlDocument,
    message_ref: &'a etdl_parser::ast::MessageRef,
    registry: &'a AsyncApiRegistry,
}

fn check_comparison_type(
    ctx: &MessageContext,
    cmp: &Comparison,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let left_type = resolve_operand_type(ctx, &cmp.left);
    let right_type = resolve_operand_type(ctx, &cmp.right);

    let key = || SpanKey::BranchField {
        tree: tree_name.to_string(),
        id: node_id.to_string(),
        branch: branch_idx,
        field: "condition",
    };

    match cmp.op {
        Comparator::Eq | Comparator::Neq => {
            if left_type != right_type
                && left_type != EcelType::Unknown
                && right_type != EcelType::Unknown
            {
                diagnostics.push(
                    Diagnostic::error(
                        "V-204",
                        format!(
                            "barrier '{}' branch {}: type mismatch in comparison {:?} {:?} {:?}: left {:?}, right {:?}",
                            node_id, branch_idx, cmp.left, cmp.op, cmp.right, left_type, right_type
                        ),
                    )
                    .at(key()),
                );
            }
        }
        Comparator::Gt | Comparator::Gte | Comparator::Lt | Comparator::Lte => {
            if left_type != EcelType::Number && left_type != EcelType::Unknown {
                diagnostics.push(
                    Diagnostic::error(
                        "V-204",
                        format!(
                            "barrier '{}' branch {}: ordering comparison requires number, got {:?}",
                            node_id, branch_idx, left_type
                        ),
                    )
                    .at(key()),
                );
            }
            if right_type != EcelType::Number && right_type != EcelType::Unknown {
                diagnostics.push(
                    Diagnostic::error(
                        "V-204",
                        format!(
                            "barrier '{}' branch {}: ordering comparison requires number, got {:?}",
                            node_id, branch_idx, right_type
                        ),
                    )
                    .at(key()),
                );
            }
        }
        Comparator::In => match &right_type {
            EcelType::Array(_) | EcelType::Unknown => {}
            _ => {
                diagnostics.push(
                    Diagnostic::error(
                        "V-204",
                        format!(
                            "barrier '{}' branch {}: 'in' right operand must be array, got {:?}",
                            node_id, branch_idx, right_type
                        ),
                    )
                    .at(key()),
                );
            }
        },
        Comparator::Matches => {
            if left_type != EcelType::String && left_type != EcelType::Unknown {
                diagnostics.push(
                    Diagnostic::error(
                        "V-204",
                        format!(
                            "barrier '{}' branch {}: 'matches' left operand must be string, got {:?}",
                            node_id, branch_idx, left_type
                        ),
                    )
                    .at(key()),
                );
            }
        }
    }
}

fn resolve_operand_type(ctx: &MessageContext, operand: &Operand) -> EcelType {
    match operand {
        Operand::Path(path_expr) => {
            let segments: Vec<&PathSegment> = path_expr.segments.iter().skip(1).collect();

            if segments.is_empty() {
                return EcelType::Object;
            }

            match ctx.registry.get_schema_for_message_ref(
                ctx.doc,
                ctx.message_ref,
                &path_expr.segments,
            ) {
                Ok(Some(schema)) => schema_to_ecel_type(&schema),
                Ok(None) => EcelType::Unknown,
                Err(_) => EcelType::Unknown,
            }
        }
        Operand::Literal(lit) => literal_to_ecel_type(lit),
    }
}

fn literal_to_ecel_type(lit: &Literal) -> EcelType {
    match lit {
        Literal::Number(_) => EcelType::Number,
        Literal::String(_) => EcelType::String,
        Literal::Bool(_) => EcelType::Bool,
        Literal::Null => EcelType::Null,
        Literal::Array(items) => {
            let inner = items
                .first()
                .map(literal_to_ecel_type)
                .unwrap_or(EcelType::Unknown);
            EcelType::Array(Box::new(inner))
        }
    }
}

fn schema_to_ecel_type(schema: &serde_json::Value) -> EcelType {
    if let Some(type_val) = schema.get("type") {
        match type_val.as_str() {
            Some("string") => return EcelType::String,
            Some("integer") | Some("number") => return EcelType::Number,
            Some("boolean") => return EcelType::Bool,
            Some("null") => return EcelType::Null,
            Some("array") => {
                if let Some(items) = schema.get("items") {
                    return EcelType::Array(Box::new(schema_to_ecel_type(items)));
                }
                return EcelType::Array(Box::new(EcelType::Unknown));
            }
            Some("object") => return EcelType::Object,
            _ => {}
        }
    }

    if let Some(properties) = schema.get("properties") {
        if properties.is_object() && !properties.as_object().unwrap().is_empty() {
            return EcelType::Object;
        }
    }

    if schema.get("items").is_some() {
        return EcelType::Array(Box::new(EcelType::Unknown));
    }

    EcelType::Unknown
}
