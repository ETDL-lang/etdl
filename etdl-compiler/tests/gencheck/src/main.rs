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
}
