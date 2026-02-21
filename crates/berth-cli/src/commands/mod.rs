//! CLI subcommand declarations and dispatch.

pub mod audit;
pub mod config;
pub mod info;
pub mod install;
pub mod link;
pub mod list;
pub mod logs;
pub mod permissions;
pub mod proxy;
pub mod restart;
pub mod search;
pub mod start;
pub mod status;
pub mod stop;
pub mod uninstall;
pub mod unlink;
pub mod update;

use clap::Subcommand;

/// Top-level CLI subcommands supported by `berth`.
#[derive(Subcommand)]
pub enum Commands {
    /// Search the registry for MCP servers
    Search {
        /// Search query
        query: String,
    },

    /// Show detailed info about an MCP server
    Info {
        /// Server name
        server: String,
    },

    /// List installed MCP servers
    List,

    /// Install an MCP server
    Install {
        /// Server name (optionally with @version)
        server: String,
    },

    /// Uninstall an MCP server
    Uninstall {
        /// Server name
        server: String,
    },

    /// Update an MCP server (or all with --all)
    Update {
        /// Server name (omit for --all)
        server: Option<String>,

        /// Update all installed servers
        #[arg(long)]
        all: bool,
    },

    /// Configure an MCP server
    Config {
        /// Server name, or 'export'/'import' for config sharing
        server: String,

        /// Path for `config export` output or `config import` input
        path: Option<String>,

        /// Set a config value (key=value)
        #[arg(long)]
        set: Option<String>,

        /// Show required environment variables
        #[arg(long)]
        env: bool,
    },

    /// Start MCP server(s)
    Start {
        /// Server name (omit to start all)
        server: Option<String>,
    },

    /// Stop MCP server(s)
    Stop {
        /// Server name (omit to stop all)
        server: Option<String>,
    },

    /// Restart an MCP server
    Restart {
        /// Server name
        server: String,
    },

    /// Show status of MCP servers
    Status,

    /// Stream logs from an MCP server
    Logs {
        /// Server name
        server: String,

        /// Number of lines to show
        #[arg(long, default_value = "50")]
        tail: u32,
    },

    /// Show or manage permissions for an MCP server
    Permissions {
        /// Server name
        server: String,

        /// Grant a permission
        #[arg(long)]
        grant: Option<String>,

        /// Revoke a permission
        #[arg(long)]
        revoke: Option<String>,

        /// Clear all local permission overrides
        #[arg(long)]
        reset: bool,

        /// Export declared/overrides/effective permissions as JSON
        #[arg(long = "export")]
        export_json: bool,
    },

    /// Show audit log of MCP tool calls
    Audit {
        /// Server name (omit for all)
        server: Option<String>,

        /// Show entries since duration (e.g. 1h, 24h)
        #[arg(long)]
        since: Option<String>,

        /// Filter to a specific action (e.g. start, stop, proxy-start)
        #[arg(long)]
        action: Option<String>,

        /// Print matching audit entries as JSON
        #[arg(long)]
        json: bool,

        /// Export matching audit entries to a file
        #[arg(long, value_name = "FILE")]
        export: Option<String>,
    },

    /// Link Berth to an AI client (e.g. claude-desktop, cursor)
    Link {
        /// Client name
        client: String,
    },

    /// Unlink Berth from an AI client
    Unlink {
        /// Client name
        client: String,
    },

    /// Run as a transparent MCP proxy for a server
    Proxy {
        /// Server name
        server: String,
    },
}

/// Dispatches a parsed CLI command to its command module.
pub fn execute(command: Commands) {
    match command {
        Commands::Search { query } => search::execute(&query),
        Commands::Info { server } => info::execute(&server),
        Commands::List => list::execute(),
        Commands::Install { server } => install::execute(&server),
        Commands::Uninstall { server } => uninstall::execute(&server),
        Commands::Update { server, all } => update::execute(server.as_deref(), all),
        Commands::Config {
            server,
            path,
            set,
            env,
        } => config::execute(&server, path.as_deref(), set.as_deref(), env),
        Commands::Start { server } => start::execute(server.as_deref()),
        Commands::Stop { server } => stop::execute(server.as_deref()),
        Commands::Restart { server } => restart::execute(&server),
        Commands::Status => status::execute(),
        Commands::Logs { server, tail } => logs::execute(&server, tail),
        Commands::Permissions {
            server,
            grant,
            revoke,
            reset,
            export_json,
        } => permissions::execute(
            &server,
            grant.as_deref(),
            revoke.as_deref(),
            reset,
            export_json,
        ),
        Commands::Audit {
            server,
            since,
            action,
            json,
            export,
        } => audit::execute(
            server.as_deref(),
            since.as_deref(),
            action.as_deref(),
            json,
            export.as_deref(),
        ),
        Commands::Link { client } => link::execute(&client),
        Commands::Unlink { client } => unlink::execute(&client),
        Commands::Proxy { server } => proxy::execute(&server),
    }
}
