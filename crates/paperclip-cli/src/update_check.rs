// src/update_check.rs
//
// The revision-comparison core shared by the binder UPDATE paths.
//
// An update never changes a binder's SCOPE (which documents it contains —
// that's fixed at creation). It only asks, per document already in the binder:
// is a newer revision available in the source? So the unit of work is:
//
//   given a binder's manifest (the recorded code→revision of what it holds)
//   and the CURRENT source revisions (code→revision available now),
//   decide whether the binder is stale (any in-scope document's rev differs).
//
// "Differs" is plain string inequality. We deliberately do NOT try to order
// revisions (is "C" newer than "B"? is "Rev2" newer than "Rev1"?) — revision
// schemes vary and aren't reliably orderable, so any difference is treated as
// "the source moved on, rebuild". This matches the rule agreed for folder
// update: rev-differs is the whole test.
//
// Documents are matched between manifest and source by their `code` (the
// parsed document number). Entries with no code can't be matched and are
// reported separately rather than silently ignored.

use std::collections::HashMap;

use crate::manifest::BinderManifest;

/// The outcome of checking one binder against the current sources.
#[derive(Debug, Default)]
pub struct UpdateReport {
    /// Documents whose source revision differs from the binder's recorded one.
    /// Each is (code, old_revision, new_revision) for human-readable reporting.
    /// `old`/`new` are Strings (revisions may be absent → shown as "?").
    pub changed: Vec<(String, String, String)>,

    /// In-scope documents (by code) that have NO matching source file present
    /// now. The source for a scoped document has gone missing. We can't refresh
    /// what isn't there; reported so the user knows the binder may be stale.
    pub missing_sources: Vec<String>,

    /// Manifest entries that had no `code` at all, so they couldn't be matched.
    /// Carries the filename for reporting. Shouldn't happen for clean binders.
    pub uncodeable: Vec<String>,
}

impl UpdateReport {
    /// True if any in-scope document has a newer/different revision available.
    /// This is the gate for "rebuild this binder". Missing sources alone do NOT
    /// trigger a rebuild (there's nothing newer to pull in); they're a warning.
    pub fn needs_rebuild(&self) -> bool {
        !self.changed.is_empty()
    }
}

/// Compares a binder's manifest against the current source revisions.
///
/// `source_revisions` maps a document's `code` to its CURRENT revision parsed
/// from the source file present now (e.g. "RHH-...-A130001" -> "C"). A `None`
/// value means a source file exists but its revision couldn't be parsed.
///
/// Returns an `UpdateReport`: which in-scope documents changed, which have no
/// source present, and which manifest entries lacked a code.
pub fn check_binder(
    manifest: &BinderManifest,
    source_revisions: &HashMap<String, Option<String>>,
) -> UpdateReport {
    let mut report = UpdateReport::default();

    for entry in &manifest.files {
        // The manifest entry must have a code to be matchable. Without one we
        // can't line it up against a source; record and move on.
        let code = match entry.code.as_deref() {
            Some(c) => c,
            None => {
                report.uncodeable.push(entry.filename.clone());
                continue;
            }
        };

        // Is there a current source for this code?
        match source_revisions.get(code) {
            None => {
                // The binder contains this document, but no source for it is
                // present now. Can't refresh it; flag as missing.
                report.missing_sources.push(code.to_string());
            }
            Some(source_rev) => {
                // Compare the recorded revision against the current one.
                // Both are Option<String>; absent shows as "?" in the report.
                let old = entry.revision.as_deref().unwrap_or("?");
                let new = source_rev.as_deref().unwrap_or("?");

                if old != new {
                    report.changed.push((
                        code.to_string(),
                        old.to_string(),
                        new.to_string(),
                    ));
                }
            }
        }
    }

    report
}

// --- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{BinderManifest, FileEntry};

    // Build a minimal manifest with the given (code, revision) documents.
    // Other FileEntry fields are filled with throwaway values — the rev check
    // only looks at `code` and `revision`.
    fn manifest_with(docs: &[(&str, &str)]) -> BinderManifest {
        let files = docs
            .iter()
            .map(|(code, rev)| FileEntry {
                filename: format!("{code}.pdf"),
                code: Some(code.to_string()),
                revision: Some(rev.to_string()),
                name: None,
                start_page: 1,
                end_page: 1,
                added_utc: "2026-01-01T00:00:00Z".to_string(),
                flag_reason: None,
            })
            .collect();

        BinderManifest {
            tool: "paperclip/1".to_string(),
            schema_version: 1,
            binder_id: "test".to_string(),
            binder_name: "TEST".to_string(),
            created_utc: "2026-01-01T00:00:00Z".to_string(),
            mapper_rows: Vec::new(),
            files,
        }
    }

    fn sources(pairs: &[(&str, &str)]) -> HashMap<String, Option<String>> {
        pairs
            .iter()
            .map(|(code, rev)| (code.to_string(), Some(rev.to_string())))
            .collect()
    }

    #[test]
    fn unchanged_binder_needs_no_rebuild() {
        let m = manifest_with(&[("DOC-1", "B"), ("DOC-2", "A")]);
        let s = sources(&[("DOC-1", "B"), ("DOC-2", "A")]);
        let report = check_binder(&m, &s);
        assert!(!report.needs_rebuild());
        assert!(report.changed.is_empty());
        assert!(report.missing_sources.is_empty());
    }

    #[test]
    fn changed_revision_triggers_rebuild() {
        let m = manifest_with(&[("DOC-1", "B")]);
        let s = sources(&[("DOC-1", "C")]); // newer rev present
        let report = check_binder(&m, &s);
        assert!(report.needs_rebuild());
        assert_eq!(report.changed.len(), 1);
        assert_eq!(report.changed[0], ("DOC-1".to_string(), "B".to_string(), "C".to_string()));
    }

    #[test]
    fn missing_source_is_reported_but_not_a_rebuild() {
        let m = manifest_with(&[("DOC-1", "B")]);
        let s = sources(&[]); // no source for DOC-1 present now
        let report = check_binder(&m, &s);
        assert!(!report.needs_rebuild());
        assert_eq!(report.missing_sources, vec!["DOC-1".to_string()]);
    }

    #[test]
    fn new_source_not_in_scope_is_ignored() {
        // Source has an extra document the binder never contained. Scope is
        // fixed at creation, so it must NOT trigger anything.
        let m = manifest_with(&[("DOC-1", "B")]);
        let s = sources(&[("DOC-1", "B"), ("DOC-99", "A")]);
        let report = check_binder(&m, &s);
        assert!(!report.needs_rebuild());
        assert!(report.changed.is_empty());
    }
}