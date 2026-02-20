# Berth — The Safe Runtime & Package Manager for MCP Servers

> A safe berth for your AI tools.

Berth is an open-source CLI tool for managing [MCP (Model Context Protocol)](https://modelcontextprotocol.io) servers. It lets you discover, install, configure, run, and secure MCP servers from a single interface — replacing manual JSON config editing with a clean developer experience and adding a security/permission layer on top.

**Think: Homebrew + Docker + npm — but for MCP servers.**

## Why Berth?

The MCP ecosystem is growing fast — Anthropic created the protocol, OpenAI adopted it, and hundreds of MCP servers now exist for GitHub, Slack, PostgreSQL, filesystems, search engines, and more. But the developer experience is broken:

- **Finding MCP servers** is fragmented across GitHub repos, blog posts, and Twitter
- **Installing MCP servers** means hand-editing JSON config files — one typo breaks everything
- **Running MCP servers** has no health checks, no auto-restart, no unified logging
- **Securing MCP servers** is nonexistent — no sandboxing, no permission model, no audit trail

Berth fixes all of this with a single binary.

## Quick Start

```bash
# Build from source (requires Rust 1.75+)
git clone https://github.com/berth-dev/berth.git
cd berth
cargo build --release
# Binary is at target/release/berth

# Search the MCP server registry
berth search github

# Get detailed info about an MCP server
berth info github

# List installed MCP servers
berth list
```

## Features

| Feature | Status | Description |
|---------|--------|-------------|
| MCP server search | Working | Search the registry by name, tag, or category |
| MCP server info | Working | View metadata, permissions, config, compatibility |
| List installed MCP servers | Working | See what's installed and its status |
| Install / uninstall MCP servers | Planned | One-command install from the registry |
| Configure MCP servers | Planned | Interactive config with credential storage |
| Start / stop / restart MCP servers | Planned | Process supervision with auto-restart |
| MCP server health & status | Planned | Health checks and status monitoring |
| MCP server log streaming | Planned | Unified structured logging |
| MCP permission management | Planned | Declare and enforce server permissions |
| MCP audit trail | Planned | Log every tool call with full context |
| AI client integration | Planned | Auto-configure Claude Desktop, Cursor, Windsurf |

## Commands

```
berth search <query>           Search the MCP server registry
berth info <server>            Show detailed MCP server info
berth list                     List installed MCP servers

berth install <server>         Install an MCP server (planned)
berth uninstall <server>       Remove an MCP server (planned)
berth update <server|--all>    Update MCP servers (planned)
berth config <server>          Configure an MCP server (planned)

berth start [server]           Start MCP server(s) (planned)
berth stop [server]            Stop MCP server(s) (planned)
berth restart <server>         Restart an MCP server (planned)
berth status                   Show MCP server status (planned)
berth logs <server>            Stream MCP server logs (planned)

berth permissions <server>     Manage MCP server permissions (planned)
berth audit [server]           View MCP tool call audit log (planned)
berth link <client>            Link to an AI client (planned)
berth unlink <client>          Unlink from an AI client (planned)
berth proxy <server>           Run as transparent MCP proxy (planned)
```

## Supported MCP Servers (seed registry)

Berth ships with a built-in registry of popular MCP servers:

| Server | Description | Category |
|--------|-------------|----------|
| `github` | Access GitHub repos, issues, PRs, and actions | Developer Tools |
| `filesystem` | Secure local filesystem access with configurable permissions | Filesystem |
| `brave-search` | Web and local search via Brave Search API | Search |
| `postgres` | Read-only PostgreSQL database access with schema inspection | Databases |
| `slack` | Access Slack workspaces, channels, messages, and users | Communication |

More MCP servers will be added as the registry grows.

## Development

```bash
# Prerequisites: Rust 1.75+ and a C linker (gcc/clang)

# Build
cargo build

# Run all tests (unit + integration)
cargo test --workspace

# Run with arguments
cargo run -- search github
cargo run -- info github

# Lint (zero warnings policy)
cargo clippy --workspace --all-targets -- -D warnings

# Format
cargo fmt --all
```

## Project Structure

```
berth/
  Cargo.toml                     # Workspace root
  crates/
    berth-cli/                   # Binary crate (the `berth` command)
    berth-registry/              # MCP server registry client, types, search engine
    berth-runtime/               # MCP server process management (stub)
```

## Related

- [Model Context Protocol (MCP)](https://modelcontextprotocol.io) — The protocol Berth manages
- [MCP Servers](https://github.com/modelcontextprotocol/servers) — Official MCP server implementations
- [Claude Desktop](https://claude.ai/download) — AI client with MCP support
- [Cursor](https://cursor.sh) — AI code editor with MCP support

## License

Apache 2.0
