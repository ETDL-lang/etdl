# ECEL — Event-tree Condition Expression Language

ECEL is ETDL's condition language for barrier branches and other boolean guards.
It is small, typed, side-effect-free, and deterministic (ETDL §6).

## Grammar

```
condition-expr   = "default" / comparison
comparison       = operand *WSP comparator *WSP operand
operand          = path-expr / literal
path-expr        = root-var *member-access
root-var         = "message"
member-access    = "." identifier / "[" ( "*" / index / quoted-key ) "]"
identifier       = ALPHA *( ALPHA / DIGIT / "_" )
index            = 1*DIGIT            (saturating; never panics on overflow)
quoted-key       = DQUOTE *(%x20-21 / %x23-7E) DQUOTE
comparator       = "==" / "!=" / ">=" / "<=" / ">" / "<" / "in" / "matches"
literal          = number / string-literal / "true" / "false" / "null"
number           = ["-"] 1*DIGIT ["." 1*DIGIT]
string-literal   = DQUOTE *(%x20-21 / %x23-7E) DQUOTE
```

Notes:
- String literals have **no escape sequences**; an embedded `"` is impossible.
- Numbers have **no exponent** notation (no `1e-3`).
- A condition is a **single comparison**; boolean combinators (`&&`, `||`, `!`)
  and function calls are not part of the grammar.

## Operators

| Operator | Operands | Result | Lowered to (Rust) |
|---|---|---|---|
| `==` / `!=` | same runtime type | bool | `==` / `!=` |
| `>` `>=` `<` `<=` | both `number` | bool | native comparison |
| `in` | left any, right array literal/path | bool | `etdl_core::condition::contains(&right, &left)` |
| `matches` | left `string`, right RE2 string literal | bool | `etdl_core::condition::matches(left, right)` |

`matches` uses the `regex` crate, an RE2-style linear-time engine (ETDL §6.5).
An invalid pattern yields `false` at runtime (not a panic).

## Wildcards

`message.payload.items[*].qty > 0` is **universal** quantification: the
condition holds iff it holds for every element. Empty arrays are vacuously true.
Generated code lowers it to `message.payload.items.iter().all(|item| item.qty > 0)`.

## Types and type checking

The compiler (`etdl-compiler/src/typeck.rs`) resolves each condition against the
referenced AsyncAPI message schema and reports `V-204` on:

- a field absent from the schema,
- a type mismatch (`number` compared to `string`, etc.),
- an ordering operator on a non-number,
- `in` on a non-array right operand,
- `matches` on a non-string left operand.

When a schema cannot be resolved (missing alias/pointer), type checking is
silently skipped so that reference errors (E-103/E-104) remain the primary
diagnostic.

## `default`

`default` is a parser-level sentinel (not an expression). It matches when no
preceding sibling branch matched and must be the last branch (V-202).

## Error semantics

- Trailing content after a comparison → parse error.
- Oversized `[index]` saturates to `usize::MAX` — **never panics** on untrusted
  input.
- Malformed literals → parse error.

## Evaluation guarantees

ECEL evaluation is pure (no side effects), deterministic, and bounded (no
recursion, no unbounded regex). There is no runtime access to env, filesystem,
or network.

## Examples

```text
message.payload.items[*].qty > 0
message.payload.amount >= 10000
message.payload.status in ["PAID", "AUTHORIZED"]
message.payload.reference matches "^ORD-[0-9]{8}$"
message.headers.trace-id != null
default
```

## Regression tests

`etdl-parser/src/ecel.rs` covers: default, `==`, wildcard, `in`, `matches`,
index `[0]`, oversized index (no panic), trailing content (error), and negative
numbers. `etdl-compiler/src/typeck.rs` is exercised through the integration
suite.
