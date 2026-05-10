use anyhow::Result;
use clap::{Parser, Subcommand};

mod cache;
mod commands;
mod github;
mod search;
mod sources;
mod symbols;
mod tree;

use commands::{cmd_install, cmd_local, cmd_search, cmd_symbols_dir};

#[derive(Parser)]
#[command(name = "vol3sm")]
#[command(
    about = "Volatility3 symbol manager — search and install ISF symbols from public symbol indexes"
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search for symbols by kernel banner or filename pattern
    Search {
        /// Query string: banner snippet, distro name, kernel version, arch, etc.
        query: String,
        /// Maximum results to display
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Download and install a symbol into volatility3
    Install {
        /// Repository path or numeric id from the last search results
        path: Option<String>,
        /// Exact kernel banner string (from `vol.py banners` output)
        #[arg(short, long, value_name = "BANNER")]
        banner: Option<String>,
    },

    /// List locally installed symbol files
    Local,

    /// Print the detected volatility3 symbols directory
    SymbolsDir,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Search { query, limit } => cmd_search(&query, limit),
        Commands::Install { path, banner } => cmd_install(path, banner),
        Commands::Local => cmd_local(),
        Commands::SymbolsDir => cmd_symbols_dir(),
    }
}
