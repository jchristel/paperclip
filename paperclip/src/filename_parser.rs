// src/filename_parser.rs
// Parses and validates PDF filenames against the project naming convention.
//
// Expected structure:
//   [P1]-[P2]-[P3]-[P4]-[P5][separator?][optional name][revision in brackets].pdf
//
// Examples:
//   XXX-XXX-XX-XXX-1234567 Drawing Title (B).pdf
//   XXX-XXX-XX-XXX-1234567-Drawing-Title[Rev2].pdf
//   XXX-XXX-XX-XXX-1234567(A).pdf

use anyhow::{bail, Result};
use regex::Regex;

// --- Result type ---------------------------------------------------------

/// The parsed components of a valid filename.
#[derive(Debug)]
pub struct ParsedFilename {
    /// The five-part code block e.g. "XXX-XXX-XX-XXX-1234567"
    pub code: String,
    /// The optional human-readable name portion e.g. "Drawing Title"
    /// None if no name is present between the code and the revision
    pub name: Option<String>,
    /// The revision value extracted from brackets e.g. "B", "Rev2", "1"
    pub revision: String,
}

// --- Public function -----------------------------------------------------

/// Parses a filename stem (without extension) and extracts its components.
/// Returns Err if the filename does not meet the naming convention.
///
/// TODO: Make the 5-part code requirement configurable per project
/// (see validate_filename stub in mapper.rs)
pub fn parse(filename_stem: &str) -> Result<ParsedFilename> {
    // --- Step 1: validate the 5-part code block at the start -------------
    // Exactly five alphanumeric segments separated by dashes
    // ^ = start of string
    // [A-Za-z0-9]+ = one or more alphanumeric characters
    // (?:-[A-Za-z0-9]+){4} = exactly four more dash-separated segments
    let code_re = Regex::new(r"^([A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+)")
        .unwrap();  // unwrap is safe here — the pattern is a compile-time constant

    let code = match code_re.find(filename_stem) {
        Some(m) => m.as_str().to_string(),
        None => bail!(
            "Filename does not start with a 5-part code block: '{}'",
            filename_stem
        ),
    };

    // --- Step 2: extract the revision from brackets ----------------------
    // Matches either (value) or [value] — but not both styles in one filename
    // The revision is always the sole bracketed expression
    // Matches either (value) or [value]
    // \( and \) = literal parentheses
    // \[ and \] = literal square brackets
    let revision_re = Regex::new(r"(?:\(|\[)([A-Za-z0-9]+)(?:\)|\])").unwrap();

    let revision = match revision_re.captures(filename_stem) {
        Some(caps) => caps[1].to_string(),
        None => bail!(
            "No revision found in brackets in filename: '{}'",
            filename_stem
        ),
    };

    // --- Step 3: extract the optional name portion -----------------------
    // Everything between the end of the code block and the opening bracket
    // e.g. "XXX-XXX-XX-XXX-1234567 Drawing Title (B)" -> "Drawing Title"
    let after_code = &filename_stem[code.len()..];

    // Find where the bracket starts
    let bracket_pos = after_code.find(|c| c == '(' || c == '[');

    let name = if let Some(pos) = bracket_pos {
        let raw = after_code[..pos].trim_matches(|c: char| {
            c == ' ' || c == '-' || c == '_'  // strip separators
        });
        if raw.is_empty() {
            None
        } else {
            Some(raw.to_string())
        }
    } else {
        None
    };

    Ok(ParsedFilename { code, name, revision })
}