// src/validator.rs
// Validation helpers for CLI input arguments.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use colored::Colorize;

/// Validates the mapper CSV path and optionally creates the file.
///
/// Returns:
///   Ok(true)  — path is valid and file exists (or was just created)
///   Ok(false) — folder invalid, or user declined to create the file
///   Err       — unexpected IO failure
pub fn resolve_mapper_path(path: &str) -> Result<bool> {
    let p = Path::new(path);

    // --- Check the folder exists -----------------------------------------
    // parent() gives us the folder portion of the path
    // e.g. "C:\data\mapper.csv" -> "C:\data"
    let folder = p.parent().filter(|f| !f.as_os_str().is_empty());

    if let Some(folder) = folder {
        if !folder.exists() {
            println!("{}", format!("ERROR: Folder does not exist: {}", folder.display()).red());
            return Ok(false);
        }
    }

    // --- Check if the file itself exists ---------------------------------
    if p.exists() {
        return Ok(true);
    }

    // --- File doesn't exist — offer to create it -------------------------
    print!("File does not exist. Create an empty file at {}? [y/N]: ", path);

    // flush ensures the prompt appears before we wait for input
    // like Console.Out.Flush() in C#
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if input.trim().eq_ignore_ascii_case("y") {
        std::fs::File::create(p)
            .context("Failed to create mapper CSV file")?;
        println!("Created empty file: {}", path);
        Ok(true)
    } else {
        println!("File not created. Mapper path not saved.");
        Ok(false)
    }
}