# paperclip (CLI)

The binder command-line tool: combines multiple source PDFs into named binder
PDFs, driven by a mapper CSV. Built in Rust, installable per-user without admin
rights. Produces `paperclip.exe`.

This crate is the `paperclip` binary within the workspace. Build and run it from
the workspace root with `cargo run -p paperclip -- <args>`.

---

## What the tool does

Discovers PDFs beneath the calling directory, classifies each one, matches them
to binders by filename prefix using a mapper CSV, merges the matched PDFs into a
single binder per `binder_name`, and embeds a self-describing manifest (as a
compressed XMP metadata stream) into each binder it writes.

---

## Module structure

```
crates/paperclip-cli/
└── src/
    ├── main.rs            # Entry point, CLI argument parsing (clap)
    ├── console.rs         # Enables ANSI colour output on Windows consoles
    ├── settings.rs        # Config file (TOML) + Windows Credential Manager
    ├── validator.rs       # Input validation (mapper path check, file creation prompt)
    ├── binder.rs          # Binder command entry point, PDF discovery, orchestration
    ├── pdf_classifier.rs  # Classifies PDFs as Regular / Binder / Unreadable / TooLarge
    ├── mapper.rs          # Mapper CSV load, validate, match PDFs to binders
    ├── filename_parser.rs # Strict + lenient filename parsing (code, revision, name)
    ├── manifest.rs        # Binder manifest data model + JSON (serde)
    ├── xmp.rs             # Wraps manifest as compressed XMP, attaches to / reads from PDF
    ├── assembler.rs       # Merges source PDFs into a binder + embeds manifest
    ├── inspect.rs         # Reads & prints a binder's manifest; rename detection
    └── log.rs             # Writes skip/flag log CSV
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
| `rpassword` | Secure password prompt (keystrokes hidden) |
| `lopdf` | PDF reading, writing, merging |
| `csv` | Mapper CSV and log CSV read/write |
| `regex` | Filename pattern matching |
| `walkdir` | Recursive directory traversal (PDF discovery) |
| `rayon` | Parallel PDF classification (`par_iter`) |
| `indicatif` | Progress bar during PDF classification |
| `chrono` | Timestamps for log CSV and manifest |
| `anyhow` | Ergonomic error handling |
| `serde_json` | Manifest JSON serialisation |
| `uuid` | Binder UUID generation (`binder_id`) |
| `flate2` | Deflate compression for the XMP manifest stream |
| `colored` | Coloured terminal output (flags, errors) |

`serde`, `serde_json`, `anyhow`, `chrono`, and `uuid` are pulled from the
workspace's shared dependency list; the rest are crate-local.

---

## What is complete

### Settings
- [x] TOML config file at `%APPDATA%\paper_clip\config.toml`
- [x] Fields: `mapper_csv_path`, `username`, `user_id`
- [x] Password stored in Windows Credential Manager via direct Win32 API
- [x] `paperclip config show` — displays all settings, password masked
- [x] `paperclip config set --username`, `--user-id`, `--mapper-path`, `--password`
- [x] Mapper path validation — checks folder exists, offers to create file if missing
- [x] Secure password prompt via `rpassword` (no plaintext in shell history)
- [x] Friendly message when no arguments supplied

### Installer
- [x] PowerShell script (`install.ps1`), no admin rights required
- [x] User-selectable install folder (defaults to `%LOCALAPPDATA%\paperclip`)
- [x] Adds install folder to user-level PATH (`HKCU`, not `HKLM`)
- [x] Broadcasts PATH change so cmd and PowerShell pick it up without reboot

### Binder command
- [x] `paperclip binder` — runs from any directory
- [x] Recursively discovers all PDFs from calling directory (via `walkdir` flat iterator, not hand-rolled recursion)
- [x] Graceful exit with message if no PDFs found
- [x] Progress bar during classification (suitable for 2000+ files)
- [x] Classifies each PDF in parallel (`rayon`) as Regular, Existing Binder, or Unreadable
- [x] Skips oversized PDFs (>500 MiB) by file-size check *before* parsing, so a single huge file can no longer exhaust memory/CPU and freeze the machine (size limit is a tunable constant in `pdf_classifier.rs`)
- [x] Loads mapper CSV path from settings
- [x] Validates all output folders exist before starting
- [x] Matches PDFs to binders by filename prefix
- [x] Reports unmatched PDFs
- [x] Parses filenames (lenient) and flags any that don't match the naming style
- [x] Flagged files are KEPT in the binder, not skipped — the flag is recorded in both the log CSV and the embedded manifest
- [x] Writes skip/flag log CSV to calling directory with timestamp

### Filename parser
- [x] `parse` (strict): validates 5-part alphanumeric dash-separated code block at start
- [x] `parse_lenient` (best-effort): never fails; extracts whatever it can and returns a `flag_reason` describing what's missing — used by the binder run so bad names are flagged, not rejected
- [x] Extracts revision from `()` or `[]` brackets
- [x] Extracts optional human-readable name between code and revision

### Mapper CSV
- [x] Reads `prefix`, `binder_name`, `output_folder` columns
- [x] Detects conflicting output folders for the same binder name (aborts)
- [x] Detects empty CSV (aborts with message)

### Binder manifest (XMP)
- [x] Full manifest serialised to JSON, wrapped as an XMP packet, deflate-compressed (`flate2`), and attached to the document catalog as a `/Metadata` stream
- [x] `BinderTool` marker kept in the Info dict as a fast "is this a binder?" check (classification reads this, not the manifest)
- [x] Manifest fields: `tool`, `schema_version`, `binder_id` (UUID, survives renaming), `binder_name`, `created_utc`, `mapper_rows`, and per-file `files`
- [x] Each `FileEntry`: `filename`, `code`, `revision`, `name`, `start_page`, `end_page`, `added_utc`, `flag_reason` (optional fields are `Option<String>`, omitted from JSON when absent)
- [x] Read side (`xmp::read_manifest_json` + `manifest::from_json`) recovers the manifest from a binder

### Inspect command
- [x] `paperclip inspect <file.pdf>` — reads and pretty-prints a binder's manifest
- [x] Reports cleanly when a PDF has no manifest (regular PDF / pre-manifest binder)
- [x] Rename detection: warns when the on-disk filename differs from the manifest's `binder_name`, citing the stable `binder_id`

### Assembler
- [x] Sorts files by filename ascending within each binder
- [x] Merges source PDF pages into a single binder document (no cover pages; page ranges have no offset)
- [x] Records each file's page range in the manifest as it merges
- [x] Writes binder PDF to output folder defined in mapper CSV
- [x] Embeds `BinderTool` / `BinderName` keys in the Info dict AND the full compressed XMP manifest
- [x] Future runs correctly identify assembled binders (via the Info-dict marker) and skip them

### Log CSV
- [x] Timestamped filename: `paperclip_log_YYYYMMDD_HHMMSS.csv`
- [x] Columns: `timestamp`, `filename`, `reason`
- [x] Reasons: `flagged: <detail>` (kept, not skipped), `no_mapper_match`, `unreadable`, `too_large`
- [x] Legacy reasons `invalid_filename_format` / `missing_revision` still defined but no longer emitted (superseded by `flagged`); retained for the future per-binder pattern check
- [x] Written to calling directory after each binder run

---

## What is still to do

### Medium priority

#### Existing-binder overwrite
A `binder` run writes `{binder_name}.pdf` to the mapper's output folder. When the
input directory and output folder are the same (common in testing), each run
overwrites the previous binder. Decide on behaviour: refresh in place, version,
or refuse when a binder already exists at the target path.

#### Folder-based binding (no mapper)
When no mapper CSV is configured, offer to bind PDFs by folder:
- One binder per folder
- Contains only PDFs directly inside that folder (not subfolders)
- Subfolders each get their own binder
- Currently stubbed with a `TODO` comment in `binder.rs`
- Now that discovery uses `walkdir`, a depth-limited walk (`.max_depth(1)`) gives the
  "this folder only, not subfolders" behaviour this feature needs

#### Filename pattern validation
Currently all filenames that pass the 5-part code check are considered valid.
The `validate_filename` stub in `mapper.rs` needs to be implemented with:
- Configurable patterns per binder (not per row)
- Pattern specified in mapper CSV or settings
- Skip validation entirely when no pattern specified
- See stub docstring in `mapper.rs` for full design notes

### Lower priority

#### Page replacement strategy
To be designed once initial binder creation is stable. The manifest's
`start_page`/`end_page` per file is already structured to support this — it
gives a precise map of where each document sits so specific pages can be located
and swapped without re-scanning the whole binder.

#### Aconex API integration
Handled by the separate [`aconex`](../aconex/README.md) library crate in this
workspace. Credentials are already stored by this CLI (username, user_id,
password in Credential Manager) and will be injected into the client. See that
crate's README for its own roadmap.

---

## Known limitations / technical debt

- Cover pages have been removed entirely in favour of the XMP manifest. If a
  human-readable contents page is wanted later, it would be a separate feature.
- Filename pattern validation is a stub returning `true` for all files.
- Folder-based binding path prompts the user but does nothing.
- The manifest embeds JSON inside a custom XMP element and recovers it by
  tag-slicing — pragmatic and valid as an XMP packet, but not modelled as strict
  RDF properties (fine for paperclip's own round-trip).
- Colour output relies on the terminal honouring ANSI; old `cmd.exe` may not, in
  which case escape codes print literally (cosmetic only).
- Oversized-PDF guard is a coarse file-size ceiling (default 500 MiB), not a true
  streaming/lazy parse — legitimately large PDFs are skipped wholesale rather than
  processed. The same guard should be reused on the binder-read path (a merged
  binder of a large source is itself large). A real fix would read only the
  trailer/Info dict without a full `Document::load`.

---

## CLI reference

```
paperclip config show
paperclip config set --username <value>
paperclip config set --user-id <value>
paperclip config set --mapper-path <path>
paperclip config set --password          # prompts securely, no echo
paperclip binder                         # run from folder containing PDFs
paperclip inspect <file.pdf>             # print a binder's manifest + rename check
paperclip --help
paperclip config --help
paperclip config set --help
```

When running through Cargo from the workspace root, prefix with `cargo run -p
paperclip --` — e.g. `cargo run -p paperclip -- inspect mybinder.pdf`.

---

## File locations

| Item | Location |
|---|---|
| Config file | `%APPDATA%\paper_clip\config.toml` |
| Password | Windows Credential Manager as `paper_clip/aconex_password` |
| Install folder | `%LOCALAPPDATA%\paperclip` (default) |
| Skip/flag log | Calling directory, timestamped |
| Binder output | Per `output_folder` column in mapper CSV |
| Binder manifest | Embedded in each binder PDF (compressed XMP `/Metadata` stream) |
