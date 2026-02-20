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

fn main() {
    let cli = Cli::parse();
    commands::execute(cli.command);
}
