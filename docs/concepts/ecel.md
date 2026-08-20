# ECEL — Event-tree Condition Expression Language

ECEL (pronounced "E-cell") is the condition language used on barrier branches. It is a small, typed, side-effect-free expression language — in the spirit of CEL (Common Expression Language) — evaluated over the initiating event's AsyncAPI message payload.

## Syntax

A condition is a comparison of an operand to a literal:

```
<path> <operator> <literal>
```

### Operands

| Form | Example | Meaning |
|---|---|---|
| Field path | `message.payload.items` | Navigate the message |
| Array index | `message.payload.items[0]` | Index into an array |
| Wildcard | `message.payload.items[*]` | Any element of the array |
| Quoted key | `message.payload["order-id"]` | Key with special characters |
| Literal | `3`, `"paid"`, `true`, `null`, `[1, 2, 3]` | Number, string, bool, null, array |

### Operators

| Operator | Meaning |
|---|---|
| `==` | Equal |
| `!=` | Not equal |
| `>=` | Greater or equal |
| `>` | Greater |
| `<=` | Less or equal |
| `<` | Less |
| `in` | Membership (element in array) |
| `matches` | Regex match (string) |

### The `default` branch

Every barrier must have exactly one branch whose condition is `default` — the fallback when no other branch matches.

```yaml
branches:
  - outcome: SUCCESS
    condition: "message.payload.items[*].qty > 0"
    probability: 0.95
  - outcome: FAILURE
    condition: "default"
    probability: 0.05
```

## Example conditions

```text
message.payload.items[*].qty > 0                      # every line item is in stock
message.payload.amount >= 10000                       # high-value order
message.payload.status in ["PAID", "AUTHORIZED"]      # membership
message.payload.reference matches "^ORD-[0-9]{8}$"    # format check
```

## Type checking at build time

The compiler resolves each ECEL expression against the referenced AsyncAPI message schema and verifies that operands and literals are type-compatible (e.g. `qty > "three"` is a **V-204** type error). This means conditions are verified against real contracts before they ever run.

## Wildcard semantics in generated code

A wildcard condition like `message.payload.items[*].qty > 0` compiles to a universal quantification over the array:

```rust
if message.payload.items.iter().all(|item| item.qty > 0) { ... }
```

## See also

- [Event Trees](event-trees.md) — where conditions are used
- [Probability Linking](probability-linking.md)
