// src/main.rs

mod settings;
mod validator;
mod binder;
mod pdf_classifier;
mod mapper;

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
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Display current settings (password is masked)
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

        /// Prompt for your Aconex password (stored in Windows Credential Manager)
        /// Never pass the password as a value — use this flag alone to be prompted
        #[arg(long)]
        password: bool,  // bool flag — present means "prompt me", absent means "skip"
    },
}

fn main() -> Result<()> {
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

                println!("username:    {}", config.username.as_deref().unwrap_or("(not set)"));
                println!("user_id:     {}", config.user_id.as_deref().unwrap_or("(not set)"));
                println!("mapper_path: {}", config.mapper_csv_path.as_deref().unwrap_or("(not set)"));
                println!("password:    {}", if pw.is_some() { "(stored)" } else { "(not set)" });
            }

            ConfigAction::Set { username, user_id, mapper_path, password } => {
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
                if let Some(ref v) = mapper_path {
                    if validator::resolve_mapper_path(v)? {
                        config.mapper_csv_path = Some(v.clone());
                        changed = true;
                    }
                }

                if changed {
                    settings::save(&config)?;
                    println!("Settings saved.");
                } else if !password {
                    // Nothing was passed or everything was rejected
                    println!("No settings were updated.");
                }

                // password is now a bool — true means the user passed --password
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
            }
        },
        Commands::Binder => {
            binder::run()?;
        }
    }

    Ok(())
}