//! Message-type stubs used by the generated-code compile check.
//!
//! These mirror what an AsyncAPI codegen crate would produce for the
//! order-fulfillment fixture's `orders_api` and `payment_api` imports: each
//! crate exposes a `messages` submodule holding the message types.

use serde::{Deserialize, Serialize};

pub mod orders_api {
    use super::*;

    pub mod messages {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct LineItem {
            pub qty: u32,
            pub sku: String,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct OrderPayload {
            pub items: Vec<LineItem>,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct OrderPlaced {
            pub payload: OrderPayload,
            pub headers: Option<serde_json::Value>,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct InventoryFailed {
            pub payload: serde_json::Value,
        }
    }
}

pub mod payment_api {
    pub mod messages {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct PaymentProcessed {
            pub payload: serde_json::Value,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct PaymentFailed {
            pub payload: serde_json::Value,
        }
    }
}
