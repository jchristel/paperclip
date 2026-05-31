// src/log.rs
// Writes a CSV log of skipped files during a binder run.
//
// Log location:
//   Mode 1 (folder-based): root input folder containing the source PDFs
//
// Columns:
//   timestamp  — date and time the file was processed
//   filename   — name of the skipped file
//   reason     — reason for skipping

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

// --- Reason enum ---------------------------------------------------------

/// All possible reasons a file can be skipped.
/// Adding a new skip reason means adding a variant here — the compiler
/// will then flag any match that doesn't handle it.
/// This is safer than passing raw strings around.
#[derive(Debug)]
pub enum SkipReason {
    /// Filename does not start with a 5-part code block
    InvalidFilenameFormat,
    /// No revision found in brackets in filename
    MissingRevision,
    /// File could not be opened or parsed by lopdf
    Unreadable(String),
    /// File matched no mapper rows
    NoMapperMatch,
}

impl SkipReason {
    /// Converts the reason to the string written in the CSV.
    /// Like ToString() in C#.
    pub fn as_str(&self) -> String {
        match self {
            SkipReason::InvalidFilenameFormat => "invalid_filename_format".to_string(),
            SkipReason::MissingRevision       => "missing_revision".to_string(),
            SkipReason::Unreadable(msg)       => format!("unreadable: {}", msg),
            SkipReason::NoMapperMatch         => "no_mapper_match".to_string(),
        }
    }
}

// --- Log entry -----------------------------------------------------------

#[derive(Debug)]
pub struct SkipEntry {
    pub filename: String,
    pub reason: SkipReason,
}

// --- Logger --------------------------------------------------------------

/// Accumulates skip entries during a run, then writes them to CSV at the end.
/// Collecting first and writing once avoids repeatedly opening/closing the file.
pub struct RunLog {
    entries: Vec<SkipEntry>,
    /// The folder where the log CSV will be written
    output_dir: PathBuf,
}

impl RunLog {
    /// Creates a new empty log targeting the given output directory.
    pub fn new(output_dir: &Path) -> Self {
        RunLog {
            entries: Vec::new(),
            output_dir: output_dir.to_path_buf(),
        }
    }

    /// Records a skipped file.
    pub fn skip(&mut self, filename: &str, reason: SkipReason) {
        self.entries.push(SkipEntry {
            filename: filename.to_string(),
            reason,
        });
    }

    /// Returns true if any entries were recorded.
    pub fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Writes all recorded entries to a timestamped CSV file.
    /// Does nothing if there are no entries.
    pub fn write(&self) -> Result<()> {
        if self.entries.is_empty() {
            return Ok(());
        }

        // Build a timestamped filename e.g. "paperclip_log_20260526_143200.csv"
        // chrono gives us the current local time — like DateTime.Now in C#
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let log_filename = format!("paperclip_log_{}.csv", timestamp);
        let log_path = self.output_dir.join(log_filename);

        let mut writer = csv::Writer::from_path(&log_path)
            .context("Failed to create log CSV file")?;

        // Write header row
        writer.write_record(&["timestamp", "filename", "reason"])
            .context("Failed to write log CSV header")?;

        // Write one row per entry
        // chrono::Local::now() called per entry so each row gets its own timestamp
        for entry in &self.entries {
            writer.write_record(&[
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                entry.filename.clone(),
                entry.reason.as_str(),
            ])
            .context("Failed to write log CSV row")?;
        }

        writer.flush().context("Failed to flush log CSV")?;

        println!("\nLog written to: {}", log_path.display());
        Ok(())
    }
}