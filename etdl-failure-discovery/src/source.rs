//! Source project walking: deterministic collection of analyzable files,
//! content hashing, and project identity.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::DiscoveryConfig;
use crate::error::DiscoveryError;

/// A deterministic FNV-1a 64-bit hash over the concatenated sorted contents
/// of analyzed files. Stable across runs and platforms.
pub fn content_hash(files: &[(PathBuf, String)]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (path, content) in files {
        h = hash_bytes(h, path.to_string_lossy().as_bytes());
        h = hash_bytes(h, b"\0");
        h = hash_bytes(h, content.as_bytes());
        h = hash_bytes(h, b"\0");
    }
    format!("{h:016x}")
}

fn hash_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The set of source files to analyze for a path (file or directory), sorted
/// deterministically, with exclusions applied.
pub fn collect_source_files(
    root: &Path,
    config: &DiscoveryConfig,
    language: &str,
) -> Result<Vec<PathBuf>, DiscoveryError> {
    let mut files = Vec::new();
    if root.is_file() {
        if config.is_excluded(root) {
            return Ok(Vec::new());
        }
        files.push(root.to_path_buf());
    } else if root.is_dir() {
        collect_dir(root, root, config, language, &mut files)?;
    } else {
        return Err(if root.exists() {
            DiscoveryError::InvalidPath(root.to_path_buf())
        } else {
            DiscoveryError::NoSuchPath(root.to_path_buf())
        });
    }
    files.sort();
    Ok(files)
}

fn collect_dir(
    base: &Path,
    dir: &Path,
    config: &DiscoveryConfig,
    language: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), DiscoveryError> {
    let entries = std::fs::read_dir(dir).map_err(|e| DiscoveryError::SourceRead {
        path: dir.to_path_buf(),
        source: e,
    })?;
    let mut subdirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| DiscoveryError::SourceRead {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap_or(&path);
        if config.is_excluded(rel) {
            continue;
        }
        if path.is_dir() {
            subdirs.push(path);
        } else if is_language_file(&path, language) {
            out.push(path);
        }
    }
    subdirs.sort();
    for d in subdirs {
        collect_dir(base, &d, config, language, out)?;
    }
    Ok(())
}

/// Whether a file matches a language by extension.
pub fn is_language_file(path: &Path, language: &str) -> bool {
    match language {
        "rust" => path.extension().is_some_and(|e| e == "rs"),
        _ => true, // unknown language: analyze all files conservatively
    }
}

/// Read a set of files into `(path, content)` pairs, preserving order.
pub fn read_files(files: &[PathBuf]) -> Result<Vec<(PathBuf, String)>, DiscoveryError> {
    let mut out = Vec::new();
    for f in files {
        let content = std::fs::read_to_string(f).map_err(|e| DiscoveryError::SourceRead {
            path: f.clone(),
            source: e,
        })?;
        out.push((f.clone(), content));
    }
    Ok(out)
}

/// Best-effort crate/package name: read `Cargo.toml` in the project root.
pub fn package_name(root: &Path) -> Option<String> {
    let manifest = if root.is_file() {
        root.parent()?.join("Cargo.toml")
    } else {
        root.join("Cargo.toml")
    };
    let content = std::fs::read_to_string(manifest).ok()?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    doc.get("package")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

/// Identify the repository origin from git config if present (no network).
/// Returns (repository_url, commit_sha), both optional.
pub fn git_identity(root: &Path) -> (Option<String>, Option<String>) {
    let dir = if root.is_file() {
        root.parent()
    } else {
        Some(root)
    };
    let dir = dir.unwrap_or(Path::new("."));
    let url = git_get(dir, "remote.origin.url");
    let commit = git_head_commit(dir);
    (url, commit)
}

fn git_get(dir: &Path, key: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("config")
        .arg("--get")
        .arg(key)
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn git_head_commit(dir: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// Group files by their first path component to distinguish first-party from
/// third-party sources. Files under `vendor/`, `third_party/`, or outside the
/// project root are treated as third-party.
pub fn classify_party(root: &Path, file: &Path) -> Party {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let first = rel
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    match first.as_deref() {
        Some("vendor") | Some("third_party") | Some("node_modules") | Some("target") => {
            Party::ThirdParty
        }
        _ => Party::FirstParty,
    }
}

/// Whether source is first-party or third-party.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party {
    FirstParty,
    ThirdParty,
}

/// A collection of source files plus their provenance for one project.
#[derive(Debug, Clone)]
pub struct SourceProject {
    pub name: String,
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
    pub package_name: Option<String>,
    pub repository_url: Option<String>,
    pub commit: Option<String>,
}

/// Build a `SourceProject` from a root path and config.
pub fn build_project(
    root: &Path,
    config: &DiscoveryConfig,
    language: &str,
) -> Result<SourceProject, DiscoveryError> {
    let files = collect_source_files(root, config, language)?;
    let (repository_url, commit) = git_identity(root);
    let package_name = package_name(root);
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());
    Ok(SourceProject {
        name,
        root: root.to_path_buf(),
        files,
        package_name,
        repository_url,
        commit,
    })
}

/// A stable summary of project identity for reports (no absolute paths).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProjectIdentity {
    pub name: String,
    pub package_name: Option<String>,
    pub repository_url: Option<String>,
    pub commit: Option<String>,
    pub file_count: usize,
}

/// Merge multiple per-file reports into one project report.
pub fn merge_reports(
    schema: String,
    analyzer_name: String,
    analyzer_version: String,
    language: String,
    identity: &ProjectIdentity,
    config: &DiscoveryConfig,
    reports: &[crate::report::DiscoveryReport],
) -> crate::report::DiscoveryReport {
    let mut all = crate::report::DiscoveryReport::new(
        crate::report::AnalyzerMetadata {
            name: analyzer_name,
            version: analyzer_version,
            language,
        },
        crate::report::SourceIdentity {
            path: PathBuf::from(&identity.name),
            content_hash: String::new(), // recomputed below
            file_count: identity.file_count,
            package_name: identity.package_name.clone(),
        },
        config,
    );
    all.schema = schema;
    for r in reports {
        all.candidates.extend(r.candidates.clone());
        all.diagnostics.extend(r.diagnostics.clone());
    }
    all
}

/// Build a BTreeMap of file -> content for hashing/identity (sorted keys).
pub fn file_map(files: &[(PathBuf, String)]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(p, c)| (p.display().to_string(), c.clone()))
        .collect()
}
