// crates/aconex/src/search.rs
//
// Document register search. Mirrors the Python `search`: GET on
//   /api/projects/{id}/register?search_query=...&search_type=PAGED&...
// looping pages until all documents are collected.
//
// Design decisions (from discussion):
//   * Auto-paginate  — read @TotalPages from the first response, loop to the
//     end, return one flat Vec<Document>. Caller never handles pages.
//   * Flexible map   — each <Document> is captured as ALL of its attributes in
//     a HashMap<String, String>, rather than ~80 typed struct fields. This is
//     the "capture everything" choice; see the parsing note below for why XML
//     makes this a custom step rather than a plain derive.
//   * Takes a &Project — caller resolves the project first (get_project), so
//     search makes no extra lookup call and the dependency is explicit.

use std::collections::HashMap;

use serde::Deserialize;

use crate::client::Client;
use crate::error::{AconexError, Result};
use crate::projects::Project;

// --- The fields we ask Aconex to return ----------------------------------
//
// The Python sends a long comma-separated `return_fields` list. We keep a
// representative core set here. Even though we capture whatever comes back as a
// map, we must still TELL Aconex which fields we want returned — unrequested
// fields simply won't appear in the response. Extend this list freely; it
// doesn't change the parsing (the map adapts to whatever attributes arrive).
//
// Temporarily unused: the diagnostic run omits return_fields entirely (see
// search_documents). We keep this list so it's ready to switch back on once the
// valid field names are confirmed against a known-good response.
#[allow(dead_code)]
const RETURN_FIELDS: &[&str] = &[
    "docno",
    "title",
    "revision",
    "revisiondate",
    "discipline",
    "doctype",
    "filename",
    "filesize",
    "filetype",
    "current",
    "status",
    "statusid",
    "author",
    "category",
    "registered",
    "modifiedby",
    "versionnumber",
    "confidential",
];

const PAGE_SIZE: u32 = 500;

// --- A single document ---------------------------------------------------

/// One document from the register search.
///
/// `attributes` holds every XML attribute on the <Document> element, keyed by
/// attribute name (e.g. "DocumentNumber", "Title", "Revision"). This is the
/// "flexible map" — no per-field struct. Use `get()` to read a value, which
/// returns Option because any given attribute may or may not be present.
#[derive(Debug, Clone)]
pub struct Document {
    pub attributes: HashMap<String, String>,
}

impl Document {
    /// Convenience reader: returns the attribute value if present.
    /// e.g. `doc.get("DocumentNumber")`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(|s| s.as_str())
    }
}

// --- Parsing the response ------------------------------------------------
//
// PARSING NOTE — why this isn't a plain #[derive(Deserialize)]:
// In JSON, "capture all fields into a map" is a one-line serde flatten. XML is
// different: quick-xml can't easily deserialize "all unknown attributes" into a
// HashMap via derive. So we deserialize the document structure with serde to
// get the page metadata and the list of raw <Document> nodes, but capture each
// document's attributes ourselves. quick-xml represents attributes specially,
// so we use a small typed shape that serde CAN handle, then convert.
//
// We model the response envelope (RegisterSearch > SearchResults > Document),
// where each Document is captured via serde's ability to collect attributes
// into a map when the element has no children we care about. If the real
// response turns out to use child ELEMENTS instead of attributes, this is the
// spot that changes — and the first live run will tell us which it is.

/// The root <RegisterSearch TotalPages="N" TotalResults="M"> envelope.
#[derive(Debug, Deserialize)]
struct RegisterSearch {
    #[serde(rename = "@TotalPages", default)]
    total_pages: u32,

    #[serde(rename = "@TotalResults", default)]
    #[allow(dead_code)] // surfaced for completeness; not used by the loop directly
    total_results: u32,

    #[serde(rename = "SearchResults", default)]
    search_results: Option<SearchResults>,
}

#[derive(Debug, Deserialize)]
struct SearchResults {
    // Each <Document>'s attributes are collected into a map by serde via the
    // special "$value"/attribute handling. We capture them as a generic map
    // keyed by attribute name. `default` makes an absent/empty result an empty
    // Vec rather than an error (same trick as projects).
    #[serde(rename = "Document", default)]
    documents: Vec<RawDocument>,
}

/// quick-xml maps element attributes into struct fields when named with '@'.
/// Because we don't know all field names ahead of time, we lean on serde's
/// flatten-into-map for the attributes. quick-xml surfaces attributes to serde
/// such that a flattened map collects them.
#[derive(Debug, Deserialize)]
struct RawDocument {
    #[serde(flatten)]
    attributes: HashMap<String, String>,
}

impl From<RawDocument> for Document {
    fn from(raw: RawDocument) -> Self {
        // quick-xml prefixes attribute keys with '@' in the flattened map.
        // Strip it so callers use clean names: doc.get("DocumentNumber"),
        // not doc.get("@DocumentNumber").
        let attributes = raw
            .attributes
            .into_iter()
            .map(|(k, v)| (k.trim_start_matches('@').to_string(), v))
            .collect();
        Document { attributes }
    }
}

// --- The client method ---------------------------------------------------

impl Client {
    /// Searches the document register for `query`, fetching ALL pages and
    /// returning every matching document.
    ///
    /// `project` is an already-resolved Project (call get_project first).
    /// `query` is the Aconex search expression, same syntax the Python passes
    /// straight through (e.g. a doc number, or a field expression).
    pub async fn search_documents(
        &self,
        project: &Project,
        query: &str,
    ) -> Result<Vec<Document>> {
        let mut all_docs: Vec<Document> = Vec::new();
        let mut page_number: u32 = 1;
        let mut total_pages: u32 = 1; // updated from the first response

        // DIAGNOSTIC: for now we request NO return_fields, so Aconex returns its
        // default field set. This (a) isolates whether the earlier HTTP 400 was
        // caused by an unrecognized field name in our list, and (b) lets the
        // first successful run reveal the real response shape. Once we confirm
        // the shape and the valid field names, we'll pass Some(&fields) here.
        let return_fields: Option<String> = None;

        loop {
            // Fetch one page.
            let page = self
                .search_documents_page(
                    project,
                    query,
                    return_fields.as_deref(),
                    page_number,
                )
                .await?;

            // On the first page, learn how many pages there are in total.
            if page_number == 1 {
                total_pages = page.total_pages.max(1);
            }

            // Convert this page's raw documents into clean Document values.
            if let Some(results) = page.search_results {
                all_docs.extend(results.documents.into_iter().map(Document::from));
            }

            // Stop once we've fetched the last page.
            if page_number >= total_pages {
                break;
            }
            page_number += 1;
        }

        Ok(all_docs)
    }

    /// Fetches a SINGLE page of register results and returns the parsed
    /// envelope. Private helper for the auto-paginating method above; kept
    /// separate so the pagination loop stays readable and so we could expose a
    /// page-at-a-time variant later without restructuring.
    async fn search_documents_page(
        &self,
        project: &Project,
        query: &str,
        return_fields: Option<&str>,
        page_number: u32,
    ) -> Result<RegisterSearch> {
        // Build the base query string. We URL-encode values to be safe; the
        // Aconex query itself can contain spaces and punctuation.
        let mut path = format!(
            "/api/projects/{id}/register?search_query={q}&search_type=PAGED&page_size={size}&page_number={page}",
            id = project.project_id,
            q = urlencode(query),
            size = PAGE_SIZE,
            page = page_number,
        );

        // return_fields is OPTIONAL. Omitting it makes Aconex return its default
        // field set — useful for isolating a 400 caused by an unrecognized field
        // name, and for discovering the real response shape on the first run.
        if let Some(fields) = return_fields {
            path.push_str(&format!("&return_fields={}", urlencode(fields)));
        }

        let body = self.get_text(&path).await?;

        // Deserialize the envelope. If parsing fails, surface a clear message —
        // this is also exactly where we'll learn whether documents come back as
        // attributes (as assumed) or child elements (which would need a tweak).
        let parsed: RegisterSearch = quick_xml::de::from_str(&body)
            .map_err(|e| AconexError::Http(format!("parsing register search XML: {e}")))?;

        Ok(parsed)
    }
}

// --- Tiny URL-encoder ----------------------------------------------------
//
// Minimal percent-encoding for query-string values, so we don't pull in a
// whole crate just for this. Encodes anything that isn't an unreserved URL
// char. Good enough for search queries; we can swap in a dedicated crate later
// if we need full correctness.
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
