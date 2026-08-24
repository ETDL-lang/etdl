use etdl_supplement_sdk::{Supplement, SupplementContext, SupplementDiagnostic};

/// A misbehaving fixture: `validate` never returns. Proves the host's
/// `wasmtime` fuel limit traps a runaway plugin instead of hanging.
#[derive(Default)]
struct LoopingFixture;

impl Supplement for LoopingFixture {
    fn id(&self) -> &str {
        "etdl.fixture-looping"
    }
    fn version(&self) -> &str {
        "1.0"
    }
    fn validate(
        &self,
        _doc: &serde_json::Value,
        _ctx: &SupplementContext,
    ) -> Vec<SupplementDiagnostic> {
        let mut x: u64 = 1;
        loop {
            x = x.wrapping_add(1).wrapping_mul(3);
            std::hint::black_box(x);
        }
    }
}

etdl_supplement_sdk::etdl_supplement!(LoopingFixture);

fn main() {}
