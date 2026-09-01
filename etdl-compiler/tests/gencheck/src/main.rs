//! Compile-check harness for ETDL-generated Rust.
//!
//! This crate is only ever built by the codegen test to verify generated output
//! compiles. Allow unused imports/items because the placeholder build (without
//! `--features gen-check`) intentionally references nothing from these modules.

#![allow(dead_code, unused_imports)]

pub mod messages;

// Re-export message modules so generated `use orders_api::messages::*` resolves
// at the crate root.
pub use messages::{orders_api, payment_api};

use etdl_core::ChannelCapturingPublisher;

pub async fn stripe_charge_handler(
    _message: &orders_api::messages::OrderPlaced,
) -> Result<payment_api::messages::PaymentProcessed, etdl_core::WorkflowError> {
    Ok(payment_api::messages::PaymentProcessed {
        payload: serde_json::json!({"ok": true}),
    })
}

// Stub handlers for the performance-check fixture's two Operations —
// `Trigger` is defined by the included `generated.rs`, resolved fine
// regardless of textual order since Rust items in one module scope aren't
// order-dependent.
#[cfg(feature = "gen-check-performance")]
static PERF_CONCURRENCY_CURRENT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(feature = "gen-check-performance")]
static PERF_CONCURRENCY_MAX_SEEN: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Tracks how many concurrent calls are actually in-flight at once — the
/// real proof `etdl_core::perf`'s `maxConcurrency` enforcement works
/// through generated code, not just in the engine's own unit tests.
#[cfg(feature = "gen-check-performance")]
async fn concurrency_op(_message: &Trigger) -> Result<(), etdl_core::WorkflowError> {
    use std::sync::atomic::Ordering;
    let now = PERF_CONCURRENCY_CURRENT.fetch_add(1, Ordering::SeqCst) + 1;
    PERF_CONCURRENCY_MAX_SEEN.fetch_max(now, Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    PERF_CONCURRENCY_CURRENT.fetch_sub(1, Ordering::SeqCst);
    Ok(())
}

/// Deliberately slow (well past `latency-budget`'s declared 5ms
/// ceiling) — `LatencyOp`'s explicit 500ms `timeoutMs` (see the fixture's
/// own comment) lets this complete naturally instead of being cut off, so
/// its true latency actually lands in the rolling window
/// `performance.in_budget` reads from.
#[cfg(feature = "gen-check-performance")]
async fn latency_op(_message: &Trigger) -> Result<(), etdl_core::WorkflowError> {
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    Ok(())
}

// The generated file is written by `etdl-compiler/tests/codegen_test.rs` into
// `src/generated.rs` before `cargo check --features gen-check` (or
// `gen-check-inline`, for a fixture whose message types are generated
// inline rather than sourced from `messages.rs`'s AsyncAPI-toolchain
// stubs — see `INLINE_MESSAGES_FIXTURE`). When missing (plain `cargo
// build`), fall back to a placeholder module so the crate still compiles
// for editor tooling.
#[cfg(not(any(feature = "gen-check", feature = "gen-check-inline")))]
mod generated {
    pub fn _placeholder() {}
}

#[cfg(any(feature = "gen-check", feature = "gen-check-inline"))]
include!("generated.rs");

#[tokio::main]
async fn main() {
    // Only the default fixture's generated code defines
    // `handle_order_placed_trigger(orders_api::messages::OrderPlaced, ...)`
    // — `gen-check-inline` proves its own fixture's generated code compiles
    // (the goal of this crate) without this hardcoded runtime smoke test,
    // which is specific to the default fixture's message types.
    #[cfg(feature = "gen-check")]
    {
        let publisher = ChannelCapturingPublisher::new();
        let msg = orders_api::messages::OrderPlaced {
            payload: orders_api::messages::OrderPayload {
                items: vec![orders_api::messages::LineItem {
                    qty: 2,
                    sku: "SKU-1".into(),
                }],
            },
            headers: None,
        };
        let _ = handle_order_placed_trigger(msg, &publisher).await;
        println!("gencheck ran");
    }

    // Proves the live-reliability fixture's generated code actually
    // behaves live, not just compiles: `record_observation` is called
    // directly here exactly as an embedding application would from its
    // own handler code (codegen has no way to know which basic event
    // corresponds to which real-world condition inside opaque business
    // logic — see docs/reference/live-reliability.md).
    #[cfg(feature = "gen-check-live-reliability")]
    {
        let publisher = ChannelCapturingPublisher::new();

        // Freshly registered: current value == baseline == the declared
        // prior (0.1), well within the fixture's 0.3 threshold.
        let msg = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_live_trigger(msg, &publisher).await.unwrap();
        assert!(
            publisher.published_to("normal-channel"),
            "expected the NORMAL branch on a fresh baseline"
        );
        assert!(!publisher.published_to("abnormal-channel"));

        // Drive the basic event's live estimate far out of range — 200
        // "occurred" observations pull it from a 0.1 prior toward 1.0,
        // well past the 0.3 threshold.
        for _ in 0..200 {
            etdl_core::live::record_observation("GatewayFailure", "GatewayUnreachable", true);
        }

        let msg2 = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_live_trigger(msg2, &publisher).await.unwrap();
        assert!(
            publisher.published_to("abnormal-channel"),
            "expected the ABNORMAL branch after the live estimate drifted"
        );

        println!("gencheck live-reliability ran");
    }

    // Proves the safety-check fixture's generated code actually behaves
    // live, not just compiles: same mechanism as `gen-check-live-reliability`
    // above (`record_observation` called directly, as an embedding
    // application would), but this fixture's branch condition is
    // `safety.sil_maintained`, not `reliability.in_range` — a genuinely
    // different codegen path (`try_render_safety_condition`) checking a
    // different band (the barrier's declared SIL, not a threshold) — see
    // docs/reference/safety-supplement.md.
    #[cfg(feature = "gen-check-safety")]
    {
        let publisher = ChannelCapturingPublisher::new();

        // Freshly registered: current value == baseline == the declared
        // prior (0.005), inside the fixture's SIL 2 band [1e-3, 1e-2).
        let msg = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_safety_trigger(msg, &publisher).await.unwrap();
        assert!(
            publisher.published_to("normal-channel"),
            "expected the SUCCESS branch (SIL maintained) on a fresh baseline"
        );
        assert!(!publisher.published_to("failsafe-channel"));

        // Drive the basic event's live estimate far out of the SIL 2 band
        // — 200 "occurred" observations pull it from a 0.005 prior toward
        // 1.0, well past the band's 0.01 upper bound.
        for _ in 0..200 {
            etdl_core::live::record_observation("GatewayFailure", "GatewayUnreachable", true);
        }

        let msg2 = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_safety_trigger(msg2, &publisher).await.unwrap();
        assert!(
            publisher.published_to("failsafe-channel"),
            "expected the FAILURE (fail-safe) branch once the live probability drifted \
             outside the declared SIL's band"
        );

        println!("gencheck safety ran");
    }

    // Proves the security-check fixture's generated code actually behaves
    // live, not just compiles: same mechanism as `gen-check-safety` above,
    // but this fixture's branch condition is `security.control_effective`,
    // not `safety.sil_maintained` — a genuinely different codegen path
    // (`try_render_security_condition`) checking a single declared
    // ceiling (the control's `maxBypassProbability`), not a band — see
    // docs/reference/security-supplement.md.
    #[cfg(feature = "gen-check-security")]
    {
        let publisher = ChannelCapturingPublisher::new();

        // Freshly registered: current value == baseline == the declared
        // prior (0.005), under the fixture's 0.02 maxBypassProbability
        // ceiling.
        let msg = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_security_trigger(msg, &publisher).await.unwrap();
        assert!(
            publisher.published_to("normal-channel"),
            "expected the SUCCESS branch (control effective) on a fresh baseline"
        );
        assert!(!publisher.published_to("failsafe-channel"));

        // Drive the basic event's live estimate above the 0.02 ceiling —
        // 200 "occurred" observations pull it from a 0.005 prior toward
        // 1.0, well past it.
        for _ in 0..200 {
            etdl_core::live::record_observation("GatewayBypass", "RateLimitBypassed", true);
        }

        let msg2 = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_security_trigger(msg2, &publisher).await.unwrap();
        assert!(
            publisher.published_to("failsafe-channel"),
            "expected the FAILURE (fail-safe) branch once the live probability drifted \
             above the declared bypass ceiling"
        );

        println!("gencheck security ran");
    }

    // Two-service cross-process proof (see live-reliability-producer.etdl /
    // live-reliability-consumer.etdl and Cargo.toml's feature comments):
    // this process plays exactly one role, decided at compile time by which
    // feature/fixture was built into `generated.rs`. The two roles never
    // run in the same process, so each has its own independent
    // `etdl_core::live` REGISTRY — the file at `ETDL_LIVE_RELIABILITY_HANDOFF`
    // is the only thing that crosses between them, standing in for a real
    // message broker.
    #[cfg(feature = "gen-check-live-reliability-producer")]
    {
        let publisher = ChannelCapturingPublisher::new();

        // Registration happens inside the handler (`etdl_ensure_live_
        // gateway_failure_registered`), guarded by a `std::sync::Once` — so
        // it must run at least once *before* `record_observation` can have
        // any effect (it's a documented no-op against an unregistered fault
        // tree; see `etdl_core::live::record_observation`'s doc comment).
        // Same ordering the single-service check
        // (`gen-check-live-reliability`, above) already uses.
        let warmup = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_producer_trigger(warmup, &publisher).await.unwrap();

        // Now drive this service's own local estimate far from its
        // declared prior (0.1).
        for _ in 0..200 {
            etdl_core::live::record_observation("GatewayFailure", "GatewayUnreachable", true);
        }

        let msg = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_producer_trigger(msg, &publisher).await.unwrap();

        let headers = publisher
            .headers_sent_to("propagated-reliability")
            .expect(
                "producer's generated code should attach outbound_snapshot headers \
                 to its outgoing message (Part 5's render_publish_call)",
            );

        let handoff = std::env::var("ETDL_LIVE_RELIABILITY_HANDOFF")
            .expect("producer role requires ETDL_LIVE_RELIABILITY_HANDOFF to be set");
        std::fs::write(&handoff, serde_json::to_vec(&headers).unwrap())
            .expect("write handoff file");

        println!("gencheck live-reliability producer ran");
    }

    #[cfg(feature = "gen-check-live-reliability-consumer")]
    {
        let publisher = ChannelCapturingPublisher::new();

        let handoff = std::env::var("ETDL_LIVE_RELIABILITY_HANDOFF")
            .expect("consumer role requires ETDL_LIVE_RELIABILITY_HANDOFF to be set");
        let bytes = std::fs::read(&handoff)
            .expect("producer's handoff file should already exist when the consumer runs");
        let headers: serde_json::Value =
            serde_json::from_slice(&bytes).expect("handoff file is valid JSON");

        // This service never calls `record_observation` for GatewayUnreachable
        // (it's declared `inbound`) — its only path to a non-cold-start live
        // value is `apply_inbound`, which the generated handler calls itself
        // from `message.headers` before RiskBarrier is evaluated.
        let msg = Trigger {
            payload: serde_json::json!({}),
            headers: Some(headers),
        };
        handle_consumer_trigger(msg, &publisher).await.unwrap();

        assert!(
            publisher.published_to("abnormal-channel"),
            "expected the consumer's independently-computed live view — fed only via \
             apply_inbound from the producer's headers, never this service's own \
             observations — to drive branch selection to ABNORMAL, proving the live \
             value genuinely propagated across the process boundary"
        );
        assert!(!publisher.published_to("normal-channel"));

        println!("gencheck live-reliability consumer ran");
    }

    // Proves the Performance Supplement's codegen is authoritative, not
    // just that it compiles: real concurrent execution and real elapsed
    // time, not synthetic assertions.
    #[cfg(feature = "gen-check-performance")]
    {
        use std::sync::atomic::Ordering;

        let publisher = ChannelCapturingPublisher::new();

        // --- Concurrency guarantee: ConcurrencyOp is reached directly (no
        // Barrier pre-check), so all 3 calls genuinely attempt to enter the
        // maxConcurrency=2 guard. If the semaphore didn't actually block,
        // PERF_CONCURRENCY_MAX_SEEN would read 3.
        let mut handles = Vec::new();
        for _ in 0..3 {
            let publisher = publisher.clone();
            handles.push(tokio::spawn(async move {
                let msg = Trigger {
                    payload: serde_json::json!({}),
                    headers: None,
                };
                handle_concurrency_trigger(msg, &publisher).await.unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let max_seen = PERF_CONCURRENCY_MAX_SEEN.load(Ordering::SeqCst);
        assert!(
            max_seen <= 2,
            "observed {max_seen} concurrent ConcurrencyOp calls against a declared \
             maxConcurrency of 2 — the semaphore did not actually block"
        );

        // --- performance.in_budget branch selection: drive exactly
        // MIN_LATENCY_OBSERVATIONS (5) slow calls — each lands on OK
        // (cold-start fail-open) — then assert the next one flips to
        // DEGRADED once the rolling window has enough samples, all of
        // which are ~50ms against a declared 5ms ceiling.
        for _ in 0..5 {
            let msg = Trigger {
                payload: serde_json::json!({}),
                headers: None,
            };
            handle_latency_trigger(msg, &publisher).await.unwrap();
        }
        assert!(
            publisher.published_to("ok-channel"),
            "expected the first (cold-start, fail-open in_budget) calls on the OK branch"
        );
        assert!(!publisher.published_to("degraded-channel"));

        let msg = Trigger {
            payload: serde_json::json!({}),
            headers: None,
        };
        handle_latency_trigger(msg, &publisher).await.unwrap();
        assert!(
            publisher.published_to("degraded-channel"),
            "expected branch selection to flip to DEGRADED once observed latency \
             (~50ms) drifted past latency-budget's declared 5ms ceiling"
        );

        println!("gencheck performance ran");
    }
}
