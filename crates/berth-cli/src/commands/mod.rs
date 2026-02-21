//! CLI subcommand declarations and dispatch.

pub mod audit;
pub mod config;
pub mod import_github;
pub mod info;
pub mod install;
pub mod link;
pub mod list;
pub mod logs;
pub mod permissions;
pub mod proxy;
pub mod publish;
pub mod registry_api;
pub mod restart;
pub mod search;
pub mod start;
pub mod status;
pub mod stop;
pub mod supervise;
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

    /// Auto-import an MCP server from a GitHub repo containing `berth.toml`
    ImportGithub {
        /// GitHub repo (`owner/repo` or GitHub URL)
        repo: String,

        /// Git ref to read from
        #[arg(long = "ref", default_value = "main")]
        git_ref: String,

        /// Manifest path in the repository
        #[arg(long, default_value = "berth.toml")]
        manifest_path: String,

        /// Validate only; do not write local server config
        #[arg(long)]
        dry_run: bool,
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

        /// Store `--set` value in secure backend (keyring by default)
        #[arg(long)]
        secure: bool,

        /// Show required environment variables
        #[arg(long)]
        env: bool,

        /// Prompt interactively for config values
        #[arg(long)]
        interactive: bool,
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

    /// Link Berth to an AI client (e.g. claude-desktop, cursor, continue, vscode)
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

    /// Publish an MCP server manifest to the registry review queue
    Publish {
        /// Path to berth manifest file
        manifest: Option<String>,

        /// Validate only; do not submit
        #[arg(long)]
        dry_run: bool,
    },

    /// Serve local registry REST API endpoints
    RegistryApi {
        /// Bind address (host:port)
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,

        /// Exit after serving this many requests (for tests/automation)
        #[arg(long)]
        max_requests: Option<u32>,
    },

    /// Internal process supervisor loop (hidden).
    #[command(hide = true, name = "__supervise")]
    Supervise {
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
        Commands::ImportGithub {
            repo,
            git_ref,
            manifest_path,
            dry_run,
        } => import_github::execute(&repo, &git_ref, &manifest_path, dry_run),
        Commands::Uninstall { server } => uninstall::execute(&server),
        Commands::Update { server, all } => update::execute(server.as_deref(), all),
        Commands::Config {
            server,
            path,
            set,
            secure,
            env,
            interactive,
        } => config::execute(
            &server,
            path.as_deref(),
            set.as_deref(),
            secure,
            env,
            interactive,
        ),
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
        Commands::Publish { manifest, dry_run } => publish::execute(manifest.as_deref(), dry_run),
        Commands::RegistryApi { bind, max_requests } => registry_api::execute(&bind, max_requests),
        Commands::Supervise { server } => supervise::execute(&server),
    }
}
