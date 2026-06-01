// src/pdf_classifier.rs
// Opens each PDF and determines whether it is a paperclip binder or a regular PDF.

use lopdf::Document;
use std::path::PathBuf;

// The marker key we write into the Info dictionary when we create a binder.
// Its presence is the "is this a binder?" test.
const BINDER_TOOL_KEY: &[u8] = b"BinderTool";

// Files larger than this are skipped without being parsed.
// Document::load reads and parses the ENTIRE file into memory, so a 1GB+
// PDF spikes CPU and RAM until the machine becomes unresponsive. We guard
// against that *before* loading.
//
// `u64` because file sizes can exceed what a 32-bit int holds.
// 500 * 1024 * 1024 = 500 MiB. Tune this to taste — it's just a ceiling.
// The `_` separators are like digit grouping; they're ignored by the compiler.
const MAX_PDF_SIZE_BYTES: u64 = 500 * 1024 * 1024;

/// Returns the file size in bytes, or None if it can't be read.
///
/// Rust separates "metadata" (size, timestamps) from opening the file's
/// contents — like `new FileInfo(path).Length` in C# or `os.path.getsize`
/// in Python. Reading metadata is cheap: it does NOT load the file.
///
/// `std::fs::metadata` returns Result<Metadata>. We use `.ok()` to turn
/// Result<T, E> into Option<T> (discarding the error), then `.map` to pull
/// out just the length. If anything fails we get None.
fn file_size_bytes(path: &PathBuf) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

// --- Result type ---------------------------------------------------------

// This enum represents the outcome of classifying a single PDF.
// In C# this would be a discriminated union or two subclasses of a base type.
#[derive(Debug)]
pub enum PdfKind {
    /// A regular PDF — not made by paperclip
    Regular,

    /// A binder made by paperclip — contains our marker key
    Binder {
        /// The binder name as stored in metadata (may differ from filename if renamed)
        binder_name: String,
    },

    /// File could not be opened or parsed — corrupt or password protected
    Unreadable {
        reason: String,
    },

    /// File exceeds the size limit and was skipped without being parsed.
    /// We carry the actual size so the log/summary can report it.
    TooLarge {
        size_bytes: u64,
    },
}

// Bundles the path and its classification together.
// This is what the caller gets back for each PDF.
#[derive(Debug)]
pub struct ClassifiedPdf {
    pub path: PathBuf,
    pub kind: PdfKind,
}

// --- Public function -----------------------------------------------------

/// Classifies a single PDF as Regular, Binder, or Unreadable.
/// Called once per file inside the progress bar loop in binder.rs.
pub fn classify(path: &PathBuf) -> ClassifiedPdf {
    // --- Size guard: bail out BEFORE loading -----------------------------
    // This must come first. Document::load below would read and parse the
    // whole file; for a 1GB+ PDF that is what freezes the machine.
    //
    // `if let Some(size) = ...` is Rust's way of saying "if this Option
    // has a value, bind it to `size` and run this block." Like a null check
    // combined with an assignment in one step.
    if let Some(size) = file_size_bytes(path) {
        if size > MAX_PDF_SIZE_BYTES {
            return ClassifiedPdf {
                path: path.clone(),
                kind: PdfKind::TooLarge { size_bytes: size },
            };
        }
    }
    // If we couldn't read the size at all (None), we fall through and let
    // Document::load try — it'll surface a real error as Unreadable.

    // lopdf::Document::load opens and parses the PDF.
    // We use match instead of ? because we want to return Unreadable
    // rather than propagate an error — the scan should continue even
    // if one file is corrupt.
    let doc = match Document::load(path) {
        Ok(d) => d,
        Err(e) => {
            return ClassifiedPdf {
                path: path.clone(),
                kind: PdfKind::Unreadable {
                    reason: e.to_string(),
                },
            };
        }
    };

    // Try to read the Info dictionary from the PDF trailer.
    // The trailer is the PDF's table of contents — it points to the Info dict.
    let info_dict = doc
        .trailer
        .get(b"Info")                          // get the Info reference from the trailer
        .ok()
        .and_then(|obj| obj.as_reference().ok())  // dereference it to get the object ID
        .and_then(|id| doc.get_object(id).ok())   // look up the actual object by ID
        .and_then(|obj| obj.as_dict().ok());       // cast it to a dictionary

    // If there's no Info dict at all, it's definitely a regular PDF
    let dict = match info_dict {
        Some(d) => d,
        None => {
            return ClassifiedPdf {
                path: path.clone(),
                kind: PdfKind::Regular,
            };
        }
    };

    // Check for our marker key — its presence means this is a paperclip binder
    if dict.get(BINDER_TOOL_KEY).is_err() {
        return ClassifiedPdf {
            path: path.clone(),
            kind: PdfKind::Regular,
        };
    }

    // Try to read the binder name from metadata.
    // If it's missing or unreadable, fall back to the filename.
    let binder_name = dict
        .get(b"BinderName")
        .ok()
        .and_then(|obj| obj.as_str().ok())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // Fallback: use the filename without extension
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

    ClassifiedPdf {
        path: path.clone(),
        kind: PdfKind::Binder { binder_name },
    }
}