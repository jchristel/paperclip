// src/assembler.rs
// Assembles source PDFs and cover pages into a single binder PDF.

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

        match assemble_one(binder_name, files, Path::new(output_folder)) {
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
fn assemble_one(
    binder_name: &str,
    files: &[&&PathBuf],
    output_folder: &Path,
) -> Result<PathBuf> {
    // Sort files by filename ascending
    let mut sorted_files = files.to_vec();
    sorted_files.sort_by_key(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    });

    // Collect all documents to merge — cover page + source PDF per file
    let mut documents: Vec<Document> = Vec::new();
    let mut current_page = 1u32;

    for file in &sorted_files {
        let filename = file.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let stem = file.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let parsed = crate::filename_parser::parse(stem)
            .context("Filename parse failed during assembly")?;

        let source_doc = Document::load(file.as_path())
            .with_context(|| format!("Failed to open: {}", filename))?;

        let source_page_count = source_doc.get_pages().len() as u32;
        let start_page = current_page + 1; // +1 for cover page
        let end_page   = start_page + source_page_count - 1;

        println!(
            "  Adding: {} (rev {}, pages {}–{})",
            filename, parsed.revision, start_page, end_page
        );

        // Generate cover page
        let datetime = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let cover_data = crate::cover_page::CoverPageData {
            filename,
            revision:   &parsed.revision,
            start_page,
            end_page,
            datetime:   &datetime,
        };
        let cover_doc = crate::cover_page::generate(&cover_data)
            .context("Failed to generate cover page")?;

        documents.push(cover_doc);
        documents.push(source_doc);

        current_page += 1 + source_page_count;
    }

    // --- Merge all documents using the lopdf pattern from the docs -------
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

    // --- Embed BinderTool marker -----------------------------------------
    embed_marker(&mut binder_doc, binder_name);

    // --- Write to disk ---------------------------------------------------
    let output_path = output_folder.join(format!("{}.pdf", binder_name));
    binder_doc.save(&output_path)
        .with_context(|| format!("Failed to write binder: {}", output_path.display()))?;

    Ok(output_path)
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