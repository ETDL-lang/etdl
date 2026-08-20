# Probability Linking

The core innovation of ETDL: **failure probabilities are design artifacts, resolved at build time and embedded as constants into generated code.**

## The problem

In most event-driven systems, an operation's failure behavior is:

- guessed ("we retry 3 times"),
- scattered (backoff constants copied across services),
- discovered in production (SLA dashboards after incidents).

## The ETDL approach

```yaml
ProcessPaymentOperation:
  type: operation
  action: execute
  handler: "stripe_charge_handler"
  onFailure: PaymentFailedConsequence
  onFailureProbabilitySource: "#/faultTrees/PaymentGatewayFailure/topEvent"
```

`onFailureProbabilitySource` is a JSON Pointer (RFC 6901) into the same document, naming the fault tree top event that quantifies this operation's failure.

### At build time

1. The compiler evaluates the fault tree (see [Fault Trees](fault-trees.md)).
2. The resulting probability is emitted as a constant:

```rust
// Computed from faultTrees.PaymentGatewayFailure.topEvent at build time (Section 5.16)
const PROCESS_PAYMENT_OPERATION_FAILURE_PROBABILITY: f64 = 0.012987;
```

### At run time

The generated operation:

```rust
match retry.execute(|| stripe_charge_handler(&message), Duration::from_millis(5000)).await {
    Ok(_result) => { publish_to_channel("FulfillmentChannel", _result).await?; }
    Err(err) => {
        inventory_check_barrier.record_failure(
            "ProcessPaymentOperation",
            &err,
            Some(PROCESS_PAYMENT_OPERATION_FAILURE_PROBABILITY),
        );
        publish_to_channel("DeadLetterChannel", message).await?;
    }
}
```

## What this enables

| Capability | Mechanism |
|---|---|
| **SLA anomaly detection** | `SlaTracker` compares observed failure frequency against the declared probability over a rolling window (`ETDL_SLA_WINDOW`, `ETDL_SLA_THRESHOLD`) |
| **Declared chaos injection** | `ChaosController` injects failures at declared rates, seeded and scoped per node (`ETDL_CHAOS`, `ETDL_CHAOS_SEED`, `ETDL_CHAOS_SCOPE`), production-guarded via `ETDL_ENV` |
| **Auditable reliability** | The probability shipped with the code is the probability from the design model — no drift |
| **Reviewable design** | Architects review fault trees, not scattered retry constants |

## Validation

The compiler verifies the JSON Pointer resolves to an actual fault tree top event, and that the referenced message type is compatible with the operation's failure path.
