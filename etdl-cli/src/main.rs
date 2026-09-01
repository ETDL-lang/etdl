use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::{Path, PathBuf};
use etdl_compiler::extension::EtdlExtension as _;

mod targets;

#[derive(Parser)]
#[command(
    name = "etdl",
    version,
    about = "ETDL parser, validator, and compiler",
    after_help = "Exit codes: 0 = success, 1 = validation/compile failure, 2 = usage error"
)]
struct Cli {
    /// Emit machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,

    /// Suppress all non-error output.
    #[arg(long, global = true)]
    quiet: bool,

    /// Emit extra detail.
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compile an .etdl document to a target language.
    ///
    /// Two runtime-only capabilities of the generated code's `etdl-core`
    /// library are not controlled by this command and don't show up in its
    /// output, since both are opt-in at the *application's own* build time
    /// (Cargo features on `etdl-core`, not `.etdl` document content): (1)
    /// observability exporters (Prometheus/Loki/OTLP), see
    /// `docs/reference/observability-exporters.md`; (2) when the document
    /// declares `etdl.live-reliability`, the generated code always contains
    /// the live-reliability calls (registration, `reliability.in_range`,
    /// header propagation) regardless of this command's flags — see
    /// `docs/reference/live-reliability.md` and `etdl capabilities`, which
    /// *does* report `etdl.live-reliability` as a compiler-visible
    /// supplement (unlike the exporters).
    Compile {
        #[arg(help = "Path to .etdl document")]
        file: PathBuf,

        #[arg(
            long,
            default_value = "rust",
            help = "Target language(s) for code generation: comma-separated, e.g. \
                    'rust,java'. `rust` is always available; run with `--help` to \
                    see every target this build actually has enabled."
        )]
        target: String,

        #[arg(
            long,
            default_value = ".",
            help = "Output directory for generated code"
        )]
        out_dir: PathBuf,

        #[arg(
            long = "library-path",
            help = "Additional search path for optional (non-std.*) libraries; repeatable"
        )]
        library_path: Vec<PathBuf>,
    },
    /// Validate one or more .etdl documents (files or directories).
    Validate {
        #[arg(help = "Path(s) to .etdl document(s) or directories")]
        files: Vec<PathBuf>,

        #[arg(
            long = "library-path",
            help = "Additional search path for optional (non-std.*) libraries; repeatable"
        )]
        library_path: Vec<PathBuf>,
    },
    /// Analyze a document and print reliability summary (fault trees, branches).
    Analyze {
        #[arg(help = "Path to .etdl document")]
        file: PathBuf,

        #[arg(
            long,
            help = "Path to a dependency model (.yaml/.json); enables dependency-aware analysis"
        )]
        dependencies: Option<PathBuf>,

        #[arg(
            long,
            help = "Run Monte Carlo uncertainty propagation with this many samples"
        )]
        monte_carlo: Option<usize>,

        #[arg(
            long,
            default_value_t = 42,
            help = "Seed for Monte Carlo (reproducibility)"
        )]
        seed: u64,

        #[arg(
            long,
            help = "Path to declared input uncertainties (.yaml/.json): event id -> sampling law"
        )]
        uncertainty: Option<PathBuf>,

        #[arg(
            long,
            default_value_t = 0.95,
            help = "Central interval level for the propagated uncertainty"
        )]
        level: f64,

        #[arg(
            long,
            default_value_t = 1e-3,
            help = "Absolute perturbation size for sensitivity analysis"
        )]
        perturbation: f64,

        #[arg(long, help = "Skip importance analysis")]
        no_importance: bool,

        #[arg(long, help = "Skip sensitivity analysis")]
        no_sensitivity: bool,

        #[arg(
            long,
            help = "Rank inputs by how much of the output uncertainty each accounts for \
                    (costs one extra propagation run per uncertain input)"
        )]
        uncertainty_ranking: bool,

        #[arg(long, help = "Write the analysis result artifact (JSON) to this path")]
        output: Option<PathBuf>,

        #[arg(
            long = "library-path",
            help = "Additional search path for optional (non-std.*) libraries; repeatable"
        )]
        library_path: Vec<PathBuf>,

        #[arg(
            long,
            help = "Allow --monte-carlo to proceed even when no basic event in scope \
                    declares uncertainty (RA013); without this flag such a run is refused, \
                    since the reported interval would have zero width"
        )]
        allow_point_estimates: bool,
    },
    /// Discover candidate failure modes in source code (reliability ontology).
    Discover {
        #[arg(help = "Path to a source file or directory to analyze")]
        path: PathBuf,

        #[arg(
            long,
            default_value = "rust",
            help = "Source language (only 'rust' is implemented)"
        )]
        language: String,

        #[arg(
            long,
            default_value = "text",
            help = "Output format: text | json | yaml"
        )]
        format: String,

        #[arg(
            long,
            default_value_t = 0.5,
            help = "Minimum discovery confidence (0.0-1.0); filters candidates"
        )]
        min_confidence: f64,

        #[arg(long, help = "Write the report to this file instead of stdout")]
        output: Option<PathBuf>,

        #[arg(long, help = "Exclude a path (repeatable)")]
        exclude: Vec<PathBuf>,

        #[arg(
            long,
            default_value = "auto",
            help = "Ontology mapping policy: auto | conservative | off"
        )]
        ontology_policy: String,
    },
    /// Resolve external reliability probabilities and show their provenance.
    Reliability {
        #[command(subcommand)]
        command: ReliabilityCommand,
    },
    /// Inspect the ETDL Standard Library architecture (built-in/optional/user
    /// library resolution).
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Inspect Generic Tree Event Supplement (`x-tree-event`) trees declared
    /// in a document.
    Tree {
        #[command(subcommand)]
        command: TreeCommand,
    },
    /// Export one or more documents as machine-consumable context for LLM
    /// pipelines (RAG/CAG) rather than for compiling — a full JSON AST
    /// dump, a unified `{nodes, edges}` graph, or RAG-ready chunks. Parse-only:
    /// does not run `etdl validate`'s diagnostics or resolve fault-tree
    /// probabilities. Accepts multiple files so a shell glob (e.g. `etdl
    /// context dump *.etdl`) can export a whole directory in one call; a
    /// per-file parse failure is reported inline and does not abort the
    /// rest of the batch. See `docs/reference/context-export.md`.
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    /// Install a supplement plugin from a local `.wasm` file or an
    /// `https://` URL (sandboxed, run via `wasmtime`; see `etdl supplement
    /// list`/`remove` to manage installed plugins). Loads it once to
    /// confirm it conforms to the expected ABI (exports
    /// id/version/validate/process and an exported `memory`) before
    /// installing — a broken module is rejected here, not silently
    /// accepted and discovered broken later. Distinct from `--target
    /// java/python/go/dotnet` (code-generation bindings) and from the
    /// built-in `reliability`/`discovery` extensions (compiled into this
    /// binary, not dynamically loaded).
    Install {
        #[arg(help = "Path to a .wasm file, or an https:// URL")]
        source: String,
    },
    /// Manage dynamically-loaded supplement plugins (sandboxed `.wasm`
    /// modules run via `wasmtime`). To install one, use `etdl install
    /// <source>`. Distinct from `--target java/python/go/dotnet`
    /// (code-generation bindings) and from the built-in `reliability`/
    /// `discovery` extensions (compiled into this binary, not dynamically
    /// loaded).
    Supplement {
        #[command(subcommand)]
        command: SupplementCommand,
    },
    /// Report the capabilities compiled into this ETDL binary — the
    /// compiler/CLI, not the generated code's own runtime. Observability
    /// exporters (Prometheus/Loki/OTLP) are a separate, runtime-only
    /// capability of the `etdl-core` library an application links
    /// against, gated by that application's own Cargo feature choices at
    /// its own build time — never reported here, since this binary never
    /// compiles `etdl-core` with those features itself. See
    /// `docs/reference/observability-exporters.md`.
    Capabilities,
    /// ETDL Conformance, Verification & Validation: objective per-area
    /// conformance status and the machine-readable conformance manifest.
    /// See `docs/reference/conformance-framework.md`.
    Conformance {
        #[command(subcommand)]
        command: ConformanceCommand,
    },
    Version,
}

#[derive(clap::Subcommand)]
enum ConformanceCommand {
    /// Objective PASS/PARTIAL/UNSUPPORTED/FAILED status per conformance
    /// area (core syntax/semantic, standard library, each supplement,
    /// artifact, runtime, WASM). Reflects compiled-in capability, not a
    /// live test run — run `cargo test -p etdl-conformance` for that.
    Status,
    /// The machine-readable conformance manifest: ETDL language version,
    /// implementation version, conformance-suite version, supported
    /// supplements/libraries/targets/artifact schemas.
    Manifest,
}

#[derive(clap::Subcommand)]
enum SupplementCommand {
    /// List built-in extensions (reliability, discovery, tree-event) and
    /// installed dynamic plugins.
    List,
    /// Remove an installed plugin by its supplement id (e.g.
    /// `etdl.mycompany-audit`, as reported by `etdl supplement list`).
    Remove {
        #[arg(help = "Supplement id to remove")]
        id: String,
    },
}

#[derive(clap::Subcommand)]
enum LibraryCommand {
    /// List the built-in standard library modules shipped with this binary.
    List,
    /// Resolve every library a document declares and show how each one
    /// resolved (built-in / optional / user), without compiling the document.
    Resolve {
        #[arg(help = "Path to .etdl document")]
        file: PathBuf,
        #[arg(
            long = "library-path",
            help = "Additional search path for optional (non-std.*) libraries; repeatable"
        )]
        library_path: Vec<PathBuf>,
    },
}

#[derive(clap::Subcommand)]
enum TreeCommand {
    /// Validate every tree declared under `x-tree-event` in a document
    /// (requires `supplements: [{id: etdl.tree-event, ...}]`).
    Validate {
        #[arg(help = "Path to .etdl document")]
        file: PathBuf,
    },
    /// Summarize each declared tree's structure (root, node count, leaves,
    /// gates) without evaluating anything.
    Inspect {
        #[arg(help = "Path to .etdl document")]
        file: PathBuf,
    },
}

#[derive(clap::Subcommand)]
enum ContextCommand {
    /// Dump each document's full parsed AST as JSON (one array entry per
    /// file). Good for CAG: stuff the whole document into a long-context
    /// prompt/cache.
    Dump {
        #[arg(help = "Paths to .etdl documents", required = true)]
        files: Vec<PathBuf>,
    },
    /// Export each document as a unified `{nodes, edges}` graph spanning
    /// event trees, fault trees, and any declared `etdl.tree-event` trees
    /// (one array entry per file).
    Graph {
        #[arg(help = "Paths to .etdl documents", required = true)]
        files: Vec<PathBuf>,
    },
    /// Export each document as RAG-ready chunks: one JSON object per line
    /// (JSON Lines), each with an auto-generated natural-language `text`
    /// summary suited for embedding, plus structured `metadata`.
    Chunks {
        #[arg(help = "Paths to .etdl documents", required = true)]
        files: Vec<PathBuf>,
    },
}

#[derive(clap::Subcommand)]
enum ReliabilityCommand {
    /// Resolve external probability sources for an .etdl document.
    Resolve {
        #[arg(help = "Path to .etdl document")]
        file: PathBuf,
    },
    /// Validate reliability artifacts referenced by an .etdl document.
    Validate {
        #[arg(help = "Path to .etdl document")]
        file: PathBuf,
    },
    /// Inspect a reliability artifact (structure, estimates, metrics).
    Inspect {
        #[arg(help = "Path to a reliability artifact (.rprob/.yaml/.json)")]
        file: PathBuf,
    },
    /// Estimate probabilities from observations and write a reliability artifact.
    Estimate {
        #[arg(help = "Path to an observations file (.yaml/.json)")]
        file: PathBuf,
        #[arg(
            long,
            default_value = "empirical",
            help = "Estimator: empirical | beta-binomial | exponential"
        )]
        method: String,
        #[arg(
            long,
            default_value_t = 0.95,
            help = "Interval confidence/credibility level"
        )]
        level: f64,
        #[arg(long, default_value_t = 1.0, help = "Beta-binomial prior alpha")]
        prior_alpha: f64,
        #[arg(long, default_value_t = 1.0, help = "Beta-binomial prior beta")]
        prior_beta: f64,
        #[arg(long, help = "Mission time for the exponential estimator")]
        mission_time: Option<f64>,
        #[arg(long, help = "Output artifact path (.rprob/.yaml/.json)")]
        output: Option<PathBuf>,
    },
    /// Compare two analysis-result artifacts (e.g. before and after a mitigation).
    Compare {
        #[arg(help = "Path to the 'before' analysis result (.json)")]
        before: PathBuf,
        #[arg(help = "Path to the 'after' analysis result (.json)")]
        after: PathBuf,
    },
    /// Print the backward trace of an estimate in a reliability artifact.
    Trace {
        #[arg(help = "Path to a reliability artifact (.rprob/.yaml/.json)")]
        file: PathBuf,
        #[arg(help = "Estimate id to trace")]
        estimate: String,
    },
    /// Compare a reliability artifact's prediction against observation
    /// dataset(s) (runtime feedback). Never modifies the artifact: the
    /// result is a report for engineering review, not a new artifact.
    Calibrate {
        #[arg(help = "Path to a reliability artifact (.rprob/.yaml/.json)")]
        artifact: PathBuf,
        #[arg(help = "Failure-mode/event id to calibrate")]
        event: String,
        #[arg(
            long = "dataset",
            required = true,
            help = "Path to an observation dataset (.yaml/.json); repeatable"
        )]
        dataset: Vec<PathBuf>,
        #[arg(long, default_value_t = 0.05, help = "Significance level for 'potential deviation'")]
        alpha: f64,
        #[arg(
            long,
            default_value_t = 0.01,
            help = "Stricter significance level for 'significant deviation' (drift)"
        )]
        strict_alpha: f64,
        #[arg(
            long,
            default_value_t = 20,
            help = "Minimum exposure below which the result is 'insufficient data'"
        )]
        min_exposure: u64,
        #[arg(long, help = "Write the calibration result (JSON) to this path")]
        output: Option<PathBuf>,
    },
}

fn main() {
    // `--target`'s help text lists only targets this build actually has
    // enabled (spec: "Only list targets that are actually
    // enabled/installed"), so it's assembled at startup from the same
    // registry `cmd_compile` dispatches through, rather than hardcoded.
    let available = targets::available_target_names().join(", ");
    let target_help = format!(
        "Target language(s) for code generation: comma-separated, e.g. \
         'rust,java'. Available in this build: {available}"
    );
    let cli_command = {
        use clap::CommandFactory;
        Cli::command().mut_subcommand("compile", |cmd| {
            cmd.mut_arg("target", |arg| arg.help(target_help))
        })
    };
    let matches = cli_command.get_matches();
    let Cli {
        json,
        quiet,
        verbose,
        command,
    } = {
        use clap::FromArgMatches;
        Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
    };

    let flags = CliFlags {
        json,
        quiet,
        verbose,
    };

    let code = match command {
        Command::Compile {
            file,
            target,
            out_dir,
            library_path,
        } => cmd_compile(&flags, &file, &target, &out_dir, &library_path),
        Command::Validate { files, library_path } => cmd_validate(&flags, &files, &library_path),
        Command::Analyze {
            file,
            dependencies,
            monte_carlo,
            seed,
            uncertainty,
            level,
            perturbation,
            no_importance,
            no_sensitivity,
            uncertainty_ranking,
            output,
            library_path,
            allow_point_estimates,
        } => {
            let args = AnalyzeArgs {
                file,
                dependencies,
                monte_carlo,
                seed,
                uncertainty,
                level,
                perturbation,
                importance: !no_importance,
                sensitivity: !no_sensitivity,
                uncertainty_ranking,
                output,
                library_path,
                allow_point_estimates,
            };
            cmd_analyze(&flags, &args)
        }
        Command::Discover {
            path,
            language,
            format,
            min_confidence,
            output,
            exclude,
            ontology_policy,
        } => {
            let args = DiscoverArgs {
                path,
                language,
                format,
                min_confidence,
                output,
                exclude,
                ontology_policy,
            };
            #[cfg(feature = "discovery")]
            {
                cmd_discover(&flags, &args)
            }
            #[cfg(not(feature = "discovery"))]
            {
                let _ = (args,);
                capability_unavailable(
                    "failure discovery",
                    "rebuild etdl-cli with the 'discovery' feature",
                );
                1
            }
        }
        Command::Reliability { command } => match command {
            ReliabilityCommand::Resolve { file } => {
                #[cfg(feature = "reliability")]
                {
                    cmd_reliability_resolve(&flags, &file)
                }
                #[cfg(not(feature = "reliability"))]
                {
                    let _ = file;
                    capability_unavailable(
                        "reliability",
                        "rebuild etdl-cli with the 'reliability' feature",
                    );
                    1
                }
            }
            ReliabilityCommand::Validate { file } => {
                #[cfg(feature = "reliability")]
                {
                    cmd_reliability_validate(&flags, &file)
                }
                #[cfg(not(feature = "reliability"))]
                {
                    let _ = file;
                    capability_unavailable(
                        "reliability",
                        "rebuild etdl-cli with the 'reliability' feature",
                    );
                    1
                }
            }
            ReliabilityCommand::Inspect { file } => {
                #[cfg(feature = "reliability")]
                {
                    cmd_reliability_inspect(&flags, &file)
                }
                #[cfg(not(feature = "reliability"))]
                {
                    let _ = file;
                    capability_unavailable(
                        "reliability",
                        "rebuild etdl-cli with the 'reliability' feature",
                    );
                    1
                }
            }
            ReliabilityCommand::Estimate {
                file,
                method,
                level,
                prior_alpha,
                prior_beta,
                mission_time,
                output,
            } => {
                let args = EstimateArgs {
                    file,
                    method,
                    level,
                    prior_alpha,
                    prior_beta,
                    mission_time,
                    output,
                };
                #[cfg(feature = "reliability")]
                {
                    cmd_reliability_estimate(&flags, &args)
                }
                #[cfg(not(feature = "reliability"))]
                {
                    let _ = args;
                    capability_unavailable(
                        "reliability",
                        "rebuild etdl-cli with the 'reliability' feature",
                    );
                    1
                }
            }
            ReliabilityCommand::Compare { before, after } => {
                #[cfg(feature = "reliability")]
                {
                    cmd_reliability_compare(&flags, &before, &after)
                }
                #[cfg(not(feature = "reliability"))]
                {
                    let _ = (before, after);
                    capability_unavailable(
                        "reliability",
                        "rebuild etdl-cli with the 'reliability' feature",
                    );
                    1
                }
            }
            ReliabilityCommand::Trace { file, estimate } => {
                #[cfg(feature = "reliability")]
                {
                    cmd_reliability_trace(&flags, &file, &estimate)
                }
                #[cfg(not(feature = "reliability"))]
                {
                    let _ = (file, estimate);
                    capability_unavailable(
                        "reliability",
                        "rebuild etdl-cli with the 'reliability' feature",
                    );
                    1
                }
            }
            ReliabilityCommand::Calibrate {
                artifact,
                event,
                dataset,
                alpha,
                strict_alpha,
                min_exposure,
                output,
            } => {
                let args = CalibrateArgs {
                    artifact,
                    event,
                    dataset,
                    alpha,
                    strict_alpha,
                    min_exposure,
                    output,
                };
                #[cfg(feature = "reliability")]
                {
                    cmd_reliability_calibrate(&flags, &args)
                }
                #[cfg(not(feature = "reliability"))]
                {
                    let _ = args;
                    capability_unavailable(
                        "reliability",
                        "rebuild etdl-cli with the 'reliability' feature",
                    );
                    1
                }
            }
        },
        Command::Library { command } => match command {
            LibraryCommand::List => cmd_library_list(&flags),
            LibraryCommand::Resolve { file, library_path } => {
                cmd_library_resolve(&flags, &file, &library_path)
            }
        },
        Command::Tree { command } => match command {
            TreeCommand::Validate { file } => cmd_tree_validate(&flags, &file),
            TreeCommand::Inspect { file } => cmd_tree_inspect(&flags, &file),
        },
        Command::Context { command } => match command {
            ContextCommand::Dump { files } => cmd_context_dump(&files),
            ContextCommand::Graph { files } => cmd_context_graph(&files),
            ContextCommand::Chunks { files } => cmd_context_chunks(&files),
        },
        Command::Install { source } => cmd_supplement_install(&flags, &source),
        Command::Supplement { command } => match command {
            SupplementCommand::List => cmd_supplement_list(&flags),
            SupplementCommand::Remove { id } => cmd_supplement_remove(&flags, &id),
        },
        Command::Capabilities => cmd_capabilities(&flags),
        Command::Conformance { command } => match command {
            ConformanceCommand::Status => cmd_conformance_status(&flags),
            ConformanceCommand::Manifest => cmd_conformance_manifest(&flags),
        },
        Command::Version => {
            if flags.json {
                println!(
                    "{}",
                    serde_json::json!({ "name": "etdl", "version": env!("CARGO_PKG_VERSION") })
                );
            } else {
                println!("etdl {}", env!("CARGO_PKG_VERSION"));
            }
            0
        }
    };

    std::process::exit(code);
}

#[derive(Clone, Copy)]
struct CliFlags {
    json: bool,
    quiet: bool,
    verbose: bool,
}

/// Arguments to the `etdl discover` command.
#[allow(dead_code)] // fields read only under the discovery feature
struct DiscoverArgs {
    path: PathBuf,
    language: String,
    format: String,
    min_confidence: f64,
    output: Option<PathBuf>,
    exclude: Vec<PathBuf>,
    ontology_policy: String,
}

/// Arguments to the `etdl analyze` command.
#[allow(dead_code)] // several fields are read only under the reliability feature
struct AnalyzeArgs {
    file: PathBuf,
    dependencies: Option<PathBuf>,
    monte_carlo: Option<usize>,
    seed: u64,
    uncertainty: Option<PathBuf>,
    level: f64,
    perturbation: f64,
    importance: bool,
    sensitivity: bool,
    uncertainty_ranking: bool,
    output: Option<PathBuf>,
    library_path: Vec<PathBuf>,
    allow_point_estimates: bool,
}

/// Arguments to the `etdl reliability estimate` command.
#[allow(dead_code)] // fields read only under the reliability feature
struct EstimateArgs {
    file: PathBuf,
    method: String,
    level: f64,
    prior_alpha: f64,
    prior_beta: f64,
    mission_time: Option<f64>,
    output: Option<PathBuf>,
}

/// Arguments to the `etdl reliability calibrate` command.
#[allow(dead_code)] // fields read only under the reliability feature
struct CalibrateArgs {
    artifact: PathBuf,
    event: String,
    dataset: Vec<PathBuf>,
    alpha: f64,
    strict_alpha: f64,
    min_exposure: u64,
    output: Option<PathBuf>,
}

/// Collect all `.etdl` files from the given paths (files or directories).
fn collect_etdl_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for p in paths {
        let meta =
            std::fs::metadata(p).map_err(|e| format!("cannot access '{}': {}", p.display(), e))?;
        if meta.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
                .map_err(|e| format!("cannot read directory '{}': {}", p.display(), e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "etdl"))
                .collect();
            entries.sort();
            out.extend(entries);
        } else {
            out.push(p.clone());
        }
    }
    Ok(out)
}

fn resolve_diagnostic_positions(
    diagnostics: &mut [etdl_compiler::validate::Diagnostic],
    content: &str,
) {
    let index = etdl_parser::spanned::build_span_index(content).ok();
    for d in diagnostics.iter_mut() {
        if d.line.is_none() {
            if let (Some(key), Some(index)) = (&d.key, &index) {
                if let Some(el) = index.resolve(key) {
                    let span = el.key_span.unwrap_or(el.span);
                    d.line = Some(span.line);
                    d.column = Some(span.column);
                    d.end_line = Some(span.end_line);
                    d.end_column = Some(span.end_column);
                }
            }
        }
    }
}

fn append_duplicate_warnings(
    diagnostics: &mut Vec<etdl_compiler::validate::Diagnostic>,
    content: &str,
) {
    if let Ok(duplicates) = etdl_parser::spanned::detect_duplicate_ids(content) {
        for dup in duplicates {
            let mut d = etdl_compiler::validate::Diagnostic::warning(
                "V-001",
                format!(
                    "duplicate {} id '{}' in tree '{}'",
                    dup.kind, dup.id, dup.tree
                ),
            )
            .with_position(dup.span.line, dup.span.column);
            d.end_line = Some(dup.span.end_line);
            d.end_column = Some(dup.span.end_column);
            diagnostics.push(d);
        }
    }
}

#[derive(Serialize)]
struct DiagnosticJson<'a> {
    code: &'a str,
    severity: &'a str,
    message: &'a str,
    line: Option<u32>,
    column: Option<u32>,
}

fn diagnostic_line(d: &etdl_compiler::validate::Diagnostic) -> String {
    let level = if d.is_error() { "ERROR" } else { "WARNING" };
    let position = match (d.line, d.column) {
        (Some(l), Some(c)) => format!(" ({}:{})", l + 1, c + 1),
        _ => String::new(),
    };
    format!("[{}] {}{}: {}", level, d.code, position, d.message)
}

fn print_diagnostics(flags: &CliFlags, diagnostics: &[etdl_compiler::validate::Diagnostic]) {
    if flags.json {
        let items: Vec<DiagnosticJson> = diagnostics
            .iter()
            .map(|d| DiagnosticJson {
                code: &d.code,
                severity: if d.is_error() { "error" } else { "warning" },
                message: &d.message,
                line: d.line,
                column: d.column,
            })
            .collect();
        println!("{}", serde_json::to_string(&items).unwrap_or_default());
    } else {
        for d in diagnostics {
            // Diagnostics go to stdout when not quiet; errors always visible.
            if flags.quiet && !d.is_error() {
                continue;
            }
            println!("{}", diagnostic_line(d));
        }
    }
}

/// Dispatches to every target named in `--target` (comma-separated; a bare
/// `compile` defaults `target` to `"rust"` at the `clap` level, so this
/// always sees at least one name). Unknown target names fail fast, before
/// the file is even read, exactly like the old hardcoded `target != "rust"`
/// check did — so `etdl compile x.etdl --target bogus` still costs nothing.
///
/// A single `--target rust` (or the default, which is the same thing)
/// produces byte-identical output to every prior release: `compile_with_base`
/// and `compile_target_with_base` now share the same `prepare()` pipeline
/// (see `etdl-compiler/src/lib.rs`), and `RustCodeGenerator::generate_all`'s
/// actual generation logic is untouched — only its signature grew a `stem`
/// parameter it already always would have used for the output filename.
fn cmd_compile(
    flags: &CliFlags,
    file: &Path,
    target: &str,
    out_dir: &Path,
    library_path: &[PathBuf],
) -> i32 {
    let target_names = targets::split_target_names(target);
    if target_names.is_empty() {
        eprintln!("error: --target requires at least one target name");
        return 2;
    }

    let mut generators = Vec::with_capacity(target_names.len());
    for name in &target_names {
        match targets::resolve_target(name) {
            Some(g) => generators.push(g),
            None => {
                eprintln!(
                    "error: unsupported target '{}'; available targets: {}",
                    name,
                    targets::available_target_names().join(", ")
                );
                return 1;
            }
        }
    }

    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read file '{}': {}", file.display(), e);
            return 1;
        }
    };

    let doc = match etdl_parser::parse_document(&content) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    let base_dir = file.parent().unwrap_or(Path::new("."));

    let registry = match etdl_parser::load_asyncapi_imports(&doc, base_dir) {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    let compiler = library_path
        .iter()
        .fold(compiler_with_plugins(etdl_compiler::Compiler::new()), |c, p| {
            c.with_library_search_path(p.clone())
        });

    let stem = file
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let multi_target = target_names.len() > 1;
    let mut any_errors = false;
    let mut wrote_manifests = false;

    for (name, generator) in target_names.iter().copied().zip(generators.into_iter()) {
        let mut result =
            compiler.compile_target_with_base(&doc, &registry, base_dir, generator.as_ref(), &stem);

        append_duplicate_warnings(&mut result.diagnostics, &content);
        resolve_diagnostic_positions(&mut result.diagnostics, &content);

        let error_count = result.diagnostics.iter().filter(|d| d.is_error()).count();
        let warning_count = result.diagnostics.iter().filter(|d| !d.is_error()).count();

        if multi_target && !flags.quiet {
            eprintln!("--- target: {} ---", name);
        }
        print_diagnostics(flags, &result.diagnostics);

        let Some(files) = result.files else {
            any_errors = true;
            if !flags.quiet {
                eprintln!(
                    "compilation failed for target '{}' with {} errors and {} warnings",
                    name, error_count, warning_count
                );
            }
            continue;
        };

        if !out_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(out_dir) {
                eprintln!("error: cannot create output directory: {}", e);
                return 1;
            }
        }

        // Each target's `GeneratedFile::relative_path` already encodes that
        // target's own output-structure convention (Rust: a single flat
        // `<stem>.rs`; Java: a package/directory layout under
        // `etdl/runtime/` and `<package>/`) — writing here never
        // special-cases a target's layout, just follows whatever paths it
        // returned.
        let mut write_failed = false;
        for gf in &files {
            let out_path = out_dir.join(&gf.relative_path);
            if let Some(parent) = out_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!(
                        "error: cannot create output directory {}: {}",
                        parent.display(),
                        e
                    );
                    write_failed = true;
                    break;
                }
            }
            if let Err(e) = std::fs::write(&out_path, &gf.contents) {
                eprintln!(
                    "error: cannot write generated code to {}: {}",
                    out_path.display(),
                    e
                );
                write_failed = true;
                break;
            }
        }
        if write_failed {
            any_errors = true;
            continue;
        }

        // Target-independent: produced once by the shared `prepare()`
        // pipeline before any target generator ever runs, so every target's
        // `build_manifest`/`resolved_libraries` for this invocation are
        // identical — write them only for the first target that succeeds.
        if !wrote_manifests {
            wrote_manifests = true;

            if let Some(manifest) = &result.build_manifest {
                let manifest_path = out_dir.join("etdl-build-manifest.json");
                if let Ok(json) = serde_json::to_string_pretty(manifest) {
                    if let Err(e) = std::fs::write(&manifest_path, json) {
                        eprintln!(
                            "warning: cannot write build manifest to {}: {}",
                            manifest_path.display(),
                            e
                        );
                    } else if flags.verbose {
                        eprintln!(
                            "reliability build manifest written to {}",
                            manifest_path.display()
                        );
                    }
                }
            }

            // Standard-library provenance: independent of the reliability
            // build manifest (and of the `reliability` feature) — any
            // document using `libraries:` gets this. Written only when at
            // least one library was actually resolved, so ordinary builds
            // with no libraries produce no extra file.
            if !result.resolved_libraries.is_empty() {
                let stdlib_manifest_path = out_dir.join("etdl-stdlib-manifest.json");
                let payload = serde_json::json!({
                    "schema": etdl_compiler::stdlib::STDLIB_SCHEMA,
                    "libraries": result.resolved_libraries,
                });
                if let Ok(json) = serde_json::to_string_pretty(&payload) {
                    if let Err(e) = std::fs::write(&stdlib_manifest_path, json) {
                        eprintln!(
                            "warning: cannot write standard-library manifest to {}: {}",
                            stdlib_manifest_path.display(),
                            e
                        );
                    } else if flags.verbose {
                        eprintln!(
                            "standard-library manifest written to {}",
                            stdlib_manifest_path.display()
                        );
                    }
                }
            }
        }

        if !flags.quiet {
            if !multi_target && name == "rust" {
                // Byte-for-byte the same summary line every prior release
                // printed for the (still overwhelmingly common) single
                // implicit/explicit `rust` case.
                let out_path = out_dir.join(format!("{stem}.rs"));
                println!(
                    "compiled '{}' to '{}' ({} errors, {} warnings)",
                    file.display(),
                    out_path.display(),
                    error_count,
                    warning_count
                );
            } else {
                println!(
                    "compiled '{}' to target '{}': {} file(s) written to '{}' ({} errors, {} warnings)",
                    file.display(),
                    name,
                    files.len(),
                    out_dir.display(),
                    error_count,
                    warning_count
                );
            }
        }
    }

    if any_errors {
        1
    } else {
        0
    }
}

fn cmd_validate(flags: &CliFlags, paths: &[PathBuf], library_path: &[PathBuf]) -> i32 {
    let files = match collect_etdl_files(paths) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    if files.is_empty() {
        eprintln!("error: no .etdl files found");
        return 1;
    }

    if flags.verbose {
        eprintln!("etdl: validating {} file(s)", files.len());
    }

    let mut worst_exit = 0;

    if flags.json {
        let mut results = Vec::new();
        for file in &files {
            let (diagnostics, ok) = validate_one(flags, file, library_path);
            if !ok {
                worst_exit = 1;
            }
            let items: Vec<DiagnosticJson> = diagnostics
                .iter()
                .map(|d| DiagnosticJson {
                    code: &d.code,
                    severity: if d.is_error() { "error" } else { "warning" },
                    message: &d.message,
                    line: d.line,
                    column: d.column,
                })
                .collect();
            results.push(serde_json::json!({
                "file": file.display().to_string(),
                "valid": ok,
                "diagnostics": items,
            }));
        }
        println!("{}", serde_json::json!({ "results": results }));
        return worst_exit;
    }

    for file in &files {
        let (diagnostics, ok) = validate_one(flags, file, library_path);
        let error_count = diagnostics.iter().filter(|d| d.is_error()).count();
        let warning_count = diagnostics.iter().filter(|d| !d.is_error()).count();

        if ok {
            if !flags.quiet {
                println!(
                    "document '{}' is valid ({} errors, {} warnings)",
                    file.display(),
                    error_count,
                    warning_count
                );
            }
        } else {
            worst_exit = 1;
            if !flags.quiet {
                eprintln!(
                    "document '{}' has {} validation errors",
                    file.display(),
                    error_count
                );
            }
        }
    }

    worst_exit
}

fn validate_one(
    flags: &CliFlags,
    file: &Path,
    library_path: &[PathBuf],
) -> (Vec<etdl_compiler::validate::Diagnostic>, bool) {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[ERROR] {}: {}", file.display(), e);
            return (Vec::new(), false);
        }
    };

    let doc = match etdl_parser::parse_document(&content) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("[ERROR] {}: {}", file.display(), e);
            return (Vec::new(), false);
        }
    };

    let base_dir = file.parent().unwrap_or(Path::new("."));
    let registry = match etdl_parser::load_asyncapi_imports(&doc, base_dir) {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("[ERROR] {}: {}", file.display(), e);
            return (Vec::new(), false);
        }
    };

    let compiler = library_path
        .iter()
        .fold(compiler_with_plugins(etdl_compiler::Compiler::new()), |c, p| {
            c.with_library_search_path(p.clone())
        });
    let base_dir = file.parent().unwrap_or(Path::new("."));
    let mut diagnostics = compiler.validate_with_base(&doc, &registry, base_dir);

    append_duplicate_warnings(&mut diagnostics, &content);
    resolve_diagnostic_positions(&mut diagnostics, &content);

    // In JSON mode the caller serializes the diagnostics; avoid duplicate output.
    if !flags.json {
        print_diagnostics(flags, &diagnostics);
    }

    let ok = !diagnostics.iter().any(|d| d.is_error());
    (diagnostics, ok)
}

fn cmd_analyze(flags: &CliFlags, args: &AnalyzeArgs) -> i32 {
    let file = args.file.as_path();
    let dependencies = args.dependencies.as_deref();
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read file '{}': {}", file.display(), e);
            return 1;
        }
    };

    let doc = match etdl_parser::parse_document(&content) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    let base_dir = file.parent().unwrap_or(Path::new("."));
    let registry = match etdl_parser::load_asyncapi_imports(&doc, base_dir) {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    // Resolve `libraries:` once, up front, and use the expanded document for
    // everything below — validation, fault-tree resolution, and (if
    // requested) dependency-aware analysis all need to see the same
    // library-provided basic events `etdl compile` would see.
    let compiler = args
        .library_path
        .iter()
        .fold(compiler_with_plugins(etdl_compiler::Compiler::new()), |c, p| {
            c.with_library_search_path(p.clone())
        });
    let (doc, _resolved_libs, _lib_errors) =
        etdl_compiler::stdlib::expand_libraries(&doc, base_dir, &compiler.library_resolver);
    let doc = &doc;

    let mut diagnostics = compiler.validate_with_base(doc, &registry, base_dir);
    append_duplicate_warnings(&mut diagnostics, &content);
    resolve_diagnostic_positions(&mut diagnostics, &content);

    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    if !errors.is_empty() {
        print_diagnostics(flags, &diagnostics);
        return 1;
    }

    // Resolve external reliability sources so probabilities reflect artifacts.
    #[cfg(feature = "reliability")]
    let (resolved_events, _manifest) =
        etdl_compiler::reliability::resolve_reliability(doc, base_dir, &mut Vec::new());
    #[cfg(feature = "reliability")]
    let overrides: std::collections::BTreeMap<String, f64> = resolved_events
        .iter()
        .map(|r| (r.override_key(), r.resolved.value))
        .collect();
    #[cfg(not(feature = "reliability"))]
    let overrides: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    let probs = etdl_compiler::fault_tree::resolve_fault_trees_with_overrides(
        doc,
        &overrides,
        &mut Vec::new(),
    );

    // Dependency-aware analysis, when requested. Also runs when uncertainty,
    // Monte Carlo or an output artifact is asked for, since those all live on
    // the analysis path rather than the plain summary path.
    #[cfg(feature = "reliability")]
    if dependencies.is_some()
        || args.monte_carlo.is_some()
        || args.uncertainty.is_some()
        || args.output.is_some()
    {
        return cmd_dependency_analysis(flags, doc, &probs, &overrides, args);
    }
    #[cfg(not(feature = "reliability"))]
    if dependencies.is_some() || args.monte_carlo.is_some() || args.uncertainty.is_some() {
        eprintln!(
            "error: uncertainty / dependency-aware analysis requires the 'reliability' feature (rebuild etdl-cli with it)"
        );
        return 1;
    }

    if flags.json {
        let ft_json: Vec<_> = probs
            .iter()
            .map(|(id, p)| serde_json::json!({ "faultTree": id, "topEventProbability": p }))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "document": file.display().to_string(),
                "eventTrees": doc.event_trees.len(),
                "faultTrees": doc.fault_trees.as_ref().map(|f| f.len()).unwrap_or(0),
                "faultTreeProbabilities": ft_json,
            })
        );
    } else {
        println!("document: {}", file.display());
        println!("event trees: {}", doc.event_trees.len());
        println!(
            "fault trees: {}",
            doc.fault_trees.as_ref().map(|f| f.len()).unwrap_or(0)
        );
        for (id, p) in &probs {
            println!("  {}: topEvent probability = {:.6}", id, p);
        }
    }

    0
}

/// Build a neutral `FaultTreeSpec` from a parsed document and resolved
/// probabilities, so the reliability analysis crate never depends on the
/// parser/compiler.
#[cfg(feature = "reliability")]
fn build_fault_tree_spec(
    doc: &etdl_parser::ast::EtlDocument,
    basic_event_overrides: &std::collections::BTreeMap<String, f64>,
) -> Result<Vec<etdl_reliability::analysis::dependence::FaultTreeSpec>, String> {
    use etdl_reliability::analysis::dependence::{FaultTreeSpec, GateKind, GateSpec};
    let mut out = Vec::new();
    let Some(fts) = &doc.fault_trees else {
        return Ok(out);
    };
    for (ft_id, ft) in fts {
        // The top event in the fault tree is `topEvent.rootCause` (a gate or
        // leaf id); the ft_id is just the document key.
        let root = ft.top_event.root_cause.clone();
        let mut spec = FaultTreeSpec::new(root);
        for (be_id, be) in &ft.basic_events {
            let p = basic_event_overrides
                .get(&etdl_compiler::fault_tree::override_key(ft_id, be_id))
                .copied()
                .or(be.probability)
                .unwrap_or(0.0);
            spec.leaves.insert(be_id.clone(), p);
        }
        if let Some(gates) = &ft.gates {
            for (gid, g) in gates {
                let kind = match g.gate_type {
                    etdl_parser::ast::GateType::And => GateKind::And,
                    etdl_parser::ast::GateType::Or => GateKind::Or,
                    etdl_parser::ast::GateType::Not => GateKind::Not,
                    etdl_parser::ast::GateType::Xor => GateKind::Xor,
                    etdl_parser::ast::GateType::Voting => GateKind::Voting,
                    etdl_parser::ast::GateType::Inhibit => GateKind::Inhibit,
                    etdl_parser::ast::GateType::PriorityAnd => GateKind::PriorityAnd,
                };
                spec.gates.insert(
                    gid.clone(),
                    GateSpec {
                        kind,
                        inputs: g.inputs.clone(),
                        k: g.k,
                    },
                );
            }
        }
        out.push(spec);
    }
    Ok(out)
}

/// Run the dependency-aware analysis and print a traceable report.
#[cfg(feature = "reliability")]
fn cmd_dependency_analysis(
    flags: &CliFlags,
    doc: &etdl_parser::ast::EtlDocument,
    _top_probs: &std::collections::BTreeMap<String, f64>,
    basic_event_overrides: &std::collections::BTreeMap<String, f64>,
    args: &AnalyzeArgs,
) -> i32 {
    use etdl_reliability::analysis::dependence::{
        analyze_with, AnalysisMetadata, AnalysisOptions, DependencyModel, InputUncertainty,
        MonteCarloConfig,
    };
    use std::collections::BTreeMap;

    // The dependency model is optional: without one, independence is the
    // explicit, recorded assumption rather than a silent default.
    let model: DependencyModel = match args.dependencies.as_deref() {
        Some(dep_file) => match load_serde_file(dep_file, "dependency model") {
            Ok(m) => m,
            Err(code) => return code,
        },
        None => DependencyModel::independent(),
    };

    // Declared input uncertainties, keyed by basic-event id.
    let inputs: BTreeMap<String, InputUncertainty> = match args.uncertainty.as_deref() {
        Some(unc_file) => match load_serde_file(unc_file, "uncertainty inputs") {
            Ok(m) => m,
            Err(code) => return code,
        },
        None => BTreeMap::new(),
    };

    let specs = match build_fault_tree_spec(doc, basic_event_overrides) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    if specs.is_empty() {
        eprintln!("error: no fault trees to analyze");
        return 1;
    }

    // Propagation is opt-in. Asking for uncertainty inputs without a sample
    // count applies the documented default and says so.
    let monte_carlo = match (args.monte_carlo, args.uncertainty.is_some()) {
        (Some(0), _) => {
            eprintln!("error: --monte-carlo sample count must be greater than zero");
            return 1;
        }
        (Some(n), _) => Some(MonteCarloConfig {
            samples: n,
            seed: args.seed,
            level: args.level,
        }),
        (None, true) => {
            if !flags.quiet {
                eprintln!(
                    "note: --uncertainty given without --monte-carlo; using the default \
                     sample count of {}",
                    etdl_reliability::analysis::dependence::DEFAULT_SAMPLES
                );
            }
            Some(MonteCarloConfig {
                samples: etdl_reliability::analysis::dependence::DEFAULT_SAMPLES,
                seed: args.seed,
                level: args.level,
            })
        }
        (None, false) => None,
    };

    if args.uncertainty_ranking && monte_carlo.is_none() {
        eprintln!(
            "error: --uncertainty-ranking requires uncertainty propagation; supply \
             --uncertainty and/or --monte-carlo"
        );
        return 1;
    }

    let mut results = Vec::new();
    for spec in &specs {
        let options = AnalysisOptions {
            monte_carlo: monte_carlo.clone(),
            inputs: inputs.clone(),
            perturbation: args.perturbation,
            compute_importance: args.importance,
            compute_sensitivity: args.sensitivity,
            compute_uncertainty_ranking: args.uncertainty_ranking,
            metadata: AnalysisMetadata {
                model_id: args
                    .file
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "fault-tree".to_string()),
                ..Default::default()
            },
        };
        match analyze_with(spec, &model, &options) {
            Ok(r) => results.push(r),
            Err(e) => {
                eprintln!(
                    "error: analysis of '{}' failed: {}\n\
                     hint: a declared dependency the analyser cannot represent is refused \
                     rather than evaluated under independence",
                    spec.top_event, e
                );
                return 1;
            }
        }
    }

    // Monte Carlo over a model where nothing declared uncertainty produces a
    // zero-width interval: a valid-looking number that answers a different
    // question than the one asked (see RA013). Refuse by default rather than
    // let that pass as a silent success; --allow-point-estimates opts in.
    if monte_carlo.is_some() && !args.allow_point_estimates {
        let unpropagated: Vec<&str> = results
            .iter()
            .filter(|r| {
                r.uncertainty
                    .as_ref()
                    .is_some_and(|mc| mc.variable_inputs == 0)
            })
            .map(|r| r.top_event.as_str())
            .collect();
        if !unpropagated.is_empty() {
            eprintln!(
                "error: --monte-carlo requested but no basic event declared propagatable \
                 uncertainty for: {}\n\
                 hint: the reported interval would have zero width, which is a modelling \
                 gap (RA013), not a finding of certainty. Declare uncertainty with \
                 --uncertainty, or pass --allow-point-estimates to proceed anyway.",
                unpropagated.join(", ")
            );
            return 1;
        }
    }

    if let Some(path) = args.output.as_deref() {
        let body = match serde_json::to_string_pretty(&results) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error: cannot serialise analysis result: {e}");
                return 1;
            }
        };
        if let Err(e) = std::fs::write(path, body) {
            eprintln!("error: cannot write '{}': {}", path.display(), e);
            return 1;
        }
        if !flags.quiet {
            eprintln!("wrote analysis result to {}", path.display());
        }
    }

    if flags.json {
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
    } else {
        for r in &results {
            print!("{}", r.render());
            println!();
        }
    }
    0
}

/// Load a YAML or JSON file into a deserialisable type, choosing the parser by
/// extension. Returns the process exit code on failure.
#[cfg(feature = "reliability")]
fn load_serde_file<T: serde::de::DeserializeOwned>(path: &Path, what: &str) -> Result<T, i32> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read {} '{}': {}", what, path.display(), e);
            return Err(1);
        }
    };
    let parsed = if path.extension().is_some_and(|e| e == "json") {
        serde_json::from_str(&content).map_err(|e| e.to_string())
    } else {
        serde_yaml::from_str(&content).map_err(|e| e.to_string())
    };
    match parsed {
        Ok(v) => Ok(v),
        Err(e) => {
            eprintln!("error: cannot parse {} '{}': {}", what, path.display(), e);
            Err(1)
        }
    }
}

/// `etdl reliability compare`: compare two analysis-result artifacts.
#[cfg(feature = "reliability")]
fn cmd_reliability_compare(flags: &CliFlags, before: &Path, after: &Path) -> i32 {
    use etdl_reliability::analysis::dependence::{compare, AnalysisResult};

    let before_result: AnalysisResult = match load_serde_file(before, "analysis result") {
        Ok(r) => r,
        Err(code) => return code,
    };
    let after_result: AnalysisResult = match load_serde_file(after, "analysis result") {
        Ok(r) => r,
        Err(code) => return code,
    };

    let comparison = compare(&before_result, &after_result);
    if flags.json {
        println!("{}", serde_json::to_string_pretty(&comparison).unwrap());
    } else {
        print!("{}", comparison.render());
    }
    0
}

/// `etdl discover`: run the failure discovery analyzer on a source file or
/// directory, printing candidate failure modes. Discovery only establishes
/// possibility, never probability.
#[cfg(feature = "discovery")]
fn cmd_discover(flags: &CliFlags, args: &DiscoverArgs) -> i32 {
    use etdl_failure_discovery::config::{DiscoveryConfig, OntologyPolicy};
    let path = &args.path;
    let language = &args.language;
    let format = &args.format;
    let min_confidence = args.min_confidence;
    let output = args.output.as_deref();
    let exclude = &args.exclude;
    let ontology_policy = &args.ontology_policy;

    if language != "rust" {
        eprintln!("error: language '{language}' is not implemented; only 'rust' is supported");
        return 1;
    }

    let policy = match ontology_policy.as_str() {
        "auto" => OntologyPolicy::Auto,
        "conservative" => OntologyPolicy::Conservative,
        "off" => OntologyPolicy::Off,
        other => {
            eprintln!("error: unknown ontology policy '{other}' (auto|conservative|off)");
            return 1;
        }
    };

    let config = DiscoveryConfig {
        language: Some(language.to_string()),
        min_confidence: min_confidence.clamp(0.0, 1.0),
        ontology_policy: policy,
        exclude: exclude.to_vec(),
        ..DiscoveryConfig::default()
    };

    let analyzer = etdl_failure_discovery::AnalyzerRegistry::new();
    let rust = match analyzer.language("rust") {
        Some(a) => a,
        None => {
            eprintln!("error: rust analyzer is not compiled in");
            return 1;
        }
    };

    let report = match rust.analyze_project(path, &config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    if let Some(out) = output {
        let text = match format.as_str() {
            "json" => match serde_json::to_string_pretty(&report) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot serialize report: {}", e);
                    return 1;
                }
            },
            "yaml" => match serde_yaml::to_string(&report) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot serialize report: {}", e);
                    return 1;
                }
            },
            _ => format_report_text(&report),
        };
        if let Err(e) = std::fs::write(out, text) {
            eprintln!("error: cannot write '{}': {}", out.display(), e);
            return 1;
        }
        if !flags.quiet {
            println!("wrote discovery report to {}", out.display());
        }
        return 0;
    }

    match format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        "yaml" => println!("{}", serde_yaml::to_string(&report).unwrap()),
        _ => print!("{}", format_report_text(&report)),
    }
    0
}

/// Human-readable discovery report text.
#[cfg(feature = "discovery")]
fn format_report_text(report: &etdl_failure_discovery::DiscoveryReport) -> String {
    let mut out = String::new();
    out.push_str("Failure Discovery Report\n");
    out.push_str("========================\n");
    out.push_str(&format!("Schema:        {}\n", report.schema));
    out.push_str(&format!(
        "Analyzer:      {} v{}\n",
        report.analyzer.name, report.analyzer.version
    ));
    out.push_str(&format!("Language:      {}\n", report.analyzer.language));
    out.push_str(&format!(
        "Source:        {}\n",
        report.source.path.display()
    ));
    out.push_str(&format!("Files:         {}\n", report.source.file_count));
    out.push_str(&format!("Content hash:  {}\n", report.source.content_hash));
    out.push('\n');

    let stats = &report.statistics;
    out.push_str(&format!("Candidates:    {}\n", stats.total_candidates));
    out.push_str(&format!(
        "High confidence (>=0.8): {}\n",
        stats.high_confidence
    ));
    out.push_str(&format!(
        "Potential panic:         {}\n",
        stats.potential_panic
    ));
    out.push_str(&format!("Mapped to ontology:      {}\n", stats.mapped));
    out.push_str(&format!("Unmapped (proposed):     {}\n", stats.unmapped));
    for (class, count) in &stats.by_classification {
        out.push_str(&format!("  - {class}: {count}\n"));
    }
    out.push('\n');

    for c in &report.candidates {
        let onto = match &c.ontology.canonical_id {
            Some(id) => id.clone(),
            None => c
                .ontology
                .proposed_concept
                .clone()
                .unwrap_or_else(|| "(unmapped)".to_string()),
        };
        let first_evidence = c
            .evidence
            .first()
            .map(|e| e.detail.clone())
            .unwrap_or_default();
        out.push_str(&format!(
            "{} [{}] {}:{} -> {} (conf={:.2}, {:?})\n",
            c.id,
            c.classification.label(),
            c.location.file.display(),
            c.location.line,
            onto,
            c.confidence,
            c.ontology.quality,
        ));
        out.push_str(&format!("    evidence: {}\n", first_evidence));
        if let Some(line) = c.evidence.first().and_then(|e| e.line_text.clone()) {
            out.push_str(&format!("    source:   {}\n", line));
        }
    }
    out
}

/// `etdl reliability resolve <file>`: resolve external reliability probability
/// sources and print the resolved values plus their provenance (artifact,
/// estimate, version). Useful for debugging a reliability-aware document.
#[cfg(feature = "reliability")]
fn cmd_reliability_resolve(flags: &CliFlags, file: &Path) -> i32 {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read file '{}': {}", file.display(), e);
            return 1;
        }
    };
    let doc = match etdl_parser::parse_document(&content) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    let base_dir = file.parent().unwrap_or(Path::new("."));
    let registry = match etdl_parser::load_asyncapi_imports(&doc, base_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    // Validate first; then resolve external sources for the manifest.
    let compiler = compiler_with_plugins(etdl_compiler::Compiler::new());
    let mut diagnostics = compiler.validate_with_base(&doc, &registry, base_dir);
    if diagnostics.iter().any(|d| d.is_error()) {
        print_diagnostics(flags, &diagnostics);
        return 1;
    }

    let (resolved, manifest) =
        etdl_compiler::reliability::resolve_reliability(&doc, base_dir, &mut diagnostics);

    if flags.json {
        println!(
            "{}",
            serde_json::json!({
                "document": file.display().to_string(),
                "resolved": resolved.iter().map(|r| serde_json::json!({
                    "basic_event": r.basic_event,
                    "value": r.resolved.value,
                    "artifact": r.resolved.artifact_id,
                    "artifact_version": r.resolved.artifact_version,
                    "estimate": r.resolved.estimate_id,
                })).collect::<Vec<_>>(),
                "manifest": manifest,
            })
        );
        return 0;
    }

    if resolved.is_empty() {
        println!("no external probability sources resolved");
    }
    for r in &resolved {
        println!(
            "basic event '{}': value={} from artifact '{}' v{} estimate '{}'",
            r.basic_event,
            r.resolved.value,
            r.resolved.artifact_id,
            r.resolved.artifact_version.as_deref().unwrap_or("?"),
            r.resolved.estimate_id
        );
    }
    0
}

/// `etdl reliability validate <file>`: validate every reliability artifact
/// referenced by an `.etdl` document. Reports structural and semantic issues
/// (duplicate estimate ids, missing versions, invalid metrics/uncertainty).
#[cfg(feature = "reliability")]
fn cmd_reliability_validate(flags: &CliFlags, file: &Path) -> i32 {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read file '{}': {}", file.display(), e);
            return 1;
        }
    };
    let doc = match etdl_parser::parse_document(&content) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    let base_dir = file.parent().unwrap_or(Path::new("."));
    let registry = match etdl_parser::load_asyncapi_imports(&doc, base_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    let compiler = compiler_with_plugins(etdl_compiler::Compiler::new());
    let diagnostics = compiler.validate_with_base(&doc, &registry, base_dir);

    // Collect every artifact declared in x-reliability.sources and validate it.
    let mut issues = Vec::new();
    if let Some(ext) = doc.extensions.get("x-reliability") {
        if let Some(obj) = ext.as_mapping() {
            if let Some(sources) = obj.get(serde_yaml::Value::String("sources".into())) {
                if let Some(arr) = sources.as_sequence() {
                    for src in arr {
                        if let Some(map) = src.as_mapping() {
                            let id = map
                                .get(serde_yaml::Value::String("id".into()))
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let file = map
                                .get(serde_yaml::Value::String("file".into()))
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            if file.is_empty() {
                                continue;
                            }
                            let path = if file.split('/').any(|seg| seg == "..") {
                                base_dir.join(&file)
                            } else {
                                let p = Path::new(&file);
                                if p.is_absolute() {
                                    p.to_path_buf()
                                } else {
                                    base_dir.join(p)
                                }
                            };
                            match etdl_compiler::reliability::load_artifact_for_cli(&path) {
                                Ok(artifact) => {
                                    let found =
                                        etdl_reliability_core::validation::validate_artifact_issues(
                                            &artifact,
                                        );
                                    for issue in &found {
                                        issues
                                            .push(format!("source '{}' ({}): {}", id, file, issue));
                                    }
                                }
                                Err(e) => issues.push(format!("source '{}' ({}): {}", id, file, e)),
                            }
                        }
                    }
                }
            }
        }
    }

    let has_errors = diagnostics.iter().any(|d| d.is_error()) || !issues.is_empty();
    if flags.json {
        println!(
            "{}",
            serde_json::json!({
                "document": file.display().to_string(),
                "diagnostics": diagnostics.iter().map(|d| serde_json::json!({
                    "code": d.code,
                    "severity": format!("{:?}", d.severity),
                    "message": d.message,
                })).collect::<Vec<_>>(),
                "artifact_issues": issues,
                "valid": !has_errors,
            })
        );
        return if has_errors { 1 } else { 0 };
    }

    print_diagnostics(flags, &diagnostics);
    if issues.is_empty() {
        println!("reliability artifacts: OK");
    } else {
        for i in &issues {
            println!("issue: {}", i);
        }
    }
    if has_errors {
        1
    } else {
        0
    }
}

/// `etdl reliability inspect <file>`: summarize a reliability artifact's
/// structure and estimates without evaluating anything, and report any
/// validation issues found.
#[cfg(feature = "reliability")]
fn cmd_reliability_inspect(flags: &CliFlags, file: &Path) -> i32 {
    let artifact = match etdl_compiler::reliability::load_artifact_for_cli(file) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    let issues = etdl_reliability_core::validation::validate_artifact_issues(&artifact);

    if flags.json {
        let estimates: Vec<_> = artifact
            .estimates
            .iter()
            .map(|(key, est)| {
                serde_json::json!({
                    "key": key,
                    "event": est.event,
                    "state": format!("{:?}", est.state),
                    "value": est.value,
                    "metric": format!("{:?}", est.metric),
                    "time_basis": est.time_basis.map(|t| t.to_string()),
                    "conditions": est.conditions,
                    "has_uncertainty": est.uncertainty.is_some(),
                    "has_provenance": est.provenance.is_some(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "schema": artifact.schema,
                "id": artifact.id,
                "version": artifact.version,
                "estimate_count": artifact.estimates.len(),
                "estimates": estimates,
                "issues": issues.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
                "valid": issues.is_empty(),
            })
        );
        return if issues.is_empty() { 0 } else { 1 };
    }

    println!("artifact: {}", artifact.id);
    println!("schema:   {}", artifact.schema);
    println!(
        "version:  {}",
        artifact.version.as_deref().unwrap_or("(missing)")
    );
    println!("estimates: {}", artifact.estimates.len());
    for (key, est) in &artifact.estimates {
        let value = est
            .value
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(unknown)".to_string());
        let tb = est
            .time_basis
            .map(|t| format!(" @{}", t))
            .unwrap_or_default();
        println!("  {} = {} ({:?}{})", key, value, est.metric, tb);
    }
    if issues.is_empty() {
        println!("validation: OK");
        0
    } else {
        for i in &issues {
            println!("validation issue: {}", i);
        }
        1
    }
}

/// `etdl reliability estimate`: read observations, run an estimator, and write
/// a reliability artifact. The estimator converts evidence to a typed estimate
/// with provenance; it never turns discovery confidence into a probability.
#[cfg(feature = "reliability")]
fn cmd_reliability_estimate(flags: &CliFlags, args: &EstimateArgs) -> i32 {
    use etdl_reliability::analysis::{builtin_estimators, EstimationConfig};
    use etdl_reliability::observations::ObservationSet;
    use etdl_reliability_core::artifact::ReliabilityArtifact;

    let file = &args.file;
    let method = &args.method;
    let output = args.output.as_deref();

    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "error: cannot read observations '{}': {}",
                file.display(),
                e
            );
            return 1;
        }
    };
    let set: ObservationSet = match serde_yaml::from_str(&content) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "error: cannot parse observations '{}': {}",
                file.display(),
                e
            );
            return 1;
        }
    };
    if let Err(e) = set.validate() {
        eprintln!("error: invalid observations: {}", e);
        return 1;
    }

    let estimator = builtin_estimators()
        .into_iter()
        .find(|e| match method.as_str() {
            "empirical" => e.name() == "empirical/binomial",
            "beta-binomial" => e.name() == "bayesian/beta-binomial",
            "exponential" => e.name() == "exponential/constant-rate",
            _ => false,
        });
    let estimator = match estimator {
        Some(e) => e,
        None => {
            eprintln!(
                "error: unknown estimator '{}' (empirical | beta-binomial | exponential)",
                method
            );
            return 1;
        }
    };

    let config = EstimationConfig {
        level: args.level,
        prior_alpha: args.prior_alpha,
        prior_beta: args.prior_beta,
        mission_time: args.mission_time,
        metric: etdl_reliability_core::probability::ProbabilityMetric::Probability,
    };

    let mut artifact = ReliabilityArtifact::new("etdl-estimate");
    artifact.version = Some("1.0.0".to_string());
    let mut results = Vec::new();
    for obs in &set.observations {
        match estimator.estimate(obs, &config) {
            Ok(est) => {
                if let Err(e) = artifact.add(est.clone()) {
                    eprintln!(
                        "error: cannot store estimate for '{}': {}",
                        obs.failure_mode, e
                    );
                    return 1;
                }
                results.push(est);
            }
            Err(e) => {
                eprintln!("error: estimation failed for '{}': {}", obs.failure_mode, e);
                return 1;
            }
        }
    }

    if let Some(out) = output {
        let text = if out.extension().is_some_and(|e| e == "json") {
            serde_json::to_string_pretty(&artifact).unwrap()
        } else {
            serde_yaml::to_string(&artifact).unwrap()
        };
        if let Err(e) = std::fs::write(out, text) {
            eprintln!("error: cannot write '{}': {}", out.display(), e);
            return 1;
        }
        if !flags.quiet {
            println!("wrote reliability artifact to {}", out.display());
        }
    }

    if flags.json {
        println!(
            "{}",
            serde_json::json!({
                "estimator": estimator.name(),
                "version": estimator.version(),
                "assumptions": estimator.assumptions(),
                "estimates": results,
            })
        );
    } else {
        println!("estimator: {} v{}", estimator.name(), estimator.version());
        for a in estimator.assumptions() {
            println!("  assumption: {}", a);
        }
        for est in &results {
            let value = est
                .value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "(unknown)".to_string());
            println!(
                "  {} = {} ({})",
                est.event,
                value,
                est.method.clone().unwrap_or_default()
            );
            if let Some(u) = &est.uncertainty {
                println!("    uncertainty: {:?}", u);
            }
        }
    }
    0
}

/// `etdl reliability trace`: print the backward trace of one estimate.
#[cfg(feature = "reliability")]
fn cmd_reliability_trace(flags: &CliFlags, file: &Path, estimate_id: &str) -> i32 {
    let artifact = match etdl_compiler::reliability::load_artifact_for_cli(file) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    let estimate = match artifact.get(estimate_id) {
        Some(e) => e,
        None => {
            eprintln!("error: no estimate '{}' in artifact", estimate_id);
            return 1;
        }
    };
    let trace = etdl_reliability::trace_from_estimate(
        estimate,
        Some(&artifact.id),
        artifact.version.as_deref(),
    );

    if flags.json {
        println!("{}", serde_json::to_string_pretty(&trace).unwrap());
    } else {
        println!("Trace for '{}':", estimate_id);
        print!("{}", trace.render());
    }
    0
}

/// `etdl library list`: list the built-in standard library modules shipped
/// with this binary. Does not require a document.
fn cmd_library_list(flags: &CliFlags) -> i32 {
    let libs = etdl_compiler::stdlib::list_builtin();
    let mut had_error = false;

    if flags.json {
        let items: Vec<serde_json::Value> = libs
            .iter()
            .map(|r| match r {
                Ok(lib) => serde_json::json!({
                    "name": lib.name,
                    "version": lib.version,
                    "kind": lib.kind.label(),
                    "description": lib.description,
                    "basic_events": lib.basic_events.keys().collect::<Vec<_>>(),
                    "gates": lib.gates.keys().collect::<Vec<_>>(),
                }),
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "schema": etdl_compiler::stdlib::STDLIB_SCHEMA,
                "libraries": items,
            })
        );
    } else {
        println!("ETDL Standard Library ({})", etdl_compiler::stdlib::STDLIB_SCHEMA);
        for r in &libs {
            match r {
                Ok(lib) => {
                    println!("  {} v{} (built-in)", lib.name, lib.version);
                    if let Some(d) = &lib.description {
                        println!("    {}", d);
                    }
                    if !lib.basic_events.is_empty() {
                        println!(
                            "    basic events: {}",
                            lib.basic_events.keys().cloned().collect::<Vec<_>>().join(", ")
                        );
                    }
                    if !lib.gates.is_empty() {
                        println!(
                            "    gates: {}",
                            lib.gates.keys().cloned().collect::<Vec<_>>().join(", ")
                        );
                    }
                }
                Err(e) => {
                    eprintln!("  error: {}", e);
                    had_error = true;
                }
            }
        }
    }
    if had_error {
        1
    } else {
        0
    }
}

/// `etdl library resolve <file>`: resolve every library the document
/// declares and show how each resolved, without compiling anything.
fn cmd_library_resolve(flags: &CliFlags, file: &Path, library_path: &[PathBuf]) -> i32 {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read file '{}': {}", file.display(), e);
            return 1;
        }
    };
    let doc = match etdl_parser::parse_document(&content) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    let base_dir = file.parent().unwrap_or(Path::new("."));

    let resolver = library_path
        .iter()
        .fold(etdl_compiler::stdlib::LibraryResolver::new(), |r, p| {
            r.with_search_path(p.clone())
        });
    let (_expanded, resolved, errors) = etdl_compiler::stdlib::expand_libraries(&doc, base_dir, &resolver);

    if flags.json {
        println!(
            "{}",
            serde_json::json!({
                "document": file.display().to_string(),
                "resolved": resolved.iter().map(|l| l.provenance()).collect::<Vec<_>>(),
                "errors": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
            })
        );
    } else {
        if doc.libraries.is_empty() {
            println!("document declares no libraries");
        }
        for import in &doc.libraries {
            match resolved.iter().find(|l| l.name == import.name) {
                Some(lib) => println!(
                    "  {} requested {} -> resolved {} ({}){}",
                    import.name,
                    import.version,
                    lib.version,
                    lib.kind.label(),
                    if import.required { "" } else { " [optional]" }
                ),
                None => println!(
                    "  {} requested {} -> UNRESOLVED{}",
                    import.name,
                    import.version,
                    if import.required { "" } else { " [optional]" }
                ),
            }
        }
        for e in &errors {
            println!("  error: {}", e);
        }
    }
    if errors.is_empty() {
        0
    } else {
        1
    }
}

// --- Dynamic supplement plugins (`etdl install`, `etdl supplement list/remove`). ---

/// `~/.etdl/plugins/` — where installed `.wasm` modules and their
/// manifest live. `$HOME` (or `%USERPROFILE%` on Windows, checked as a
/// fallback since no cross-platform directories crate is otherwise
/// pulled into this binary) must be set; there is no other configurable
/// location in this version.
fn plugins_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "cannot determine home directory (HOME/USERPROFILE not set)".to_string())?;
    Ok(PathBuf::from(home).join(".etdl").join("plugins"))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PluginManifestEntry {
    id: String,
    version: String,
    source: String,
    file: String,
    installed_at: String,
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("manifest.json")
}

fn load_manifest(dir: &Path) -> Vec<PluginManifestEntry> {
    std::fs::read_to_string(manifest_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_manifest(dir: &Path, entries: &[PluginManifestEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    std::fs::write(manifest_path(dir), json).map_err(|e| e.to_string())
}

/// Registers every installed plugin (`~/.etdl/plugins/*.wasm`, per the
/// manifest) onto `compiler` via the pre-existing, previously-unused
/// `Compiler::with_extension` — additive: a document behaves identically
/// whether or not any plugins are installed, and installing zero plugins
/// leaves this a no-op.
/// Load failures are logged to stderr and skipped rather than aborting
/// the whole command — one broken plugin should not block ordinary
/// `etdl validate`/`compile`/`analyze` use.
fn compiler_with_plugins(compiler: etdl_compiler::Compiler) -> etdl_compiler::Compiler {
    let Ok(dir) = plugins_dir() else {
        return compiler;
    };
    let entries = load_manifest(&dir);
    entries.into_iter().fold(compiler, |c, entry| {
        let path = dir.join(&entry.file);
        match std::fs::read(&path) {
            Ok(bytes) => match etdl_compiler::wasm_extension::WasmExtension::load(&bytes) {
                Ok(ext) => c.with_extension(Box::new(ext)),
                Err(e) => {
                    eprintln!(
                        "warning: installed plugin '{}' failed to load, skipping: {}",
                        entry.id, e
                    );
                    c
                }
            },
            Err(e) => {
                eprintln!(
                    "warning: installed plugin '{}' file missing ({}), skipping: {}",
                    entry.id,
                    path.display(),
                    e
                );
                c
            }
        }
    })
}

fn fetch_plugin_bytes(source: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if source.starts_with("https://") {
        let response = ureq::get(source)
            .call()
            .map_err(|e| format!("download failed: {e}"))?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("failed to read response body: {e}"))?;
        Ok(bytes)
    } else if source.starts_with("http://") {
        Err("only https:// URLs are accepted (a supplement plugin is executable code; \
             fetching it over plain http is not supported)"
            .to_string())
    } else {
        std::fs::read(source).map_err(|e| format!("cannot read '{}': {}", source, e))
    }
}

fn cmd_supplement_install(flags: &CliFlags, source: &str) -> i32 {
    let _ = flags;

    let bytes = match fetch_plugin_bytes(source) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    // Load once now, before installing anything, so a broken/non-conforming
    // module is rejected here rather than silently accepted and only
    // discovered broken on first real use.
    let ext = match etdl_compiler::wasm_extension::WasmExtension::load(&bytes) {
        Ok(ext) => ext,
        Err(e) => {
            eprintln!("error: not a conforming supplement plugin: {}", e);
            return 1;
        }
    };

    let dir = match plugins_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("error: cannot create '{}': {}", dir.display(), e);
        return 1;
    }

    let file_name = format!("{}.wasm", ext.id().replace(['/', '\\', ':'], "_"));
    let dest = dir.join(&file_name);
    if let Err(e) = std::fs::write(&dest, &bytes) {
        eprintln!("error: cannot write '{}': {}", dest.display(), e);
        return 1;
    }

    let mut entries = load_manifest(&dir);
    entries.retain(|e| e.id != ext.id());
    entries.push(PluginManifestEntry {
        id: ext.id().to_string(),
        version: ext.version().to_string(),
        source: source.to_string(),
        file: file_name,
        installed_at: format!("{:?}", std::time::SystemTime::now()),
    });
    if let Err(e) = save_manifest(&dir, &entries) {
        eprintln!("error: cannot update manifest: {}", e);
        return 1;
    }

    println!("installed {} v{} (from {})", ext.id(), ext.version(), source);
    0
}

fn cmd_supplement_list(flags: &CliFlags) -> i32 {
    let registry = etdl_compiler::extension::builtin_registry();
    let dir = plugins_dir().ok();
    let installed = dir.as_deref().map(load_manifest).unwrap_or_default();

    if flags.json {
        println!(
            "{}",
            serde_json::json!({
                "builtin": registry.list().iter().map(|id| {
                    let ext = registry.lookup(id).expect("listed");
                    let d = ext.descriptor();
                    serde_json::json!({
                        "id": ext.id(),
                        "version": ext.version(),
                        "summary": d.summary,
                        "schema": d.schema,
                        "diagnostic_codes": d.diagnostic_codes,
                        "requires": d.requires,
                    })
                }).collect::<Vec<_>>(),
                "installed": installed,
            })
        );
        return 0;
    }

    println!("Built-in extensions:");
    for id in registry.list() {
        let ext = registry.lookup(id).expect("listed");
        let summary = ext.descriptor().summary;
        if summary.is_empty() {
            println!("  {}", id);
        } else {
            println!("  {} — {}", id, summary);
        }
    }
    println!("Installed plugins:");
    if installed.is_empty() {
        println!("  (none)");
    } else {
        for entry in &installed {
            println!(
                "  {} v{} (from {}, installed {})",
                entry.id, entry.version, entry.source, entry.installed_at
            );
        }
    }
    0
}

fn cmd_supplement_remove(flags: &CliFlags, id: &str) -> i32 {
    let _ = flags;
    let dir = match plugins_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    let mut entries = load_manifest(&dir);
    let Some(pos) = entries.iter().position(|e| e.id == id) else {
        eprintln!("error: no installed plugin with id '{}'", id);
        return 1;
    };
    let entry = entries.remove(pos);
    let path = dir.join(&entry.file);
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!("error: cannot remove '{}': {}", path.display(), e);
            return 1;
        }
    }
    if let Err(e) = save_manifest(&dir, &entries) {
        eprintln!("error: cannot update manifest: {}", e);
        return 1;
    }
    println!("removed {}", id);
    0
}

fn parse_document_or_exit(file: &Path) -> Result<etdl_parser::ast::EtlDocument, i32> {
    let content = std::fs::read_to_string(file).map_err(|e| {
        eprintln!("error: cannot read file '{}': {}", file.display(), e);
        1
    })?;
    etdl_parser::parse_document(&content).map_err(|e| {
        eprintln!("error: {}", e);
        1
    })
}

/// `etdl tree validate <file>`: validate every tree declared under
/// `x-tree-event` in a document.
fn cmd_tree_validate(flags: &CliFlags, file: &Path) -> i32 {
    let doc = match parse_document_or_exit(file) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let (trees, diagnostics) = etdl_compiler::tree_event::parse_and_validate_trees(&doc);

    if flags.json {
        println!(
            "{}",
            serde_json::json!({
                "document": file.display().to_string(),
                "valid": diagnostics.is_empty(),
                "trees": trees.iter().map(|t| &t.id).collect::<Vec<_>>(),
                "diagnostics": diagnostics.iter().map(|d| serde_json::json!({
                    "code": d.code,
                    "severity": if d.is_error() { "error" } else { "warning" },
                    "message": d.message,
                })).collect::<Vec<_>>(),
            })
        );
    } else if diagnostics.is_empty() {
        if trees.is_empty() {
            println!("document declares no trees under x-tree-event (or the supplement is not declared)");
        } else {
            println!(
                "{} tree(s) valid: {}",
                trees.len(),
                trees.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    } else {
        for d in &diagnostics {
            println!("[{}] {}", d.code, d.message);
        }
    }
    if diagnostics.iter().any(|d| d.is_error()) {
        1
    } else {
        0
    }
}

/// `etdl tree inspect <file>`: summarize each declared tree's structure.
fn cmd_tree_inspect(flags: &CliFlags, file: &Path) -> i32 {
    let doc = match parse_document_or_exit(file) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let (trees, diagnostics) = etdl_compiler::tree_event::parse_and_validate_trees(&doc);

    if flags.json {
        let items: Vec<_> = trees
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "version": t.version,
                    "schema": t.schema,
                    "root": t.root,
                    "node_count": t.nodes.len(),
                    "leaves": t.leaves(),
                    "preorder": t.preorder(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "document": file.display().to_string(),
                "trees": items,
                "diagnostics": diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>(),
            })
        );
    } else {
        if trees.is_empty() && diagnostics.is_empty() {
            println!("document declares no trees under x-tree-event (or the supplement is not declared)");
        }
        for t in &trees {
            println!("tree: {} v{} ({})", t.id, t.version, t.schema);
            println!("  root: {}", t.root);
            println!("  nodes: {}", t.nodes.len());
            println!("  leaves: {}", t.leaves().join(", "));
            println!("  preorder: {}", t.preorder().join(" -> "));
        }
        for d in &diagnostics {
            println!("issue: [{}] {}", d.code, d.message);
        }
    }
    if diagnostics.iter().any(|d| d.is_error()) {
        1
    } else {
        0
    }
}

/// Like [`parse_document_or_exit`], but returns the error message instead
/// of printing it and an exit code — `cmd_context_*` need the message
/// embedded in their own per-file JSON/JSONL output, not just written to
/// stderr, since a batch continues past a bad file rather than aborting.
fn parse_document_for_context(file: &Path) -> Result<etdl_parser::ast::EtlDocument, String> {
    let content = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read file '{}': {}", file.display(), e))?;
    etdl_parser::parse_document(&content).map_err(|e| e.to_string())
}

/// `etdl context dump <files...>`: dump each document's full parsed AST as
/// JSON, one array entry per file. Mirrors `etdl-wasm::parse_for_diagram`
/// (`serde_json::to_value(&doc)`) — parse-only, no validation.
fn cmd_context_dump(files: &[PathBuf]) -> i32 {
    let mut any_error = false;
    let mut entries = Vec::with_capacity(files.len());
    for file in files {
        match parse_document_for_context(file) {
            Ok(doc) => match serde_json::to_value(&doc) {
                Ok(ast) => entries.push(serde_json::json!({ "file": file.display().to_string(), "ast": ast })),
                Err(e) => {
                    any_error = true;
                    entries.push(serde_json::json!({ "file": file.display().to_string(), "error": e.to_string() }));
                }
            },
            Err(e) => {
                any_error = true;
                entries.push(serde_json::json!({ "file": file.display().to_string(), "error": e }));
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&entries).expect("Vec<Value> always serializes"));
    if any_error {
        1
    } else {
        0
    }
}

/// `etdl context graph <files...>`: export each document as a unified
/// `{nodes, edges}` graph (`etdl_compiler::context::build_graph`), one
/// array entry per file.
fn cmd_context_graph(files: &[PathBuf]) -> i32 {
    let mut any_error = false;
    let mut entries = Vec::with_capacity(files.len());
    for file in files {
        match parse_document_for_context(file) {
            Ok(doc) => {
                let graph = etdl_compiler::context::build_graph(&doc);
                entries.push(serde_json::json!({ "file": file.display().to_string(), "graph": graph }));
            }
            Err(e) => {
                any_error = true;
                entries.push(serde_json::json!({ "file": file.display().to_string(), "error": e }));
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&entries).expect("Vec<Value> always serializes"));
    if any_error {
        1
    } else {
        0
    }
}

/// `etdl context chunks <files...>`: export each document as RAG-ready
/// chunks (`etdl_compiler::context::build_chunks`) — JSON Lines, one
/// compact object per line (the standard ingestion format for
/// embedding/vector-store pipelines), each tagged with its source `file`.
fn cmd_context_chunks(files: &[PathBuf]) -> i32 {
    let mut any_error = false;
    for file in files {
        match parse_document_for_context(file) {
            Ok(doc) => {
                for chunk in etdl_compiler::context::build_chunks(&doc) {
                    let line = serde_json::json!({
                        "file": file.display().to_string(),
                        "chunk_id": chunk.chunk_id,
                        "kind": chunk.kind,
                        "text": chunk.text,
                        "metadata": chunk.metadata,
                    });
                    println!("{line}");
                }
            }
            Err(e) => {
                any_error = true;
                println!("{}", serde_json::json!({ "file": file.display().to_string(), "error": e }));
            }
        }
    }
    if any_error {
        1
    } else {
        0
    }
}

/// `etdl reliability calibrate`: compare a reliability artifact's prediction
/// for `event` against one or more observation datasets. Loads and validates
/// each dataset, aggregates the compatible observations for `event` (refusing
/// to combine incompatible exposure units/conditions), and runs the binomial
/// calibration test. Never writes back to the artifact: the output is a
/// report for engineering review.
#[cfg(feature = "reliability")]
fn cmd_reliability_calibrate(flags: &CliFlags, args: &CalibrateArgs) -> i32 {
    use etdl_reliability::calibration::{calibrate, CalibrationConfig};
    use etdl_reliability::dataset::{aggregate_across, ObservationDataset};

    let artifact = match etdl_compiler::reliability::load_artifact_for_cli(&args.artifact) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    let mut datasets: Vec<ObservationDataset> = Vec::new();
    for path in &args.dataset {
        let ds: ObservationDataset = match load_serde_file(path, "observation dataset") {
            Ok(d) => d,
            Err(code) => return code,
        };
        if let Err(e) = ds.validate() {
            eprintln!(
                "error: dataset '{}' ({}) is invalid: {}",
                ds.id,
                path.display(),
                e
            );
            return 1;
        }
        datasets.push(ds);
    }

    let refs: Vec<&ObservationDataset> = datasets.iter().collect();
    let aggregated = match aggregate_across(&refs, &args.event) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: cannot aggregate observations for '{}': {}", args.event, e);
            return 1;
        }
    };

    if args.alpha <= 0.0 || args.alpha >= 1.0 {
        eprintln!("error: --alpha must be in (0, 1), got {}", args.alpha);
        return 1;
    }
    if args.strict_alpha <= 0.0 || args.strict_alpha >= args.alpha {
        eprintln!(
            "error: --strict-alpha must be in (0, {}), got {}",
            args.alpha, args.strict_alpha
        );
        return 1;
    }

    let config = CalibrationConfig {
        alpha: args.alpha,
        strict_alpha: args.strict_alpha,
        min_exposure: args.min_exposure,
    };

    let result = match calibrate(
        &artifact,
        &args.event,
        &aggregated.observation,
        aggregated.provenance.source_datasets.clone(),
        &config,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    if let Some(path) = args.output.as_deref() {
        match serde_json::to_string_pretty(&result) {
            Ok(body) => {
                if let Err(e) = std::fs::write(path, body) {
                    eprintln!("error: cannot write '{}': {}", path.display(), e);
                    return 1;
                }
                if !flags.quiet {
                    eprintln!("wrote calibration result to {}", path.display());
                }
            }
            Err(e) => {
                eprintln!("error: cannot serialise calibration result: {e}");
                return 1;
            }
        }
    }

    if flags.json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        print!("{}", result.render());
        if result.is_drift() {
            println!(
                "note: this indicates model drift under the configured significance level; \
                 review before publishing a new estimate. Nothing has been changed \
                 automatically."
            );
        }
    }
    0
}

/// Print a clear message when a requested capability is not compiled into this
/// binary (no automatic download; the user rebuilds with the feature enabled).
#[allow(dead_code)] // used only in not(feature=...) branches
fn capability_unavailable(capability: &str, how: &str) {
    eprintln!(
        "{} support is not enabled in this build.\n  {}",
        capability, how
    );
}

/// The predictive-reliability schema id, when the `reliability` feature
/// (and thus `etdl-reliability`) is compiled in; `"unavailable"` otherwise.
/// Kept as a small cfg-gated helper (rather than referencing
/// `etdl_reliability::predictive::PREDICTIVE_SCHEMA` inline) because
/// `etdl-reliability` is an optional dependency — a lean
/// `--no-default-features` build must still compile.
#[cfg(feature = "reliability")]
fn predictive_reliability_schema(_analysis: bool) -> &'static str {
    etdl_reliability::predictive::PREDICTIVE_SCHEMA
}

#[cfg(not(feature = "reliability"))]
fn predictive_reliability_schema(_analysis: bool) -> &'static str {
    "unavailable"
}

/// The `ReliabilityArtifact` schema id, when compiled in; `None`
/// otherwise. Same cfg-gated-helper discipline as
/// `predictive_reliability_schema` above, for the same reason.
#[cfg(feature = "reliability")]
fn reliability_artifact_schema() -> Option<&'static str> {
    Some(etdl_reliability_core::artifact::ARTIFACT_SCHEMA)
}

#[cfg(not(feature = "reliability"))]
fn reliability_artifact_schema() -> Option<&'static str> {
    None
}

/// `etdl conformance status`: objective PASS/PARTIAL/UNSUPPORTED status
/// per conformance area, reusing `etdl-conformance::report` (never
/// duplicating the diagnostic/extension-registry plumbing `capabilities`
/// already owns).
fn cmd_conformance_status(flags: &CliFlags) -> i32 {
    let reliability = cfg!(feature = "reliability");
    let areas = etdl_conformance::report::area_statuses(reliability);

    if flags.json {
        println!("{}", serde_json::to_string_pretty(&areas).unwrap());
    } else {
        println!("ETDL Conformance Status ({})", etdl_conformance::CONFORMANCE_SUITE_VERSION);
        for area in &areas {
            println!("  [{}] {:<45} {}", area.status, area.area, area.detail);
        }
    }
    0
}

/// `etdl conformance manifest`: the machine-readable conformance manifest
/// (task §47).
fn cmd_conformance_manifest(flags: &CliFlags) -> i32 {
    let reliability = cfg!(feature = "reliability");
    let manifest = etdl_conformance::manifest::ConformanceManifest::build(
        env!("CARGO_PKG_VERSION"),
        reliability,
        etdl_tree_core::TREE_SCHEMA,
        etdl_compiler::performance::PERFORMANCE_SCHEMA,
        etdl_compiler::safety::SAFETY_SCHEMA,
        etdl_compiler::diagnostics::DIAGNOSTICS_SCHEMA,
        etdl_compiler::security::SECURITY_SCHEMA,
        etdl_probability_core::STD_PROBABILITY_SCHEMA,
        etdl_compiler::stdlib::STDLIB_SCHEMA,
        predictive_reliability_schema(reliability),
        reliability_artifact_schema(),
    );

    if flags.json {
        println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
    } else {
        println!("ETDL language version:      {}", manifest.etdl_language_version);
        println!("Implementation version:     {}", manifest.implementation_version);
        println!("Conformance suite version:  {}", manifest.conformance_suite_version);
        println!("Supplements:");
        for s in &manifest.supported_supplements {
            println!(
                "  {} ({}) — {}",
                s.id,
                s.version,
                if s.available { "available" } else { "unavailable" }
            );
        }
        println!("Libraries: {}", manifest.supported_libraries.join(", "));
        println!("Targets: {}", manifest.supported_targets.join(", "));
        println!("Artifact schemas: {}", manifest.supported_artifact_schemas.join(", "));
    }
    0
}

/// `etdl capabilities`: report which features are compiled into this binary.
fn cmd_capabilities(flags: &CliFlags) -> i32 {
    let reliability = cfg!(feature = "reliability");
    let discovery = cfg!(feature = "discovery");
    let ontology = discovery; // ontology is compiled in with discovery support

    // Every advanced analysis capability is compiled in together with the rich
    // `etdl-reliability` crate, so they share one flag today. They are reported
    // separately because they are separately meaningful, and because a future
    // build may enable them independently. Nothing here reports a capability
    // the binary does not actually have.
    let analysis = reliability;

    let registry = etdl_compiler::extension::builtin_registry();
    let caps = serde_json::json!({
        "core": true,
        "reliability": {
            "available": reliability,
            "kind": if reliability { "built-in" } else { "unavailable" },
        },
        "reliability_analysis": analysis,
        "statistical_estimation": analysis,
        "uncertainty_analysis": analysis,
        "monte_carlo": {
            "available": analysis,
            "sampler": if analysis { "xorshift64star" } else { "unavailable" },
            "sampler_version": if analysis { "1" } else { "unavailable" },
            "method": if analysis { "monte-carlo-propagation" } else { "unavailable" },
        },
        "importance": {
            "available": analysis,
            "measures": if analysis {
                serde_json::json!([
                    "birnbaum",
                    "fussell-vesely",
                    "criticality",
                    "risk-achievement-worth",
                    "risk-reduction-worth"
                ])
            } else {
                serde_json::json!([])
            },
        },
        "sensitivity": {
            "available": analysis,
            "method": if analysis {
                "finite-perturbation/absolute/two-sided"
            } else {
                "unavailable"
            },
        },
        "uncertainty_ranking": analysis,
        "analysis_comparison": analysis,
        "standard_library": {
            "available": true,
            "schema": etdl_compiler::stdlib::STDLIB_SCHEMA,
            "builtin_libraries": etdl_compiler::stdlib::LibraryResolver::builtin_names(),
        },
        "std_probability": {
            "available": true,
            "schema": etdl_probability_core::STD_PROBABILITY_SCHEMA,
            "kind": "built-in",
            "distributions": ["bernoulli", "binomial", "beta", "exponential", "normal"],
            "sampling": "unavailable (deterministic math only; see docs/reference/standard-probability-library.md)",
        },
        "runtime_feedback": {
            "available": analysis,
            "observation_dataset": analysis,
            "calibration": {
                "available": analysis,
                "method": if analysis { "binomial-two-sided-exact" } else { "unavailable" },
            },
        },
        "predictive_reliability": {
            "available": analysis,
            "schema": predictive_reliability_schema(analysis),
            "kind": if analysis { "built-in" } else { "unavailable" },
            "models": if analysis { serde_json::json!(["exponential", "weibull"]) } else { serde_json::json!([]) },
            "quantities": if analysis {
                serde_json::json!(["survival", "reliability", "failure-probability", "hazard", "cumulative-hazard", "density"])
            } else {
                serde_json::json!([])
            },
            "sampling": "unavailable (deterministic closed-form math only; see docs/reference/predictive-reliability-supplement.md)",
            "censored_data_fitting": "unavailable (censored observations can be represented, not fit; see docs)",
        },
        "correlated_parameter_uncertainty": false,
        "conditional_probability_evaluation": false,
        "failure_discovery": discovery,
        "ontology": ontology,
        // Every built-in supplement extension (`etdl.tree-event`,
        // `etdl.performance`, `etdl.safety`, `etdl.diagnostics`,
        // `etdl.security`, and `etdl.reliability` when the `reliability`
        // feature is on), described by its own `EtdlExtension::descriptor()`
        // — colocated with each supplement's own validation code
        // (`etdl-compiler/src/{tree_event,performance,safety,diagnostics,
        // security,reliability}.rs`), not hand-duplicated here. Adding a
        // new supplement's `descriptor()` makes it show up here
        // automatically; nothing in this command needs editing.
        "extensions": registry
            .list()
            .iter()
            .map(|id| {
                let ext = registry.lookup(id).expect("listed");
                let d = ext.descriptor();
                serde_json::json!({
                    "id": ext.id(),
                    "version": ext.version(),
                    "summary": d.summary,
                    "schema": d.schema,
                    "diagnostic_codes": d.diagnostic_codes,
                    "requires": d.requires,
                })
            })
            .collect::<Vec<_>>(),
        "compiler_version": env!("CARGO_PKG_VERSION"),
    });

    if flags.json {
        println!("{}", caps);
    } else {
        println!("etdl {}", env!("CARGO_PKG_VERSION"));
        println!("Core: yes");
        println!(
            "Standard library: available ({}) — built-in: {}",
            etdl_compiler::stdlib::STDLIB_SCHEMA,
            etdl_compiler::stdlib::LibraryResolver::builtin_names().join(", ")
        );
        println!(
            "std.probability: available ({}) — distributions: bernoulli, binomial, beta, \
             exponential, normal; sampling: unavailable (deterministic math only)",
            etdl_probability_core::STD_PROBABILITY_SCHEMA
        );
        // Every built-in supplement extension except `etdl.reliability`
        // (which gets its own detailed capability breakdown just below,
        // since its analysis sub-capabilities aren't reducible to a single
        // descriptor) — described by its own `EtdlExtension::descriptor()`,
        // colocated with each supplement's own validation code. Adding a
        // new supplement's `descriptor()` makes it print here
        // automatically; nothing in this command needs editing.
        for id in registry.list() {
            if id == "etdl.reliability" {
                continue;
            }
            let ext = registry.lookup(id).expect("listed");
            let d = ext.descriptor();
            let schema = d
                .schema
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            let requires = if d.requires.is_empty() {
                String::new()
            } else {
                format!(" [requires: {}]", d.requires.join(", "))
            };
            println!("{}: available{}{} — {}", id, schema, requires, d.summary);
        }
        if reliability {
            println!("Reliability: built-in");
            println!("Reliability Analysis: available");
            println!("Statistical estimation: available");
            println!("Uncertainty analysis: available");
            println!("Monte Carlo: available (xorshift64star/1, monte-carlo-propagation/1)");
            println!(
                "Importance: available (birnbaum, fussell-vesely, criticality, \
                 risk-achievement-worth, risk-reduction-worth)"
            );
            println!("Sensitivity: available (finite-perturbation/absolute/two-sided)");
            println!("Uncertainty ranking: available");
            println!("Analysis comparison: available");
            println!("Runtime feedback / calibration: available (binomial-two-sided-exact)");
        } else {
            println!("Reliability: unavailable (rebuild with 'reliability')");
            println!("Reliability Analysis: unavailable (rebuild with 'reliability')");
            println!("Statistical estimation: unavailable (rebuild with 'reliability')");
            println!("Uncertainty analysis: unavailable (rebuild with 'reliability')");
            println!("Monte Carlo: unavailable (rebuild with 'reliability')");
            println!("Importance: unavailable (rebuild with 'reliability')");
            println!("Sensitivity: unavailable (rebuild with 'reliability')");
            println!("Runtime feedback / calibration: unavailable (rebuild with 'reliability')");
        }
        // Not implemented in any build; never reported as available.
        println!("Correlated parameter uncertainty: unsupported");
        println!("Conditional probability evaluation: unsupported");
        if discovery {
            println!("Failure Discovery: available");
            println!("Ontology: available");
        } else {
            println!("Failure Discovery: unavailable (rebuild with 'discovery')");
            println!("Ontology: unavailable (rebuild with 'discovery')");
        }
    }
    0
}
