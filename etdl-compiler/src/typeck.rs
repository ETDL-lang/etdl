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

                    if let Condition::Expr(ref expr) = branch.condition {
                        let ctx = MessageContext {
                            doc,
                            message_ref: init_msg_ref,
                            registry,
                        };

                        let key = || SpanKey::BranchField {
                            tree: tree_name.to_string(),
                            id: node_id.to_string(),
                            branch: i,
                            field: "condition",
                        };
                        // Rule V-206 (spec §6.2): a Conforming Parser MUST
                        // accept at least 32 total operands; this
                        // implementation's floor is `MAX_CONDITION_OPERANDS`.
                        if count_operands(expr) > MAX_CONDITION_OPERANDS {
                            diagnostics.push(
                                Diagnostic::error(
                                    "V-206",
                                    format!(
                                        "barrier '{}' branch {}: condition exceeds the {}-operand limit",
                                        node_id, i, MAX_CONDITION_OPERANDS
                                    ),
                                )
                                .at(key()),
                            );
                        }

                        check_bool_expr(&ctx, expr, tree_name, node_id, i, diagnostics);
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
/// this module needs; bundled to keep functions in this module under
/// clippy's argument-count lint.
struct MessageContext<'a> {
    doc: &'a EtlDocument,
    message_ref: &'a etdl_parser::ast::MessageRef,
    registry: &'a AsyncApiRegistry,
}

fn check_bool_expr(
    ctx: &MessageContext,
    expr: &BoolExpr,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        BoolExpr::And(a, b) | BoolExpr::Or(a, b) => {
            check_bool_expr(ctx, a, tree_name, node_id, branch_idx, diagnostics);
            check_bool_expr(ctx, b, tree_name, node_id, branch_idx, diagnostics);
        }
        BoolExpr::Not(a) => check_bool_expr(ctx, a, tree_name, node_id, branch_idx, diagnostics),
        BoolExpr::Comparison(cmp) => {
            check_comparison_type(ctx, cmp, tree_name, node_id, branch_idx, diagnostics)
        }
        BoolExpr::Quantifier(q) => {
            check_quantifier(ctx, q, tree_name, node_id, branch_idx, diagnostics)
        }
        BoolExpr::Defined(path) => {
            check_defined(ctx, path, tree_name, node_id, branch_idx, diagnostics)
        }
    }
}

fn check_quantifier(
    ctx: &MessageContext,
    q: &QuantifierExpr,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path_type = resolve_value_expr_type(ctx, &ValueExpr::Path(q.path.clone()));
    if !matches!(path_type, EcelType::Array(_) | EcelType::Unknown) {
        diagnostics.push(
            Diagnostic::error(
                "V-204",
                format!(
                    "barrier '{}' branch {}: quantifier requires an array-typed path, got {:?}",
                    node_id, branch_idx, path_type
                ),
            )
            .at(SpanKey::BranchField {
                tree: tree_name.to_string(),
                id: node_id.to_string(),
                branch: branch_idx,
                field: "condition",
            }),
        );
    }
    check_comparison_type(ctx, &q.comparison, tree_name, node_id, branch_idx, diagnostics);
}

/// Rule V-208 (spec §6.4.1): a `defined-expr`'s `path-expr` must resolve
/// against the schema at all (required or optional) — that's distinct from
/// "resolves but may legitimately be absent at runtime", which is exactly
/// what `defined()` exists to test and is not an error.
fn check_defined(
    ctx: &MessageContext,
    path: &PathExpr,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match ctx
        .registry
        .get_schema_for_message_ref(ctx.doc, ctx.message_ref, &path.segments)
    {
        Ok(Some(_)) => {}
        Ok(None) => {
            diagnostics.push(
                Diagnostic::error(
                    "V-208",
                    format!(
                        "barrier '{}' branch {}: defined() path does not resolve against the resolved AsyncAPI schema",
                        node_id, branch_idx
                    ),
                )
                .at(SpanKey::BranchField {
                    tree: tree_name.to_string(),
                    id: node_id.to_string(),
                    branch: branch_idx,
                    field: "condition",
                }),
            );
        }
        Err(_) => {}
    }
}

fn check_comparison_type(
    ctx: &MessageContext,
    cmp: &Comparison,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    check_operand(ctx, &cmp.left, tree_name, node_id, branch_idx, diagnostics);
    check_operand(ctx, &cmp.right, tree_name, node_id, branch_idx, diagnostics);

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
            // §6.7's coercion table: `null` is "comparable only via ==/!=" —
            // deliberately against any declared type, not just `EcelType::
            // Null` on both sides. A schema's `type` describes the shape a
            // *present* value has; it says nothing about whether a OPTIONAL
            // field's absence should be excluded from an ==/!= null check,
            // and JSON has no way to declare "this string is never null" as
            // a distinct constraint most schemas actually author. Treating
            // `x != null` as a mismatch whenever `x`'s declared type isn't
            // itself `null` would make the single most common presence
            // check impossible to write for any typed field.
            if left_type != right_type
                && left_type != EcelType::Unknown
                && right_type != EcelType::Unknown
                && left_type != EcelType::Null
                && right_type != EcelType::Null
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
            // §6.7.1 exception: ordering is also permitted between two
            // `string` operands when both resolve to a `date-time`/`date`/
            // `time` schema `format`. Mixing a temporal string with
            // anything else (a plain string, a number) is still V-204.
            let both_temporal_strings = left_type == EcelType::String
                && right_type == EcelType::String
                && operand_temporal_format(ctx, &cmp.left).is_some()
                && operand_temporal_format(ctx, &cmp.right).is_some();

            if !both_temporal_strings {
                if left_type != EcelType::Number && left_type != EcelType::Unknown {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-204",
                            format!(
                                "barrier '{}' branch {}: ordering comparison requires number (or matching date-time/date/time strings), got {:?}",
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
                                "barrier '{}' branch {}: ordering comparison requires number (or matching date-time/date/time strings), got {:?}",
                                node_id, branch_idx, right_type
                            ),
                        )
                        .at(key()),
                    );
                }
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

/// Checks a `value-expr`/`literal` operand recursively for arithmetic- and
/// function-argument type errors (V-204) and static division-by-zero
/// (V-207, spec §6.5). Distinct from `resolve_operand_type`, which only
/// *queries* the resulting type — this function is the one with the
/// diagnostic side effects, mirroring how `check_comparison_type` and
/// `resolve_operand_type` already split query from validation.
fn check_operand(
    ctx: &MessageContext,
    operand: &Operand,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Operand::Value(v) = operand {
        check_value_expr(ctx, v, tree_name, node_id, branch_idx, diagnostics);
    }
}

fn check_value_expr(
    ctx: &MessageContext,
    expr: &ValueExpr,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let key = || SpanKey::BranchField {
        tree: tree_name.to_string(),
        id: node_id.to_string(),
        branch: branch_idx,
        field: "condition",
    };

    match expr {
        ValueExpr::Path(_) | ValueExpr::Number(_) => {}
        ValueExpr::Call(func, arg) => {
            check_value_expr(ctx, arg, tree_name, node_id, branch_idx, diagnostics);
            let arg_type = resolve_value_expr_type(ctx, arg);
            let ok = match (func, &arg_type) {
                (_, EcelType::Unknown) => true,
                (FuncName::Length, EcelType::String) | (FuncName::Length, EcelType::Array(_)) => {
                    true
                }
                (FuncName::Abs, EcelType::Number) => true,
                (FuncName::Lower, EcelType::String) | (FuncName::Upper, EcelType::String) => true,
                _ => false,
            };
            if !ok {
                let expected = match func {
                    FuncName::Length => "string or array",
                    FuncName::Abs => "number",
                    FuncName::Lower | FuncName::Upper => "string",
                };
                diagnostics.push(
                    Diagnostic::error(
                        "V-204",
                        format!(
                            "barrier '{}' branch {}: {}() argument must be {}, got {:?}",
                            node_id,
                            branch_idx,
                            func_name_str(*func),
                            expected,
                            arg_type
                        ),
                    )
                    .at(key()),
                );
            }
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) | ValueExpr::Mul(a, b) => {
            check_value_expr(ctx, a, tree_name, node_id, branch_idx, diagnostics);
            check_value_expr(ctx, b, tree_name, node_id, branch_idx, diagnostics);
            check_arithmetic_operand(ctx, a, tree_name, node_id, branch_idx, diagnostics);
            check_arithmetic_operand(ctx, b, tree_name, node_id, branch_idx, diagnostics);
        }
        ValueExpr::Div(a, b) => {
            check_value_expr(ctx, a, tree_name, node_id, branch_idx, diagnostics);
            check_value_expr(ctx, b, tree_name, node_id, branch_idx, diagnostics);
            check_arithmetic_operand(ctx, a, tree_name, node_id, branch_idx, diagnostics);
            check_arithmetic_operand(ctx, b, tree_name, node_id, branch_idx, diagnostics);
            if let ValueExpr::Number(n) = b.as_ref() {
                if *n == 0.0 {
                    diagnostics.push(
                        Diagnostic::error(
                            "V-207",
                            format!(
                                "barrier '{}' branch {}: division by literal zero",
                                node_id, branch_idx
                            ),
                        )
                        .at(key()),
                    );
                }
            }
        }
    }
}

fn check_arithmetic_operand(
    ctx: &MessageContext,
    expr: &ValueExpr,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let t = resolve_value_expr_type(ctx, expr);
    if t != EcelType::Number && t != EcelType::Unknown {
        diagnostics.push(
            Diagnostic::error(
                "V-204",
                format!(
                    "barrier '{}' branch {}: arithmetic operand must be number, got {:?}",
                    node_id, branch_idx, t
                ),
            )
            .at(SpanKey::BranchField {
                tree: tree_name.to_string(),
                id: node_id.to_string(),
                branch: branch_idx,
                field: "condition",
            }),
        );
    }
}

fn func_name_str(func: FuncName) -> &'static str {
    match func {
        FuncName::Length => "length",
        FuncName::Abs => "abs",
        FuncName::Lower => "lower",
        FuncName::Upper => "upper",
    }
}

/// The resolved AsyncAPI schema `format` string for a bare-path operand
/// (spec §6.7.1) — `None` for anything else (arithmetic, function calls,
/// literals), which have no JSON Schema `format` to consult.
fn operand_temporal_format(ctx: &MessageContext, operand: &Operand) -> Option<String> {
    let Operand::Value(ValueExpr::Path(path_expr)) = operand else {
        return None;
    };
    let schema = ctx
        .registry
        .get_schema_for_message_ref(ctx.doc, ctx.message_ref, &path_expr.segments)
        .ok()
        .flatten()?;
    let format = schema.get("format")?.as_str()?.to_string();
    if matches!(format.as_str(), "date-time" | "date" | "time") {
        Some(format)
    } else {
        None
    }
}

fn resolve_operand_type(ctx: &MessageContext, operand: &Operand) -> EcelType {
    match operand {
        Operand::Value(v) => resolve_value_expr_type(ctx, v),
        Operand::Literal(lit) => literal_to_ecel_type(lit),
    }
}

fn resolve_value_expr_type(ctx: &MessageContext, expr: &ValueExpr) -> EcelType {
    match expr {
        ValueExpr::Path(path_expr) => {
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
        ValueExpr::Number(_) => EcelType::Number,
        // Arithmetic and functions have a fixed static result type (spec
        // §6.5, §6.5.1) regardless of whether their operands actually
        // type-check — `check_value_expr` is what reports a mismatched
        // operand; this function only answers "what does this expression
        // evaluate to when it's valid".
        ValueExpr::Add(_, _) | ValueExpr::Sub(_, _) | ValueExpr::Mul(_, _) | ValueExpr::Div(_, _) => {
            EcelType::Number
        }
        ValueExpr::Call(func, _) => match func {
            FuncName::Length | FuncName::Abs => EcelType::Number,
            FuncName::Lower | FuncName::Upper => EcelType::String,
        },
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
