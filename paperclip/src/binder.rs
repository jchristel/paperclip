// src/binder.rs

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};

pub fn run() -> Result<()> {
    let current_dir = std::env::current_dir()?;
    println!("Scanning for PDFs in: {}", current_dir.display());

    let pdfs = find_pdfs(&current_dir);

    if pdfs.is_empty() {
        println!("No PDF files found. Nothing to do.");
        return Ok(());
    }

    println!("Found {} PDF(s) — classifying...", pdfs.len());

    // --- Set up the progress bar -----------------------------------------
    // ProgressBar::new takes the total count as u64
    let pb = ProgressBar::new(pdfs.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            // {bar:40} = 40-character wide bar
            // {pos}/{len} = current/total count
            // {msg} = message we set per file
            "[{bar:40}] {pos}/{len} {msg}"
        )
        .unwrap()
        .progress_chars("=> "),  // characters for filled / head / empty
    );

    // --- Classify each PDF -----------------------------------------------
    let mut regular_pdfs: Vec<PathBuf> = Vec::new();
    let mut existing_binders: Vec<(PathBuf, String)> = Vec::new();  // (path, binder_name)
    let mut unreadable: Vec<(PathBuf, String)> = Vec::new();        // (path, reason)

    for path in &pdfs {
        // Show the current filename in the progress bar (truncated to keep it tidy)
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("...");
        pb.set_message(format!("{}", filename));

        let classified = crate::pdf_classifier::classify(path);

        // Match on the kind and sort into the right bucket
        // This is equivalent to a C# switch on a discriminated union
        match classified.kind {
            crate::pdf_classifier::PdfKind::Regular =>{
                regular_pdfs.push(classified.path);
            }
            crate::pdf_classifier::PdfKind::Binder { binder_name } => {
                existing_binders.push((classified.path, binder_name));
            }
            crate::pdf_classifier::PdfKind::Unreadable { reason } => {
                unreadable.push((classified.path, reason));
            }
        }

        pb.inc(1);  // advance the bar by 1
    }

    // finish_and_clear removes the bar from the terminal so our
    // summary output isn't mixed in with the progress display
    pb.finish_and_clear();

    // --- Print summary ---------------------------------------------------
    println!("\nClassification complete:");
    println!("  Regular PDFs  : {}", regular_pdfs.len());
    println!("  Existing binders : {}", existing_binders.len());
    println!("  Unreadable    : {}", unreadable.len());

    if !existing_binders.is_empty() {
        println!("\nExisting binders found (will be skipped):");
        for (path, name) in &existing_binders {
            println!("  [{}]  {}", name, path.display());
        }
    }

    if !unreadable.is_empty() {
        println!("\nUnreadable files (will be skipped):");
        for (path, reason) in &unreadable {
            println!("  {}  — {}", path.file_name().unwrap_or_default().to_str().unwrap_or("?"), reason);
        }
    }

    // Next steps: mapper check, then binding logic
    // --- Mapper check ----------------------------------------------------
    let config = crate::settings::load()?;

    match config.mapper_csv_path {
        None => {
            // No mapper configured at all
            handle_no_mapper(&regular_pdfs)?;
        }
        Some(ref path) => {
            let mapper_path = std::path::Path::new(path);
            if !mapper_path.exists() {
                // Path is configured but file is gone
                println!("\nWarning: Mapper file configured but not found at:");
                println!("  {}", path);
                println!("Run `paperclip config set --mapper-path` to update it.");
                handle_no_mapper(&regular_pdfs)?;
            } else {
                // Mapper exists — read and validate it
                handle_mapper(mapper_path, &regular_pdfs)?;
            }
        }
    }
    Ok(())
}


/// Called when no mapper file is available.
/// Offers the user folder-based binding instead.
fn handle_no_mapper(regular_pdfs: &[PathBuf]) -> Result<()> {
    use std::io::Write;

    if regular_pdfs.is_empty() {
        println!("\nNo regular PDFs to bind.");
        return Ok(());
    }

    println!("\nNo mapper file configured.");
    println!("Folder-based binding will create one binder per folder,");
    println!("containing all PDFs directly inside that folder (not subfolders).");
    print!("\nProceed with folder-based binding? [y/N]: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if input.trim().eq_ignore_ascii_case("y") {
        println!("Folder-based binding — not yet implemented.");
        // TODO: next step
    } else {
        println!("Binding cancelled.");
    }

    Ok(())
}

/// Called when a mapper file is present.
/// Reads and validates the CSV.
fn handle_mapper(mapper_path: &std::path::Path, _regular_pdfs: &[PathBuf]) -> Result<()> {
    println!("\nMapper file found: {}", mapper_path.display());

    let rows = crate::mapper::load(mapper_path)?;
    println!("Loaded {} mapper row(s).", rows.len());

    // TODO: match PDFs against mapper rows and build binders

    Ok(())
}

/// Recursively finds all PDF files under the given folder.
/// Returns a sorted list of absolute paths.
fn find_pdfs(root: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    collect_pdfs(root, &mut results);
    results.sort();
    results
}

/// Recursive helper — walks the directory tree and collects .pdf paths.
/// This is the equivalent of Directory.GetFiles("*.pdf", SearchOption.AllDirectories) in C#.
fn collect_pdfs(dir: &Path, results: &mut Vec<PathBuf>) {
    // read_dir returns an iterator over directory entries
    // we silently skip folders we can't read rather than crashing
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {  // .flatten() silently skips any entries that errored
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subfolders
            collect_pdfs(&path, results);
        } else if path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))  // case-insensitive: .PDF .pdf .Pdf all match
            .unwrap_or(false)
        {
            results.push(path);
        }
    }
}