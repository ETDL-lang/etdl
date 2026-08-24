# Developer Guide — How do I use ETDL?

**Audience:** developers who want to write, validate, compile, and run `.etdl`
documents. Companion docs: `docs/getting-started.md`, `docs/CLI.md`,
`docs/ECEL.md`.

## 1. Install

```bash
cargo install etdl-cli
etdl --version
```

## 2. Write a document

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
            condition: default
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
etdl validate --json order-fulfillment.etdl   # machine-readable
```

## 4. Compile to Rust

```bash
etdl compile order-fulfillment.etdl --target rust --out-dir ./generated
```

## 5. Run the generated code

The generated handler needs an `etdl_core::Publisher` and a runtime:

```rust
use etdl_core::{ChannelCapturingPublisher, Publisher};

#[tokio::main]
async fn main() {
    let publisher = ChannelCapturingPublisher::new();
    let message = orders_api::messages::OrderPlaced { /* ... */ };
    handle_order_placed_trigger(message, &publisher).await?;
    Ok(())
}
```

For production, implement `Publisher` for your transport (Kafka, NATS, HTTP,
etc.) and inject the W3C `traceparent` on every outbound message
(`etdl_core::telemetry::inject_traceparent`).

## 6. In your editor

Install the ETDL VS Code extension (`usamassem.etdl-language`) for highlighting,
diagnostics that navigate to exact source, IntelliSense (go-to-definition,
references, hover, outline, completion), and event/fault-tree visualization.

## The 10-minute path

1. `cargo install etdl-cli`
2. copy `examples/business/order-fulfillment.etdl`
3. `etdl validate` it
4. `etdl compile` it
5. open the VS Code panel to visualize

## Where to go next

- Concepts: `docs/concepts/event-trees.md`, `docs/concepts/fault-trees.md`
- Language: `docs/ECEL.md`, `docs/CLI.md`
- Examples: `docs/examples/`
- Extending: `docs/reference/supplement-plugins.md` — writing a third-party,
  dynamically loaded supplement (no rebuild of `etdl-cli`)
- Building from source: `CONTRIBUTING.md`
