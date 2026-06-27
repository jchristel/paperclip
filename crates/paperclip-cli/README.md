# paperclip (CLI)

The binder command-line tool: combines multiple source PDFs into named binder
PDFs, driven by a mapper CSV. Also hosts the Aconex API commands. Built in Rust,
installable per-user without admin rights. Produces `paperclip.exe`.

This crate is the `paperclip` binary within the workspace. Build and run it from
the workspace root with `cargo run -p paperclip -- <args>`.

---

## What the tool does

Two areas of functionality:

**Binder assembly.** Discovers PDFs, classifies each one, matches them to
binders, merges the matched PDFs into a single binder per `binder_name`, and
embeds a self-describing manifest (compressed XMP metadata) into each binder.
Binders can be **created** from a local folder or from Aconex search results,
and **updated** in place when a source document's revision changes — an update
rebuilds only the binders whose contents actually changed, preserving the
original scope (no documents added or removed).

**Aconex client.** Authenticates against the Aconex API and provides commands to
list/resolve projects, search the document register, download documents, and run
low-level diagnostic probes. These are thin wrappers over the `aconex` library
crate.

---

## Module structure

```
crates/paperclip-cli/
└── src/
    ├── main.rs            # Entry point, CLI argument parsing (clap); async main
    ├── console.rs         # Enables ANSI colour output on Windows consoles
    ├── settings.rs        # Global config (TOML) + Windows Credential Manager
    ├── project_config.rs  # Project-local paperclip.toml load + merge over global
    ├── validator.rs       # Input validation (mapper path check, file creation prompt)
    ├── binder.rs          # Binder command entry points, PDF discovery, orchestration
    ├── pdf_classifier.rs  # Classifies PDFs as Regular / Binder / Unreadable / TooLarge
    ├── mapper.rs          # Mapper CSV load, validate, match PDFs to binders
    ├── filename_parser.rs # Lenient filename parsing + doc-number mask compiler
    ├── manifest.rs        # Binder manifest data model + JSON (serde)
    ├── xmp.rs             # Wraps manifest as compressed XMP, attaches to / reads from PDF
    ├── assembler.rs       # Merges source PDFs into a binder; create + in-place rebuild
    ├── update_check.rs    # Revision-comparison core for binder updates
    ├── inspect.rs         # Reads & prints a binder's manifest; rename detection
    ├── log.rs             # Writes skip/flag log CSV
    ├── aconex_cmd.rs      # Real Aconex commands: typed project + register-search ops
    └── aconex_diag.rs     # Low-level Aconex diagnostic probes (raw XML, schema)
```

(`install.ps1` lives at the workspace root, not in this crate.)

---

## Crate dependencies

| Crate | Purpose |
|---|---|
| `clap` | CLI argument parsing |
| `serde` + `toml` | Config file serialisation |
| `dirs` | OS-standard config path (`%APPDATA%`) |
| `windows` | Direct Win32 Credential Manager API |
| `rpassword` | Secure password / app-key prompt (keystrokes hidden) |
| `lopdf` | PDF reading, writing, merging |
| `csv` | Mapper CSV and log CSV read/write |
| `regex` | Filename pattern matching + doc-number mask |
| `walkdir` | Recursive directory traversal (PDF discovery) |
| `rayon` | Parallel PDF classification (`par_iter`) |
| `indicatif` | Progress bar during PDF classification |
| `chrono` | Timestamps for log CSV and manifest |
| `anyhow` | Ergonomic error handling |
| `serde_json` | Manifest JSON serialisation |
| `uuid` | Binder UUID generation (`binder_id`) |
| `flate2` | Deflate compression for the XMP manifest stream |
| `colored` | Coloured terminal output (flags, errors) |
| `aconex` | The workspace's Aconex client library (path dependency) |
| `tokio` | Async runtime — `main` is async to drive Aconex calls |
| `tempfile` | Auto-cleaned temp dir for Aconex downloads (create/update aconex) |

`serde`, `serde_json`, `anyhow`, `chrono`, and `uuid` are pulled from the
workspace's shared dependency list; the rest are crate-local.

---

## Binder commands

The `binder` command is a 2×2 of action × source:

```
paperclip binder create folder            # build binders from PDFs in the working folder
paperclip binder create aconex            # build binders from Aconex search results
paperclip binder update folder [PATH]     # refresh binders from the working folder
paperclip binder update aconex [PATH]     # refresh binders from current Aconex revisions
```

**Working folder.** Source PDFs (for the folder modes) are always read from the
directory you run paperclip in — run it from inside the project folder. `create
folder` takes no path; both `update folder` and `update aconex` take an optional
PATH naming where the *binders* live (defaults to the working folder). For
`update aconex` the sources come from Aconex rather than the folder, so PATH only
locates the binders to refresh.

**Create** builds binders fresh and writes them to the `output_folder` named in
the mapper CSV. `create folder` uses local PDFs; `create aconex` searches Aconex
once per mapper row (using `prefix` as the query), keeps the PDF results,
downloads them to a temp folder, then runs the same match-and-assemble pipeline.

**Update** reads each existing binder's embedded manifest, compares the revision
of each contained document against the current source, and rebuilds in place only
the binders where something changed. An update never changes a binder's scope —
documents are never added or removed, only refreshed to newer revisions.
Unchanged documents keep their pages and original metadata; changed documents get
the new pages and a fresh timestamp. The binder's stable `binder_id` is preserved
across the rebuild.

The rebuild loads the existing binder once and carries unchanged documents'
pages straight from it (no need for their source files to be present), splicing
in only the changed documents from the source. A document whose source is absent
keeps its existing version, with a note.

For the **aconex** variants, the current revision of each document comes from the
Aconex search result (`Document.revision`), not a local filename. `update aconex`
searches every mapper row once, builds a `document_number → revision` index,
rev-checks each binder against that index, and downloads **only** the documents
whose revision changed — unchanged documents are never fetched. Downloads land in
a temp folder that is auto-cleaned when the run ends. Documents are matched to the
manifest purely by document number; the uploaded filename is used only to confirm
the `.pdf` extension, never as a code or a name.

---

## Project configuration (paperclip.toml)

Settings that vary per project live in an optional `paperclip.toml` in the
working folder, layered over the global config. The global config holds your
Aconex credentials and username (shared across all projects); the project file
holds the per-project bits. A value in `paperclip.toml` overrides the global one;
anything it omits falls back to global.

```toml
# paperclip.toml — sits in the project's working folder
mapper = "mapper.csv"                      # path, relative to this file
project_name = "RHH"                       # Aconex project short name
doc_number_pattern = "AAA-AAA-AA-AAA-NNNNNNN"
```

- **mapper** — path to the mapper CSV. A relative path is resolved against the
  `paperclip.toml`'s own folder, so the project folder stays portable. An
  absolute path also works.
- **project_name** — the Aconex project to operate on, overriding the global
  setting. Lets you switch projects by `cd`-ing between folders rather than
  re-running `config set --project`.
- **doc_number_pattern** — the document-number mask (see below). Optional; if
  absent, a default five-part dash-separated code is assumed.

Credentials are never stored here — they stay global, in Credential Manager.

### Document-number pattern

Describes the shape of the code block at the start of each filename, using a
simple mask:

| Symbol | Matches |
|---|---|
| `A` | a letter (A–Z, a–z) |
| `N` | a digit (0–9) |
| `X` | a letter or digit |
| anything else | that literal character (e.g. `-`) |

Each symbol matches exactly one character, so `AAA` means exactly three letters.
Example: `AAA-AAA-AA-AAA-NNNNNNN` matches `RHH-HDR-AR-DRG-A130001`. The pattern is
anchored at the start of the filename; the rest of the name (title, revision)
follows. A bad pattern stops the run immediately with a clear message rather than
mis-parsing every file.

---

## Aconex commands

All Aconex operations live under the `aconex` subcommand. They authenticate with
the stored username, password, and application key, and operate on the project
named via `config set --project` (or `project_name` in `paperclip.toml`).

### Configuration

```
paperclip config set --username <value>    # your Aconex username
paperclip config set --password            # prompted, hidden → Credential Manager
paperclip config set --app-key             # prompted, hidden → Credential Manager
paperclip config set --project RHH         # project short name to operate on
paperclip config show                      # secrets shown only as "(stored)"
```

### Real commands

```
paperclip aconex projects                       # list every project visible to you
paperclip aconex project                        # resolve the configured project name → id
paperclip aconex search "<query>"               # search the document register (all pages)
paperclip aconex download <document_id> <dest>  # download a document to a local path
```

`search` auto-paginates and prints a one-line summary (document number, revision,
title) per match. The query cannot start with a wildcard (`*`/`?`) — Aconex
rejects that with a server error, so the client refuses it up front.

`download` fetches a single register document by its `DocumentId` (from a search
result) and writes it to the given path.

### Diagnostic commands

The `diag` group holds low-level probes that talk to the API at the raw level:
they print unparsed responses and bypass the typed layer, so they keep working
(and keep showing what's on the wire) even when a typed command breaks on a
response change.

```
paperclip aconex diag ping                            # raw connectivity test
paperclip aconex diag search-raw "<query>" [fields...]  # raw search XML, optional fields
paperclip aconex diag schema                          # raw register schema (custom-field discovery)
```

Quirks learned from the live API (also documented in the `aconex` crate):

- **Field request names are lowercase** (`docno`, `title`, `revisiondate`) and
  differ from the PascalCase response element names (`DocumentNumber`, `Title`,
  `RevisionDate`). Requesting a response-style name returns HTTP 400.
- `search-raw` with **no fields** returns `DocumentId`-only stubs; pass field
  names to populate the rest.
- `page_size` is constrained — 500 is known-good, some smaller values 400.
- `diag schema` discovers custom, project-specific fields beyond the core set
  the typed `search` models.

---

## What is complete

### Settings
- [x] Global TOML config at `%APPDATA%\paper_clip\config.toml`
- [x] Project-local `paperclip.toml` (mapper, project name, doc-number pattern), layered over global
- [x] Password and application key stored in Windows Credential Manager via direct Win32 API
- [x] `paperclip config show` / `config set` for username, user-id, mapper path, project, password, app-key
- [x] Mapper path validation — checks folder exists, offers to create file if missing
- [x] Secure prompts via `rpassword` for password and app key

### Binder commands
- [x] `binder create folder` — assemble binders from the working folder via the mapper CSV
- [x] `binder create aconex` — search Aconex per mapper row, download PDF results, assemble
- [x] `binder update folder [PATH]` — rev-checked rebuild of existing binders, in place
- [x] `binder update aconex [PATH]` — rev-checked rebuild from Aconex; downloads only changed documents
- [x] Recursive PDF discovery (`walkdir`), parallel classification (`rayon`), progress bar
- [x] Oversized-PDF guard (size check before parsing, default 500 MiB)
- [x] Filenames parsed leniently and flagged (not skipped) when they don't match the convention
- [x] Doc-number pattern configurable per project via `paperclip.toml`
- [x] Update preserves scope, `binder_id`, and unchanged documents' metadata; loads the old binder once

### Aconex-sourced binders
- [x] `create aconex` — per-row search, PDF-only filter, dedup by document id, collision-safe temp downloads
- [x] `update aconex` — revision map built from `Document.revision` / `document_number`; changed-only downloads
- [x] Both reuse the existing match/assemble and rev-check/rebuild pipelines (no duplicate assembly logic)
- [x] Downloads land in an auto-cleaned temp folder (`tempfile`); matching is by document number, not filename

### Filename parser
- [x] `parse_lenient` (best-effort): never fails; extracts what it can, reports a `flag_reason`
- [x] Configurable doc-number mask compiled to a regex (`A`/`N`/`X` + literals)
- [x] Revision extracted from `()` or `[]` brackets; optional human-readable name

### Binder manifest (XMP)
- [x] Manifest serialised to JSON, wrapped as XMP, deflate-compressed, attached as `/Metadata`
- [x] `BinderTool` marker in the Info dict as a fast "is this a binder?" check
- [x] Per-file entries: filename, code, revision, name, page range, added-UTC, flag reason
- [x] Read side recovers the manifest for inspect and update

### Inspect command
- [x] `paperclip inspect <file.pdf>` — reads and pretty-prints a binder's manifest
- [x] Rename detection via the stable `binder_id`

### Aconex commands
- [x] Authenticated client from stored credentials
- [x] `aconex projects` / `aconex project` / `aconex search` (typed)
- [x] `aconex download <id> <dest>` — typed document download
- [x] `aconex diag ping` / `search-raw` / `schema` (raw probes)

---

## What is still to do

### Existing-binder overwrite (create)
A `create` run writes `{binder_name}.pdf` to the mapper's output folder, which
overwrites any existing binder there. `update` is now the deliberate in-place
refresh path; `create`'s overwrite behaviour is unchanged and still worth a guard
(refuse / version / refresh) when a binder already exists.

### New / removed documents on update
Update deliberately fixes scope at creation: a source document with no matching
manifest entry (a genuinely new drawing) and a manifest entry with no matching
source (removed / superseded) are both left untouched — the second is reported as
a missing source. If "scope drift" handling is ever wanted, it would be an opt-in
on top of the current rev-only rule, for both folder and aconex update.

### Filename collisions (create aconex)
`create aconex` namespaces a second file that shares a filename with a different
document (by prefixing its document id) and prints a note. A namespaced filename
can then miss its mapper prefix in `match_pdfs`; if real collisions turn out to be
common, matching would need to key on something other than the on-disk name.

### Folder-based binding (no mapper)
When no mapper CSV is configured, offer to bind PDFs by folder (one binder per
folder, depth-limited). Currently stubbed.

### Filename pattern validation per binder
`validate_filename` in `mapper.rs` is still a stub. The doc-number pattern now
covers the code-block shape per project via `paperclip.toml`; a per-binder
pattern check, if still wanted, would build on that.

---

## Known limitations / technical debt

- Cover pages were removed in favour of the XMP manifest. A human-readable
  contents page, if wanted, would be a separate feature.
- The update rebuild clones the loaded old binder once per contiguous run of
  unchanged documents (a memory copy, not a re-parse). Fine for typical binders;
  a true single-document page-graph copy would be leaner for very large binders.
- Oversized-PDF guard is a coarse file-size ceiling, not a streaming parse —
  legitimately large PDFs are skipped wholesale.
- Aconex downloads run sequentially (one request at a time), matching the
  client's request model. For binders with many changed documents this is the
  slow path; it is intentional, not yet optimised.
- Aconex XML parse failures currently surface as the `Http` error variant; a
  dedicated `Parse` variant is planned (see the `aconex` roadmap).
- Colour output relies on the terminal honouring ANSI; old `cmd.exe` may print
  escape codes literally (cosmetic only).

---

## CLI reference

```
paperclip config show
paperclip config set --username <value>
paperclip config set --user-id <value>
paperclip config set --mapper-path <path>
paperclip config set --project <short name>
paperclip config set --password          # prompts securely, no echo
paperclip config set --app-key           # prompts securely, no echo
paperclip binder create folder
paperclip binder create aconex
paperclip binder update folder [path]
paperclip binder update aconex [path]
paperclip inspect <file.pdf>             # print a binder's manifest + rename check
paperclip aconex projects                # list visible projects
paperclip aconex project                 # resolve configured project → id
paperclip aconex search "<query>"        # search the document register
paperclip aconex download <document_id> <dest>
paperclip aconex diag ping               # raw connectivity test
paperclip aconex diag search-raw "<query>" [fields...]
paperclip aconex diag schema             # raw register schema
paperclip --help
```

When running through Cargo from the workspace root, prefix with `cargo run -p
paperclip --` — e.g. `cargo run -p paperclip -- aconex search "RHH-..."`.

---

## File locations

| Item | Location |
|---|---|
| Global config | `%APPDATA%\paper_clip\config.toml` |
| Project config | `paperclip.toml` in the working folder |
| Password | Windows Credential Manager as `paper_clip/aconex_password` |
| Application key | Windows Credential Manager as `paper_clip/aconex_app_key` |
| Install folder | `%LOCALAPPDATA%\paperclip` (default) |
| Skip/flag log | Working/scan directory, timestamped |
| Binder output (create) | Per `output_folder` column in mapper CSV |
| Binder manifest | Embedded in each binder PDF (compressed XMP `/Metadata` stream) |
