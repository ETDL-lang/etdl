use etdl_supplement_sdk::{Severity, Supplement, SupplementContext, SupplementDiagnostic};

/// A well-behaved fixture plugin: always reports one warning, to prove
/// diagnostic propagation carries real content across the WASM boundary,
/// not just an empty list.
#[derive(Default)]
struct ValidFixture;

impl Supplement for ValidFixture {
    fn id(&self) -> &str {
        "etdl.fixture-valid"
    }
    fn version(&self) -> &str {
        "1.0"
    }
    fn validate(
        &self,
        _doc: &serde_json::Value,
        _ctx: &SupplementContext,
    ) -> Vec<SupplementDiagnostic> {
        vec![SupplementDiagnostic {
            code: "FIXTURE-001".to_string(),
            severity: Severity::Warning,
            message: "fixture plugin ran successfully".to_string(),
        }]
    }
    fn process(&self, _doc: &serde_json::Value, _ctx: &SupplementContext) -> Vec<(String, f64)> {
        vec![("FT.SomeEvent".to_string(), 0.042)]
    }
}

etdl_supplement_sdk::etdl_supplement!(ValidFixture);

fn main() {}
