// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Schwimmbeck Dominik

//! Berth CLI binary entrypoint.

mod commands;
pub mod paths;
pub mod permission_filter;
pub mod policy_engine;
pub mod runtime_policy;
pub mod sandbox_policy;
pub mod sandbox_runtime;
pub mod secrets;

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
