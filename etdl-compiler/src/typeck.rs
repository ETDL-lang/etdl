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

                        check_bool_expr(&ctx, expr, tree_name, node_id, i, true, diagnostics);
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

/// `top_level`: `true` only when `expr` *is* the branch's entire
/// condition (the call from `type_check_conditions`); `false` for
/// anything reached through `And`/`Or`/`Not` recursion. Distinguishes
/// "the whole condition is `reliability.in_range == true`" (supported,
/// see `docs/reference/live-reliability.md`) from "`reliability.in_range`
/// combined with other terms" (rejected as E-173 — codegen's
/// `condition_to_rust_code` only special-cases the bare top-level shape;
/// nesting it would otherwise silently fall through to the ordinary
/// message-path renderer and produce meaningless generated code).
fn check_bool_expr(
    ctx: &MessageContext,
    expr: &BoolExpr,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    top_level: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        BoolExpr::And(a, b) | BoolExpr::Or(a, b) => {
            check_bool_expr(ctx, a, tree_name, node_id, branch_idx, false, diagnostics);
            check_bool_expr(ctx, b, tree_name, node_id, branch_idx, false, diagnostics);
        }
        BoolExpr::Not(a) => {
            check_bool_expr(ctx, a, tree_name, node_id, branch_idx, false, diagnostics)
        }
        BoolExpr::Comparison(cmp) => {
            check_comparison_type(ctx, cmp, tree_name, node_id, branch_idx, top_level, diagnostics)
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
    check_comparison_type(ctx, &q.comparison, tree_name, node_id, branch_idx, false, diagnostics);
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
    top_level: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    check_operand(ctx, &cmp.left, tree_name, node_id, branch_idx, top_level, diagnostics);
    check_operand(ctx, &cmp.right, tree_name, node_id, branch_idx, top_level, diagnostics);

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
    top_level: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Operand::Value(v) = operand {
        check_value_expr(ctx, v, tree_name, node_id, branch_idx, top_level, diagnostics);
    }
}

fn check_value_expr(
    ctx: &MessageContext,
    expr: &ValueExpr,
    tree_name: &str,
    node_id: &str,
    branch_idx: usize,
    top_level: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let key = || SpanKey::BranchField {
        tree: tree_name.to_string(),
        id: node_id.to_string(),
        branch: branch_idx,
        field: "condition",
    };

    match expr {
        ValueExpr::Path(path_expr) if is_reliability_root(path_expr) => {
            if !crate::validate::declares_supplement(ctx.doc, "etdl.live-reliability") {
                diagnostics.push(
                    Diagnostic::error(
                        "E-173",
                        format!(
                            "barrier '{}' branch {}: '{}' requires the document to declare supplement etdl.live-reliability",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if reliability_path_type(path_expr) == EcelType::Unknown {
                diagnostics.push(
                    Diagnostic::error(
                        "E-173",
                        format!(
                            "barrier '{}' branch {}: 'reliability' path must be exactly 'reliability.in_range', got '{}'",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if !top_level {
                diagnostics.push(
                    Diagnostic::error(
                        "E-173",
                        format!(
                            "barrier '{}' branch {}: 'reliability.in_range' must be the entire branch condition, not combined with &&/||/! — split it into its own branch",
                            node_id, branch_idx
                        ),
                    )
                    .at(key()),
                );
            }
        }
        ValueExpr::Path(path_expr) if is_performance_root(path_expr) => {
            if !crate::validate::declares_supplement(ctx.doc, "etdl.performance") {
                diagnostics.push(
                    Diagnostic::error(
                        "E-163",
                        format!(
                            "barrier '{}' branch {}: '{}' requires the document to declare supplement etdl.performance",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if performance_path_type(path_expr) == EcelType::Unknown {
                diagnostics.push(
                    Diagnostic::error(
                        "E-163",
                        format!(
                            "barrier '{}' branch {}: 'performance' path must be exactly 'performance.in_budget', got '{}'",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if !top_level {
                diagnostics.push(
                    Diagnostic::error(
                        "E-163",
                        format!(
                            "barrier '{}' branch {}: 'performance.in_budget' must be the entire branch condition, not combined with &&/||/! — split it into its own branch",
                            node_id, branch_idx
                        ),
                    )
                    .at(key()),
                );
            }
        }
        ValueExpr::Path(path_expr) if is_safety_root(path_expr) => {
            // Two supplements gate this one path, not just one — a
            // genuine dependency `reliability.in_range`/`performance.in_budget`
            // don't have: the SIL band comes from etdl.safety, the live
            // value comes from etdl.live-reliability.
            if !crate::validate::declares_supplement(ctx.doc, "etdl.safety") {
                diagnostics.push(
                    Diagnostic::error(
                        "E-135",
                        format!(
                            "barrier '{}' branch {}: '{}' requires the document to declare supplement etdl.safety",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if !crate::validate::declares_supplement(ctx.doc, "etdl.live-reliability") {
                diagnostics.push(
                    Diagnostic::error(
                        "E-135",
                        format!(
                            "barrier '{}' branch {}: '{}' also requires the document to declare supplement etdl.live-reliability",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if safety_path_type(path_expr) == EcelType::Unknown {
                diagnostics.push(
                    Diagnostic::error(
                        "E-135",
                        format!(
                            "barrier '{}' branch {}: 'safety' path must be exactly 'safety.sil_maintained', got '{}'",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if !top_level {
                diagnostics.push(
                    Diagnostic::error(
                        "E-135",
                        format!(
                            "barrier '{}' branch {}: 'safety.sil_maintained' must be the entire branch condition, not combined with &&/||/! — split it into its own branch",
                            node_id, branch_idx
                        ),
                    )
                    .at(key()),
                );
            }
        }
        ValueExpr::Path(path_expr) if is_security_root(path_expr) => {
            // Two supplements gate this one path, not just one — a
            // genuine dependency `reliability.in_range`/`performance.in_budget`
            // don't have: the bypass threshold comes from etdl.security,
            // the live value comes from etdl.live-reliability.
            if !crate::validate::declares_supplement(ctx.doc, "etdl.security") {
                diagnostics.push(
                    Diagnostic::error(
                        "E-143",
                        format!(
                            "barrier '{}' branch {}: '{}' requires the document to declare supplement etdl.security",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if !crate::validate::declares_supplement(ctx.doc, "etdl.live-reliability") {
                diagnostics.push(
                    Diagnostic::error(
                        "E-143",
                        format!(
                            "barrier '{}' branch {}: '{}' also requires the document to declare supplement etdl.live-reliability",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if security_path_type(path_expr) == EcelType::Unknown {
                diagnostics.push(
                    Diagnostic::error(
                        "E-143",
                        format!(
                            "barrier '{}' branch {}: 'security' path must be exactly 'security.control_effective', got '{}'",
                            node_id, branch_idx, path_to_string(path_expr)
                        ),
                    )
                    .at(key()),
                );
            } else if !top_level {
                diagnostics.push(
                    Diagnostic::error(
                        "E-143",
                        format!(
                            "barrier '{}' branch {}: 'security.control_effective' must be the entire branch condition, not combined with &&/||/! — split it into its own branch",
                            node_id, branch_idx
                        ),
                    )
                    .at(key()),
                );
            }
        }
        ValueExpr::Path(_) | ValueExpr::Number(_) => {}
        ValueExpr::Call(func, arg) => {
            check_value_expr(ctx, arg, tree_name, node_id, branch_idx, false, diagnostics);
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
            check_value_expr(ctx, a, tree_name, node_id, branch_idx, false, diagnostics);
            check_value_expr(ctx, b, tree_name, node_id, branch_idx, false, diagnostics);
            check_arithmetic_operand(ctx, a, tree_name, node_id, branch_idx, diagnostics);
            check_arithmetic_operand(ctx, b, tree_name, node_id, branch_idx, diagnostics);
        }
        ValueExpr::Div(a, b) => {
            check_value_expr(ctx, a, tree_name, node_id, branch_idx, false, diagnostics);
            check_value_expr(ctx, b, tree_name, node_id, branch_idx, false, diagnostics);
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
        ValueExpr::Path(path_expr) if is_reliability_root(path_expr) => {
            reliability_path_type(path_expr)
        }
        ValueExpr::Path(path_expr) if is_performance_root(path_expr) => {
            performance_path_type(path_expr)
        }
        ValueExpr::Path(path_expr) if is_safety_root(path_expr) => {
            safety_path_type(path_expr)
        }
        ValueExpr::Path(path_expr) if is_security_root(path_expr) => {
            security_path_type(path_expr)
        }
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

/// Whether `path_expr` is rooted at the `reliability` keyword (the Live
/// Reliability Supplement's `reliability.in_range`, see `docs/reference/
/// live-reliability.md`) rather than the ordinary `message` root.
fn is_reliability_root(path_expr: &PathExpr) -> bool {
    matches!(path_expr.segments.first(), Some(PathSegment::Field(s)) if s == "reliability")
}

/// `reliability.in_range` (exactly two segments, the second literally
/// `in_range`) types as `Bool`; anything else under the `reliability` root
/// is `Unknown` here — `check_value_expr`'s dedicated E-173 check is what
/// actually rejects it, since (unlike an unresolvable `message.*` path,
/// which is tolerated as legitimately dynamic) every `reliability.*` shape
/// is fully known at compile time and a typo should never be silently
/// accepted.
fn reliability_path_type(path_expr: &PathExpr) -> EcelType {
    match path_expr.segments.as_slice() {
        [PathSegment::Field(_root), PathSegment::Field(name)] if name == "in_range" => {
            EcelType::Bool
        }
        _ => EcelType::Unknown,
    }
}

/// Whether `path_expr` is rooted at the `performance` keyword (the
/// Performance Supplement's `performance.in_budget`, see `docs/reference/
/// performance-supplement.md`) rather than the ordinary `message` root.
fn is_performance_root(path_expr: &PathExpr) -> bool {
    matches!(path_expr.segments.first(), Some(PathSegment::Field(s)) if s == "performance")
}

/// `performance.in_budget` (exactly two segments, the second literally
/// `in_budget`) types as `Bool`; anything else under the `performance`
/// root is `Unknown` here — mirrors `reliability_path_type` exactly, same
/// rationale: every `performance.*` shape is fully known at compile time,
/// so a typo should never be silently accepted the way an unresolvable
/// `message.*` path is.
fn performance_path_type(path_expr: &PathExpr) -> EcelType {
    match path_expr.segments.as_slice() {
        [PathSegment::Field(_root), PathSegment::Field(name)] if name == "in_budget" => {
            EcelType::Bool
        }
        _ => EcelType::Unknown,
    }
}

/// Whether `path_expr` is rooted at the `safety` keyword (the Safety
/// Supplement's `safety.sil_maintained`, see `docs/reference/
/// safety-supplement.md`) rather than the ordinary `message` root.
fn is_safety_root(path_expr: &PathExpr) -> bool {
    matches!(path_expr.segments.first(), Some(PathSegment::Field(s)) if s == "safety")
}

/// `safety.sil_maintained` (exactly two segments, the second literally
/// `sil_maintained`) types as `Bool`; anything else under the `safety`
/// root is `Unknown` here — mirrors `reliability_path_type`/
/// `performance_path_type` exactly.
fn safety_path_type(path_expr: &PathExpr) -> EcelType {
    match path_expr.segments.as_slice() {
        [PathSegment::Field(_root), PathSegment::Field(name)] if name == "sil_maintained" => {
            EcelType::Bool
        }
        _ => EcelType::Unknown,
    }
}

/// Whether `path_expr` is rooted at the `security` keyword (the Security
/// Supplement's `security.control_effective`, see `docs/reference/
/// security-supplement.md`) rather than the ordinary `message` root.
fn is_security_root(path_expr: &PathExpr) -> bool {
    matches!(path_expr.segments.first(), Some(PathSegment::Field(s)) if s == "security")
}

/// `security.control_effective` (exactly two segments, the second
/// literally `control_effective`) types as `Bool`; anything else under
/// the `security` root is `Unknown` here — mirrors `safety_path_type`
/// exactly.
fn security_path_type(path_expr: &PathExpr) -> EcelType {
    match path_expr.segments.as_slice() {
        [PathSegment::Field(_root), PathSegment::Field(name)] if name == "control_effective" => {
            EcelType::Bool
        }
        _ => EcelType::Unknown,
    }
}

fn path_to_string(path_expr: &PathExpr) -> String {
    path_expr
        .segments
        .iter()
        .map(|s| match s {
            PathSegment::Field(f) => f.clone(),
            PathSegment::Wildcard => "[*]".to_string(),
            PathSegment::Index(i) => format!("[{i}]"),
            PathSegment::QuotedKey(k) => format!("[\"{k}\"]"),
        })
        .collect::<Vec<_>>()
        .join(".")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal document (inline messages, no `asyncapi_imports` needed —
    /// see spec Section 5.4.1) with one barrier whose branch condition is
    /// `condition_yaml`, optionally declaring `etdl.live-reliability`.
    fn doc_with_condition(condition_yaml: &str, declare_supplement: bool) -> EtlDocument {
        doc_with_condition_and_supplement(condition_yaml, declare_supplement, "etdl.live-reliability", "1.0")
    }

    /// Generalizes [`doc_with_condition`] to declare an arbitrary
    /// supplement id/version — used for `performance.in_budget`'s own
    /// mirrored test set below.
    fn doc_with_condition_and_supplement(
        condition_yaml: &str,
        declare_supplement: bool,
        supplement_id: &str,
        supplement_version: &str,
    ) -> EtlDocument {
        let supplements = if declare_supplement {
            format!("supplements:\n  - id: {supplement_id}\n    version: \"{supplement_version}\"\n")
        } else {
            String::new()
        };
        let yaml = format!(
            r##"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
{supplements}components:
  messages:
    M:
      payload: {{ type: object }}
eventTrees:
  T:
    initiatingEvent: {{ id: I, message: "#/components/messages/M", next: B }}
    nodes:
      B:
        type: barrier
        branches:
          - outcome: NORMAL
            condition: {condition_yaml}
            next: C
          - outcome: ABNORMAL
            condition: default
            next: C
      C: {{ type: consequence, operation: terminate }}
"##
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    fn type_check(doc: &EtlDocument) -> Vec<Diagnostic> {
        let registry = AsyncApiRegistry::new();
        let mut diagnostics = Vec::new();
        type_check_conditions(doc, &registry, &mut diagnostics);
        diagnostics
    }

    #[test]
    fn reliability_in_range_without_the_supplement_declared_is_e173() {
        let doc = doc_with_condition("\"reliability.in_range == true\"", false);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-173"), "got {diagnostics:?}");
    }

    #[test]
    fn reliability_in_range_with_the_supplement_declared_has_no_diagnostics() {
        let doc = doc_with_condition("\"reliability.in_range == true\"", true);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    }

    #[test]
    fn reliability_wrong_shape_is_e173_even_with_the_supplement_declared() {
        let doc = doc_with_condition("\"reliability.something_else == true\"", true);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-173"), "got {diagnostics:?}");
    }

    #[test]
    fn reliability_compared_against_a_string_is_still_e173_not_v204() {
        // Wrong shape is always E-173, regardless of what it's compared
        // against — V-204 would otherwise also fire (Bool vs String) and
        // duplicate the report with a less specific message.
        let doc = doc_with_condition("\"reliability.something_else == \\\"x\\\"\"", true);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-173"), "got {diagnostics:?}");
    }

    #[test]
    fn reliability_in_range_combined_with_and_is_e173() {
        // Must be the entire branch condition, not nested inside a
        // combinator — codegen only special-cases the bare top-level
        // comparison shape.
        let doc = doc_with_condition(
            "\"reliability.in_range == true && message.payload.qty > 0\"",
            true,
        );
        let diagnostics = type_check(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E-173" && d.message.contains("entire branch condition")),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn reliability_in_range_negated_is_e173() {
        let doc = doc_with_condition("\"!(reliability.in_range == true)\"", true);
        let diagnostics = type_check(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E-173" && d.message.contains("entire branch condition")),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn reliability_in_range_compared_against_a_number_is_v204() {
        // Correct path shape (typed Bool), but a mismatched comparison
        // partner — this is an ordinary V-204 type mismatch, not E-173.
        let doc = doc_with_condition("\"reliability.in_range == 1\"", true);
        let diagnostics = type_check(&doc);
        assert!(!diagnostics.iter().any(|d| d.code == "E-173"), "got {diagnostics:?}");
        assert!(diagnostics.iter().any(|d| d.code == "V-204"), "got {diagnostics:?}");
    }

    fn doc_with_performance_condition(condition_yaml: &str, declare_supplement: bool) -> EtlDocument {
        doc_with_condition_and_supplement(condition_yaml, declare_supplement, "etdl.performance", "1.0")
    }

    /// Generalizes further still: an arbitrary set of declared supplement
    /// ids (all version `"1.0"`) — needed for `safety.sil_maintained`,
    /// the one ECEL path gated on *two* supplements at once rather than
    /// just one.
    fn doc_with_condition_and_supplements(condition_yaml: &str, supplement_ids: &[&str]) -> EtlDocument {
        let supplements = if supplement_ids.is_empty() {
            String::new()
        } else {
            let mut s = String::from("supplements:\n");
            for id in supplement_ids {
                s.push_str(&format!("  - id: {id}\n    version: \"1.0\"\n"));
            }
            s
        };
        let yaml = format!(
            r##"
etdl: "1.0.0"
info: {{ title: "T", version: "1.0.0", domain: "D" }}
{supplements}components:
  messages:
    M:
      payload: {{ type: object }}
eventTrees:
  T:
    initiatingEvent: {{ id: I, message: "#/components/messages/M", next: B }}
    nodes:
      B:
        type: barrier
        branches:
          - outcome: NORMAL
            condition: {condition_yaml}
            next: C
          - outcome: ABNORMAL
            condition: default
            next: C
      C: {{ type: consequence, operation: terminate }}
"##
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn performance_in_budget_without_the_supplement_declared_is_e163() {
        let doc = doc_with_performance_condition("\"performance.in_budget == true\"", false);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-163"), "got {diagnostics:?}");
    }

    #[test]
    fn performance_in_budget_with_the_supplement_declared_has_no_diagnostics() {
        let doc = doc_with_performance_condition("\"performance.in_budget == true\"", true);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    }

    #[test]
    fn performance_wrong_shape_is_e163_even_with_the_supplement_declared() {
        let doc = doc_with_performance_condition("\"performance.something_else == true\"", true);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-163"), "got {diagnostics:?}");
    }

    #[test]
    fn performance_compared_against_a_string_is_still_e163_not_v204() {
        let doc = doc_with_performance_condition("\"performance.something_else == \\\"x\\\"\"", true);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-163"), "got {diagnostics:?}");
    }

    #[test]
    fn performance_in_budget_combined_with_and_is_e163() {
        let doc = doc_with_performance_condition(
            "\"performance.in_budget == true && message.payload.qty > 0\"",
            true,
        );
        let diagnostics = type_check(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E-163" && d.message.contains("entire branch condition")),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn performance_in_budget_negated_is_e163() {
        let doc = doc_with_performance_condition("\"!(performance.in_budget == true)\"", true);
        let diagnostics = type_check(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E-163" && d.message.contains("entire branch condition")),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn performance_in_budget_compared_against_a_number_is_v204() {
        let doc = doc_with_performance_condition("\"performance.in_budget == 1\"", true);
        let diagnostics = type_check(&doc);
        assert!(!diagnostics.iter().any(|d| d.code == "E-163"), "got {diagnostics:?}");
        assert!(diagnostics.iter().any(|d| d.code == "V-204"), "got {diagnostics:?}");
    }

    #[test]
    fn safety_sil_maintained_without_either_supplement_is_e135() {
        let doc = doc_with_condition_and_supplements("\"safety.sil_maintained == true\"", &[]);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-135"), "got {diagnostics:?}");
    }

    #[test]
    fn safety_sil_maintained_with_only_safety_declared_is_e135() {
        // The two-supplement gate `safety.sil_maintained` alone has:
        // `etdl.safety` isn't enough on its own, unlike every other ECEL
        // path in this codebase, each gated on exactly one supplement.
        let doc = doc_with_condition_and_supplements(
            "\"safety.sil_maintained == true\"",
            &["etdl.safety"],
        );
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-135"), "got {diagnostics:?}");
    }

    #[test]
    fn safety_sil_maintained_with_only_live_reliability_declared_is_e135() {
        let doc = doc_with_condition_and_supplements(
            "\"safety.sil_maintained == true\"",
            &["etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-135"), "got {diagnostics:?}");
    }

    #[test]
    fn safety_sil_maintained_with_both_supplements_declared_has_no_diagnostics() {
        let doc = doc_with_condition_and_supplements(
            "\"safety.sil_maintained == true\"",
            &["etdl.safety", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    }

    #[test]
    fn safety_wrong_shape_is_e135_even_with_both_supplements_declared() {
        let doc = doc_with_condition_and_supplements(
            "\"safety.something_else == true\"",
            &["etdl.safety", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-135"), "got {diagnostics:?}");
    }

    #[test]
    fn safety_sil_maintained_combined_with_and_is_e135() {
        let doc = doc_with_condition_and_supplements(
            "\"safety.sil_maintained == true && message.payload.qty > 0\"",
            &["etdl.safety", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E-135" && d.message.contains("entire branch condition")),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn safety_sil_maintained_negated_is_e135() {
        let doc = doc_with_condition_and_supplements(
            "\"!(safety.sil_maintained == true)\"",
            &["etdl.safety", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E-135" && d.message.contains("entire branch condition")),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn safety_sil_maintained_compared_against_a_number_is_v204() {
        let doc = doc_with_condition_and_supplements(
            "\"safety.sil_maintained == 1\"",
            &["etdl.safety", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(!diagnostics.iter().any(|d| d.code == "E-135"), "got {diagnostics:?}");
        assert!(diagnostics.iter().any(|d| d.code == "V-204"), "got {diagnostics:?}");
    }

    #[test]
    fn security_control_effective_without_either_supplement_is_e143() {
        let doc = doc_with_condition_and_supplements("\"security.control_effective == true\"", &[]);
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-143"), "got {diagnostics:?}");
    }

    #[test]
    fn security_control_effective_with_only_security_declared_is_e143() {
        // The two-supplement gate `security.control_effective` alone has:
        // `etdl.security` isn't enough on its own, unlike every other ECEL
        // path in this codebase (except `safety.sil_maintained`), each
        // gated on exactly one supplement.
        let doc = doc_with_condition_and_supplements(
            "\"security.control_effective == true\"",
            &["etdl.security"],
        );
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-143"), "got {diagnostics:?}");
    }

    #[test]
    fn security_control_effective_with_only_live_reliability_declared_is_e143() {
        let doc = doc_with_condition_and_supplements(
            "\"security.control_effective == true\"",
            &["etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-143"), "got {diagnostics:?}");
    }

    #[test]
    fn security_control_effective_with_both_supplements_declared_has_no_diagnostics() {
        let doc = doc_with_condition_and_supplements(
            "\"security.control_effective == true\"",
            &["etdl.security", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    }

    #[test]
    fn security_wrong_shape_is_e143_even_with_both_supplements_declared() {
        let doc = doc_with_condition_and_supplements(
            "\"security.something_else == true\"",
            &["etdl.security", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(diagnostics.iter().any(|d| d.code == "E-143"), "got {diagnostics:?}");
    }

    #[test]
    fn security_control_effective_combined_with_and_is_e143() {
        let doc = doc_with_condition_and_supplements(
            "\"security.control_effective == true && message.payload.qty > 0\"",
            &["etdl.security", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E-143" && d.message.contains("entire branch condition")),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn security_control_effective_negated_is_e143() {
        let doc = doc_with_condition_and_supplements(
            "\"!(security.control_effective == true)\"",
            &["etdl.security", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E-143" && d.message.contains("entire branch condition")),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn security_control_effective_compared_against_a_number_is_v204() {
        let doc = doc_with_condition_and_supplements(
            "\"security.control_effective == 1\"",
            &["etdl.security", "etdl.live-reliability"],
        );
        let diagnostics = type_check(&doc);
        assert!(!diagnostics.iter().any(|d| d.code == "E-143"), "got {diagnostics:?}");
        assert!(diagnostics.iter().any(|d| d.code == "V-204"), "got {diagnostics:?}");
    }
}
