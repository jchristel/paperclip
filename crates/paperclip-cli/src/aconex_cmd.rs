// src/aconex_cmd.rs
// The "real" Aconex commands: typed operations over the aconex library.
// Diagnostic/raw-probe commands live in aconex_diag.rs instead, so this file
// stays focused on the clean, typed API surface.

use anyhow::{Context, Result};

/// Builds an authenticated client from the three stored credentials.
/// Factored out so every command reuses the same setup instead of repeating
/// the credential-gathering each time.
///
/// `pub(crate)` = visible to other modules in THIS crate (e.g. aconex_diag),
/// but NOT part of the binary's public surface. It's the Rust equivalent of
/// C#'s `internal`: shared across the crate, hidden from outside.
pub(crate) fn build_client() -> Result<aconex::Client> {
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

/// Loads the configured project name from settings, or returns a helpful error.
/// Shared by the commands that operate on "the current project".
///
/// Also pub(crate) so the diagnostic commands can resolve the same project.
pub(crate) fn current_project_name() -> Result<String> {
    let config = crate::settings::load()?;
    config
        .project_name
        .context("No project set — run `paperclip config set --project <short name>`")
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
        // The short name is what users put in config as `project_name`.
        println!("  {} — {} (id: {})", p.project_short_name, p.project_name, p.project_id);
    }

    Ok(())
}

/// Resolves the project name stored in config to its full record + numeric id —
/// the id every other Aconex endpoint needs.
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
/// Resolves the configured project first, then runs the (auto-paginating)
/// search and shows a few identifying attributes per document.
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
        // NOTE: exact key names are still being confirmed against a real
        // response (see the diag search-raw command). These fall back
        // gracefully until we pin down the true child-element names.
        let docno = d
            .get("DocumentNumber")
            .or_else(|| d.get("docno"))
            .unwrap_or("(no docno)");
        let title = d
            .get("Title")
            .or_else(|| d.get("title"))
            .unwrap_or("(no title)");
        let rev = d
            .get("Revision")
            .or_else(|| d.get("revision"))
            .unwrap_or("");

        println!("  {} — {} {}", docno, title, rev);
    }

    Ok(())
}
