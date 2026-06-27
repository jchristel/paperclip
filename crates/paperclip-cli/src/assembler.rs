// src/assembler.rs
// Merges source PDFs into a single binder PDF and attaches an XMP manifest
// describing its contents. (Per-document cover pages were removed in favour
// of the embedded manifest.)
//
// Two entry points:
//   * assemble_all / assemble_one — CREATE: build binders fresh from source
//     PDFs, driven by the mapper.
//   * rebuild_in_place           — UPDATE: rebuild an existing binder from a
//     mix of new source files (changed revisions) and pages carried over from
//     the old binder (unchanged documents). No mapper; rebuilds in place.
// Both share the lopdf page-merge core in `merge_documents`.

use anyhow::{Context, Result};
use lopdf::{dictionary, Document, Object, ObjectId};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use colored::Colorize;

/// Assembles all binders described in binder_map.
pub fn assemble_all(
    binder_map: &HashMap<&String, Vec<&&PathBuf>>,
    rows: &[crate::mapper::MapperRow],
    run_log: &mut crate::log::RunLog,
    code_pattern: Option<&regex::Regex>,
) -> Result<()> {
    let output_folder_map: HashMap<&str, &str> = rows
        .iter()
        .map(|r| (r.binder_name.as_str(), r.output_folder.as_str()))
        .collect();

    let mut binder_names: Vec<&&String> = binder_map.keys().collect();
    binder_names.sort();

    for binder_name in binder_names {
        let files = &binder_map[binder_name];
        let output_folder = output_folder_map
            .get(binder_name.as_str())
            .context("No output folder found for binder")?;

        println!("\nAssembling binder: {}", binder_name);

        // Collect just the mapper rows that belong to THIS binder, so the
        // manifest records only the rows that drove it (not every row in the
        // CSV). `binder_name` here is `&&String`, hence the double deref.
        let binder_rows: Vec<crate::manifest::MapperRow> = rows
            .iter()
            .filter(|r| r.binder_name == **binder_name)
            .map(|r| crate::manifest::MapperRow {
                prefix: r.prefix.clone(),
                binder_name: r.binder_name.clone(),
                output_folder: r.output_folder.clone(),
            })
            .collect();

        match assemble_one(binder_name, files, Path::new(output_folder), binder_rows, code_pattern) {
            Ok(output_path) => {
                println!("{}", format!("  Written to: {}", output_path.display()).green());
            }
            Err(e) => {
                println!("{}", format!("  ERROR: {}", e).red());
                run_log.skip(
                    binder_name,
                    crate::log::SkipReason::Unreadable(e.to_string()),
                );
            }
        }
    }

    Ok(())
}

/// Assembles a single binder PDF from a list of source files.
/// `mapper_rows` are the rows that drove this binder; they're recorded in
/// the embedded manifest so the binder is self-describing.
fn assemble_one(
    binder_name: &str,
    files: &[&&PathBuf],
    output_folder: &Path,
    mapper_rows: Vec<crate::manifest::MapperRow>,
    code_pattern: Option<&regex::Regex>,
) -> Result<PathBuf> {
    // Sort files by filename ascending
    let mut sorted_files = files.to_vec();
    sorted_files.sort_by_key(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    });

    // Collect the source documents to merge (no cover pages anymore — the
    // binder is just the merged source pages) and, in parallel, build one
    // manifest FileEntry per file describing where it landed.
    let mut documents: Vec<Document> = Vec::new();
    let mut file_entries: Vec<crate::manifest::FileEntry> = Vec::new();

    // 1-based running page counter. With no cover pages, the first file's
    // content starts at page 1 — no +1 offset anywhere now.
    let mut current_page = 1u32;

    for file in &sorted_files {
        let filename = file.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let stem = file.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        // Lenient parse: never fails. Pulls out whatever it can and reports a
        // flag_reason for anything missing. Files are KEPT regardless now.
        let parsed = crate::filename_parser::parse_lenient(stem, code_pattern);

        let source_doc = Document::load(file.as_path())
            .with_context(|| format!("Failed to open: {}", filename))?;

        let source_page_count = source_doc.get_pages().len() as u32;
        let start_page = current_page;
        let end_page   = start_page + source_page_count - 1;

        // For display, fall back to "?" when revision is absent.
        let rev_display = parsed.revision.as_deref().unwrap_or("?");
        if let Some(reason) = &parsed.flag_reason {
            println!(
                "  Adding: {} (rev {}, pages {}–{}) [flagged: {}]",
                filename, rev_display, start_page, end_page, reason
            );
        } else {
            println!(
                "  Adding: {} (rev {}, pages {}–{})",
                filename, rev_display, start_page, end_page
            );
        }

        // Record this file in the manifest. Option fields carry through
        // directly: None where the parser couldn't extract a value.
        file_entries.push(crate::manifest::FileEntry {
            filename: filename.to_string(),
            code: parsed.code.clone(),
            revision: parsed.revision.clone(),
            name: parsed.name.clone(),
            start_page,
            end_page,
            added_utc: chrono::Utc::now().to_rfc3339(),
            flag_reason: parsed.flag_reason.clone(),
        });

        documents.push(source_doc);

        current_page += source_page_count;
    }

    // Merge all collected source documents into one binder document.
    let mut binder_doc = merge_documents(documents)?;

    // --- Embed the fast-ID marker in the Info dict -----------------------
    // The full data lives in the XMP manifest below; this Info-dict marker
    // stays as a cheap "is this one of ours?" check for the classifier.
    embed_marker(&mut binder_doc, binder_name);

    // --- Build and attach the XMP manifest -------------------------------
    let manifest = crate::manifest::BinderManifest::new(
        binder_name,
        mapper_rows,
        file_entries,
    );
    let manifest_json = manifest.to_json()?;
    crate::xmp::attach_manifest(&mut binder_doc, &manifest_json)
        .context("Failed to attach manifest to binder")?;

    // --- Write to disk ---------------------------------------------------
    let output_path = output_folder.join(format!("{}.pdf", binder_name));
    binder_doc.save(&output_path)
        .with_context(|| format!("Failed to write binder: {}", output_path.display()))?;

    Ok(output_path)
}

// --- Shared page-merge core ----------------------------------------------

/// Merges an ordered list of `Document`s into one binder `Document` (no marker,
/// no manifest, no save — just the page merge). Shared by create
/// (`assemble_one`) and update (`rebuild_in_place`). Pages come out in the
/// order the input Vec gives them.
///
/// This is the lopdf merge pattern from the lopdf docs, kept in ONE place so
/// both callers share the tricky object-renumbering logic.
fn merge_documents(documents: Vec<Document>) -> Result<Document> {
    let mut max_id = 1;
    let mut documents_pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut binder_doc = Document::with_version("1.5");

    for mut doc in documents {
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        documents_pages.extend(
            doc.get_pages()
                .into_iter()
                .map(|(_, object_id)| {
                    (object_id, doc.get_object(object_id).unwrap().to_owned())
                })
                .collect::<BTreeMap<ObjectId, Object>>(),
        );
        documents_objects.extend(doc.objects);
    }

    // Process all objects except Page type
    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object:   Option<(ObjectId, Object)> = None;

    for (object_id, object) in documents_objects.iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalog_object = Some((
                    if let Some((id, _)) = catalog_object { id } else { *object_id },
                    object.clone(),
                ));
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, ref obj)) = pages_object {
                        if let Ok(old_dict) = obj.as_dict() {
                            dictionary.extend(old_dict);
                        }
                    }
                    pages_object = Some((
                        if let Some((id, _)) = pages_object { id } else { *object_id },
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            b"Page"     => {}
            b"Outlines" => {}
            b"Outline"  => {}
            _ => { binder_doc.objects.insert(*object_id, object.clone()); }
        }
    }

    let pages_object   = pages_object.context("No Pages object found in source PDFs")?;
    let catalog_object = catalog_object.context("No Catalog object found in source PDFs")?;

    // Attach all pages to the unified Pages tree
    for (object_id, object) in documents_pages.iter() {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_object.0);
            binder_doc.objects.insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    // Build final Pages dictionary
    if let Ok(dictionary) = pages_object.1.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Count", documents_pages.len() as u32);
        dictionary.set(
            "Kids",
            documents_pages
                .iter()
                .map(|(id, _)| Object::Reference(*id))
                .collect::<Vec<_>>(),
        );
        binder_doc.objects.insert(pages_object.0, Object::Dictionary(dictionary));
    }

    // Build final Catalog
    if let Ok(dictionary) = catalog_object.1.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_object.0);
        dictionary.remove(b"Outlines");
        binder_doc.objects.insert(catalog_object.0, Object::Dictionary(dictionary));
    }

    binder_doc.trailer.set("Root", catalog_object.0);
    binder_doc.max_id = binder_doc.objects.len() as u32;
    binder_doc.renumber_objects();
    binder_doc.adjust_zero_pages();

    Ok(binder_doc)
}

// --- Update: rebuild in place --------------------------------------------

/// Rebuilds an existing binder in place from mixed page sources, loading the
/// old binder only ONCE.
///
/// Walks the binder's manifest scope in recorded order. For each document:
///   * changed (a file for its `code` is in `new_sources`): the new file's
///     pages are used, with fresh metadata.
///   * unchanged: its existing pages are taken from the old binder. To avoid
///     re-reading the (possibly large) binder per document, we load it once and
///     CLONE-and-trim that in-memory copy per contiguous RUN of unchanged
///     documents — cloning is a memory copy, not a PDF re-parse.
///
/// Page ranges for the NEW manifest are computed in this single forward pass
/// from known lengths — for unchanged docs the length comes straight from the
/// old manifest (end - start + 1), for changed docs from the new file's page
/// count. No post-merge recomputation is needed.
///
/// The rebuilt binder keeps the original `binder_id`; unchanged documents keep
/// their original metadata (added_utc, revision), changed documents get fresh
/// entries.
///
/// `binder_path` is both read source (old pages) and write target (in place).
/// `new_sources` maps a document `code` to the path of its new-revision file.
pub fn rebuild_in_place(
    binder_path: &Path,
    old_manifest: &crate::manifest::BinderManifest,
    new_sources: &HashMap<String, PathBuf>,
    code_pattern: Option<&regex::Regex>,
) -> Result<()> {
    // Load the old binder ONCE. All unchanged pages come from clones of this.
    let old_binder = Document::load(binder_path)
        .with_context(|| format!("Failed to open binder: {}", binder_path.display()))?;

    // The ordered list of documents to merge, and the matching manifest entries.
    let mut documents: Vec<Document> = Vec::new();
    let mut file_entries: Vec<crate::manifest::FileEntry> = Vec::new();

    // 1-based running page counter in the REBUILT binder. Each document, new or
    // carried-over, advances it by its own page count — this is what gives every
    // entry a correct start/end without any second pass.
    let mut current_page: u32 = 1;

    // Accumulator for a contiguous run of UNCHANGED documents. We collect their
    // old-binder page span so the whole run is extracted in ONE clone-and-trim,
    // while still recording each document's own range from manifest arithmetic.
    let mut run_old_start: Option<u32> = None; // first old-binder page of the run
    let mut run_old_end: u32 = 0;              // last old-binder page of the run

    for entry in &old_manifest.files {
        let new_file = entry
            .code
            .as_deref()
            .and_then(|code| new_sources.get(code));

        match new_file {
            // --- changed: flush any pending unchanged run, then add new file -
            Some(path) => {
                flush_unchanged_run(
                    &old_binder,
                    &mut run_old_start,
                    run_old_end,
                    &mut documents,
                );

                let doc = Document::load(path)
                    .with_context(|| format!("Failed to open new source: {}", path.display()))?;
                let page_count = doc.get_pages().len() as u32;

                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let parsed = crate::filename_parser::parse_lenient(stem, code_pattern);

                let start_page = current_page;
                let end_page = start_page + page_count - 1;
                current_page += page_count;

                println!(
                    "  Updating: {} -> rev {} (pages {}–{})",
                    path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                    parsed.revision.as_deref().unwrap_or("?"),
                    start_page, end_page
                );

                documents.push(doc);
                file_entries.push(crate::manifest::FileEntry {
                    filename: path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string(),
                    code: parsed.code,
                    revision: parsed.revision,
                    name: parsed.name,
                    start_page,
                    end_page,
                    added_utc: chrono::Utc::now().to_rfc3339(), // entered now
                    flag_reason: parsed.flag_reason,
                });
            }
            // --- unchanged: extend the run, record this doc's new range -----
            None => {
                // Length of this document in the OLD binder, straight from its
                // recorded span. We trust the manifest's page map here.
                let page_count = entry.end_page - entry.start_page + 1;

                // Extend the old-binder run span so the flush extracts it all.
                if run_old_start.is_none() {
                    run_old_start = Some(entry.start_page);
                }
                run_old_end = entry.end_page;

                // This document's range in the REBUILT binder.
                let start_page = current_page;
                let end_page = start_page + page_count - 1;
                current_page += page_count;

                file_entries.push(crate::manifest::FileEntry {
                    filename: entry.filename.clone(),
                    code: entry.code.clone(),
                    revision: entry.revision.clone(),
                    name: entry.name.clone(),
                    start_page,
                    end_page,
                    added_utc: entry.added_utc.clone(), // preserved
                    flag_reason: entry.flag_reason.clone(),
                });
            }
        }
    }

    // Flush a trailing unchanged run, if the scope ended on unchanged docs.
    flush_unchanged_run(&old_binder, &mut run_old_start, run_old_end, &mut documents);

    // Merge new files + extracted unchanged runs, in order.
    let mut binder_doc = merge_documents(documents)?;

    // Re-embed marker + fresh manifest with the SAME binder_id.
    embed_marker(&mut binder_doc, &old_manifest.binder_name);

    let mut manifest = crate::manifest::BinderManifest::new(
        &old_manifest.binder_name,
        old_manifest.mapper_rows.clone(),
        file_entries,
    );
    manifest.binder_id = old_manifest.binder_id.clone(); // identity survives

    let manifest_json = manifest.to_json()?;
    crate::xmp::attach_manifest(&mut binder_doc, &manifest_json)
        .context("Failed to attach manifest to rebuilt binder")?;

    binder_doc.save(binder_path)
        .with_context(|| format!("Failed to write rebuilt binder: {}", binder_path.display()))?;

    Ok(())
}

/// Extracts a pending run of unchanged pages [run_old_start, run_old_end] from
/// the already-loaded old binder by CLONING it (memory copy) and deleting every
/// page outside the run. Pushes the result onto `documents` and clears the run.
/// A no-op if no run is pending (run_old_start is None).
fn flush_unchanged_run(
    old_binder: &Document,
    run_old_start: &mut Option<u32>,
    run_old_end: u32,
    documents: &mut Vec<Document>,
) {
    let start = match run_old_start.take() {
        Some(s) => s,
        None => return,
    };

    let mut chunk = old_binder.clone();
    let total = chunk.get_pages().len() as u32;
    let to_delete: Vec<u32> = (1..=total)
        .filter(|p| *p < start || *p > run_old_end)
        .collect();
    chunk.delete_pages(&to_delete);

    documents.push(chunk);
}

/// Embeds the BinderTool marker into the document Info dictionary.
fn embed_marker(doc: &mut Document, binder_name: &str) {
    let info = dictionary! {
        "BinderTool" => Object::string_literal("paperclip/1"),
        "BinderName" => Object::string_literal(binder_name),
    };
    let info_id = doc.add_object(Object::Dictionary(info));
    doc.trailer.set("Info", Object::Reference(info_id));
}