// src/aconex_cmd.rs
// The "real" Aconex commands: typed operations over the aconex library.
// Diagnostic/raw-probe commands live in aconex_diag.rs instead, so this file
// stays focused on the clean, typed API surface.

use anyhow::{Context, Result};

/// Builds an authenticated client from the stored credentials.
///
/// Username comes from the resolved config (global, layered under any
/// paperclip.toml in the working folder — though username is global-only today,
/// so in practice it's the global value). Password and app key are ALWAYS read
/// straight from Credential Manager: secrets never live in a project file.
///
/// `pub(crate)` = visible to other modules in THIS crate (e.g. aconex_diag),
/// but NOT part of the binary's public surface — Rust's equivalent of C#'s
/// `internal`.
pub(crate) fn build_client() -> Result<aconex::Client> {
    // Resolve config from the working folder, so a paperclip.toml there is
    // honoured — the same "working folder" rule the binder folder modes use
    // (they pass current_dir() too). `?` propagates an unreadable-cwd error.
    let dir = std::env::current_dir()?;
    let config = crate::project_config::resolve_config(&dir)?;

    let username = config
        .username
        .context("No username set — run `paperclip config set --username`")?;

    // Secrets bypass resolve_config entirely — they're only in Credential
    // Manager, never in any TOML.
    let password = crate::settings::load_password()?
        .context("No password stored — run `paperclip config set --password`")?;
    let app_key = crate::settings::load_app_key()?
        .context("No app key stored — run `paperclip config set --app-key`")?;

    let auth = aconex::BasicAuth::new(username, password, app_key)?;
    Ok(aconex::Client::new(auth))
}

/// Resolves the project short name to operate on.
///
/// Now goes through `resolve_config`, so a `project_name` in a working-folder
/// paperclip.toml overrides the global setting — letting you switch projects by
/// `cd`-ing between folders, exactly as the binder modes already do. Falls back
/// to the global value when no project file sets it.
///
/// pub(crate) so the diagnostic commands resolve the same project.
pub(crate) fn current_project_name() -> Result<String> {
    let dir = std::env::current_dir()?;
    let config = crate::project_config::resolve_config(&dir)?;
    config
        .project_name
        .context("No project set — run `paperclip config set --project <short name>`, or set project_name in paperclip.toml")
}

/// Lists every project visible to the authenticated user, with typed parsing.
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
        println!("  {} — {} (id: {})", p.project_short_name, p.project_name, p.project_id);
    }

    Ok(())
}

/// Resolves the project name stored in config to its full record + numeric id.
pub async fn show_current_project() -> Result<()> {
    let name = current_project_name()?;
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
            println!("Run `paperclip aconex projects` to see the available short names.");
        }
    }

    Ok(())
}

/// Searches the document register and prints a summary of each match.
pub async fn search_documents(query: &str) -> Result<()> {
    let name = current_project_name()?;
    let client = build_client()?;

    println!("Resolving project '{}'...", name);
    let project = client
        .get_project(&name)
        .await?
        .with_context(|| format!("Project '{}' not found in your visible projects", name))?;

    println!("Searching for '{}'...", query);
    let docs = client.search_documents(&project, query).await?;

    if docs.is_empty() {
        println!("No documents matched.");
        return Ok(());
    }

    println!("\nFound {} document(s):\n", docs.len());
    for d in &docs {
        // Typed fields now — each is Option since a field only appears if it
        // was requested and present. unwrap_or gives a readable fallback.
        let docno = d.document_number.as_deref().unwrap_or("(no docno)");
        let title = d.title.as_deref().unwrap_or("(no title)");
        let rev = d.revision.as_deref().unwrap_or("");
        println!("  {} [{}] — {}", docno, rev, title);
    }

    Ok(())
}

/// Downloads a document by id to a local path.
pub async fn download_document(document_id: &str, dest: &str) -> Result<()> {
    let name = current_project_name()?;
    let client = build_client()?;

    println!("Resolving project '{}'...", name);
    let project = client
        .get_project(&name)
        .await?
        .with_context(|| format!("Project '{}' not found in your visible projects", name))?;

    println!("Downloading document {} ...", document_id);
    let written = client.download_document(&project, document_id, dest).await?;

    println!("Saved to: {}", written.display());
    Ok(())
}