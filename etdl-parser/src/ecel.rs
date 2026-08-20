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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    Default,
    Comparison(Comparison),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub left: Operand,
    pub op: Comparator,
    pub right: Operand,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operand {
    Path(PathExpr),
    Literal(Literal),
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

    match parse_comparison(input) {
        Ok((remaining, comparison)) => {
            if remaining.trim().is_empty() {
                Ok(Condition::Comparison(comparison))
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
        map(parse_path_expr, Operand::Path),
        map(parse_literal, Operand::Literal),
    ))(input)
}

fn parse_path_expr(input: &str) -> IResult<&str, PathExpr> {
    let (input, root) = parse_root_var(input)?;
    let (input, segments) = many0(parse_member_access)(input)?;
    let mut all_segments = vec![PathSegment::Field(root)];
    all_segments.extend(segments);
    Ok((input, PathExpr::new(all_segments)))
}

fn parse_root_var(input: &str) -> IResult<&str, String> {
    let (input, _) = tag("message")(input)?;
    Ok((input, "message".to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_condition() {
        assert_eq!(parse_condition("default").unwrap(), Condition::Default);
    }

    #[test]
    fn test_simple_comparison() {
        let cond = parse_condition("message.payload.status == \"ok\"").unwrap();
        match cond {
            Condition::Comparison(c) => {
                assert_eq!(c.op, Comparator::Eq);
                match &c.left {
                    Operand::Path(p) => assert_eq!(p.segments.len(), 3),
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
    fn test_wildcard_path() {
        let cond = parse_condition("message.payload.items[*].qty > 0").unwrap();
        match cond {
            Condition::Comparison(c) => {
                match &c.left {
                    Operand::Path(p) => {
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
            Condition::Comparison(c) => {
                assert_eq!(c.op, Comparator::In);
            }
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_matches_operator() {
        let cond = parse_condition("message.payload.email matches \"@\"").unwrap();
        match cond {
            Condition::Comparison(c) => {
                assert_eq!(c.op, Comparator::Matches);
            }
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_bracket_index() {
        let cond = parse_condition("message.payload.items[0].name == \"test\"").unwrap();
        match cond {
            Condition::Comparison(c) => match &c.left {
                Operand::Path(p) => {
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
            Condition::Comparison(c) => match &c.left {
                Operand::Path(p) => {
                    assert_eq!(p.segments[3], PathSegment::Index(usize::MAX));
                }
                _ => panic!("expected path"),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn trailing_content_is_error() {
        assert!(parse_condition("message.payload.ok == true && message.payload.x").is_err());
    }

    #[test]
    fn default_is_parsed() {
        assert!(matches!(parse_condition("default"), Ok(Condition::Default)));
    }

    #[test]
    fn negated_numbers_parse() {
        let cond = parse_condition("message.payload.temp < -5").unwrap();
        match cond {
            Condition::Comparison(c) => match &c.right {
                Operand::Literal(Literal::Number(n)) => assert_eq!(*n, -5.0),
                _ => panic!("expected negative number"),
            },
            _ => panic!("expected comparison"),
        }
    }
}
