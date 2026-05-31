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
fn handle_mapper(mapper_path: &std::path::Path, regular_pdfs: &[PathBuf]) -> Result<()> {
    println!("\nMapper file found: {}", mapper_path.display());

    let rows = crate::mapper::load(mapper_path)?;
    println!("Loaded {} mapper row(s).", rows.len());

    // --- Validate output folders before doing any work -------------------
    let missing_folders = crate::mapper::validate_output_folders(&rows);
    if !missing_folders.is_empty() {
        println!("\nError: The following output folders do not exist:");
        for folder in &missing_folders {
            println!("  {}", folder);
        }
        println!("Please create them or update the mapper CSV.");
        return Ok(());
    }

    // --- Create the run log ----------------------------------------------
    // Log goes to the current directory (where the user called paperclip from)
    let current_dir = std::env::current_dir()?;
    let mut run_log = crate::log::RunLog::new(&current_dir);

    // --- Match PDFs against mapper rows ----------------------------------
    let (binder_map, unmatched) = crate::mapper::match_pdfs(regular_pdfs, &rows);

    // Record unmatched files in the log
    for pdf in &unmatched {
        let filename = pdf.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        run_log.skip(filename, crate::log::SkipReason::NoMapperMatch);
    }

    // --- Parse filenames and report invalid ones -------------------------
    println!("\nValidating filenames...");
    // Collect invalid paths into a HashSet for fast lookup during filtering
    let mut invalid_paths: std::collections::HashSet<&PathBuf> = std::collections::HashSet::new();

    for pdf in regular_pdfs {
        let stem = pdf.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        match crate::filename_parser::parse(stem) {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                let filename = pdf.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                // Pick the right skip reason based on the error message
                let reason = if msg.contains("5-part") {
                    crate::log::SkipReason::InvalidFilenameFormat
                } else {
                    crate::log::SkipReason::MissingRevision
                };

                println!("  Skipping: {} — {}", stem, msg);
                run_log.skip(filename, reason);
                invalid_paths.insert(pdf);
            }
        }
    }

    if invalid_paths.is_empty() {
        println!("  All filenames valid.");
    }

    // --- Filter binder_map to remove invalid files -----------------------
    // Retain only files that are NOT in the invalid set
    let filtered_binder_map: std::collections::HashMap<&String, Vec<&&PathBuf>> = binder_map
        .iter()
        .map(|(binder_name, files)| {
            let valid_files: Vec<&&PathBuf> = files
                .iter()
                .filter(|f| !invalid_paths.contains(*f))
                .collect();
            (binder_name, valid_files)
        })
        .filter(|(_, files)| !files.is_empty())  // drop binders with no valid files left
        .collect();

    // --- Print binder plan -----------------------------------------------
    println!("\nBinders to assemble:");
    if filtered_binder_map.is_empty() {
        println!("  (none — no valid PDFs matched any mapper row)");
    } else {
        let mut binder_names: Vec<&&String> = filtered_binder_map.keys().collect();
        binder_names.sort();

        for name in binder_names {
            let files = &filtered_binder_map[name];
            println!("  {} ({} file(s)):", name, files.len());
            for f in files {
                println!("    {}", f.file_name().unwrap_or_default().to_str().unwrap_or("?"));
            }
        }
    }

    if !unmatched.is_empty() {
        println!("\nUnmatched PDFs (no mapper row covers these):");
        for f in &unmatched {
            println!("  {}", f.file_name().unwrap_or_default().to_str().unwrap_or("?"));
        }
    }

    // --- Assemble binders ------------------------------------------------
    if !filtered_binder_map.is_empty() {
        crate::assembler::assemble_all(&filtered_binder_map, &rows, &mut run_log)?;
    }

    // --- Write log -------------------------------------------------------
    run_log.write()?;

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