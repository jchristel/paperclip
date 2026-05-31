# Paperclip — Project Progress Summary

## What the Tool Does

A Windows CLI tool that combines multiple PDFs into named binder PDFs, driven by a
mapper CSV file. Built in Rust, installable per-user without admin rights.

---

## Module Structure

```
paperclip/
├── main.rs               # Entry point, CLI argument parsing (clap)
├── settings.rs           # Config file (TOML) + Windows Credential Manager
├── validator.rs          # Input validation (mapper path check, file creation prompt)
├── binder.rs             # Binder command entry point, PDF discovery, orchestration
├── pdf_classifier.rs     # Classifies PDFs as Regular / Binder / Unreadable
├── mapper.rs             # Mapper CSV load, validate, match PDFs to binders
├── filename_parser.rs    # Validates filenames, extracts revision and name
├── cover_page.rs         # Generates cover pages using lopdf
├── assembler.rs          # Assembles binder PDFs from cover pages + source PDFs
├── log.rs                # Writes skip log CSV
└── install.ps1           # Per-user PowerShell installer (no admin required)
```

---

## Crate Dependencies

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
| `indicatif` | Progress bar during PDF classification |
| `colored` | Colour-coded console UI output (errors red, warnings yellow, success green) |
| `chrono` | Timestamps for log CSV and cover pages |
| `anyhow` | Ergonomic error handling |
| `serde_json` | JSON serialisation (ready for manifest) |
| `uuid` | Binder UUID generation (ready for manifest) |

---

## What Is Complete

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

### Binder Command
- [x] `paperclip binder` — runs from any directory
- [x] Recursively discovers all PDFs from calling directory (via `walkdir` flat iterator, not hand-rolled recursion)
- [x] Graceful exit with message if no PDFs found
- [x] Progress bar during classification (suitable for 2000+ files)
- [x] Classifies each PDF as Regular, Existing Binder, or Unreadable
- [x] Loads mapper CSV path from settings
- [x] Validates all output folders exist before starting
- [x] Matches PDFs to binders by filename prefix
- [x] Reports unmatched PDFs
- [x] Validates filenames against 5-part code structure
- [x] Filters invalid filenames out of binder plan
- [x] Writes skip log CSV to calling directory with timestamp
- [x] Colour-coded console output via `colored` (errors red, warnings/skips yellow, success green)

### Filename Parser
- [x] Validates 5-part alphanumeric dash-separated code block at start of filename
- [x] Extracts revision from `()` or `[]` brackets
- [x] Extracts optional human-readable name between code and revision

### Mapper CSV
- [x] Reads `prefix`, `binder_name`, `output_folder` columns
- [x] Detects conflicting output folders for the same binder name (aborts)
- [x] Detects empty CSV (aborts with message)

### Cover Page
- [x] Generated using `lopdf` directly (no external font files)
- [x] Fields: File Name, Revision, Start Page, End Page, Date/Time
- [x] A4 portrait, Helvetica regular and bold, divider line

### Assembler
- [x] Sorts files by filename ascending within each binder
- [x] Generates cover page per source PDF
- [x] Merges cover pages and source PDF pages into single binder document
- [x] Writes binder PDF to output folder defined in mapper CSV
- [x] Embeds `BinderTool` and `BinderName` keys in PDF Info dictionary
- [x] Future runs correctly identify assembled binders and skip them

### Log CSV
- [x] Timestamped filename: `paperclip_log_YYYYMMDD_HHMMSS.csv`
- [x] Columns: `timestamp`, `filename`, `reason`
- [x] Reasons: `invalid_filename_format`, `missing_revision`, `no_mapper_match`, `unreadable`
- [x] Written to calling directory after each binder run

### Console Output (Colour)
Console UI is colour-coded with the `colored` crate. This covers *user-facing UI
output only* — diagnostic logging is a separate concern (see To Do).

Colour vocabulary (kept deliberately small and consistent):

| Colour | Meaning | Example |
|---|---|---|
| Red | Errors and failures | folder missing, binder write failed |
| Yellow | Warnings and skips | mapper file not found, file skipped |
| Green | Success confirmations | binder written to disk |
| Plain | Neutral UI | binder plan listing, `[y/N]` prompts, summaries |

Usage pattern — import the trait per file, then colour the finished string:
```rust
use colored::Colorize;

// Literal strings:
println!("{}", "\nError: output folders do not exist:".red());

// Interpolated strings — build with format! first, then colour the result.
// (.red() consumes the literal before {} is filled, so format! must run first.)
println!("{}", format!("  Written to: {}", output_path.display()).green());
```

Currently applied in `validator.rs` (folder-missing error), `binder.rs`
(mapper-not-found warning, output-folder error, per-file skip), and
`assembler.rs` (per-binder error, binder-written success).

---

## What Is Still To Do

### High Priority

#### Binder Metadata — XMP Stream (next session)
The current implementation stores only two keys in the PDF Info dictionary:
```
BinderTool = "paperclip/1"
BinderName = "test"
```
This needs to be replaced with a full manifest stored as a **compressed XMP stream**
on the document catalog. The Info dict is deprecated and unsuitable for large binders
(up to ~700 files = ~100KB uncompressed JSON).

The full manifest struct to implement:
```rust
struct FileEntry {
    filename: String,
    revision: String,
    start_page: u32,
    end_page: u32,
    added_utc: String,
}

struct BinderManifest {
    tool: String,
    schema_version: u32,
    binder_id: String,       // UUID — survives renaming
    binder_name: String,
    created_utc: String,
    mapper_rows: Vec<MapperRow>,
    files: Vec<FileEntry>,
}
```

The `BinderTool` marker key should remain in the Info dict as a fast identification
check, but the full manifest moves to XMP.

#### Rename Detection
Once the manifest is in place, detect when a binder has been renamed on disk
and warn the user:
```
Warning: file is named 'old_name' but was generated as 'test' (binder_id abc-123)
```

### Medium Priority

#### Folder-based Binding (no mapper)
When no mapper CSV is configured, offer to bind PDFs by folder:
- One binder per folder
- Contains only PDFs directly inside that folder (not subfolders)
- Subfolders each get their own binder
- Currently stubbed with a `TODO` comment in `binder.rs`
- Now that discovery uses `walkdir`, a depth-limited walk (`.max_depth(1)`) gives the
  "this folder only, not subfolders" behaviour this feature needs

#### Diagnostic Logging (`log` + `env_logger`)
Console colouring (`colored`) currently handles *user-facing UI* only. Separate
from that, diagnostic messages (e.g. "loaded N mapper rows", skip reasons, open
failures) should move to the `log` facade with an `env_logger` backend, which
level-codes output automatically (warn yellow, error red) and supports
`--verbose`/`--quiet` without affecting the UI. Note: `indicatif-log-bridge` may
be needed so log lines don't garble the live progress bar.

#### Filename Pattern Validation
Currently all filenames that pass the 5-part code check are considered valid.
The `validate_filename` stub in `mapper.rs` needs to be implemented with:
- Configurable patterns per binder (not per row)
- Pattern specified in mapper CSV or settings
- Skip validation entirely when no pattern specified
- See stub docstring in `mapper.rs` for full design notes

### Lower Priority

#### Page Replacement Strategy
To be designed once initial binder creation is stable. The manifest's
`start_page`/`end_page` per file is already structured to support this —
it gives a precise map of where each document sits so specific pages can
be located and swapped without re-scanning the whole binder.

#### Aconex API Integration
HTTP client using `reqwest` crate. Credentials are already stored
(username, user_id, password in Credential Manager). Needs:
- OAuth flow (or password auth — OAuth reportedly poorly implemented)
- Document upload endpoint
- Error handling for auth failures

### Known Limitations / Technical Debt

- Cover page decision pending team review — may be removed or made optional
- `serde_json` and `uuid` crates are in `Cargo.toml` but not yet used
  (added in preparation for the XMP manifest work)
- Filename pattern validation is a stub returning `true` for all files
- Folder-based binding path prompts the user but does nothing

---

## CLI Reference

```
paperclip config show
paperclip config set --username <value>
paperclip config set --user-id <value>
paperclip config set --mapper-path <path>
paperclip config set --password          # prompts securely, no echo
paperclip binder                         # run from folder containing PDFs
paperclip --help
paperclip config --help
paperclip config set --help
```

---

## File Locations

| Item | Location |
|---|---|
| Config file | `%APPDATA%\paper_clip\config.toml` |
| Password | Windows Credential Manager as `paper_clip/aconex_password` |
| Install folder | `%LOCALAPPDATA%\paperclip` (default) |
| Skip log | Calling directory, timestamped |
| Binder output | Per `output_folder` column in mapper CSV |
