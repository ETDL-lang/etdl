//! The syn-based Rust AST visitor that detects failure candidates.

use proc_macro2::LineColumn;
use std::path::PathBuf;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::candidate::{CandidateStatus, DiscoveryCandidate, Evidence};
use crate::config::DiscoveryConfig;
use crate::location::{FunctionContext, SourceLocation};
use crate::mapping::OntologyMapping;
use crate::report::{AnalyzerMetadata, DiscoveryReport, ReportStatistics, SourceIdentity};

use super::patterns::{pattern_mapping, RustPattern};

/// Configuration for the Rust analyzer.
#[derive(Debug, Clone)]
pub struct RustAnalyzerConfig {
    /// Whether to attempt module/function context extraction.
    pub extract_context: bool,
}

impl Default for RustAnalyzerConfig {
    fn default() -> Self {
        RustAnalyzerConfig {
            extract_context: true,
        }
    }
}

/// The deterministic Rust source analyzer.
#[derive(Debug, Clone)]
pub struct RustAnalyzer {
    config: RustAnalyzerConfig,
}

impl Default for RustAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl RustAnalyzer {
    pub fn new() -> Self {
        RustAnalyzer {
            config: RustAnalyzerConfig::default(),
        }
    }

    pub fn with_config(config: RustAnalyzerConfig) -> Self {
        RustAnalyzer { config }
    }
    pub fn analyze_source(
        &self,
        file_name: &str,
        source: &str,
        config: &DiscoveryConfig,
    ) -> Result<DiscoveryReport, crate::error::DiscoveryError> {
        let path = PathBuf::from(file_name);
        let content_hash = crate::source::content_hash(&[(path.clone(), source.to_string())]);
        let mut report = DiscoveryReport::new(
            AnalyzerMetadata {
                name: "etdl-rust".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                language: "rust".to_string(),
            },
            SourceIdentity {
                path,
                content_hash,
                file_count: 1,
                package_name: None,
            },
            config,
        );

        let file = match syn::parse_file(source) {
            Ok(f) => f,
            Err(e) => {
                report
                    .diagnostics
                    .push(format!("parse error in {file_name}: {e}"));
                return Ok(report);
            }
        };

        let mut visitor = HitVisitor::new(source, file_name, config);
        visitor.extract_context = self.config.extract_context;
        visitor.visit_file(&file);

        let ontology = crate::ontology::OntologyView::generic_service();
        for hit in visitor.hits {
            let mut evidence = vec![Evidence::new(
                "source-pattern",
                hit.pattern.evidence_label(),
                hit.detail,
            )];
            if let Some(line_text) = hit.line_text {
                evidence[0].line_text = Some(line_text);
            }

            let mut candidate = DiscoveryCandidate {
                id: hit.id,
                classification: hit.pattern.classification(),
                severity: hit.pattern.severity(),
                location: hit.location,
                context: hit.context,
                evidence,
                ontology: pattern_mapping(hit.pattern, &ontology),
                confidence: hit.confidence,
                possible: true,
                status: CandidateStatus::Candidate,
            };

            if candidate.confidence < config.min_confidence {
                continue;
            }
            match config.ontology_policy {
                crate::config::OntologyPolicy::Auto => {}
                crate::config::OntologyPolicy::Conservative => {
                    if candidate.ontology.quality == crate::mapping::MappingQuality::Probable {
                        candidate.ontology = OntologyMapping::unmapped(candidate.id.clone());
                    }
                }
                crate::config::OntologyPolicy::Off => {
                    candidate.ontology = OntologyMapping::unmapped(candidate.id.clone());
                }
            }
            report.candidates.push(candidate);
        }

        report.sort();
        report.statistics = ReportStatistics::compute(&report.candidates);
        Ok(report)
    }
}

impl crate::analyzer::SourceAnalyzer for RustAnalyzer {
    fn language(&self) -> &str {
        "rust"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn analyze_file(
        &self,
        path: &std::path::Path,
        config: &DiscoveryConfig,
    ) -> Result<DiscoveryReport, crate::error::DiscoveryError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::DiscoveryError::SourceRead {
                path: path.to_path_buf(),
                source: e,
            }
        })?;
        let mut report = self.analyze_source(&path.display().to_string(), &content, config)?;
        report.source.path = path.to_path_buf();
        Ok(report)
    }

    fn analyze_project(
        &self,
        root: &std::path::Path,
        config: &DiscoveryConfig,
    ) -> Result<DiscoveryReport, crate::error::DiscoveryError> {
        let project = crate::source::build_project(root, config, "rust")?;
        let files = crate::source::read_files(&project.files)?;
        let hash = crate::source::content_hash(&files);

        let mut report = DiscoveryReport::new(
            AnalyzerMetadata {
                name: "etdl-rust".to_string(),
                version: self.version().to_string(),
                language: "rust".to_string(),
            },
            SourceIdentity {
                path: project.root.clone(),
                content_hash: hash,
                file_count: project.files.len(),
                package_name: project.package_name.clone(),
            },
            config,
        );

        for (path, content) in &files {
            let rel = path
                .strip_prefix(&project.root)
                .unwrap_or(path)
                .display()
                .to_string();
            let sub = self.analyze_source(&rel, content, config)?;
            report.candidates.extend(sub.candidates);
            report.diagnostics.extend(sub.diagnostics);
        }

        report.sort();
        report.statistics = ReportStatistics::compute(&report.candidates);
        Ok(report)
    }
}

/// A single raw detection before it becomes a candidate.
struct Hit {
    pattern: RustPattern,
    location: SourceLocation,
    context: FunctionContext,
    detail: String,
    line_text: Option<String>,
    confidence: f64,
    id: String,
}

/// The AST visitor. Uses `syn` to walk the full Rust syntax tree and record
/// deterministic hits with precise spans.
struct HitVisitor<'a> {
    source: &'a str,
    file: &'a str,
    hits: Vec<Hit>,
    /// Line-start byte offsets for span -> byte offset conversion.
    line_starts: Vec<usize>,
    context: FunctionContext,
    /// Whether we are inside a function that returns `Result`.
    in_result_fn: bool,
    /// Whether to extract module/function context.
    extract_context: bool,
}

impl<'a> HitVisitor<'a> {
    fn new(source: &'a str, file: &'a str, _config: &'a DiscoveryConfig) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        HitVisitor {
            source,
            file,
            hits: Vec::new(),
            line_starts,
            context: FunctionContext::default(),
            in_result_fn: false,
            extract_context: true,
        }
    }

    /// Convert a `proc_macro2` span into a `SourceLocation`.
    fn location(&self, span: proc_macro2::Span) -> SourceLocation {
        let start = span.start();
        let end = span.end();
        SourceLocation {
            file: PathBuf::from(self.file),
            line: start.line as u32,
            column: start.column as u32 + 1,
            end_line: end.line as u32,
            end_column: end.column as u32 + 1,
            byte_start: self.byte_offset(start),
            byte_end: self.byte_offset(end),
        }
    }

    fn byte_offset(&self, pos: LineColumn) -> usize {
        let line_idx = pos.line.saturating_sub(1);
        let line_start = *self.line_starts.get(line_idx).unwrap_or(&0);
        let mut byte = line_start;
        let mut col = 0usize;
        let bytes = self.source.as_bytes();
        while byte < bytes.len() && col < pos.column {
            let b = bytes[byte];
            if b == b'\n' {
                break;
            }
            let width = if b & 0x80 == 0 {
                1
            } else if b & 0xe0 == 0xc0 {
                2
            } else if b & 0xf0 == 0xe0 {
                3
            } else {
                4
            };
            byte += width;
            col += 1;
        }
        byte
    }

    /// The trimmed text of the source line containing `byte`.
    fn line_text_at(&self, byte: usize) -> Option<String> {
        let line_idx = self
            .line_starts
            .partition_point(|&s| s <= byte)
            .saturating_sub(1);
        let start = *self.line_starts.get(line_idx)?;
        let end = self
            .line_starts
            .get(line_idx + 1)
            .copied()
            .unwrap_or(self.source.len());
        let line = &self.source[start..end.min(self.source.len())];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn push(&mut self, pattern: RustPattern, span: proc_macro2::Span, detail: String) {
        let byte_start = self.byte_offset(span.start());
        let id = crate::identity::candidate_id(pattern.domain(), pattern.concept());
        self.hits.push(Hit {
            pattern,
            location: self.location(span),
            context: self.context.clone(),
            detail,
            line_text: self.line_text_at(byte_start),
            confidence: pattern.base_confidence(),
            id,
        });
    }

    /// Handle a macro invocation that maps to a failure pattern.
    fn push_macro(&mut self, mac: &syn::Macro, span: proc_macro2::Span) {
        let name = mac.path.segments.last().map(|s| s.ident.to_string());
        match name.as_deref() {
            Some("panic") => self.push(RustPattern::Panic, span, "explicit panic!".to_string()),
            Some("unreachable") => {
                self.push(RustPattern::Unreachable, span, "unreachable!".to_string())
            }
            Some("todo") | Some("unimplemented") => self.push(
                RustPattern::Unimplemented,
                span,
                format!("{}! (unimplemented path)", name.unwrap_or_default()),
            ),
            Some("assert") => self.push(RustPattern::Assertion, span, "assert!".to_string()),
            Some("assert_eq") => self.push(RustPattern::Assertion, span, "assert_eq!".to_string()),
            Some("assert_ne") => self.push(RustPattern::Assertion, span, "assert_ne!".to_string()),
            _ => {}
        }
    }

    fn fn_name(&self) -> String {
        self.context
            .function
            .clone()
            .unwrap_or_else(|| "<anonymous>".to_string())
    }
}

impl<'ast> Visit<'ast> for HitVisitor<'ast> {
    // ---- context tracking --------------------------------------------------

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !self.extract_context {
            return;
        }
        let name = node.ident.to_string();
        let prev = std::mem::take(&mut self.context.module);
        self.context.module = prev.clone();
        self.context.module.push(name);
        visit::visit_item_mod(self, node);
        self.context.module = prev;
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let prev_fn = self.context.function.clone();
        let prev_in_result = self.in_result_fn;
        self.context.function = Some(node.sig.ident.to_string());
        self.in_result_fn = returns_result(&node.sig.output);
        visit::visit_item_fn(self, node);
        self.context.function = prev_fn;
        self.in_result_fn = prev_in_result;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let prev_fn = self.context.function.clone();
        let prev_in_result = self.in_result_fn;
        self.context.function = Some(node.sig.ident.to_string());
        self.in_result_fn = returns_result(&node.sig.output);
        visit::visit_impl_item_fn(self, node);
        self.context.function = prev_fn;
        self.in_result_fn = prev_in_result;
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let prev_impl = self.context.impl_type.clone();
        self.context.impl_type = Some(type_name(&node.self_ty));
        visit::visit_item_impl(self, node);
        self.context.impl_type = prev_impl;
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let name = node.ident.to_string();
        if is_error_type(name.as_str(), &node.attrs) {
            self.push(
                RustPattern::CustomError,
                node.ident.span(),
                format!("custom error enum '{name}' defined"),
            );
        }
        visit::visit_item_enum(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let name = node.ident.to_string();
        if is_error_type(name.as_str(), &node.attrs) {
            self.push(
                RustPattern::CustomError,
                node.ident.span(),
                format!("custom error struct '{name}' defined"),
            );
        }
        visit::visit_item_struct(self, node);
    }

    // ---- statement-level patterns -----------------------------------------

    fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
        if let syn::Stmt::Expr(expr, _) = node {
            if let syn::Expr::Return(ret) = expr {
                if let Some(inner) = &ret.expr {
                    if is_err_call(inner) {
                        self.push(
                            RustPattern::ExplicitErrReturn,
                            expr.span(),
                            format!("explicit `return Err(...)` in fn '{}'", self.fn_name()),
                        );
                    }
                }
            }
        }
        // Statement-level macros: `panic!`, `assert!`, `assert_eq!`, ...
        if let syn::Stmt::Macro(sm) = node {
            self.push_macro(&sm.mac, sm.mac.span());
        }
        visit::visit_stmt(self, node);
    }

    // ---- expression patterns ----------------------------------------------

    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        match node {
            syn::Expr::Try(t) => {
                self.push(
                    RustPattern::ErrorPropagation,
                    t.question_token.span(),
                    format!("`?` error propagation in fn '{}'", self.fn_name()),
                );
            }
            syn::Expr::Index(idx) => {
                self.push(
                    RustPattern::Indexing,
                    idx.bracket_token.span.join(),
                    "index expression can panic on out-of-bounds".to_string(),
                );
            }
            syn::Expr::Binary(bin) => {
                if matches!(bin.op, syn::BinOp::Div(_) | syn::BinOp::Rem(_)) {
                    self.push(
                        RustPattern::Division,
                        bin.op.span(),
                        "division/remainder can panic on zero divisor".to_string(),
                    );
                }
            }
            syn::Expr::MethodCall(mc) => {
                let method = mc.method.to_string();
                let receiver = expr_path(mc.receiver.as_ref()).unwrap_or_default();
                match method.as_str() {
                    "unwrap" => self.push(
                        RustPattern::Unwrap,
                        mc.span(),
                        format!("unwrap() on receiver '{receiver}'"),
                    ),
                    "expect" => self.push(
                        RustPattern::Expect,
                        mc.span(),
                        format!("expect(...) on receiver '{receiver}'"),
                    ),
                    "parse" => self.push(
                        RustPattern::Parsing,
                        mc.span(),
                        format!("parse::<T>() on receiver '{receiver}'"),
                    ),
                    "send" | "send_timeout" | "recv" | "recv_timeout" | "try_send" | "try_recv" => {
                        // Channel operations: `Sender::send` / `Receiver::recv`.
                        // Disambiguate from HTTP `RequestBuilder::send` by the
                        // receiver's channel-y name or explicit channel calls.
                        if is_channel_receiver(&receiver) {
                            self.push(
                                RustPattern::Channel,
                                mc.span(),
                                format!("channel {method}() can fail (closed/slow)"),
                            );
                        }
                    }
                    "lock" | "read" | "write" | "try_lock" | "try_read" | "try_write" => {
                        self.push(
                            RustPattern::Lock,
                            mc.span(),
                            format!("{method}() on a lock can poison"),
                        );
                    }
                    "timeout" | "timeout_at" => {
                        self.push(
                            RustPattern::Timeout,
                            mc.span(),
                            "explicit timeout API".to_string(),
                        );
                    }
                    _ => {
                        if receiver_contains(&receiver, &["fs", "file", "std::fs"]) {
                            self.push(
                                RustPattern::Filesystem,
                                mc.span(),
                                format!("filesystem operation via '{receiver}'"),
                            );
                        } else if receiver_contains(
                            &receiver,
                            &["reqwest", "hyper", "tcp", "udp", "client", "http"],
                        ) {
                            self.push(
                                RustPattern::Network,
                                mc.span(),
                                format!("network/client operation via '{receiver}'"),
                            );
                        } else if receiver_contains(
                            &receiver,
                            &["serde_json", "serde_yaml", "bincode", "toml"],
                        ) {
                            self.push(
                                RustPattern::Serialization,
                                mc.span(),
                                format!("serialization via '{receiver}'"),
                            );
                        }
                    }
                }
            }
            syn::Expr::Call(call) => {
                let callee = expr_path(call.func.as_ref()).unwrap_or_default();
                if path_contains(&callee, &["fs", "std::fs", "tokio::fs"]) {
                    self.push(
                        RustPattern::Filesystem,
                        call.span(),
                        format!("filesystem function '{callee}'"),
                    );
                } else if path_contains(
                    &callee,
                    &["reqwest", "hyper", "connect", "TcpStream", "UdpSocket"],
                ) {
                    self.push(
                        RustPattern::Network,
                        call.span(),
                        format!("network function '{callee}'"),
                    );
                } else if path_contains(&callee, &["serde_json", "serde_yaml", "bincode", "toml"]) {
                    self.push(
                        RustPattern::Serialization,
                        call.span(),
                        format!("serialization function '{callee}'"),
                    );
                } else if path_contains(&callee, &["channel", "mpsc", "tokio::sync"]) {
                    self.push(
                        RustPattern::Channel,
                        call.span(),
                        format!("channel function '{callee}'"),
                    );
                } else if path_contains(
                    &callee,
                    &[
                        "reqwest", "client", "http", "rpc", "grpc", "rabbitmq", "kafka",
                    ],
                ) {
                    self.push(
                        RustPattern::Dependency,
                        call.span(),
                        format!("external dependency function '{callee}'"),
                    );
                }
            }
            syn::Expr::Macro(m) => {
                self.push_macro(&m.mac, m.mac.span());
            }
            _ => {}
        }
        visit::visit_expr(self, node);
    }
}

// ---- helpers --------------------------------------------------------------

fn returns_result(output: &syn::ReturnType) -> bool {
    let syn::ReturnType::Type(_, ty) = output else {
        return false;
    };
    type_path_last_segment(ty).is_some_and(|s| s == "Result")
}

fn type_path_last_segment(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Paren(p) => type_path_last_segment(&p.elem),
        _ => None,
    }
}

fn type_name(ty: &syn::Type) -> String {
    type_path_last_segment(ty).unwrap_or_else(|| "impl".to_string())
}

fn is_error_type(name: &str, attrs: &[syn::Attribute]) -> bool {
    if name.to_lowercase().ends_with("error") {
        return true;
    }
    attrs.iter().any(|a| {
        if !a.path().is_ident("derive") {
            return false;
        }
        a.parse_args_with(|stream: syn::parse::ParseStream| {
            let paths =
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated(stream)?;
            Ok(paths.iter().any(|p| {
                p.segments.last().map(|s| s.ident.to_string()).as_deref() == Some("Error")
            }))
        })
        .unwrap_or_default()
    })
}

/// The full dotted path of a function-call expression, e.g.
/// `fs::read_to_string` -> `fs::read_to_string`, `reqwest::get` -> `reqwest::get`.
fn expr_path(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(p) => {
            let segs: Vec<String> = p
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            Some(segs.join("::"))
        }
        syn::Expr::Call(c) => expr_path(&c.func),
        syn::Expr::MethodCall(m) => Some(m.method.to_string()),
        _ => None,
    }
}

fn is_err_call(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Call(c) => expr_path(&c.func).as_deref() == Some("Err"),
        syn::Expr::Try(t) => is_err_call(&t.expr),
        _ => false,
    }
}

fn path_contains(path: &str, needles: &[&str]) -> bool {
    let lower = path.to_lowercase();
    needles.iter().any(|n| lower.contains(n))
}

fn receiver_contains(receiver: &str, needles: &[&str]) -> bool {
    path_contains(receiver, needles)
}

/// Heuristic: does the receiver look like a channel sender/receiver?
/// (`tx`, `sender`, `chan`, `rx`, `receiver`, `send`).
fn is_channel_receiver(receiver: &str) -> bool {
    let lower = receiver.to_lowercase();
    ["tx", "sender", "chan", "rx", "receiver", "send"]
        .iter()
        .any(|n| lower == *n || lower.ends_with(&format!("::{n}")))
}

#[cfg(test)]
mod tests {
    use crate::config::DiscoveryConfig;
    use crate::rust::RustAnalyzer;

    fn analyze(src: &str) -> crate::report::DiscoveryReport {
        let cfg = DiscoveryConfig::default();
        RustAnalyzer::new()
            .analyze_source("test.rs", src, &cfg)
            .unwrap()
    }

    fn has_id(report: &crate::report::DiscoveryReport, id: &str) -> bool {
        report.candidates.iter().any(|c| c.id == id)
    }

    #[test]
    fn detects_unwrap_expect_panic() {
        let src = r#"
fn f() {
    let x = foo().unwrap();
    let y = bar().expect("msg");
    panic!("boom");
}
"#;
        let r = analyze(src);
        assert!(
            has_id(&r, "failure.runtime.unwrap"),
            "got: {:?}",
            r.candidates.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        assert!(has_id(&r, "failure.runtime.expect"));
        assert!(has_id(&r, "failure.runtime.panic"));
    }

    #[test]
    fn detects_error_propagation_and_assertions() {
        let src = r#"
fn f() -> Result<(), String> {
    let x = g()?;
    assert!(x > 0);
    assert_eq!(x, 1);
    Ok(())
}
"#;
        let r = analyze(src);
        assert!(has_id(&r, "failure.runtime.error_propagation"));
        assert!(has_id(&r, "failure.runtime.assertion"));
    }

    #[test]
    fn detects_indexing_division_parsing() {
        let src = r#"
fn f(a: &[u8], x: usize) {
    let v = a[x];
    let q = 10 / x;
    let n: u32 = "42".parse().unwrap();
}
"#;
        let r = analyze(src);
        assert!(has_id(&r, "failure.runtime.index_out_of_bounds"));
        assert!(has_id(&r, "failure.runtime.division_by_zero"));
        assert!(has_id(&r, "failure.validation.parse_failure"));
    }

    #[test]
    fn detects_fs_network_serialization_channel_lock() {
        let src = r#"
use std::sync::Mutex;
use std::fs;

fn f(m: &Mutex<i32>) {
    let _ = fs::read_to_string("x");
    let _ = reqwest::get("http://x");
    let _ = serde_json::from_str::<serde_json::Value>("{}");
    let _ = m.lock();
}
"#;
        let r = analyze(src);
        assert!(has_id(&r, "failure.io.io_failure"));
        assert!(has_id(&r, "failure.network.network_operation"));
        assert!(has_id(&r, "failure.serialization.serialization_failure"));
        assert!(has_id(&r, "failure.concurrency.lock_poisoning"));
    }

    #[test]
    fn detects_custom_error_type() {
        let src = r#"
#[derive(Debug)]
enum PaymentError {
    Timeout,
    Rejected,
}
"#;
        let r = analyze(src);
        assert!(
            has_id(&r, "failure.application.custom_error"),
            "got: {:?}",
            r.candidates.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detection_is_deterministic() {
        let src = "fn f() { let _ = a().unwrap(); let _ = b().expect(\"x\"); }";
        let cfg = DiscoveryConfig::default();
        let a = RustAnalyzer::new()
            .analyze_source("d.rs", src, &cfg)
            .unwrap();
        let b = RustAnalyzer::new()
            .analyze_source("d.rs", src, &cfg)
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn confidence_is_not_probability() {
        // All candidates have possible=true and confidence < 1; none sets a
        // probability field.
        let src = "fn f() { let _ = x().unwrap(); }";
        let r = analyze(src);
        for c in &r.candidates {
            assert!(c.possible);
            assert!(c.confidence <= 1.0);
        }
    }
}
