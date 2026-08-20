# Getting Started

This guide walks you from an empty directory to a compiled, runnable event tree.

## Prerequisites

- Rust toolchain (for `cargo install` and for the generated code)
- AsyncAPI 3.0 YAML/JSON documents describing your channels and messages (or stub them, as in the examples)

## 1. Install the CLI

```bash
cargo install etdl-cli
```

This installs the `etdl` binary with `compile` and `validate` subcommands.

## 2. Write an `.etdl` document

An ETDL document has three parts: `info`, `eventTrees`, and `faultTrees` (plus optional `asyncapi_imports`).

```yaml
etdl: "1.0.0"
info:
  title: "Order Fulfillment"
  version: "1.0.0"
  domain: "FulfillmentContext"

asyncapi_imports:
  orders_api: "./asyncapi/orders.yaml"

eventTrees:
  OrderFulfillment:
    initiatingEvent:
      id: OrderPlacedTrigger
      message: "orders_api#/components/messages/OrderPlaced"
      next: InventoryCheckBarrier
    nodes:
      InventoryCheckBarrier:
        type: barrier
        branches:
          - outcome: SUCCESS
            condition: "message.payload.items[*].qty > 0"
            probability: 0.95
            next: FulfillmentConsequence
          - outcome: FAILURE
            condition: "default"
            probability: 0.05
            next: OutOfStockConsequence
      FulfillmentConsequence:
        type: consequence
        operation: send
        channel: "orders_api#/channels/FulfillmentChannel"
        message: "orders_api#/components/messages/OrderFulfilled"
      OutOfStockConsequence:
        type: consequence
        operation: send
        channel: "orders_api#/channels/DeadLetterChannel"
        message: "orders_api#/components/messages/InventoryFailed"
```

## 3. Validate

```bash
etdl validate order-fulfillment.etdl
```

The validator reports:

- **E-errors** — structural problems (missing fields, malformed references)
- **V-errors** — semantic problems (invalid probabilities, type errors, gate cycles, undefined handlers)
- **W-warnings** — suspicious but non-fatal patterns

A valid document exits 0:

```
valid: order-fulfillment.etdl (3 diagnostics cleared)
```

## 4. Compile to Rust

```bash
etdl compile order-fulfillment.etdl --target rust --out-dir ./generated
```

Output:

```
compiled 'order-fulfillment.etdl' to './generated/order-fulfillment.rs' (0 errors, 0 warnings)
```

## 5. Use the generated code

The generated module:

- imports message types from your AsyncAPI-generated crates (`orders_api::messages::*`)
- defines a `pub async fn` per initiating event, e.g. `handle_order_placed_trigger`
- embeds resolved fault-tree probabilities as `const`s
- uses `etdl_core` for `BranchMonitor`, `RetryPolicy`, SLA tracking, and chaos injection

Add `etdl-core` to your service's `Cargo.toml`:

```toml
[dependencies]
etdl-core = "0.1"
```

Call the handler from your message consumer:

```rust
use orders_api::messages::OrderPlaced;

consumer.on_message::<OrderPlaced>(|msg| async move {
    handle_order_placed_trigger(msg).await?;
    Ok(())
});
```

## Next steps

- Read [Event Trees](concepts/event-trees.md) and [Fault Trees](concepts/fault-trees.md) for the full model
- Follow the annotated [Order Fulfillment example](examples/order-fulfillment.md)
- See the [CLI reference](reference/cli.md) for all flags
