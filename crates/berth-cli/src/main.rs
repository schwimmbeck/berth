//! Berth CLI binary entrypoint.

mod commands;
pub mod paths;

use clap::Parser;
use commands::Commands;

/// Berth — The safe runtime & package manager for MCP servers
#[derive(Parser)]
#[command(name = "berth", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Parses CLI arguments and dispatches to command handlers.
fn main() {
    let cli = Cli::parse();
    commands::execute(cli.command);
}
