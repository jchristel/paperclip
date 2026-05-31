// src/pdf_classifier.rs
// Opens each PDF and determines whether it is a paperclip binder or a regular PDF.

use lopdf::Document;
use std::path::PathBuf;

// The marker key we write into the Info dictionary when we create a binder.
// Its presence is the "is this a binder?" test.
const BINDER_TOOL_KEY: &[u8] = b"BinderTool";

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