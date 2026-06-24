// crates/aconex/src/download.rs
//
// Document download. Mirrors the Python `download`:
//   GET /api/projects/{id}/register/{document_id}/
// then streams the (binary) body to disk under the caller-supplied path.
//
// Design (confirmed against the Python and your chosen options):
//   * Caller passes the EXACT output path (like the Python's `filename` arg).
//   * Returns the path it wrote to, so callers can print/echo it.
//   * Binary transport lives in Client::get_bytes (see client.rs); this file
//     is just "ask for the bytes, write them where asked".
//
// Note on streaming: the Python streamed in 8 KiB chunks straight to disk. We
// read the whole body into memory first (Client::get_bytes), then write it in
// one go. For register documents that's fine; if you later need to handle
// multi-GB files without buffering, that's a `get_bytes`-level change (return
// a stream), not a change here.

use std::path::{Path, PathBuf};

use crate::client::Client;
use crate::error::{AconexError, Result};
use crate::projects::Project;

impl Client {
    /// Downloads the document with id `document_id` and writes it to `dest`.
    ///
    /// `project` is an already-resolved Project (call get_project first), the
    /// same as search_documents. `dest` is the exact path to write — the
    /// caller decides the filename, mirroring the Python.
    ///
    /// Returns the path written on success (an owned PathBuf), so the CLI can
    /// report where the file landed.
    ///
    /// `impl AsRef<Path>` lets callers pass a `&str`, `String`, `&Path`, or
    /// `PathBuf` — the same ergonomic trick as `impl Into<String>` elsewhere in
    /// the crate, but for filesystem paths.
    pub async fn download_document(
        &self,
        project: &Project,
        document_id: &str,
        dest: impl AsRef<Path>,
    ) -> Result<PathBuf> {
        let dest = dest.as_ref();

        // Build the register-document path. The trailing slash matters: the
        // Python hits `.../register/{id}/` (with it), and Aconex is picky.
        let path = format!(
            "/api/projects/{id}/register/{doc}/",
            id = project.project_id,
            doc = document_id,
        );

        // Fetch the raw bytes. We ignore the server's suggested filename here
        // because the caller gave us an explicit destination; it's still
        // available on the response if a future caller wants it.
        let response = self.get_bytes(&path).await?;

        // Write the bytes to disk. `std::fs::write` creates or truncates the
        // file and writes the whole buffer in one call — the std-library
        // equivalent of `open(path, "wb").write(bytes)` in Python.
        //
        // `map_err` converts std::io::Error into our AconexError so the crate's
        // public API stays free of std::io — callers only ever match on
        // AconexError, same discipline as the HTTP errors.
        std::fs::write(dest, &response.bytes).map_err(|e| {
            AconexError::Http(format!("writing {} to disk: {}", dest.display(), e))
        })?;

        // Hand back the path we wrote, as an owned PathBuf the caller keeps.
        Ok(dest.to_path_buf())
    }
}