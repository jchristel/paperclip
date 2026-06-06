// src/inspect.rs
// The `paperclip inspect <file.pdf>` command: reads the embedded XMP manifest
// out of a binder and prints it in a human-readable form.
//
// This is both a debugging tool (confirm the manifest round-trips) and the
// read path that rename detection will build on — both go PDF -> manifest via
// the same xmp::read_manifest_json + manifest::from_json chain.

use anyhow::{bail, Context, Result};
use colored::Colorize;
use lopdf::Document;
use std::path::Path;

/// Loads the PDF at `path`, extracts its manifest, and prints it.
/// Returns an error if the file can't be read or has no paperclip manifest.
pub fn run(path: &str) -> Result<()> {
    let p = Path::new(path);

    if !p.exists() {
        bail!("File not found: {}", path);
    }

    println!("Inspecting: {}", path);

    // Load the whole PDF. (Same coarse load the classifier uses — fine for a
    // single explicit file the user named.)
    let doc = Document::load(p)
        .with_context(|| format!("Failed to open PDF: {}", path))?;

    // Pull the manifest JSON back out of the /Metadata stream.
    // Ok(None) means "this PDF has no paperclip manifest" — a normal outcome
    // for any non-binder PDF, so we report it plainly rather than erroring.
    let json = match crate::xmp::read_manifest_json(&doc)? {
        Some(j) => j,
        None => {
            println!("{}", "No paperclip manifest found in this PDF.".yellow());
            println!("(It may be a regular PDF, or a binder made before manifests were added.)");
            return Ok(());
        }
    };

    // Parse the JSON back into the typed struct. If this fails, the manifest
    // is present but malformed — worth surfacing as an error.
    let manifest = crate::manifest::BinderManifest::from_json(&json)
        .context("Manifest stream found but could not be parsed")?;

    // --- Pretty-print ----------------------------------------------------
    println!("\n{}", "Binder manifest".bold());
    println!("  Tool          : {}", manifest.tool);
    println!("  Schema version: {}", manifest.schema_version);
    println!("  Binder ID     : {}", manifest.binder_id);
    println!("  Binder name   : {}", manifest.binder_name);
    println!("  Created (UTC) : {}", manifest.created_utc);

    // --- Rename detection ------------------------------------------------
    // The manifest records the name the binder was CREATED with. The file's
    // stem (filename without extension) is its name NOW. If they differ, the
    // file was renamed on disk since assembly. The binder_id above is the
    // stable anchor — it survives renaming, so identity is never in doubt.
    let file_stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    if file_stem != manifest.binder_name {
        println!(
            "{}",
            format!(
                "  RENAMED: on disk as '{}' but created as '{}' (binder_id {})",
                file_stem, manifest.binder_name, manifest.binder_id
            )
            .yellow()
        );
    }

    println!("\n  Mapper rows ({}):", manifest.mapper_rows.len());
    for row in &manifest.mapper_rows {
        println!(
            "    prefix='{}'  binder='{}'  out='{}'",
            row.prefix, row.binder_name, row.output_folder
        );
    }

    println!("\n  Files ({}):", manifest.files.len());
    for f in &manifest.files {
        // as_deref turns Option<String> into Option<&str>; unwrap_or supplies
        // a placeholder when the field is None.
        let code = f.code.as_deref().unwrap_or("—");
        let rev  = f.revision.as_deref().unwrap_or("—");

        let line = format!(
            "    p{:>4}-{:<4} [{}] rev {}  {}",
            f.start_page, f.end_page, code, rev, f.filename
        );

        // Tint flagged entries so problems stand out at a glance.
        match &f.flag_reason {
            Some(reason) => {
                println!("{}", line.yellow());
                println!("{}", format!("           flagged: {}", reason).yellow());
            }
            None => println!("{}", line),
        }
    }

    // Quick summary line.
    let flagged = manifest.files.iter().filter(|f| f.flag_reason.is_some()).count();
    println!(
        "\n  {} file(s), {} flagged.",
        manifest.files.len(),
        flagged
    );

    Ok(())
}