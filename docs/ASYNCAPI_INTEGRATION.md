# ETDL × AsyncAPI Integration

ETDL resolves every message, channel, and payload against AsyncAPI 3.0 documents
imported via `asyncapi_imports`. This document describes how that integration
works, its guarantees, and its error semantics.

## How imports work

```yaml
asyncapi_imports:
  orders_api: "./asyncapi/orders.yaml"
  payment_api: "./asyncapi/payments.yaml"
```

Each alias maps to a YAML or JSON AsyncAPI 3.0 document (`.json` is parsed as
JSON; anything else as YAML). References into those documents use the External
Reference form:

```
<alias>#<json-pointer>
orders_api#/components/messages/OrderPlaced
```

JSON Pointers follow RFC 6901 (`~0` → `~`, `~1` → `/`).

## What ETDL resolves

| ETDL field | AsyncAPI target | Purpose |
|---|---|---|
| `initiatingEvent.message` | message | entry-point message type |
| `operation.emits` | message | message published on success |
| `consequence.channel` / `consequence.message` | channel / message | `send` target |
| `topEvent.message`, `basicEvent.message` | message | observable fault signals |
| ECEL conditions | message `payload`/`headers` schema | type checking (V-204) |

ETDL never redefines schemas: every message/channel is a reference into an
AsyncAPI document (ETDL §1.3). The compiler reads the payload schema to
type-check ECEL conditions against the message.

## Guarantees

- At least one `asyncapi_imports` entry is required (ETDL §1.3).
- Every external reference resolves (E-103 unknown alias, E-104 unresolvable
  pointer).
- Local imports are confined to the project root: a `..` segment in an import
  path is rejected (ETDL §12) — no path traversal.
- Remote (`http(s)://`) imports are rejected in the reference implementation.
- Files are read once into a registry; no repeated disk I/O during validation.

## Error semantics

| Case | Code | Message includes |
|---|---|---|
| alias not in `asyncapi_imports` | E-103 | alias, field (`nodes.X.message`) |
| pointer does not resolve | E-104 | alias, pointer, field |
| import file missing | E-101 | file path (via WASM) |
| malformed YAML/JSON | E-101 | alias, parse error |
| `..` in import path | (load error) | the offending import path |

All reference diagnostics carry a span key mapping to the exact source location
when the span index is available.

## Type checking against schemas

`AsyncApiRegistry::get_schema_for_path` resolves an ECEL path into the payload
schema; `etdl-compiler/src/typeck.rs` maps JSON Schema types to ECEL types:

- `number`/`integer` → `number`
- `string` → `string`
- `boolean` → `boolean`
- `array` → `array` (of `items`)
- `object` → `object` (members accessed via `properties`)
- unresolved constructs (`$ref`, `allOf`, `oneOf`, `enum`, `format`) → `unknown`
  (type checks pass; the schema is not fully introspected)

## Example (order fulfillment)

```yaml
asyncapi_imports:
  orders_api: "./asyncapi/orders.yaml"
  payment_api: "./asyncapi/payments.yaml"

eventTrees:
  OrderFulfillment:
    initiatingEvent:
      id: OrderPlacedTrigger
      message: "orders_api#/components/messages/OrderPlaced"
      next: InventoryCheckBarrier
```

`orders.yaml` (stub):

```yaml
asyncapi: "3.0.0"
info:
  title: Orders API
  version: "1.0.0"
channels:
  FulfillmentChannel:
    address: "orders.fulfilled"
    messages:
      OrderFulfilled:
        $ref: "#/components/messages/OrderFulfilled"
components:
  messages:
    OrderPlaced:
      name: OrderPlaced
      payload:
        type: object
        properties:
          items:
            type: array
            items:
              type: object
              properties:
                qty: { type: integer }
```

The condition `message.payload.items[*].qty > 0` resolves against `OrderPlaced`
payload and type-checks cleanly.

## Security (ETDL §12)

- Local imports cannot escape the project root (no `..`).
- Remote imports are disabled.
- The WASM validator has **no filesystem access**: callers pass file contents
  explicitly, which confines resolution to caller-controlled data.
- Resource limits (document size, `$ref` depth) are recommended for untrusted
  AsyncAPI documents and are on the roadmap.

## Fixtures

- `etdl-cli/tests/fixtures/asyncapi/{orders,payments}.yaml` — order-fulfillment.
- `etdl-cli/tests/fixtures/asyncapi/api.yaml` — advanced fixture.
