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

use regex::Regex;

use std::sync::LazyLock;

static DEFAULT_CODE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+-[A-Za-z0-9]+)").unwrap()
});

static REVISION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\(|\[)([A-Za-z0-9]+)(?:\)|\])").unwrap()
});


// --- Lenient (best-effort) parse -----------------------------------------

/// The best-effort result of parsing a filename.
///
/// Every field here is optional, because this parse
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
/// This returns a `LenientParse` that always
/// succeeds (it's a reporter). Missing pieces come back as `None`, and the
/// reason(s) they're missing are collected into `flag_reason`.
///
/// Note there is no `Result` here — the return type is the struct directly,
/// because this function genuinely cannot fail.
pub fn parse_lenient(filename_stem: &str, code_pattern: Option<&Regex>) -> LenientParse {

    // We collect problems as we go, then join them into one message at the end.
    // A Vec<&str> is like a List<string> in C#.
    let mut problems: Vec<&str> = Vec::new();

    // --- Step 1: try the code block --------------------------------------
    // Use the project's configured pattern if supplied; otherwise fall back to
    // the default five-part dash-separated code (preserves old behaviour when
    // no paperclip.toml pattern is set).
    let code_re: &Regex = code_pattern.unwrap_or(&DEFAULT_CODE_RE);

    let code = match code_re.find(filename_stem) {
        Some(m) => Some(m.as_str().to_string()),
        None => {
            problems.push("code does not match document-number pattern"); 
            None
        }
    };

    // --- Step 2: try the revision ----------------------------------------
    let revision = match REVISION_RE.captures(filename_stem) {
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
    // Some problems -> join them: "code does not match document-number pattern; missing revision".
    let flag_reason = if problems.is_empty() {
        None
    } else {
        Some(problems.join("; "))
    };

    LenientParse { code, name, revision, flag_reason }
}

// --- Doc-number mask compiler --------------------------------------------
//
// Users describe their document-number shape with a simple mask instead of a
// raw regex. Vocabulary:
//   A = a letter        -> [A-Za-z]
//   N = a digit         -> [0-9]
//   X = a letter OR digit -> [A-Za-z0-9]
//   anything else       -> that literal character, regex-escaped
//
// Each placeholder matches EXACTLY ONE character, so "AAA" = three letters.
// The compiled pattern is anchored at the start (^) and wrapped in a capture
// group, mirroring the hard-coded code regex it replaces. It is NOT anchored
// at the end, because a filename continues past the code block (name,
// revision, extension) — same as the original `^(...)` design.
//
// Example: "AAA-AAA-AA-AAA-NNNNNNN"
//   -> ^([A-Za-z][A-Za-z][A-Za-z]-...-[0-9][0-9][0-9][0-9][0-9][0-9][0-9])
//   which matches "RHH-HDR-AR-DRG-A130001".

/// Compiles a doc-number mask into an anchored, capturing `Regex`.
///
/// Returns `Err(String)` with a human-readable message if the mask is empty or
/// the generated pattern somehow fails to compile (shouldn't happen for valid
/// vocabulary, but we surface it rather than panic). Compile this ONCE when
/// config loads, not per filename.
pub fn compile_doc_number_mask(mask: &str) -> Result<Regex, String> {
    if mask.is_empty() {
        return Err("document-number pattern is empty".to_string());
    }

    // Build the inner pattern character by character. We start with the
    // capture-group open paren and the start anchor before it.
    let mut pattern = String::from("^(");

    for ch in mask.chars() {
        match ch {
            'A' => pattern.push_str("[A-Za-z]"),
            'N' => pattern.push_str("[0-9]"),
            'X' => pattern.push_str("[A-Za-z0-9]"),
            // Any other character is a literal. `regex::escape` turns it into
            // a form the engine treats verbatim — so '.', '(', '+', etc. match
            // themselves instead of acting as metacharacters. For a plain '-'
            // this is a harmless no-op.
            other => pattern.push_str(&regex::escape(&other.to_string())),
        }
    }

    pattern.push(')'); // close the capture group

    // Compile. The regex crate has no catastrophic backtracking, so even a
    // pathological mask can't hang us; the only realistic failure is a literal
    // we somehow didn't escape, which `regex::escape` prevents.
    Regex::new(&pattern).map_err(|e| {
        format!("could not compile document-number pattern '{}': {}", mask, e)
    })
}