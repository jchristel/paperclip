// src/manifest.rs
// The binder manifest data model and its JSON serialisation.
//
// This module is pure data: no PDF handling, no file I/O. It defines the
// structs that describe a binder's contents and converts them to/from JSON.
// The XMP module (xmp.rs) takes the JSON produced here, wraps and compresses
// it, and attaches it to the PDF. Keeping the two concerns in separate
// modules means this part is trivial to unit-test on its own.
//
// The `#[derive(Serialize, Deserialize)]` attributes are serde's equivalent
// of C#'s [JsonSerializable] — they auto-generate the to-JSON / from-JSON
// code at compile time. We need Deserialize as well as Serialize because the
// rename-detection feature reads the manifest back out of an existing binder.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// --- Schema version ------------------------------------------------------

/// Bump this whenever the manifest shape changes in a way that older readers
/// couldn't handle. Stored in the manifest so a future paperclip can detect
/// "this binder was written by a newer/older schema than I understand".
pub const SCHEMA_VERSION: u32 = 1;

/// The tool marker string. Also written into the PDF Info dict separately as
/// a fast "is this a binder?" check (see assembler.rs / pdf_classifier.rs).
pub const TOOL_MARKER: &str = "paperclip/1";

// --- One file inside a binder --------------------------------------------

/// Describes a single source PDF as it sits inside the assembled binder.
///
/// `revision`, `name`, and `flag_reason` are all `Option<String>` — the Rust
/// equivalent of C#'s `string?`. `None` serialises to absent (we skip nulls,
/// see the serde attribute) so a clean entry stays compact.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    /// The source file's name, including extension, as it was on disk.
    pub filename: String,

    /// The 5-part code block from the filename, or None if it was missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Revision extracted from the filename brackets, or None if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,

    /// Human-readable name portion from the filename, or None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 1-based page number where this file's content starts in the binder.
    pub start_page: u32,

    /// 1-based page number where this file's content ends (inclusive).
    pub end_page: u32,

    /// UTC timestamp recording when this entry was added to the binder.
    pub added_utc: String,

    /// None  = filename validated cleanly.
    /// Some(reason) = the file was kept but flagged; reason is the same
    /// string written to the CSV skip log. Durable record of WHY it was odd.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flag_reason: Option<String>,
}

// --- A mapper row, captured into the manifest ----------------------------

/// A snapshot of the mapper CSV row(s) that drove this binder. Stored so the
/// binder is self-describing — you can see how it was built without the
/// original CSV. Mirrors crate::mapper::MapperRow but lives here so the
/// manifest module has no dependency on the mapper module's internals.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapperRow {
    pub prefix: String,
    pub binder_name: String,
    pub output_folder: String,
}

// --- The whole manifest --------------------------------------------------

/// The complete description of one assembled binder. This is what gets
/// serialised to JSON, wrapped as XMP, compressed, and attached to the PDF.
#[derive(Debug, Serialize, Deserialize)]
pub struct BinderManifest {
    /// Tool marker, e.g. "paperclip/1".
    pub tool: String,

    /// Schema version (see SCHEMA_VERSION).
    pub schema_version: u32,

    /// Stable UUID for this binder. Survives the file being renamed on disk,
    /// which is what makes rename detection possible.
    pub binder_id: String,

    /// The binder's logical name (from the mapper). May differ from the
    /// on-disk filename if the file was later renamed.
    pub binder_name: String,

    /// UTC timestamp of when the binder was created.
    pub created_utc: String,

    /// The mapper rows that produced this binder.
    pub mapper_rows: Vec<MapperRow>,

    /// One entry per source file, in binder page order.
    pub files: Vec<FileEntry>,
}

impl BinderManifest {
    /// Creates a new manifest with a fresh random UUID and current timestamp.
    /// `files` is built up by the assembler as it merges pages.
    pub fn new(
        binder_name: &str,
        mapper_rows: Vec<MapperRow>,
        files: Vec<FileEntry>,
    ) -> Self {
        BinderManifest {
            tool: TOOL_MARKER.to_string(),
            schema_version: SCHEMA_VERSION,
            // uuid::Uuid::new_v4() makes a random v4 UUID; .to_string() gives
            // the familiar hyphenated form, e.g. "abc12345-...".
            binder_id: uuid::Uuid::new_v4().to_string(),
            binder_name: binder_name.to_string(),
            created_utc: chrono::Utc::now().to_rfc3339(),
            mapper_rows,
            files,
        }
    }

    /// Serialises the manifest to a pretty JSON string.
    /// `serde_json::to_string_pretty` is like JsonSerializer.Serialize with
    /// indentation turned on.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .context("Failed to serialise binder manifest to JSON")
    }

    /// Parses a manifest back from a JSON string. Used by rename detection
    /// when reading an existing binder.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .context("Failed to parse binder manifest from JSON")
    }
}
