// src/mapper.rs
// Reads and validates the mapper CSV file.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// --- Struct --------------------------------------------------------------

/// One row from the mapper CSV.
#[derive(Debug)]
pub struct MapperRow {
    pub prefix: String,
    pub binder_name: String,
    pub output_folder: String,
}

// --- Public function -----------------------------------------------------

/// Reads the mapper CSV and returns a validated list of rows.
/// Errors if:
///   - The file cannot be read
///   - A required column is missing
///   - The same binder_name maps to two different output_folders
pub fn load(path: &Path) -> Result<Vec<MapperRow>> {
    let content = std::fs::read_to_string(path)
        .context("Failed to read mapper CSV")?;

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(content.as_bytes());

    let mut rows: Vec<MapperRow> = Vec::new();

    // binder_name -> output_folder seen so far — used to detect conflicts
    // HashMap is Rust's equivalent of Dictionary<K,V> in C#
    let mut binder_output_map: HashMap<String, String> = HashMap::new();

    let headers = reader.headers()?.clone();  // clone once before the loop

    for (i, result) in reader.records().enumerate() {
        let record = result
            .with_context(|| format!("Failed to parse row {}", i + 2))?;

        let prefix        = get_field(&record, &headers, "prefix",        i + 2)?;
        let binder_name   = get_field(&record, &headers, "binder_name",   i + 2)?;
        let output_folder = get_field(&record, &headers, "output_folder", i + 2)?;

        // Check for conflicting output folders for the same binder name
        if let Some(existing) = binder_output_map.get(&binder_name) {
            if existing != &output_folder {
                bail!(
                    "Mapper error: binder '{}' maps to two different output folders:\n  {}\n  {}",
                    binder_name, existing, output_folder
                );
            }
        } else {
            binder_output_map.insert(binder_name.clone(), output_folder.clone());
        }

        rows.push(MapperRow { prefix, binder_name, output_folder });
    }

    if rows.is_empty() {
        bail!("Mapper CSV is empty — no rows found.");
    }

    Ok(rows)
}

// --- Matching ------------------------------------------------------------

/// Matches a list of PDF paths against mapper rows.
/// Returns:
///   - A map of binder_name -> list of matching PDF paths
///   - A list of PDFs that matched no rows (unmatched)
pub fn match_pdfs<'a>(
    pdfs: &'a [PathBuf],
    rows: &'a [MapperRow],
) -> (HashMap<String, Vec<&'a PathBuf>>, Vec<&'a PathBuf>) {
    // HashMap<binder_name, Vec<pdf_path>>
    // like Dictionary<string, List<string>> in C#
    let mut binder_map: HashMap<String, Vec<&PathBuf>> = HashMap::new();
    let mut unmatched: Vec<&PathBuf> = Vec::new();

    for pdf in pdfs {
        // Get just the filename without the folder path
        // e.g. "C:\docs\20251123_drawing.pdf" -> "20251123_drawing.pdf"
        let filename = match pdf.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => {
                unmatched.push(pdf);
                continue;  // like `continue` in C# — skip to next iteration
            }
        };

        let mut matched_any = false;

        for row in rows {
            // starts_with is case-sensitive — adjust if needed later
            if filename.starts_with(&row.prefix) {
                binder_map
                    .entry(row.binder_name.clone())  // get or create the Vec for this binder
                    .or_insert_with(Vec::new)         // like GetOrAdd in C# ConcurrentDictionary
                    .push(pdf);
                matched_any = true;
            }
        }

        if !matched_any {
            unmatched.push(pdf);
        }
    }

    (binder_map, unmatched)
}

/// Checks that all output folders referenced in the mapper rows exist on disk.
/// Returns a list of missing folders — empty means all good.
/// Called before assembly begins so we fail early rather than mid-run.
pub fn validate_output_folders(rows: &[MapperRow]) -> Vec<String> {
    use std::collections::HashSet;

    // Deduplicate first — multiple rows may share the same output folder
    // HashSet is like HashSet<T> in C#
    let unique_folders: HashSet<&String> = rows.iter()
        .map(|r| &r.output_folder)
        .collect();

    unique_folders
        .into_iter()
        .filter(|folder| !std::path::Path::new(folder).exists())
        .map(|folder| folder.clone())
        .collect()
}

// --- Helper --------------------------------------------------------------

/// Extracts a field by column name from a CSV record.
/// Returns a clear error message if the column doesn't exist.
fn get_field(
    record: &csv::StringRecord,
    headers: &csv::StringRecord,
    column: &str,
    row_num: usize,
) -> Result<String> {
    // Find the column index by name
    let index = headers
        .iter()
        .position(|h| h == column)
        .with_context(|| format!("Column '{}' not found in mapper CSV header", column))?;

    let value = record
        .get(index)
        .with_context(|| format!("Row {}: missing value for column '{}'", row_num, column))?;

    Ok(value.trim().to_string())
}

/// Validates a PDF filename against a project-specific pattern.
///
/// TODO: Filename validation is not yet implemented.
/// When implemented, this should support:
///   - Multiple configurable patterns (e.g. 5-part code, free-form, none)
///   - Pattern defined per binder, not per row
///   - Skipping validation entirely when no pattern is specified
///   - Logging skipped files to the CSV log with reason "invalid_filename_format"
///
/// For now all filenames are considered valid.
pub fn validate_filename(_filename: &str, _pattern: Option<&str>) -> bool {
    true
}