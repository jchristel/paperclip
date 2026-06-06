// src/aconex_cmd.rs
// Bridges stored CLI credentials into the aconex library and makes a test call.

use anyhow::{Context, Result};

/// Reads the three stored credentials, builds an authenticated client, and
/// hits the projects endpoint to prove the chain works. Prints the raw body
/// (XML) for now — typed parsing comes next.
pub async fn ping() -> Result<()> {
    // Gather credentials from settings + Credential Manager.
    let config = crate::settings::load()?;

    let username = config.username
        .context("No username set — run `paperclip config set --username`")?;
    let password = crate::settings::load_password()?
        .context("No password stored — run `paperclip config set --password`")?;
    let app_key = crate::settings::load_app_key()?
        .context("No app key stored — run `paperclip config set --app-key`")?;

    // Build the authenticator and client. The `?` here converts a
    // aconex::AconexError into anyhow::Error automatically (anyhow absorbs any
    // std error), which is exactly why the CLI uses anyhow and the library
    // uses a typed error.
    let auth = aconex::BasicAuth::new(username, password, app_key)?;
    let client = aconex::Client::new(auth);

    println!("Calling Aconex...");

    // The projects list endpoint — same path the Python uses.
    let body = client.get_text("/api/projects/").await?;

    // For now, just show what came back. Trim so we don't dump megabytes.
    let preview: String = body.chars().take(2000).collect();
    println!("\n--- Response (first 2000 chars) ---\n{}", preview);

    Ok(())
}