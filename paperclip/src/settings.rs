// src/settings.rs

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// --- Constants -----------------------------------------------------------

const APP_NAME: &str = "paper_clip";      // folder name under %APPDATA%
const CONFIG_FILE: &str = "config.toml";   // filename inside that folder

// --- Struct --------------------------------------------------------------

// Derive macros auto-implement serialise/deserialise (like [JsonSerializable])
// and Debug lets us print it with {:?}
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Settings {
    pub mapper_csv_path: Option<String>,  // Option<T> = nullable in C# — None means not set yet
    pub username: Option<String>,
    pub user_id: Option<String>,
    // password is NOT stored here — it goes to Credential Manager
}

// --- Path helper ---------------------------------------------------------

/// Returns the path to the config file, e.g.:
/// C:\Users\alice\AppData\Roaming\paper_clip\config.toml
/// 
/// -> Result<T> is Rust's equivalent of either returning T or throwing.
/// The caller must handle both cases.
pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir()               // %APPDATA% on Windows
        .context("Could not locate %APPDATA%")?; // ? unwraps Ok, or returns Err early (like `throw` in C#)
    Ok(base.join(APP_NAME).join(CONFIG_FILE))
}

// --- Load ----------------------------------------------------------------

/// Reads config.toml from disk and deserialises it into a Settings struct.
/// If the file doesn't exist yet, returns a default (all None) Settings.
pub fn load() -> Result<Settings> {
    let path = config_path()?;

    if !path.exists() {
        // No file yet — return empty settings, not an error
        return Ok(Settings::default());
    }

    let content = fs::read_to_string(&path)
        .context("Failed to read config file")?;

    let settings: Settings = toml::from_str(&content)
        .context("Config file is not valid TOML")?;

    Ok(settings)
}

// --- Save ----------------------------------------------------------------

/// Serialises Settings to TOML and writes it to disk.
/// Creates the directory if it doesn't exist.
pub fn save(settings: &Settings) -> Result<()> {
    let path = config_path()?;

    // Create the directory if needed — like Directory.CreateDirectory() in C#
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .context("Failed to create config directory")?;
    }

    let content = toml::to_string_pretty(settings)
        .context("Failed to serialise settings")?;

    fs::write(&path, content)
        .context("Failed to write config file")?;

    Ok(())
}

// --- Password (Windows Credential Manager — direct API) ------------------

use windows::core::PWSTR;
use windows::Win32::Security::Credentials::{
    CredFree, CredReadW, CredWriteW,
    CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

// The "target name" is how Windows identifies the credential entry.
// It will appear exactly as this string in Credential Manager.
const CRED_TARGET: &str = "paper_clip/aconex_password";

/// Encode a Rust &str to a null-terminated UTF-16 Vec — required by Windows W (wide) APIs.
/// In C# this is handled automatically; in Rust we do it manually.
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Saves the password to Windows Credential Manager.
pub fn save_password(password: &str) -> Result<()> {
    let target = to_wide(CRED_TARGET);
    let mut blob: Vec<u8> = password.encode_utf16()
        .flat_map(|c| c.to_le_bytes())   // store as UTF-16LE bytes, same as Windows expects
        .collect();

    let mut cred = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_ptr() as *mut u16),
        Comment: PWSTR::null(),
        LastWritten: Default::default(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,  // survives reboot, stored per-user
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR::null(),
    };

    // CredWriteW returns a Win32 BOOL — unsafe because it's a raw FFI call
    unsafe {
        CredWriteW(&mut cred, 0).ok()  // .ok() converts Win32 BOOL error to Result
            .context("CredWriteW failed — could not save to Credential Manager")?;
    }
    Ok(())
}

/// Retrieves the password from Windows Credential Manager.
/// Returns None if no credential has been stored yet.
pub fn load_password() -> Result<Option<String>> {
    let target = to_wide(CRED_TARGET);
    let mut pcred: *mut CREDENTIALW = std::ptr::null_mut();

    let found = unsafe {
        CredReadW(
            PWSTR(target.as_ptr() as *mut u16),
            CRED_TYPE_GENERIC,
            0,
            &mut pcred,
        )
    };

    if found.is_err() {
        // ERROR_NOT_FOUND means no entry yet — not a crash
        return Ok(None);
    }

    // SAFETY: CredReadW succeeded so pcred is valid; we free it below with CredFree
    let password = unsafe {
        let cred = &*pcred;
        let blob_ptr = cred.CredentialBlob as *const u16;
        let blob_len = cred.CredentialBlobSize as usize / 2;  // bytes -> u16 count
        let wide_slice = std::slice::from_raw_parts(blob_ptr, blob_len);
        let result = String::from_utf16(wide_slice)
            .context("Credential blob is not valid UTF-16")?;
        CredFree(pcred as *mut _);  // must free memory allocated by Windows
        result
    };

    Ok(Some(password))
}