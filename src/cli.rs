//! CLI argument parsing.

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "postui", version, about = "Terminal UI for PostgreSQL")]
pub struct Cli {
    /// Path to config file. Defaults to ~/.config/postui/config.toml.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Named connection from the config file.
    #[arg(long, conflicts_with = "uri")]
    pub connection: Option<String>,

    /// A postgres:// URI to connect to immediately.
    #[arg()]
    pub uri: Option<String>,
}
