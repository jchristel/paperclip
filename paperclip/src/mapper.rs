// src/mapper.rs
// Reads and validates the mapper CSV file.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;

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