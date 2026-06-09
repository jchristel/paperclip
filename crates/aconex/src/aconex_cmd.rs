// src/aconex_cmd.rs
// Bridges stored CLI credentials into the aconex library and makes API calls.

use anyhow::{Context, Result};

/// Builds an authenticated client from the three stored credentials.
/// Factored out so every command (ping, projects, ...) reuses the same setup
/// instead of repeating the credential-gathering each time.
fn build_client() -> Result<aconex::Client> {
    let config = crate::settings::load()?;

    let username = config
        .username
        .context("No username set — run `paperclip config set --username`")?;
    let password = crate::settings::load_password()?
        .context("No password stored — run `paperclip config set --password`")?;
    let app_key = crate::settings::load_app_key()?
        .context("No app key stored — run `paperclip config set --app-key`")?;

    // The `?` converts aconex::AconexError into anyhow::Error automatically
    // (anyhow absorbs any std error) — the reason the CLI uses anyhow while the
    // library uses a typed error.
    let auth = aconex::BasicAuth::new(username, password, app_key)?;
    Ok(aconex::Client::new(auth))
}

/// Lists every project visible to the authenticated user, now with typed
/// parsing — each project's id, short name, and name are pulled from the XML.
pub async fn list_projects() -> Result<()> {
    let client = build_client()?;

    println!("Fetching projects...");
    let projects = client.get_projects().await?;

    if projects.is_empty() {
        println!("No projects found.");
        return Ok(());
    }

    println!("\nFound {} project(s):\n", projects.len());
    for p in &projects {
        // The short name is what users put in config as `project_name`.
        println!("  {} — {} (id: {})", p.project_short_name, p.project_name, p.project_id);
    }

    Ok(())
}

/// Resolves the project name stored in config to its full record + numeric id —
/// the id every other Aconex endpoint needs.
pub async fn show_current_project() -> Result<()> {
    let config = crate::settings::load()?;
    let name = config
        .project_name
        .context("No project set — run `paperclip config set --project <short name>`")?;

    let client = build_client()?;

    println!("Looking up project '{}'...", name);
    match client.get_project(&name).await? {
        Some(p) => {
            println!("\nMatched:");
            println!("  short name: {}", p.project_short_name);
            println!("  name:       {}", p.project_name);
            println!("  id:         {}", p.project_id);
            println!("  active:     {}", p.active);
        }
        None => {
            println!(
                "\nNo project with short name '{}' was found in your visible projects.",
                name
            );
            println!("Run `paperclip projects` to see the list of available short names.");
        }
    }

    Ok(())
}
