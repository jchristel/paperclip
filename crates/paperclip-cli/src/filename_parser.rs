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

// --- Lenient (best-effort) parse -----------------------------------------

/// The best-effort result of parsing a filename.
///
/// Unlike `ParsedFilename`, every field here is optional, because this parse
/// never fails — it extracts whatever it can and reports problems instead of
/// rejecting the file. This is what the binder uses now that files with bad
/// names are KEPT and flagged rather than skipped.
///
/// Think of `Option<String>` as C#'s `string?`: `Some(x)` is a value,
/// `None` is "not present".
#[derive(Debug)]
pub struct LenientParse {
    /// The 5-part code block, if it was found at the start. None if missing.
    pub code: Option<String>,
    /// The human-readable name portion, if any.
    pub name: Option<String>,
    /// The revision from brackets, if found. None if missing.
    pub revision: Option<String>,
    /// None  = filename fully valid (nothing to flag).
    /// Some(msg) = what was wrong. This string is written to both the CSV log
    /// and the XMP manifest, so it should read as a short human explanation.
    pub flag_reason: Option<String>,
}

/// Parses a filename stem and extracts whatever it can WITHOUT failing.
///
/// This is the counterpart to `parse`. Where `parse` returns `Err` on the
/// first problem (it's a gate), this returns a `LenientParse` that always
/// succeeds (it's a reporter). Missing pieces come back as `None`, and the
/// reason(s) they're missing are collected into `flag_reason`.
///
/// Note there is no `Result` here — the return type is the struct directly,
/// because this function genuinely cannot fail.
pub fn parse_lenient(filename_stem: &str) -> LenientParse {
    // Same two patterns as `parse`. Compiling them here keeps this function
    // self-contained; if this ever runs in a hot loop we'd lift them out.
    let code_re = Regex::new(r"^([A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+)")
        .unwrap();
    let revision_re = Regex::new(r"(?:\(|\[)([A-Za-z0-9]+)(?:\)|\])").unwrap();

    // We collect problems as we go, then join them into one message at the end.
    // A Vec<&str> is like a List<string> in C#.
    let mut problems: Vec<&str> = Vec::new();

    // --- Step 1: try the 5-part code -------------------------------------
    let code = match code_re.find(filename_stem) {
        Some(m) => Some(m.as_str().to_string()),
        None => {
            problems.push("missing 5-part code");
            None
        }
    };

    // --- Step 2: try the revision ----------------------------------------
    let revision = match revision_re.captures(filename_stem) {
        Some(caps) => Some(caps[1].to_string()),
        None => {
            problems.push("missing revision");
            None
        }
    };

    // --- Step 3: try the optional name -----------------------------------
    // The name lives between the code and the first bracket. If we have no
    // code, there's nothing reliable to anchor on, so we treat the whole
    // string up to the first bracket as the candidate region.
    //
    // `code.as_deref()` turns Option<String> into Option<&str> so we can
    // measure its length without moving it out of `code`.
    let after_code = match code.as_deref() {
        Some(c) => &filename_stem[c.len()..],
        None => filename_stem,
    };

    let bracket_pos = after_code.find(|c| c == '(' || c == '[');
    let name = if let Some(pos) = bracket_pos {
        let raw = after_code[..pos].trim_matches(|c: char| {
            c == ' ' || c == '-' || c == '_'
        });
        if raw.is_empty() { None } else { Some(raw.to_string()) }
    } else {
        None
    };

    // --- Build the flag reason -------------------------------------------
    // No problems  -> None (file is clean).
    // Some problems -> join them: "missing 5-part code; missing revision".
    let flag_reason = if problems.is_empty() {
        None
    } else {
        Some(problems.join("; "))
    };

    LenientParse { code, name, revision, flag_reason }
}