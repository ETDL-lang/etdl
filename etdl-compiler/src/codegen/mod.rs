use etdl_parser::ast::EtlDocument;
use etdl_parser::asyncapi::AsyncApiRegistry;
use std::collections::BTreeMap;

use crate::validate::Diagnostic;

mod rust;
pub use rust::RustCodeGenerator;

pub trait CodeGenerator {
    fn generate_all(
        &self,
        doc: &EtlDocument,
        fault_tree_probs: &BTreeMap<String, f64>,
        registry: &AsyncApiRegistry,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<String, String>;
}
