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

/// Searches the document register and prints a summary of each match.
/// Resolves the configured project first, then runs the (auto-paginating)
/// search and shows a few identifying attributes per document.
pub async fn search_documents(query: &str) -> Result<()> {
    let config = crate::settings::load()?;
    let name = config
        .project_name
        .context("No project set — run `paperclip config set --project <short name>`")?;

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
        // We don't know the exact attribute names until the first live run,
        // so print a few likely ones and fall back gracefully when absent.
        // After we see the real response we'll tidy this to the true keys.
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

    // On the very first run it's invaluable to see the ACTUAL attribute keys
    // that came back, so we can confirm the attribute-map assumption and fix
    // the key names above if needed. Print the keys of the first document.
    if let Some(first) = docs.first() {
        let mut keys: Vec<&String> = first.attributes.keys().collect();
        keys.sort();
        println!("\n[debug] attribute keys on first document:");
        println!("  {:?}", keys);
    }

    Ok(())
}

/// Raw connectivity test — prints the first chunk of the response body as-is.
/// Useful for debugging when typed parsing fails.
pub async fn ping() -> Result<()> {
    let client = build_client()?;
    println!("Calling Aconex...");
    let body = client.get_text("/api/projects/").await?;
    let preview: String = body.chars().take(2000).collect();
    println!("\n--- Response (first 2000 chars) ---\n{}", preview);
    Ok(())
}