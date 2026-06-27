// src/project_config.rs
//
// Project-local configuration: an optional `paperclip.toml` that sits in the
// folder being worked on and overrides the global config for the few things
// that vary per project.
//
// Layering (lowest priority first):
//   1. Global config  (%APPDATA%\paper_clip\config.toml — settings.rs)
//        credentials, username, and fallback values.
//   2. Project-local  (paperclip.toml in the scan directory — THIS module)
//        mapper path, doc-number pattern, project name.
// Local overrides global field-by-field; anything the local file omits falls
// back to the global value. The merged view is `ResolvedConfig`.
//
// Credentials (password, app key) are NEVER part of the project file — they
// stay global, in Credential Manager. A project file that might be copied or
// committed must not carry secrets.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The filename we look for in the working/scan directory.
const PROJECT_CONFIG_FILE: &str = "paperclip.toml";

// --- The on-disk project file --------------------------------------------

/// The shape of `paperclip.toml` as it sits on disk. Every field is optional:
/// a project file may set only the bits it cares about, and the rest fall back
/// to global. Only Deserialize is derived — we never write this file from code
/// (the user authors it, or `config set --local` will later).
///
/// `mapper` is stored as the user typed it (typically RELATIVE). It is resolved
/// against the file's own directory in `resolve_config`, so a relative path
/// means "next to this paperclip.toml" regardless of where paperclip was
/// invoked from.
#[derive(Debug, Deserialize, Default)]
pub struct ProjectConfig {
    /// Path to the mapper CSV, relative to this file's directory (or absolute).
    pub mapper: Option<String>,

    /// Document-number mask (e.g. "AAA-AAA-AA-AAA-NNNNNNN"). The raw string is
    /// stored here; it's compiled to a Regex on demand via
    /// `filename_parser::compile_doc_number_mask`, so a bad mask surfaces its
    /// error at point of use rather than failing the whole load.
    pub doc_number_pattern: Option<String>,

    /// The Aconex project short name to operate on. Overrides the global
    /// `project_name` when present — this is what lets `cd`-ing between project
    /// folders switch projects without `config set --project`.
    pub project_name: Option<String>,
}

// --- The merged, ready-to-use view ---------------------------------------

/// The effective configuration after layering local over global. This is what
/// the command code consumes; it doesn't need to know which file a value came
/// from.
///
/// Fields are still `Option` because a value may be set in neither file — e.g.
/// no mapper configured anywhere. Callers handle "missing" the same way they
/// did when reading global `Settings` directly.
#[derive(Debug, Default)]
pub struct ResolvedConfig {
    /// Mapper CSV path, already resolved to an absolute/usable path if it came
    /// from the project file (relative-to-the-toml). If it came from global,
    /// it's used as-is (global stores absolute paths today).
    pub mapper_csv_path: Option<PathBuf>,

    /// Raw doc-number mask string, if any. Compile with
    /// `filename_parser::compile_doc_number_mask` where a Regex is needed.
    pub doc_number_pattern: Option<String>,

    /// Effective Aconex project short name.
    pub project_name: Option<String>,

    /// Aconex username — global only (credentials never go in the project
    /// file). Carried here so callers have one config object to read.
    pub username: Option<String>
}

// --- Loading -------------------------------------------------------------

/// Reads `paperclip.toml` from `dir`, if present. Returns `Ok(None)` when the
/// file doesn't exist — a project file is optional, so absence is normal, not
/// an error. A present-but-malformed file IS an error (we don't silently
/// ignore a typo'd config).
///
/// `dir` is the directory to look in — the caller passes the scan directory
/// (the folder being worked on), so the project file travels with its PDFs.
pub fn load_project_config(dir: &Path) -> Result<Option<ProjectConfig>> {
    let path = dir.join(PROJECT_CONFIG_FILE);

    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let cfg: ProjectConfig = toml::from_str(&content)
        .with_context(|| format!("{} is not valid TOML", path.display()))?;

    Ok(Some(cfg))
}

/// Produces the effective config for a run in `dir`, layering an optional
/// `paperclip.toml` (in `dir`) over the global `Settings`.
///
/// Resolution rules:
///   * project_name: local wins, else global.
///   * doc_number_pattern: local only (global has no such field).
///   * mapper: local wins (resolved RELATIVE TO `dir`), else global's
///     `mapper_csv_path` (used as-is — global stores absolute paths).
///   * username / user_id: global only (never in the project file).
pub fn resolve_config(dir: &Path) -> Result<ResolvedConfig> {
    // Global config first — the base layer.
    let global = crate::settings::load()?;

    // Optional project file on top.
    let local = load_project_config(dir)?;

    // --- mapper path -----------------------------------------------------
    // If the project file gives a mapper, resolve it against `dir` (the toml's
    // directory). `dir.join(rel)` leaves absolute paths untouched and prefixes
    // relative ones — exactly the "relative means next to the toml" rule.
    let mapper_csv_path = match local.as_ref().and_then(|l| l.mapper.as_ref()) {
        Some(rel) => Some(dir.join(rel)),
        None => global.mapper_csv_path.as_ref().map(PathBuf::from),
    };

    // --- doc-number pattern ---------------------------------------------
    // Local only; global has no equivalent. `.clone()` copies the String out
    // of the borrowed Option so we own it in the result.
    let doc_number_pattern = local
        .as_ref()
        .and_then(|l| l.doc_number_pattern.clone());

    // --- project name ----------------------------------------------------
    // Local wins, else global. `.or(...)` keeps the first Some.
    let project_name = local
        .as_ref()
        .and_then(|l| l.project_name.clone())
        .or(global.project_name.clone());

    Ok(ResolvedConfig {
        mapper_csv_path,
        doc_number_pattern,
        project_name,
        username: global.username.clone(),
    })
}