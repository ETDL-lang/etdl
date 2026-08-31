use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::{alpha1, alphanumeric1, char, digit1, multispace0},
    combinator::{map, opt, recognize, value},
    multi::{many0, separated_list0},
    sequence::{delimited, pair, preceded},
    IResult,
};
use serde::{Deserialize, Serialize};

/// Conformance floor for the total operand count in one `condition-expr`
/// (spec §6.2): a Conforming Parser MUST accept at least this many. This is
/// the actual limit this implementation enforces (well above the floor);
/// see `count_operands` in `etdl-compiler`'s typeck module (rule V-206).
pub const MAX_CONDITION_OPERANDS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    Default,
    Expr(BoolExpr),
}

/// A boolean expression: any combination of `comparison`/`quantifier-expr`/
/// `defined-expr` joined by `&&`, `||`, and unary `!` (spec §6.2). A bare
/// `Comparison` (no combinator) is the pre-existing, common case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BoolExpr {
    And(Box<BoolExpr>, Box<BoolExpr>),
    Or(Box<BoolExpr>, Box<BoolExpr>),
    Not(Box<BoolExpr>),
    Comparison(Comparison),
    Quantifier(QuantifierExpr),
    Defined(PathExpr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub left: Operand,
    pub op: Comparator,
    pub right: Operand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuantifierKind {
    Any,
    All,
}

/// `quantifier "(" path-expr "," comparison ")"` (spec §6.2, §6.4). The
/// inner `comparison` is deliberately a plain `Comparison`, not a
/// `BoolExpr` — a quantifier's inner test MUST NOT contain `&&`/`||`/`!`
/// (spec §6.4), so this can't nest arbitrary boolean expressions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuantifierExpr {
    pub kind: QuantifierKind,
    pub path: PathExpr,
    pub comparison: Comparison,
}

/// An `operand` (spec §6.2): either a `value-expr` (a path, number,
/// arithmetic expression, or built-in function call) or a non-numeric
/// `literal` (string/bool/null/array). A bare numeric literal always
/// parses as `Value(ValueExpr::Number(_))`, never `Literal(Literal::Number(_))`
/// — `value-expr` is tried first in the grammar's ordered choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operand {
    Value(ValueExpr),
    Literal(Literal),
}

/// `value-expr` (spec §6.2): a `path-expr`/`number`/`func-call`, optionally
/// combined with `+`/`-`/`*`/`/`. A bare `Path`/`Number`, with no
/// arithmetic operator, is the pre-existing, common case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ValueExpr {
    Path(PathExpr),
    Number(f64),
    Call(FuncName, Box<ValueExpr>),
    Add(Box<ValueExpr>, Box<ValueExpr>),
    Sub(Box<ValueExpr>, Box<ValueExpr>),
    Mul(Box<ValueExpr>, Box<ValueExpr>),
    Div(Box<ValueExpr>, Box<ValueExpr>),
}

/// The fixed, non-extensible built-in function set (spec §6.5.1) — not a
/// general function-call mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FuncName {
    Length,
    Abs,
    Lower,
    Upper,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathExpr {
    pub segments: Vec<PathSegment>,
}

impl PathExpr {
    pub fn new(segments: Vec<PathSegment>) -> Self {
        PathExpr { segments }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PathSegment {
    Field(String),
    Wildcard,
    Index(usize),
    QuotedKey(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Comparator {
    Eq,
    Neq,
    Gte,
    Lte,
    Gt,
    Lt,
    In,
    Matches,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Array(Vec<Literal>),
}

pub fn parse_condition(input: &str) -> Result<Condition, String> {
    if input.trim() == "default" {
        return Ok(Condition::Default);
    }

    match parse_bool_expr(input) {
        Ok((remaining, expr)) => {
            if remaining.trim().is_empty() {
                Ok(Condition::Expr(expr))
            } else {
                Err(format!(
                    "trailing content in condition expression: '{}'",
                    remaining
                ))
            }
        }
        Err(e) => Err(format!("failed to parse condition expression: {}", e)),
    }
}

// --- bool-expr / bool-term / bool-factor / bool-atom (standard precedence
// climbing: `!` binds tightest, then `&&`, then `||`; spec §6.2, §6.5). ---

fn parse_bool_expr(input: &str) -> IResult<&str, BoolExpr> {
    let (input, first) = parse_bool_term(input)?;
    let (input, rest) = many0(preceded(
        delimited(multispace0, tag("||"), multispace0),
        parse_bool_term,
    ))(input)?;
    Ok((
        input,
        rest.into_iter()
            .fold(first, |acc, next| BoolExpr::Or(Box::new(acc), Box::new(next))),
    ))
}

fn parse_bool_term(input: &str) -> IResult<&str, BoolExpr> {
    let (input, first) = parse_bool_factor(input)?;
    let (input, rest) = many0(preceded(
        delimited(multispace0, tag("&&"), multispace0),
        parse_bool_factor,
    ))(input)?;
    Ok((
        input,
        rest.into_iter()
            .fold(first, |acc, next| BoolExpr::And(Box::new(acc), Box::new(next))),
    ))
}

fn parse_bool_factor(input: &str) -> IResult<&str, BoolExpr> {
    alt((
        map(
            preceded(pair(char('!'), multispace0), parse_bool_atom),
            |e| BoolExpr::Not(Box::new(e)),
        ),
        parse_bool_atom,
    ))(input)
}

fn parse_bool_atom(input: &str) -> IResult<&str, BoolExpr> {
    alt((
        map(parse_quantifier_expr, BoolExpr::Quantifier),
        map(parse_defined_expr, BoolExpr::Defined),
        map(parse_comparison, BoolExpr::Comparison),
        delimited(
            pair(char('('), multispace0),
            parse_bool_expr,
            pair(multispace0, char(')')),
        ),
    ))(input)
}

fn parse_quantifier_expr(input: &str) -> IResult<&str, QuantifierExpr> {
    let (input, kind) = alt((
        value(QuantifierKind::Any, tag("any")),
        value(QuantifierKind::All, tag("all")),
    ))(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, path) = parse_path_expr(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(',')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, comparison) = parse_comparison(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    Ok((
        input,
        QuantifierExpr {
            kind,
            path,
            comparison,
        },
    ))
}

fn parse_defined_expr(input: &str) -> IResult<&str, PathExpr> {
    let (input, _) = tag("defined")(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, path) = parse_path_expr(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    Ok((input, path))
}

fn parse_comparison(input: &str) -> IResult<&str, Comparison> {
    let (input, left) = parse_operand(input)?;
    let (input, _) = multispace0(input)?;
    let (input, op) = parse_comparator(input)?;
    let (input, _) = multispace0(input)?;
    let (input, right) = parse_operand(input)?;
    Ok((input, Comparison { left, op, right }))
}

fn parse_operand(input: &str) -> IResult<&str, Operand> {
    alt((
        map(parse_value_expr, Operand::Value),
        map(parse_literal, Operand::Literal),
    ))(input)
}

// --- value-expr / value-term / value-atom (spec §6.2, §6.5: `*`/`/` bind
// tighter than `+`/`-`). ---

fn parse_value_expr(input: &str) -> IResult<&str, ValueExpr> {
    let (input, first) = parse_value_term(input)?;
    let (input, rest) = many0(pair(
        delimited(multispace0, alt((char('+'), char('-'))), multispace0),
        parse_value_term,
    ))(input)?;
    Ok((
        input,
        rest.into_iter().fold(first, |acc, (op, next)| match op {
            '+' => ValueExpr::Add(Box::new(acc), Box::new(next)),
            _ => ValueExpr::Sub(Box::new(acc), Box::new(next)),
        }),
    ))
}

fn parse_value_term(input: &str) -> IResult<&str, ValueExpr> {
    let (input, first) = parse_value_atom(input)?;
    let (input, rest) = many0(pair(
        delimited(multispace0, alt((char('*'), char('/'))), multispace0),
        parse_value_atom,
    ))(input)?;
    Ok((
        input,
        rest.into_iter().fold(first, |acc, (op, next)| match op {
            '*' => ValueExpr::Mul(Box::new(acc), Box::new(next)),
            _ => ValueExpr::Div(Box::new(acc), Box::new(next)),
        }),
    ))
}

fn parse_value_atom(input: &str) -> IResult<&str, ValueExpr> {
    alt((
        parse_func_call,
        map(parse_path_expr, ValueExpr::Path),
        map(parse_number, ValueExpr::Number),
        delimited(
            pair(char('('), multispace0),
            parse_value_expr,
            pair(multispace0, char(')')),
        ),
    ))(input)
}

fn parse_func_call(input: &str) -> IResult<&str, ValueExpr> {
    let (input, name) = alt((
        value(FuncName::Length, tag("length")),
        value(FuncName::Abs, tag("abs")),
        value(FuncName::Lower, tag("lower")),
        value(FuncName::Upper, tag("upper")),
    ))(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, arg) = parse_value_expr(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    Ok((input, ValueExpr::Call(name, Box::new(arg))))
}

fn parse_path_expr(input: &str) -> IResult<&str, PathExpr> {
    let (input, root) = parse_root_var(input)?;
    let (input, segments) = many0(parse_member_access)(input)?;
    let mut all_segments = vec![PathSegment::Field(root)];
    all_segments.extend(segments);
    Ok((input, PathExpr::new(all_segments)))
}

/// `message` is the ordinary root every path expression has always used.
/// `reliability` and `performance` are narrower roots two optional,
/// off-by-default supplements each reserve — grammar-wise both parse like
/// any other path (`reliability.in_range`, `performance.in_budget`,
/// generic dotted chains), but only their one specific exact shape is
/// meaningful; anything else under either root is a type error reported by
/// `etdl-compiler::typeck`, not a parse error, matching how this codebase
/// already prefers "structure via grammar, rules via explicit checks" over
/// rejecting shapes at parse time.
fn parse_root_var(input: &str) -> IResult<&str, String> {
    alt((
        value("message".to_string(), tag("message")),
        value("reliability".to_string(), tag("reliability")),
        value("performance".to_string(), tag("performance")),
    ))(input)
}

fn parse_member_access(input: &str) -> IResult<&str, PathSegment> {
    alt((parse_dot_access, parse_bracket_access))(input)
}

fn parse_dot_access(input: &str) -> IResult<&str, PathSegment> {
    let (input, _) = char('.')(input)?;
    let (input, ident) = parse_identifier(input)?;
    Ok((input, PathSegment::Field(ident)))
}

fn parse_bracket_access(input: &str) -> IResult<&str, PathSegment> {
    delimited(
        char('['),
        alt((
            map(tag("*"), |_| PathSegment::Wildcard),
            map(parse_index, PathSegment::Index),
            map(parse_quoted_key, PathSegment::QuotedKey),
        )),
        char(']'),
    )(input)
}

fn parse_identifier(input: &str) -> IResult<&str, String> {
    map(
        recognize(pair(alpha1, many0(alt((alphanumeric1, tag("_")))))),
        |s: &str| s.to_string(),
    )(input)
}

fn parse_index(input: &str) -> IResult<&str, usize> {
    // `s.parse::<usize>()` panics on overflow (>= 20 digits); use a saturating
    // fold so untrusted input can never crash the parser.
    map(digit1, |s: &str| {
        s.bytes().fold(0usize, |acc, b| {
            acc.saturating_mul(10).saturating_add((b - b'0') as usize)
        })
    })(input)
}

fn parse_quoted_key(input: &str) -> IResult<&str, String> {
    delimited(
        char('"'),
        map(
            take_while1(|c: char| c != '"' && ('\x20'..='\x7e').contains(&c)),
            |s: &str| s.to_string(),
        ),
        char('"'),
    )(input)
}

fn parse_comparator(input: &str) -> IResult<&str, Comparator> {
    alt((
        value(Comparator::Eq, tag("==")),
        value(Comparator::Neq, tag("!=")),
        value(Comparator::Gte, tag(">=")),
        value(Comparator::Lte, tag("<=")),
        value(Comparator::Gt, tag(">")),
        value(Comparator::Lt, tag("<")),
        value(Comparator::In, tag("in")),
        value(Comparator::Matches, tag("matches")),
    ))(input)
}

fn parse_literal(input: &str) -> IResult<&str, Literal> {
    alt((
        value(Literal::Bool(true), tag("true")),
        value(Literal::Bool(false), tag("false")),
        value(Literal::Null, tag("null")),
        map(parse_number, Literal::Number),
        map(parse_string_literal, Literal::String),
        map(parse_array_literal, Literal::Array),
    ))(input)
}

fn parse_array_literal(input: &str) -> IResult<&str, Vec<Literal>> {
    delimited(
        char('['),
        preceded(
            multispace0,
            separated_list0(
                preceded(multispace0, char(',')),
                preceded(multispace0, parse_literal),
            ),
        ),
        preceded(multispace0, char(']')),
    )(input)
}

fn parse_number(input: &str) -> IResult<&str, f64> {
    let (input, sign) = opt(char('-'))(input)?;
    let (input, int_part) = digit1(input)?;
    let (input, frac_part) = opt(preceded(char('.'), digit1))(input)?;

    let num_str = format!(
        "{}{}{}",
        sign.map(|_| "-").unwrap_or(""),
        int_part,
        frac_part.map(|f| format!(".{}", f)).unwrap_or_default()
    );
    let value = num_str.parse::<f64>().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
    })?;
    Ok((input, value))
}

fn parse_string_literal(input: &str) -> IResult<&str, String> {
    delimited(
        char('"'),
        map(take_while1(|c: char| c != '"'), |s: &str| s.to_string()),
        char('"'),
    )(input)
}

/// Counts operands (every `Comparison` leaf's two operands, plus every
/// arithmetic/function operand nested within them) in a `BoolExpr` tree, for
/// rule V-206's conformance-floor check (spec §6.2).
pub fn count_operands(expr: &BoolExpr) -> usize {
    match expr {
        BoolExpr::And(a, b) | BoolExpr::Or(a, b) => count_operands(a) + count_operands(b),
        BoolExpr::Not(a) => count_operands(a),
        BoolExpr::Comparison(cmp) => count_operand(&cmp.left) + count_operand(&cmp.right),
        BoolExpr::Quantifier(q) => {
            count_operand(&q.comparison.left) + count_operand(&q.comparison.right)
        }
        BoolExpr::Defined(_) => 1,
    }
}

fn count_operand(operand: &Operand) -> usize {
    match operand {
        Operand::Value(v) => count_value_expr(v),
        Operand::Literal(_) => 1,
    }
}

fn count_value_expr(expr: &ValueExpr) -> usize {
    match expr {
        ValueExpr::Path(_) | ValueExpr::Number(_) => 1,
        ValueExpr::Call(_, arg) => count_value_expr(arg),
        ValueExpr::Add(a, b)
        | ValueExpr::Sub(a, b)
        | ValueExpr::Mul(a, b)
        | ValueExpr::Div(a, b) => count_value_expr(a) + count_value_expr(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_comparison(expr: &BoolExpr) -> &Comparison {
        match expr {
            BoolExpr::Comparison(c) => c,
            _ => panic!("expected a bare comparison, got {:?}", expr),
        }
    }

    #[test]
    fn test_default_condition() {
        assert_eq!(parse_condition("default").unwrap(), Condition::Default);
    }

    #[test]
    fn test_simple_comparison() {
        let cond = parse_condition("message.payload.status == \"ok\"").unwrap();
        match cond {
            Condition::Expr(expr) => {
                let c = as_comparison(&expr);
                assert_eq!(c.op, Comparator::Eq);
                match &c.left {
                    Operand::Value(ValueExpr::Path(p)) => assert_eq!(p.segments.len(), 3),
                    _ => panic!("expected path"),
                }
                match &c.right {
                    Operand::Literal(Literal::String(s)) => assert_eq!(s, "ok"),
                    _ => panic!("expected string literal"),
                }
            }
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_reliability_root_parses_like_any_other_path() {
        // Grammar-level only: parses generically under the new `reliability`
        // root exactly like `message.*` paths always have. Whether
        // `reliability.in_range` specifically is meaningful (vs. any other
        // suffix under this root) is `etdl-compiler::typeck`'s job, not the
        // parser's — see `parse_root_var`'s doc comment.
        let cond = parse_condition("reliability.in_range == true").unwrap();
        match cond {
            Condition::Expr(expr) => {
                let c = as_comparison(&expr);
                assert_eq!(c.op, Comparator::Eq);
                match &c.left {
                    Operand::Value(ValueExpr::Path(p)) => {
                        assert_eq!(p.segments.len(), 2);
                        assert_eq!(p.segments[0], PathSegment::Field("reliability".to_string()));
                        assert_eq!(p.segments[1], PathSegment::Field("in_range".to_string()));
                    }
                    _ => panic!("expected path"),
                }
                match &c.right {
                    Operand::Literal(Literal::Bool(b)) => assert!(*b),
                    _ => panic!("expected bool literal"),
                }
            }
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_performance_root_parses_like_any_other_path() {
        // Grammar-level only, mirroring the `reliability` root test above:
        // `performance.in_budget` (the Performance Supplement's own
        // `barrierChecks`-linked ECEL path) parses generically, no special
        // grammar case — see `parse_root_var`'s doc comment.
        let cond = parse_condition("performance.in_budget == true").unwrap();
        match cond {
            Condition::Expr(expr) => {
                let c = as_comparison(&expr);
                assert_eq!(c.op, Comparator::Eq);
                match &c.left {
                    Operand::Value(ValueExpr::Path(p)) => {
                        assert_eq!(p.segments.len(), 2);
                        assert_eq!(p.segments[0], PathSegment::Field("performance".to_string()));
                        assert_eq!(p.segments[1], PathSegment::Field("in_budget".to_string()));
                    }
                    _ => panic!("expected path"),
                }
                match &c.right {
                    Operand::Literal(Literal::Bool(b)) => assert!(*b),
                    _ => panic!("expected bool literal"),
                }
            }
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_wildcard_path() {
        let cond = parse_condition("message.payload.items[*].qty > 0").unwrap();
        match cond {
            Condition::Expr(expr) => {
                let c = as_comparison(&expr);
                match &c.left {
                    Operand::Value(ValueExpr::Path(p)) => {
                        assert_eq!(p.segments.len(), 5); // message, payload, items, [*], qty
                        assert_eq!(p.segments[3], PathSegment::Wildcard);
                    }
                    _ => panic!("expected path"),
                }
            }
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_in_operator() {
        let cond = parse_condition("message.payload.type in [\"A\", \"B\"]").unwrap();
        match cond {
            Condition::Expr(expr) => assert_eq!(as_comparison(&expr).op, Comparator::In),
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_matches_operator() {
        let cond = parse_condition("message.payload.email matches \"@\"").unwrap();
        match cond {
            Condition::Expr(expr) => assert_eq!(as_comparison(&expr).op, Comparator::Matches),
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_bracket_index() {
        let cond = parse_condition("message.payload.items[0].name == \"test\"").unwrap();
        match cond {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Path(p)) => {
                    assert_eq!(p.segments.len(), 5);
                    assert_eq!(p.segments[3], PathSegment::Index(0));
                }
                _ => panic!("expected path"),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn oversized_index_does_not_panic() {
        // 40 digits overflows usize; must parse to usize::MAX, never panic.
        let idx = "9999999999999999999999999999999999999999";
        let cond =
            parse_condition(&format!("message.payload.items[{}].name == \"test\"", idx)).unwrap();
        match cond {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Path(p)) => {
                    assert_eq!(p.segments[3], PathSegment::Index(usize::MAX));
                }
                _ => panic!("expected path"),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn trailing_content_is_error() {
        // `&&` is now valid grammar — use content the grammar genuinely has
        // no production for.
        assert!(parse_condition("message.payload.ok == true } extra").is_err());
    }

    #[test]
    fn default_is_parsed() {
        assert!(matches!(parse_condition("default"), Ok(Condition::Default)));
    }

    #[test]
    fn negated_numbers_parse() {
        let cond = parse_condition("message.payload.temp < -5").unwrap();
        match cond {
            Condition::Expr(expr) => match &as_comparison(&expr).right {
                Operand::Value(ValueExpr::Number(n)) => assert_eq!(*n, -5.0),
                _ => panic!("expected negative number"),
            },
            _ => panic!("expected comparison"),
        }
    }

    // --- new grammar: boolean combinators ---

    #[test]
    fn and_combinator_parses() {
        let cond =
            parse_condition("message.payload.a > 0 && message.payload.b > 0").unwrap();
        match cond {
            Condition::Expr(BoolExpr::And(_, _)) => {}
            other => panic!("expected And, got {:?}", other),
        }
    }

    #[test]
    fn or_combinator_parses() {
        let cond =
            parse_condition("message.payload.a > 0 || message.payload.b > 0").unwrap();
        match cond {
            Condition::Expr(BoolExpr::Or(_, _)) => {}
            other => panic!("expected Or, got {:?}", other),
        }
    }

    #[test]
    fn not_binds_tighter_than_and() {
        // !a && b  =>  And(Not(a), b)
        let cond =
            parse_condition("!message.payload.a == true && message.payload.b == true").unwrap();
        match cond {
            Condition::Expr(BoolExpr::And(left, _)) => {
                assert!(matches!(*left, BoolExpr::Not(_)));
            }
            other => panic!("expected And(Not(_), _), got {:?}", other),
        }
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // a || b && c  =>  Or(a, And(b, c))
        let cond = parse_condition(
            "message.payload.a == true || message.payload.b == true && message.payload.c == true",
        )
        .unwrap();
        match cond {
            Condition::Expr(BoolExpr::Or(_, right)) => {
                assert!(matches!(*right, BoolExpr::And(_, _)));
            }
            other => panic!("expected Or(_, And(_, _)), got {:?}", other),
        }
    }

    #[test]
    fn parens_override_precedence() {
        // (a || b) && c  =>  And(Or(a, b), c)
        let cond = parse_condition(
            "(message.payload.a == true || message.payload.b == true) && message.payload.c == true",
        )
        .unwrap();
        match cond {
            Condition::Expr(BoolExpr::And(left, _)) => {
                assert!(matches!(*left, BoolExpr::Or(_, _)));
            }
            other => panic!("expected And(Or(_, _), _), got {:?}", other),
        }
    }

    // --- new grammar: arithmetic ---

    #[test]
    fn arithmetic_precedence_mul_over_add() {
        // a + b * c  =>  Add(a, Mul(b, c))
        let cond = parse_condition("message.payload.a + message.payload.b * 2 > 0").unwrap();
        match cond {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Add(_, right)) => {
                    assert!(matches!(**right, ValueExpr::Mul(_, _)));
                }
                other => panic!("expected Add(_, Mul(_, _)), got {:?}", other),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn subtraction_parses() {
        let cond = parse_condition("message.payload.subtotal - message.payload.fee > 0").unwrap();
        match cond {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Sub(_, _)) => {}
                other => panic!("expected Sub, got {:?}", other),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn division_parses() {
        let cond = parse_condition("message.payload.a / message.payload.b > 0").unwrap();
        match cond {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Div(_, _)) => {}
                other => panic!("expected Div, got {:?}", other),
            },
            _ => panic!("expected comparison"),
        }
    }

    // --- new grammar: built-in functions ---

    #[test]
    fn length_function_parses() {
        let cond = parse_condition("length(message.payload.items) > 0").unwrap();
        match cond {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Call(FuncName::Length, _)) => {}
                other => panic!("expected Call(Length, _), got {:?}", other),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn abs_function_parses() {
        let cond = parse_condition("abs(message.payload.delta) < 1").unwrap();
        match cond {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Call(FuncName::Abs, _)) => {}
                other => panic!("expected Call(Abs, _), got {:?}", other),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn lower_and_upper_functions_parse() {
        let lower = parse_condition("lower(message.payload.status) == \"paid\"").unwrap();
        match lower {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Call(FuncName::Lower, _)) => {}
                other => panic!("expected Call(Lower, _), got {:?}", other),
            },
            _ => panic!("expected comparison"),
        }
        let upper = parse_condition("upper(message.payload.status) == \"PAID\"").unwrap();
        match upper {
            Condition::Expr(expr) => match &as_comparison(&expr).left {
                Operand::Value(ValueExpr::Call(FuncName::Upper, _)) => {}
                other => panic!("expected Call(Upper, _), got {:?}", other),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn unknown_function_name_is_rejected() {
        assert!(parse_condition("now(message.payload.x) > 0").is_err());
    }

    // --- new grammar: defined() ---

    #[test]
    fn defined_expr_parses() {
        let cond = parse_condition("defined(message.payload.discountCode)").unwrap();
        match cond {
            Condition::Expr(BoolExpr::Defined(path)) => {
                assert_eq!(path.segments.len(), 3); // message, payload, discountCode
            }
            other => panic!("expected Defined, got {:?}", other),
        }
    }

    #[test]
    fn defined_combines_with_and() {
        let cond = parse_condition(
            "defined(message.payload.discountCode) && message.payload.amount > 0",
        )
        .unwrap();
        assert!(matches!(cond, Condition::Expr(BoolExpr::And(_, _))));
    }

    // --- new grammar: explicit quantifiers ---

    #[test]
    fn explicit_any_quantifier_parses() {
        let cond = parse_condition(
            "any(message.payload.items, message.payload.items[*].qty > 0)",
        )
        .unwrap();
        match cond {
            Condition::Expr(BoolExpr::Quantifier(q)) => {
                assert_eq!(q.kind, QuantifierKind::Any);
            }
            other => panic!("expected Quantifier(Any), got {:?}", other),
        }
    }

    #[test]
    fn explicit_all_quantifier_parses() {
        let cond = parse_condition(
            "all(message.payload.items, message.payload.items[*].qty > 0)",
        )
        .unwrap();
        match cond {
            Condition::Expr(BoolExpr::Quantifier(q)) => {
                assert_eq!(q.kind, QuantifierKind::All);
            }
            other => panic!("expected Quantifier(All), got {:?}", other),
        }
    }

    #[test]
    fn quantifier_inner_expression_rejects_combinators() {
        // The inner test is exactly a `comparison`, not a `bool-expr` — it
        // MUST NOT contain `&&`/`||`/`!` (spec §6.4).
        assert!(parse_condition(
            "any(message.payload.items, message.payload.items[*].qty > 0 && true)"
        )
        .is_err());
    }

    // --- operand-count ceiling (rule V-206 groundwork) ---

    #[test]
    fn count_operands_counts_comparison_leaves() {
        let cond = parse_condition(
            "message.payload.a > 0 && message.payload.b > 0 && message.payload.c > 0",
        )
        .unwrap();
        match cond {
            Condition::Expr(expr) => assert_eq!(count_operands(&expr), 6),
            _ => panic!("expected expr"),
        }
    }

    #[test]
    fn count_operands_counts_nested_arithmetic() {
        // a + b * c > 0  => operands: a, b, c, 0  (four leaves)
        let cond = parse_condition("message.payload.a + message.payload.b * 2 > 0").unwrap();
        match cond {
            Condition::Expr(expr) => assert_eq!(count_operands(&expr), 4),
            _ => panic!("expected expr"),
        }
    }
}
