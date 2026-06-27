// src/binder.rs

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use rayon::prelude::*;   // brings par_iter() into scope, like `using System.Linq;` for PLINQ
use regex::Regex;

/// `binder create folder [PATH]` — scan a folder for PDFs and assemble binders
/// via the mapper CSV. 
pub fn create_from_folder() -> Result<()> {
    // Source PDFs are always read from the current working folder — you run
    // paperclip from inside the project folder. This keeps create and update
    // consistent: both treat the working folder as the source of truth.
    let scan_dir = std::env::current_dir()?;
    println!("Scanning for PDFs in: {}", scan_dir.display());

    // Resolve effective config: global (settings.toml) with an optional
    // paperclip.toml in THIS folder layered on top. The project file is where
    // the mapper, doc-number pattern, and project name come from per-project.
    let config = crate::project_config::resolve_config(&scan_dir)?;

    // Compile the doc-number pattern up front, so a typo'd mask stops the run
    // immediately with a clear message rather than flagging every single file.
    // `None` if no pattern is configured — parse_lenient then uses its default
    // 5-part code regex. `map_err` turns the compiler's plain-String error into
    // an anyhow error with context (the `?` then propagates it out of the run).
    let code_pattern = match config.doc_number_pattern.as_deref() {
        Some(mask) => Some(
            crate::filename_parser::compile_doc_number_mask(mask)
                .map_err(|e| anyhow::anyhow!("invalid document-number pattern: {e}"))?,
        ),
        None => None,
    };

    let pdfs = find_pdfs(&scan_dir);

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

    // --- Classify each PDF in parallel -----------------------------------
    // Sort heaviest-first so expensive Document::load calls are spread across
    // threads early, when rayon has the most room to balance and steal work.
    // Cheap files fall to the tail, where any imbalance costs little. One
    // metadata() syscall per file — no full parse, same cheap call the size
    // guard uses. This is what flattens the long-straggler thread.
    let mut pdfs = pdfs;
    pdfs.sort_by_key(|p| std::cmp::Reverse(
        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    ));
    let classified: Vec<crate::pdf_classifier::ClassifiedPdf> = pdfs
        .par_iter()                          // <-- the only real change: iter() -> par_iter()
        .map(|path| {
            let filename = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("...");
            pb.set_message(filename.to_string());

            let result = crate::pdf_classifier::classify(path);
            pb.inc(1);                       // ProgressBar is thread-safe — safe to call here
            result
        })
        .collect();                          // gather all results into a Vec

    pb.finish_and_clear();

    // --- Bucket the results (sequential — mutates shared Vecs) -----------
    let mut regular_pdfs: Vec<PathBuf> = Vec::new();
    let mut existing_binders: Vec<(PathBuf, String)> = Vec::new();
    let mut unreadable: Vec<(PathBuf, String)> = Vec::new();
    let mut too_large: Vec<(PathBuf, u64)> = Vec::new();

    for classified_pdf in classified {
        match classified_pdf.kind {
            crate::pdf_classifier::PdfKind::Regular => {
                regular_pdfs.push(classified_pdf.path);
            }
            crate::pdf_classifier::PdfKind::Binder { binder_name } => {
                existing_binders.push((classified_pdf.path, binder_name));
            }
            crate::pdf_classifier::PdfKind::Unreadable { reason } => {
                unreadable.push((classified_pdf.path, reason));
            }
            crate::pdf_classifier::PdfKind::TooLarge { size_bytes } => {
                too_large.push((classified_pdf.path, size_bytes));
            }
        }
    }

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
    // Log goes to the same folder we scanned (for a custom PATH, that's where
    // the user pointed us; for the default, it's the current dir as before).
    let mut run_log = crate::log::RunLog::new(&scan_dir);

    // Record oversized files now, while we still have their sizes.
    // These were skipped before parsing, independent of any mapper.
    for (path, size_bytes) in &too_large {
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        run_log.skip(filename, crate::log::SkipReason::TooLarge(*size_bytes));
    }

    // --- Mapper check ----------------------------------------------------
    // `config` was resolved at the top of the function; reuse it here (don't
    // reload). mapper_csv_path is now a PathBuf (resolved relative to the
    // paperclip.toml when it came from there).
    match config.mapper_csv_path.as_ref() {
        None => {
            // No mapper configured at all
            handle_no_mapper(&regular_pdfs)?;
        }
        Some(mapper_path) => {
            if !mapper_path.exists() {
                // Path is configured but file is gone
                println!("\nWarning: Mapper file configured but not found at:");
                println!("  {}", mapper_path.display());
                println!("Set `mapper` in paperclip.toml or run `paperclip config set --mapper-path`.");
                handle_no_mapper(&regular_pdfs)?;
            } else {
                // Mapper exists — read and validate it. Pass the compiled
                // doc-number pattern through for filename validation.
                handle_mapper(mapper_path, &regular_pdfs, &mut run_log, code_pattern.as_ref())?;
            }
        }
    }

    // --- Write log -------------------------------------------------------
    // Single write point for the whole run. Does nothing if no entries.
    run_log.write()?;

    Ok(())
}


/// `binder create aconex` — build binders from Aconex search results.
///
/// Flow (will be async once wired to the client):
///   - load mapper rows
///   - for each row: run its search (row.prefix → search query) via the
///     aconex client, download each matched document to a temp/working area
///   - match downloaded files to the binder, assemble (fresh)
/// TODO: needs build_client()/project resolution like aconex_cmd; make async.
pub fn create_from_aconex() -> Result<()> {
    println!("[stub] binder create aconex — not yet implemented");
    Ok(())
}

/// `binder update folder [PATH]` — refresh existing binders where a source
/// document's revision has changed.
///
/// Source PDFs are read from the current working folder. `binder_path` says
/// where the binders to update live; `None` means they're in the working folder
/// alongside the sources.
///
/// Flow:
///   1. Scan + classify the working folder (gives current source PDFs).
///   2. Scan the binder folder for existing binders.
///   3. For each binder: read its manifest, build a code→newfile map from the
///      current sources whose revision differs, and if any differ, rebuild it
///      in place via the assembler.
/// Scope is fixed at creation — update never adds or removes documents.
pub fn update_from_folder(binder_path: Option<&str>) -> Result<()> {
    use std::collections::HashMap;

    // Sources always come from the working folder; that's also where
    // paperclip.toml (mapper/pattern) lives.
    let work_dir = std::env::current_dir()?;
    let binder_dir = match binder_path {
        Some(p) => PathBuf::from(p),
        None => work_dir.clone(),
    };

    println!("Source PDFs from: {}", work_dir.display());
    println!("Binders in:       {}", binder_dir.display());

    // Resolve config + compile the doc-number pattern, same as create. A typo'd
    // pattern stops the run up front rather than mis-parsing every source.
    let config = crate::project_config::resolve_config(&work_dir)?;
    let code_pattern = match config.doc_number_pattern.as_deref() {
        Some(mask) => Some(
            crate::filename_parser::compile_doc_number_mask(mask)
                .map_err(|e| anyhow::anyhow!("invalid document-number pattern: {e}"))?,
        ),
        None => None,
    };

    // --- 1. Find the current source PDFs in the working folder -----------
    // We only need REGULAR pdfs as sources; classify also tells us which are
    // themselves binders (skipped as sources).
    let source_pdfs = find_pdfs(&work_dir);
    if source_pdfs.is_empty() {
        println!("No source PDFs found in the working folder. Nothing to update from.");
        return Ok(());
    }

    // Build a code -> path map of the current sources. Parse each filename for
    // its code; sources without a parseable code can't be matched and are
    // skipped (they could never line up with a manifest entry).
    let mut sources_by_code: HashMap<String, PathBuf> = HashMap::new();
    for pdf in &source_pdfs {
        let stem = pdf.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let parsed = crate::filename_parser::parse_lenient(stem, code_pattern.as_ref());
        if let Some(code) = parsed.code {
            // Last-write-wins if two sources share a code; unusual, but we don't
            // try to resolve it here.
            sources_by_code.insert(code, pdf.clone());
        }
    }

    // --- 2. Find the existing binders in the binder folder ---------------
    // Classify the binder folder's PDFs and keep only the ones marked as our
    // binders (via the Info-dict marker the classifier reads).
    let binder_candidates = find_pdfs(&binder_dir);
    let mut binders: Vec<PathBuf> = Vec::new();
    for pdf in &binder_candidates {
        match crate::pdf_classifier::classify(pdf).kind {
            crate::pdf_classifier::PdfKind::Binder { .. } => binders.push(pdf.clone()),
            _ => {} // regular/unreadable/too-large: not a binder to update
        }
    }

    if binders.is_empty() {
        println!("No existing binders found in {}.", binder_dir.display());
        return Ok(());
    }

    println!("\nFound {} binder(s) to check.", binders.len());

    // --- 3. Check each binder and rebuild the stale ones -----------------
    let mut rebuilt = 0usize;
    let mut unchanged = 0usize;

    for binder in &binders {
        let label = binder.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        println!("\nChecking: {}", label);

        // Read the binder's manifest. No manifest = not ours / pre-manifest;
        // skip it rather than error.
        let doc = match lopdf::Document::load(binder) {
            Ok(d) => d,
            Err(e) => {
                println!("  Skipped — could not open ({e}).");
                continue;
            }
        };
        let json = match crate::xmp::read_manifest_json(&doc)? {
            Some(j) => j,
            None => {
                println!("  Skipped — no paperclip manifest.");
                continue;
            }
        };
        let manifest = crate::manifest::BinderManifest::from_json(&json)
            .with_context(|| format!("parsing manifest of {}", label))?;

        // Build the code -> current revision map for THIS binder's scope only:
        // for each scoped document, what revision is present in the sources now?
        let mut source_revisions: HashMap<String, Option<String>> = HashMap::new();
        for entry in &manifest.files {
            if let Some(code) = entry.code.as_deref() {
                if let Some(path) = sources_by_code.get(code) {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    let parsed = crate::filename_parser::parse_lenient(stem, code_pattern.as_ref());
                    source_revisions.insert(code.to_string(), parsed.revision);
                }
                // No source for this code → simply absent from the map, which
                // check_binder reads as "missing source" (a warning, not a
                // rebuild).
            }
        }

        // Rev-check: which scoped documents changed?
        let report = crate::update_check::check_binder(&manifest, &source_revisions);

        // Report missing sources (scoped docs with no current source).
        for code in &report.missing_sources {
            println!("  Note: no source present for {} — keeping existing version.", code);
        }

        if !report.needs_rebuild() {
            println!("  Up to date.");
            unchanged += 1;
            continue;
        }

        // Announce what's changing.
        for (code, old, new) in &report.changed {
            println!("  Changed: {}  rev {} -> {}", code, old, new);
        }

        // Build the new_sources map (code -> new file path) for only the
        // changed documents — that's what rebuild_in_place swaps in.
        let mut new_sources: HashMap<String, PathBuf> = HashMap::new();
        for (code, _, _) in &report.changed {
            if let Some(path) = sources_by_code.get(code) {
                new_sources.insert(code.clone(), path.clone());
            }
        }

        // Rebuild in place.
        crate::assembler::rebuild_in_place(
            binder,
            &manifest,
            &new_sources,
            code_pattern.as_ref(),
        )
        .with_context(|| format!("rebuilding {}", label))?;

        println!("  Rebuilt.");
        rebuilt += 1;
    }

    println!(
        "\nUpdate complete: {} rebuilt, {} unchanged.",
        rebuilt, unchanged
    );

    Ok(())
}

/// `binder update aconex` — rev-checked rebuild from Aconex.
///
/// Combines create_from_aconex's fetch with update_from_folder's rev check:
///   - for each mapper row, search Aconex; the search result carries the
///     document Revision directly (no filename parse needed — search.rs's
///     Document has `revision`)
///   - compare against the existing binder manifest's recorded revisions
///   - download + rebuild only where they differ
/// TODO: async; reuse the rev-comparison helper from update_from_folder.
pub fn update_from_aconex() -> Result<()> {
    println!("[stub] binder update aconex — not yet implemented");
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
    code_pattern: Option<&Regex>,
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
        // `None` for now → uses the default 5-part code regex. Once the
        // paperclip.toml doc-number pattern is loaded, the compiled Regex
        // gets threaded through to here as Some(&pattern).
        let parsed = crate::filename_parser::parse_lenient(stem, code_pattern);

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
        crate::assembler::assemble_all(&filtered_binder_map, &rows, run_log, code_pattern)?;
    }

    Ok(())
}

/// Recursively finds all PDF files under the given folder.
/// Returns a list of absolute paths.
///
/// Uses the `walkdir` crate instead of a hand-rolled recursion. WalkDir flattens
/// the whole tree into one iterator — the clean version of the "get the tree, then
/// walk the list" idea. It's roughly:
///   C#:     Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories)
///   Python: os.walk(root)
fn find_pdfs(root: &Path) -> Vec<PathBuf> {
    let results: Vec<PathBuf> = WalkDir::new(root)
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

    // Discovery order doesn't matter — classification buckets results and the
    // assembler re-sorts each binder's files by name anyway. We deliberately
    // skip a sort here; classification sorts by size instead (see run()).
    results
}