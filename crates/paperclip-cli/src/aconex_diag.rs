// src/aconex_diag.rs
//
// Diagnostic Aconex commands — surfaced under `paperclip aconex diag ...`.
//
// These are intentionally LOW-LEVEL: they reach for the client's raw methods
// (get_text) rather than the typed ones, so they keep working — and keep
// showing you what's actually on the wire — even when the typed parsing layer
// is the thing that's broken. That's exactly what you want when tracing a
// problem or when Aconex changes a response shape.
//
// They reuse the same credential/project helpers as the real commands
// (build_client, current_project_name) via their pub(crate) visibility.

use anyhow::{Context, Result};

use crate::aconex_cmd::{build_client, current_project_name};

/// Raw connectivity test. Hits the projects endpoint and prints the first chunk
/// of the response verbatim. Proves auth + transport WITHOUT depending on any
/// parsing — so if `aconex projects` breaks on a response change, this still
/// tells you whether the connection itself is healthy.
pub async fn ping() -> Result<()> {
    let client = build_client()?;
    println!("Calling Aconex...");
    let body = client.get_text("/api/projects/").await?;
    let preview: String = body.chars().take(2000).collect();
    println!("\n--- Response (first 2000 chars) ---\n{}", preview);
    Ok(())
}

/// Raw register search. Prints the unparsed XML body so the exact <Document>
/// structure is visible.
///
/// `fields` controls return_fields from the command line:
///   * empty   → request nothing; Aconex returns bare DocumentId stubs.
///   * provided→ request exactly those fields, to confirm which REQUEST names
///               are valid. NOTE: request names are lowercase (docno, title);
///               feeding back a RESPONSE element name (DocumentNumber) returns
///               HTTP 400 — the two vocabularies differ.
pub async fn search_raw(query: &str, fields: &[String]) -> Result<()> {
    let name = current_project_name()?;
    let client = build_client()?;

    let project = client
        .get_project(&name)
        .await?
        .with_context(|| format!("Project '{}' not found", name))?;

    // Base request — page_size=500 mirrors the working typed search (Aconex
    // rejects some other sizes, e.g. 10 → HTTP 400 on this endpoint).
    let mut path = format!(
        "/api/projects/{id}/register?search_query={q}&search_type=PAGED&page_size=500&page_number=1",
        id = project.project_id,
        q = urlencode(query),
    );

    if !fields.is_empty() {
        let joined = fields.join(",");
        path.push_str(&format!("&return_fields={}", urlencode(&joined)));
        println!("[diag] requesting fields: {}", joined);
    } else {
        println!("[diag] no fields requested — expect DocumentId-only stubs");
    }

    let body = client.get_text(&path).await?;
    println!("{}", body);
    Ok(())
}

/// Raw register schema. Prints the unparsed schema XML for the current project,
/// which describes the fields that exist for documents — INCLUDING any custom,
/// project-specific fields. Use this to discover what's available beyond the
/// core fields the typed search models.
///
/// Caveat worth checking in the output: the schema describes the register's
/// fields, but the identifiers it lists may be a DIFFERENT vocabulary again
/// from the lowercase `return_fields` request names. Inspect before assuming a
/// schema field name is directly usable as a return_field.
pub async fn schema() -> Result<()> {
    let name = current_project_name()?;
    let client = build_client()?;

    let project = client
        .get_project(&name)
        .await?
        .with_context(|| format!("Project '{}' not found", name))?;

    let path = format!("/api/projects/{id}/register/schema", id = project.project_id);

    println!("[diag] fetching register schema for project '{}'...", name);
    let body = client.get_text(&path).await?;
    println!("{}", body);
    Ok(())
}

/// Minimal percent-encoder for diagnostic query values.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{:02X}", other)),
        }
    }
    out
}
