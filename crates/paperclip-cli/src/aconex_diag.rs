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
// (build_client, current_project_name) via their pub(crate) visibility, so
// there's no duplicated setup.

use anyhow::{Context, Result};

use crate::aconex_cmd::{build_client, current_project_name};

/// Raw connectivity test. Hits the projects endpoint and prints the first chunk
/// of the response verbatim. Proves auth + transport work WITHOUT depending on
/// any parsing — so if `aconex projects` breaks on a response change, this still
/// tells you whether the connection itself is healthy.
pub async fn ping() -> Result<()> {
    let client = build_client()?;
    println!("Calling Aconex...");
    let body = client.get_text("/api/projects/").await?;
    let preview: String = body.chars().take(2000).collect();
    println!("\n--- Response (first 2000 chars) ---\n{}", preview);
    Ok(())
}

/// Raw register search. Runs a search and prints the unparsed XML body, so the
/// exact <Document> structure (attributes vs child elements) is visible. Use
/// this to confirm or correct the typed search model when results look wrong.
///
/// `fields` lets you control return_fields from the command line:
///   * empty   → request nothing; Aconex returns bare DocumentId stubs (the
///               minimal connectivity/structure probe).
///   * provided→ request exactly those fields, so you can confirm which names
///               are valid (a bad name returns HTTP 400 — bisect to find it).
pub async fn search_raw(query: &str, fields: &[String]) -> Result<()> {
    let name = current_project_name()?;
    let client = build_client()?;

    let project = client
        .get_project(&name)
        .await?
        .with_context(|| format!("Project '{}' not found", name))?;

    // Base request — mirror the working typed search: page_size=500 (Aconex
    // rejects some other sizes, e.g. 10 returns HTTP 400 on this endpoint).
    let mut path = format!(
        "/api/projects/{id}/register?search_query={q}&search_type=PAGED&page_size=500&page_number=1",
        id = project.project_id,
        q = urlencode(query),
    );

    // Append return_fields only if the caller supplied any. Joining with ',' is
    // what Aconex expects; we encode the whole joined value.
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

/// Minimal percent-encoder for diagnostic query values. The aconex crate has
/// its own (private) copy for the real search path; this one keeps the diag
/// module self-contained.
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