// src/main.rs

mod settings;
mod validator;
mod binder;
mod pdf_classifier;
mod mapper;
mod filename_parser;
mod log;
mod manifest;
mod xmp;
mod inspect;
mod assembler;
mod aconex_cmd;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "paperclip", about = "Aconex CLI tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage stored settings
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Assemble PDFs into binders
    Binder,
    /// Read and print the embedded manifest of a binder PDF
    Inspect {
        /// Path to the binder PDF to inspect
        path: String,
    },
    /// Interact with the Aconex API
    Aconex {
        #[command(subcommand)]
        action: AconexAction,
    },
}

#[derive(Subcommand)]
enum AconexAction {
    /// Test Aconex API connectivity (prints raw response)
    Ping,
    /// List all Aconex projects visible to you
    Projects,
    /// Show the project currently set in config (resolves name → id)
    Project,
    /// Search the document register
    Search {
        /// The Aconex search query (e.g. a document number)
        query: String,
    },
    /// DIAGNOSTIC: print raw search XML to inspect document structure
    SearchRaw {
        query: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Display current settings (secrets are masked)
    Show,

    /// Set one or more settings values
    Set {
        /// Your Aconex username
        #[arg(long)]
        username: Option<String>,

        /// Your Aconex user ID
        #[arg(long)]
        user_id: Option<String>,

        /// Path to the mapper CSV file
        #[arg(long)]
        mapper_path: Option<String>,

        /// The Aconex project name to operate on
        #[arg(long)]
        project: Option<String>,

        /// Prompt for your Aconex password (stored in Windows Credential Manager)
        /// Never pass the password as a value — use this flag alone to be prompted
        #[arg(long)]
        password: bool,  // bool flag — present means "prompt me", absent means "skip"

        /// Prompt for your Aconex application key (stored in Windows Credential Manager)
        /// Like --password, this is a flag: present means "prompt me", absent means "skip".
        /// The key is a secret, so it's prompted (not passed as a value) and never echoed.
        #[arg(long)]
        app_key: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // If no arguments supplied, print a friendly message and exit cleanly
    // std::env::args() is equivalent to args[] in C# Main(string[] args)
    // The first argument is always the executable path itself, so len() == 1 means nothing was passed
    if std::env::args().len() == 1 {
        println!("No arguments supplied — run `paperclip --help` to see available commands.");
        return Ok(());
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Config { action } => match action {

            ConfigAction::Show => {
                let config = settings::load()?;
                let pw = settings::load_password()?;
                let app_key = settings::load_app_key()?;

                println!("username:     {}", config.username.as_deref().unwrap_or("(not set)"));
                println!("user_id:      {}", config.user_id.as_deref().unwrap_or("(not set)"));
                println!("project:      {}", config.project_name.as_deref().unwrap_or("(not set)"));
                println!("mapper_path:  {}", config.mapper_csv_path.as_deref().unwrap_or("(not set)"));
                println!("password:     {}", if pw.is_some() { "(stored)" } else { "(not set)" });
                println!("app_key:      {}", if app_key.is_some() { "(stored)" } else { "(not set)" });
            }

            ConfigAction::Set { username, user_id, mapper_path, project, password, app_key } => {
                let mut config = settings::load()?;
                let mut changed = false;  // track whether anything was actually updated

                if let Some(v) = username {
                    config.username = Some(v);
                    changed = true;
                }
                if let Some(v) = user_id {
                    config.user_id = Some(v);
                    changed = true;
                }
                if let Some(v) = project {
                    config.project_name = Some(v);
                    changed = true;
                }
                if let Some(ref v) = mapper_path {
                    if validator::resolve_mapper_path(v)? {
                        config.mapper_csv_path = Some(v.clone());
                        changed = true;
                    }
                }

                if changed {
                    settings::save(&config)?;
                    println!("Settings saved.");
                } else if !password && !app_key {
                    // Nothing was passed or everything was rejected. We check
                    // BOTH secret flags here so that e.g. `--app-key` alone
                    // doesn't wrongly print "No settings were updated."
                    println!("No settings were updated.");
                }

                // password is a bool — true means the user passed --password.
                // rpassword::prompt_password prints the prompt but hides the keystrokes,
                // like Console.ReadLine() with ConsoleKey interception in C#
                if password {
                    let pw = rpassword::prompt_password("Enter Aconex password: ")
                        .context("Failed to read password from prompt")?;

                    if pw.is_empty() {
                        println!("Password not changed (empty input).");
                    } else {
                        settings::save_password(&pw)?;
                        println!("Password saved to Credential Manager.");
                    }
                }

                // app_key follows the exact same prompt-and-store pattern as the
                // password: secret, so prompted rather than passed as a value.
                if app_key {
                    let key = rpassword::prompt_password("Enter Aconex application key: ")
                        .context("Failed to read application key from prompt")?;

                    if key.is_empty() {
                        println!("Application key not changed (empty input).");
                    } else {
                        settings::save_app_key(&key)?;
                        println!("Application key saved to Credential Manager.");
                    }
                }
            }
        },
        Commands::Binder => {
            binder::run()?;
        },
        Commands::Inspect { path } => {
            inspect::run(&path)?;
        },
        Commands::Aconex { action } => match action {
            AconexAction::Ping => {
                aconex_cmd::ping().await?;
            }
            AconexAction::Projects => {
                aconex_cmd::list_projects().await?;
            }
            AconexAction::Project => {
                aconex_cmd::show_current_project().await?;
            }
            AconexAction::Search { query } => {
                aconex_cmd::search_documents(&query).await?;
            }
            AconexAction::SearchRaw { query } => {
                aconex_cmd::search_documents_raw(&query).await?;
            }
        },
    }

    Ok(())
}