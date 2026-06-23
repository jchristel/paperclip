// crates/aconex/src/search.rs
//
// Document register search. Mirrors the Python `search`: GET on
//   /api/projects/{id}/register?search_query=...&search_type=PAGED&...
// looping pages until all documents are collected.
//
// Design (confirmed against a real response):
//   * Auto-paginate — read @TotalPages from the first response, loop to the
//     end, return one flat Vec<Document>.
//   * Takes a &Project — caller resolves the project first (get_project).
//   * Each <Document> carries DocumentId as an ATTRIBUTE and the requested
//     fields as CHILD ELEMENTS (PascalCase: DocumentNumber, Title, Revision,
//     ...). We model the fields we need as a typed struct; unrequested or
//     unknown elements are simply ignored by serde.

use serde::Deserialize;

use crate::client::Client;
use crate::error::{AconexError, Result};
use crate::projects::Project;

// --- The return_fields we request ----------------------------------------
//
// IMPORTANT: these are the REQUEST field names (lowercase, e.g. "docno"),
// which differ from the RESPONSE element names (PascalCase, e.g.
// "DocumentNumber"). You ask in one vocabulary and receive another. Aconex
// only populates fields you request here; omit one and its element won't
// appear in the response.
const RETURN_FIELDS: &[&str] = &[
    "docno",        // -> <DocumentNumber>
    "doctype",      // -> <DocumentType>
    "title",        // -> <Title>
    "revision",     // -> <Revision>
    "revisiondate", // -> <RevisionDate>
    "discipline",   // -> <Discipline>
    "filename",     // -> <Filename>
];

const PAGE_SIZE: u32 = 500;

// --- The typed document --------------------------------------------------

/// One document from the register search.
///
/// `DocumentId` is the element's attribute (hence '@'); the rest are child
/// elements. Every field except the id is Option, because a field only appears
/// if it was requested AND has a value — so the struct stays valid whatever
/// subset of RETURN_FIELDS is in play.
#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    #[serde(rename = "@DocumentId")]
    pub document_id: String,

    #[serde(rename = "DocumentNumber", default)]
    pub document_number: Option<String>,

    #[serde(rename = "Title", default)]
    pub title: Option<String>,

    #[serde(rename = "Revision", default)]
    pub revision: Option<String>,

    #[serde(rename = "RevisionDate", default)]
    pub revision_date: Option<String>,

    #[serde(rename = "Discipline", default)]
    pub discipline: Option<String>,

    #[serde(rename = "DocumentType", default)]
    pub document_type: Option<String>,

    #[serde(rename = "Filename", default)]
    pub filename: Option<String>,
    // Note: nested/structured fields like <Attribute1> are intentionally NOT
    // modelled here. serde ignores unmodelled elements, so requesting them does
    // no harm — they just won't be captured. If a structured field is needed
    // later, it gets its own typed sub-struct rather than a String.
}

// --- Response envelope ---------------------------------------------------

/// The root <RegisterSearch TotalPages="N" TotalResults="M" ...> element.
#[derive(Debug, Deserialize)]
struct RegisterSearch {
    #[serde(rename = "@TotalPages", default)]
    total_pages: u32,

    #[serde(rename = "SearchResults", default)]
    search_results: Option<SearchResults>,
}

#[derive(Debug, Deserialize)]
struct SearchResults {
    // Every <Document> child collects into the Vec — one or many, no special
    // casing. `default` makes an empty result an empty Vec, not an error.
    #[serde(rename = "Document", default)]
    documents: Vec<Document>,
}

// --- The client methods --------------------------------------------------

impl Client {
    /// Searches the document register for `query`, fetching ALL pages and
    /// returning every matching document.
    ///
    /// `project` is an already-resolved Project (call get_project first).
    /// `query` is the Aconex search expression. NOTE: Aconex rejects queries
    /// that START with a wildcard (`*`/`?`) — it returns HTTP 500 — so we guard
    /// against that here and fail with a clear message instead.
    pub async fn search_documents(
        &self,
        project: &Project,
        query: &str,
    ) -> Result<Vec<Document>> {
        // Guard: leading wildcards make Aconex 500 (matches its web UI, which
        // refuses them too). Catch it client-side for a clear error + no wasted
        // round-trip.
        if let Some(first) = query.trim_start().chars().next() {
            if first == '*' || first == '?' {
                return Err(AconexError::Http(
                    "search query cannot start with a wildcard (* or ?) — Aconex rejects these".into(),
                ));
            }
        }

        let mut all_docs: Vec<Document> = Vec::new();
        let mut page_number: u32 = 1;
        let mut total_pages: u32 = 1; // learned from the first response

        let return_fields = RETURN_FIELDS.join(",");

        loop {
            let page = self
                .search_documents_page(project, query, &return_fields, page_number)
                .await?;

            if page_number == 1 {
                total_pages = page.total_pages.max(1);
            }

            if let Some(results) = page.search_results {
                all_docs.extend(results.documents);
            }

            if page_number >= total_pages {
                break;
            }
            page_number += 1;
        }

        Ok(all_docs)
    }

    /// Fetches a SINGLE page of register results. Private helper for the
    /// auto-paginating method above.
    async fn search_documents_page(
        &self,
        project: &Project,
        query: &str,
        return_fields: &str,
        page_number: u32,
    ) -> Result<RegisterSearch> {
        let path = format!(
            "/api/projects/{id}/register?search_query={q}&search_type=PAGED&page_size={size}&page_number={page}&return_fields={fields}",
            id = project.project_id,
            q = urlencode(query),
            size = PAGE_SIZE,
            page = page_number,
            fields = urlencode(return_fields),
        );

        let body = self.get_text(&path).await?;

        let parsed: RegisterSearch = quick_xml::de::from_str(&body)
            .map_err(|e| AconexError::Http(format!("parsing register search XML: {e}")))?;

        Ok(parsed)
    }
}

// --- Tiny URL-encoder ----------------------------------------------------

/// Minimal percent-encoding for query-string values.
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

// --- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // A real response shape (from a live diag run), trimmed to two documents.
    const SEARCH_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<RegisterSearch CurrentPage="1" PageSize="500" TotalPages="1" TotalResults="2" TotalResultsOnPage="2">
<SearchResults>
<Document DocumentId="1678716762798514691">
<Discipline>Architectural</Discipline>
<DocumentNumber>RHH-HDR-AR-DRG-A130001</DocumentNumber>
<DocumentType>Drawing</DocumentType>
<Filename>RHH-HDR-AR-DRG-A130001-[B] - GRID SET-OUT PLAN.pdf</Filename>
<Revision>B</Revision>
<RevisionDate>2026-03-30T13:00:00.000Z</RevisionDate>
<Title>GRID SET-OUT PLAN</Title>
</Document>
<Document DocumentId="1678716762798514680">
<Discipline>Architectural</Discipline>
<DocumentNumber>RHH-HDR-AR-DRG-A130001_DWG</DocumentNumber>
<DocumentType>Drawing</DocumentType>
<Filename>RHH-HDR-AR-DRG-A130001_DWG[B] - GRID SET-OUT PLAN.dwg</Filename>
<Revision>B</Revision>
<RevisionDate>2026-03-30T13:00:00.000Z</RevisionDate>
<Title>GRID SET-OUT PLAN</Title>
</Document>
</SearchResults>
</RegisterSearch>"#;

    fn parse(xml: &str) -> RegisterSearch {
        quick_xml::de::from_str(xml).expect("search XML should deserialize")
    }

    #[test]
    fn parses_document_fields() {
        let r = parse(SEARCH_XML);
        assert_eq!(r.total_pages, 1);

        let docs = r.search_results.expect("has results").documents;
        assert_eq!(docs.len(), 2);

        let first = &docs[0];
        // Attribute on the element.
        assert_eq!(first.document_id, "1678716762798514691");
        // Child elements, by their PascalCase response names.
        assert_eq!(first.document_number.as_deref(), Some("RHH-HDR-AR-DRG-A130001"));
        assert_eq!(first.title.as_deref(), Some("GRID SET-OUT PLAN"));
        assert_eq!(first.revision.as_deref(), Some("B"));
        assert_eq!(first.discipline.as_deref(), Some("Architectural"));
        assert_eq!(first.document_type.as_deref(), Some("Drawing"));
    }

    #[test]
    fn missing_fields_become_none() {
        // A document with only the id and a title — every other field absent.
        let xml = r#"<RegisterSearch TotalPages="1"><SearchResults>
            <Document DocumentId="42"><Title>Only Title</Title></Document>
        </SearchResults></RegisterSearch>"#;
        let r = parse(xml);
        let docs = r.search_results.unwrap().documents;
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].document_id, "42");
        assert_eq!(docs[0].title.as_deref(), Some("Only Title"));
        // Unrequested/absent fields are None, not an error.
        assert!(docs[0].revision.is_none());
        assert!(docs[0].filename.is_none());
    }

    #[test]
    fn empty_results_is_empty_vec() {
        let xml = r#"<RegisterSearch TotalPages="1"><SearchResults></SearchResults></RegisterSearch>"#;
        let r = parse(xml);
        assert!(r.search_results.unwrap().documents.is_empty());
    }
}
