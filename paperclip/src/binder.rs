// src/binder.rs

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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
    let mut too_large: Vec<(PathBuf, u64)> = Vec::new();            // (path, size_bytes)

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
            crate::pdf_classifier::PdfKind::TooLarge { size_bytes } => {
                too_large.push((classified.path, size_bytes));
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
    println!("  Too large     : {}", too_large.len());

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

    if !too_large.is_empty() {
        println!("\nFiles too large to process (will be skipped):");
        for (path, size_bytes) in &too_large {
            // Convert bytes -> MB for display. `{}` formats the integer.
            println!(
                "  {}  — {} MB",
                path.file_name().unwrap_or_default().to_str().unwrap_or("?"),
                size_bytes / (1024 * 1024)
            );
        }
    }

    // Next steps: mapper check, then binding logic
    // --- Create the run log ----------------------------------------------
    // Created here (not inside handle_mapper) so every path can log to it,
    // including the no-mapper path, and so we can record the oversized
    // files we found during classification. Log goes to the current
    // directory — where the user invoked paperclip from.
    let current_dir = std::env::current_dir()?;
    let mut run_log = crate::log::RunLog::new(&current_dir);

    // Record oversized files now, while we still have their sizes.
    // These were skipped before parsing, independent of any mapper.
    for (path, size_bytes) in &too_large {
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        run_log.skip(filename, crate::log::SkipReason::TooLarge(*size_bytes));
    }

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
                handle_mapper(mapper_path, &regular_pdfs, &mut run_log)?;
            }
        }
    }

    // --- Write log -------------------------------------------------------
    // Single write point for the whole run. Does nothing if no entries.
    run_log.write()?;

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
fn handle_mapper(
    mapper_path: &std::path::Path,
    regular_pdfs: &[PathBuf],
    run_log: &mut crate::log::RunLog,
) -> Result<()> {
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

    // --- Match PDFs against mapper rows ----------------------------------
    let (binder_map, unmatched) = crate::mapper::match_pdfs(regular_pdfs, &rows);

    // Record unmatched files in the log
    for pdf in &unmatched {
        let filename = pdf.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        run_log.skip(filename, crate::log::SkipReason::NoMapperMatch);
    }

    // --- Parse filenames and FLAG (not skip) odd ones --------------------
    // Files that don't match the naming style are now KEPT in the binder and
    // merely reported. We run the lenient parser purely to detect and log
    // problems; the file stays in `binder_map` either way.
    println!("\nValidating filenames...");
    let mut flagged_count = 0usize;

    for pdf in regular_pdfs {
        let stem = pdf.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        // Lenient parse never errors; flag_reason is Some(..) when something
        // didn't match the naming convention.
        let parsed = crate::filename_parser::parse_lenient(stem);

        if let Some(reason) = &parsed.flag_reason {
            let filename = pdf.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            // Record the full reason verbatim — same text the manifest stores,
            // so the CSV and the embedded manifest agree exactly even when a
            // file has more than one naming problem.
            println!("  Flagging (kept): {} — {}", stem, reason);
            run_log.skip(filename, crate::log::SkipReason::Flagged(reason.clone()));
            flagged_count += 1;
        }
    }

    if flagged_count == 0 {
        println!("  All filenames valid.");
    } else {
        println!("  {} file(s) flagged but kept in their binders.", flagged_count);
    }

    // --- Binder plan: every matched file is kept -------------------------
    // No filtering anymore — flagged files remain. We just rebind the name so
    // the downstream plan/assembly code is unchanged. The inner Vec<&PathBuf>
    // is wrapped to Vec<&&PathBuf> to match assemble_all's expected shape.
    let filtered_binder_map: std::collections::HashMap<&String, Vec<&&PathBuf>> = binder_map
        .iter()
        .map(|(binder_name, files)| {
            let kept: Vec<&&PathBuf> = files.iter().collect();
            (binder_name, kept)
        })
        .collect();

    // --- Print binder plan -----------------------------------------------
    println!("\nBinders to assemble:");
    if filtered_binder_map.is_empty() {
        println!("  (none — no PDFs matched any mapper row)");
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
        crate::assembler::assemble_all(&filtered_binder_map, &rows, run_log)?;
    }

    Ok(())
}

/// Recursively finds all PDF files under the given folder.
/// Returns a sorted list of absolute paths.
///
/// Uses the `walkdir` crate instead of a hand-rolled recursion. WalkDir flattens
/// the whole tree into one iterator — the clean version of the "get the tree, then
/// walk the list" idea. It's roughly:
///   C#:     Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories)
///   Python: os.walk(root)
fn find_pdfs(root: &Path) -> Vec<PathBuf> {
    let mut results: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()                       // turn the WalkDir into an iterator of entries
        .filter_map(|entry| entry.ok())    // skip any entry we couldn't read (permissions, etc.)
                                           //   instead of crashing — like the old `match … Err => return`
        .map(|entry| entry.into_path())    // DirEntry -> owned PathBuf
        .filter(|path| {
            // Keep only files whose extension is "pdf" (case-insensitive: .PDF .pdf .Pdf)
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("pdf"))
                .unwrap_or(false)
        })
        .collect();                        // gather the matches into a Vec

    results.sort();                        // keep the stable, filename-ascending order you had before
    results
}