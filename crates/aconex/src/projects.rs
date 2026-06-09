// crates/aconex/src/projects.rs
//
// Typed model of the Aconex "list projects" response, plus the methods that
// fetch and parse it. This replaces the Python get_projects, which dug through
// xmltodict dicts (["ProjectResults"]["SearchResults"]["Project"]) and had to
// hand-handle the one-vs-many case. Here serde + quick-xml deserialize the XML
// straight into structs, and the one-vs-many problem disappears (see below).
//
// The XML we're modelling (values blanked) looks like:
//   <ProjectResults TotalResults="1">
//     <SearchResults>
//       <Project AccessLevel="NORMAL" Active="true" Hidden="false" ...>
//         <ProjectId>...</ProjectId>
//         <ProjectName>...</ProjectName>
//         <ProjectShortName>...</ProjectShortName>
//         ... (many other elements we ignore)
//       </Project>
//     </SearchResults>
//   </ProjectResults>

use serde::Deserialize;

use crate::client::Client;
use crate::error::{AconexError, Result};

// --- The typed model -----------------------------------------------------
//
// serde maps struct FIELDS to XML by name. Two annotations do the work:
//   #[serde(rename = "Foo")]  → this field corresponds to <Foo> or attribute Foo
//   #[serde(rename = "@Foo")] → the leading '@' means it's an ATTRIBUTE, not a
//                               child element. (quick-xml's convention.)
// Any XML element we DON'T declare a field for is simply ignored — that's why
// we can model just the handful of fields we need and skip the address/fax/etc.

/// The root element: <ProjectResults TotalResults="...">.
#[derive(Debug, Deserialize)]
pub struct ProjectResults {
    // TotalResults is an attribute on the root, hence the '@'.
    #[serde(rename = "@TotalResults")]
    pub total_results: u32,

    // <SearchResults> is a single child element wrapping the projects.
    #[serde(rename = "SearchResults")]
    pub search_results: SearchResults,
}

/// The <SearchResults> wrapper. Its job is just to hold the project(s).
#[derive(Debug, Deserialize)]
pub struct SearchResults {
    // THE ONE-VS-MANY FIX:
    // The Python had to check "is Project a dict (one) or a list (many)?".
    // With serde, declaring this as a Vec means quick-xml collects *every*
    // <Project> child into the vector — whether there's one or twenty. One
    // project yields a Vec of length 1; no special-casing needed.
    //
    // `default` handles the zero case: if there are no <Project> elements at
    // all (an empty result set), the field becomes an empty Vec instead of a
    // deserialization error.
    #[serde(rename = "Project", default)]
    pub projects: Vec<Project>,
}

/// A single <Project>. We model only the fields we actually use; everything
/// else in the XML (postal address, fax, dates, ...) is ignored by serde.
#[derive(Debug, Deserialize, Clone)]
pub struct Project {
    // --- Attributes (note the '@') ---
    #[serde(rename = "@AccessLevel")]
    pub access_level: String,

    // Active comes through as the string "true"/"false" in the XML. We let it
    // deserialize as a bool — serde + quick-xml parse "true"/"false" into a
    // Rust bool automatically.
    #[serde(rename = "@Active")]
    pub active: bool,

    // --- Child elements ---
    // These are the three that matter: the ID every other endpoint needs, and
    // the two names used to find a project. ProjectShortName is what the user
    // stores in config as `project_name` and what we match against.
    #[serde(rename = "ProjectId")]
    pub project_id: String,

    #[serde(rename = "ProjectName")]
    pub project_name: String,

    #[serde(rename = "ProjectShortName")]
    pub project_short_name: String,
}

// --- The client methods --------------------------------------------------
//
// We add methods to Client by writing a *second* `impl Client` block here.
// Rust lets a type's methods be split across multiple impl blocks (even across
// files in the same crate), so the HTTP plumbing stays in client.rs and the
// project-specific logic lives here. This keeps each file focused.

impl Client {
    /// Fetches all projects visible to the authenticated user.
    ///
    /// Calls the same endpoint as `ping` did, but parses the XML into typed
    /// `Project` values instead of returning a raw string.
    pub async fn get_projects(&self) -> Result<Vec<Project>> {
        // Reuse the transport we already proved out.
        let body = self.get_text("/api/projects/").await?;

        // quick-xml's serde entry point: deserialize the whole XML document
        // into our root struct. `from_str` infers the target type from the
        // return annotation (`ProjectResults`).
        let parsed: ProjectResults = quick_xml::de::from_str(&body)
            // Convert quick-xml's error into our own error type so the crate's
            // public API stays free of quick-xml — same discipline as the HTTP
            // errors in client.rs. We'll likely add a dedicated `Parse` variant
            // later; for now reuse Http with a clear prefix.
            .map_err(|e| AconexError::Http(format!("parsing projects XML: {e}")))?;

        // Dig down to the projects Vec and hand it back.
        Ok(parsed.search_results.projects)
    }

    /// Finds a single project by its short name (the value users store in
    /// config as `project_name`). Returns None if no project matches.
    ///
    /// This mirrors the Python get_project(name), which looped projects looking
    /// for a matching ProjectShortName.
    pub async fn get_project(&self, short_name: &str) -> Result<Option<Project>> {
        let projects = self.get_projects().await?;

        // `into_iter().find(...)` walks the Vec and returns the first match.
        // We compare against project_short_name. `find` gives an Option, which
        // is exactly the "maybe found, maybe not" we want to return.
        Ok(projects
            .into_iter()
            .find(|p| p.project_short_name == short_name))
    }
}

// --- Tests ---------------------------------------------------------------
//
// Unit tests live in the same file as the code (a `#[cfg(test)] mod tests`
// block) so they can reach private items and sit next to what they test.
// `#[cfg(test)]` means this whole block is only compiled when running tests —
// it adds nothing to the shipped library. Run with `cargo test -p aconex`.

#[cfg(test)]
mod tests {
    // Pull the parent module's items (Project, ProjectResults, ...) into scope.
    // `super` = the module enclosing this one, i.e. the file's top level.
    use super::*;

    // A trimmed copy of the real response shape, with one project. Values are
    // placeholders — only the structure matters for parsing. We keep a couple
    // of the "ignored" elements (DeliveryCity, FaxNumber) to prove that fields
    // we DIDN'T model are silently skipped rather than causing a failure.
    const ONE_PROJECT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<ProjectResults TotalResults="1">
    <SearchResults>
        <Project AccessLevel="NORMAL" Active="true" DisconnectionStatus="NONE" Hidden="false">
            <DeliveryCity>Sydney</DeliveryCity>
            <FaxNumber>12345</FaxNumber>
            <ProjectId>268454637</ProjectId>
            <ProjectName>Example Project Full Name</ProjectName>
            <ProjectShortName>EXAMPLE</ProjectShortName>
        </Project>
    </SearchResults>
</ProjectResults>"#;

    // Two projects in one response — the case the Python had to special-case
    // (dict vs list). With a Vec field, both "one" and "many" parse uniformly.
    const TWO_PROJECTS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProjectResults TotalResults="2">
    <SearchResults>
        <Project AccessLevel="NORMAL" Active="true" Hidden="false">
            <ProjectId>1</ProjectId>
            <ProjectName>First</ProjectName>
            <ProjectShortName>ONE</ProjectShortName>
        </Project>
        <Project AccessLevel="LIMITED" Active="false" Hidden="false">
            <ProjectId>2</ProjectId>
            <ProjectName>Second</ProjectName>
            <ProjectShortName>TWO</ProjectShortName>
        </Project>
    </SearchResults>
</ProjectResults>"#;

    // An empty result set — <SearchResults> with no <Project> children. The
    // `default` on the projects field should yield an empty Vec, not an error.
    const EMPTY_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProjectResults TotalResults="0">
    <SearchResults></SearchResults>
</ProjectResults>"#;

    // Helper: parse a string into ProjectResults, panicking with a readable
    // message if it fails. Keeps each test to the assertion that matters.
    fn parse(xml: &str) -> ProjectResults {
        quick_xml::de::from_str(xml).expect("XML should deserialize")
    }

    #[test]
    fn parses_single_project() {
        let result = parse(ONE_PROJECT_XML);

        // Root attribute came through.
        assert_eq!(result.total_results, 1);

        // Exactly one project collected into the Vec.
        assert_eq!(result.search_results.projects.len(), 1);

        let p = &result.search_results.projects[0];
        assert_eq!(p.project_id, "268454637");
        assert_eq!(p.project_name, "Example Project Full Name");
        assert_eq!(p.project_short_name, "EXAMPLE");

        // Attributes (the '@' fields) parsed correctly.
        assert_eq!(p.access_level, "NORMAL");
        assert!(p.active); // Active="true" -> bool true
    }

    #[test]
    fn ignores_unmodelled_fields() {
        // DeliveryCity and FaxNumber exist in the XML but aren't fields on
        // Project. Parsing must succeed regardless — proving we can model just
        // the fields we need. (If this failed, the parse() helper would panic.)
        let result = parse(ONE_PROJECT_XML);
        assert_eq!(result.search_results.projects.len(), 1);
    }

    #[test]
    fn parses_multiple_projects() {
        // The one-vs-many case: two <Project> elements both land in the Vec,
        // no special-casing — the thing the Python had to handle manually.
        let result = parse(TWO_PROJECTS_XML);

        assert_eq!(result.total_results, 2);
        assert_eq!(result.search_results.projects.len(), 2);

        assert_eq!(result.search_results.projects[0].project_short_name, "ONE");
        assert_eq!(result.search_results.projects[1].project_short_name, "TWO");

        // The differing Active attributes parsed independently.
        assert!(result.search_results.projects[0].active);   // "true"
        assert!(!result.search_results.projects[1].active);  // "false"
    }

    #[test]
    fn parses_empty_result_set() {
        // No <Project> children -> empty Vec via #[serde(default)], not a
        // deserialization error.
        let result = parse(EMPTY_XML);

        assert_eq!(result.total_results, 0);
        assert!(result.search_results.projects.is_empty());
    }
}