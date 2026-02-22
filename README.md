<p align="center">
  <img src="assets/berth-header.png" alt="Berth header logo" width="1024" />
</p>

<h1 align="center">
  <img src="assets/berth-icon.png" alt="Berth repository icon" width="52" />
  Berth — The Safe Runtime & Package Manager for MCP Servers
</h1>

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

## Install

```bash
# Binary install (latest GitHub release)
curl -fsSL https://raw.githubusercontent.com/berth-dev/berth/main/install.sh | sh

# Optional: install from source via Homebrew (HEAD formula in this repo)
brew install --HEAD ./Formula/berth.rb
```

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
| Install / uninstall / update MCP servers | Working | Install, remove, and update from the registry (supports `npx`, `uvx`, and binary artifacts) |
| Configure MCP servers | Working | Set and view server configuration |
| Start / stop / restart MCP servers | Working | Subprocess lifecycle controls with PID/state tracking, graceful-first shutdown, and optional bounded auto-restart policy |
| MCP server health & status | Working | Runtime status with running/stopped/error plus PID and memory where available |
| MCP server log streaming | Working | Tail lifecycle events from persisted server logs |
| MCP proxy mode | Working | Transparent stdio proxy execution for configured servers |
| MCP permission management | Working | Inspect declared/effective permissions, enforce env overrides at launch/link time, and block launch when network is fully revoked |
| MCP audit trail | Working | JSONL audit events for lifecycle actions with server/time filters |
| AI client integration | Working | Auto-configure Claude Desktop, Cursor, Windsurf, Continue, and VS Code |
| Registry API (local) | Working | Serve REST endpoints for health, search, server detail, and download counts from the current registry dataset |
| Registry website (local) | Working | Browser UI at `/site` with catalog filters, server detail pages, and copy-ready install commands |
| Registry publish workflow | Working | Validate `berth.toml`, run local quality checks, and enqueue submission artifacts for manual review |
| Community signals | Working | Submit stars/reports via API and website detail UI; manage verified publisher badges with persisted local counters |

## Commands

```
berth search <query>           Search the MCP server registry
berth info <server>            Show detailed MCP server info
berth list                     List installed MCP servers

berth install <server[@version]> Install an MCP server
berth import-github <owner/repo> Auto-import server from GitHub `berth.toml` (`--ref`, `--manifest-path`, `--dry-run`)
berth uninstall <server>       Remove an MCP server
berth update <server|--all>    Update MCP servers
berth config <server>          Configure an MCP server (`--set`, `--secure`, `--env`, or `--interactive`)
berth config export [file]     Export installed server config values as TOML bundle
berth config import <file>     Import server config values from TOML bundle

berth start [server]           Start MCP server(s)
berth stop [server]            Stop MCP server(s)
berth restart <server>         Restart an MCP server
berth status                   Show MCP server status (state, PID, memory)
berth logs <server>            Show recent MCP server logs

berth permissions <server>     Show/manage/export MCP server permissions (--grant/--revoke/--reset/--export)
berth audit [server]           View/export runtime audit log (supports --since, --action, --json, and --export)
berth link <client>            Link Berth-managed servers to claude-desktop, cursor, windsurf, continue, or vscode
berth unlink <client>          Unlink Berth-managed servers from claude-desktop, cursor, windsurf, continue, or vscode
berth proxy <server>           Run as transparent MCP proxy
berth registry-api             Serve local registry REST API (supports --bind and --max-requests)
berth publish [manifest]       Validate + submit `berth.toml` to local review queue (`--dry-run` available)
```

Registry API endpoints:
- `GET /health`
- `GET /servers?q=<query>&category=<category>&platform=<platform>&trustLevel=<level>&offset=<n>&limit=<n>&sortBy=<field>&order=<asc|desc>`
- `GET /servers/suggest?q=<query>&limit=<n>&category=<category>`
- `GET /servers/facets?q=<query>&category=<category>&platform=<platform>&trustLevel=<level>`
- `GET /servers/filters`
- `GET /servers/trending?limit=<n>&category=<category>&platform=<platform>&trustLevel=<level>`
- `GET /stats?top=<n>`
- `GET /servers/<name>`
- `GET /servers/<name>/related?limit=<n>`
- `GET /servers/<name>/downloads`
- `GET /servers/<name>/community`
- `GET /servers/<name>/reports?limit=<n>&offset=<n>`
- `GET /reports/filters`
- `GET /reports?server=<name>&reason=<reason>&offset=<n>&limit=<n>`
- `GET /publish/submissions?status=<status>&server=<name>&offset=<n>&limit=<n>`
- `GET /publish/submissions/filters`
- `GET /publish/submissions/<id>`
- `POST /publish/submissions/<id>/status` (JSON body: `status`, optional `note`)
- `GET /publish/review-events?status=<status>&server=<name>&submission=<id>&offset=<n>&limit=<n>`
- `GET /publish/review-events/filters`
- `GET /publishers?maintainer=<name>&verified=<verified|unverified|true|false>&offset=<n>&limit=<n>`
- `GET /publishers/filters`
- `GET /publishers/<maintainer>`
- `POST /servers/<name>/star`
- `POST /servers/<name>/report`
- `GET /publishers/verified`
- `POST /publishers/verify`
- `POST /publishers/unverify`
- `GET /site` (HTML catalog page with `q`, `category`, `platform`, `trustLevel`, `sortBy`, `order`, `limit`, `offset`)
- `GET /site/reports` (HTML moderation feed with `server`, `reason`, `limit`, `offset`)
- `GET /site/submissions` (HTML publish review queue with `status`, `server`, `limit`, `offset`)
- `GET /site/review-events` (HTML publish review event feed with `status`, `server`, `submission`, `limit`, `offset`)
- `GET /site/publishers` (HTML publisher verification dashboard with `maintainer`, `verified`, `limit`, `offset`)
- `GET /site/publishers/<maintainer>` (HTML publisher detail page with maintainer signals and server list)
- `GET /site/submissions/<id>` (HTML submission detail with full manifest and quality checks)
- `GET /site/servers/<name>` (HTML server detail page)
- `OPTIONS <any-endpoint>` (browser preflight; CORS enabled)

`GET /servers` and `GET /servers/<name>` include:
- `maintainerVerified` + `badges`
- `qualityScore` (deterministic ranking signal)
- `readmeUrl` (best-effort repository README link for detail pages)
- `permissionsSummary` (website-friendly permission counts/flags)
- `installCommandCopy` (copy-ready install command text)

Permission override formats:
- `env:<VAR>` (example: `env:GITHUB_TOKEN`)
- `env:*`
- `network:<host>:<port>` (examples: `network:api.github.com:443`, `network:*:443`)
- `network:*`
- `filesystem:<read|write>:<path>` (examples: `filesystem:read:/workspace`, `filesystem:write:/tmp`)
- `filesystem:*`
- `exec:<command>` (example: `exec:git`)
- `exec:*`

Runtime and sandbox config keys:
- `berth.auto-restart` (`true` or `false`)
- `berth.max-restarts` (positive integer, default `3`)
- `berth.sandbox` (`basic` or `off`)
- `berth.sandbox-network` (`inherit` or `deny-all`)

Sandbox runtime note:
- On Linux, `berth.sandbox=basic` applies Landlock filesystem restrictions via `landlock-restrict` when available and also applies `setpriv --no-new-privs` hardening when available.
- On macOS, `berth.sandbox=basic` uses `sandbox-exec` with a generated profile (default-deny baseline, declared write-path allowances).

Registry source overrides (optional):
- `BERTH_REGISTRY_INDEX_URL` fetch registry JSON via `curl`/`wget` and use it for lookups.
- `BERTH_REGISTRY_INDEX_FILE` load registry JSON from a local file path.
- `BERTH_REGISTRY_CACHE` cache path for downloaded/overridden registry JSON.

Security behavior examples:
- Env secret filtering at launch:
  - `berth permissions github --revoke env:GITHUB_TOKEN`
  - `berth start github` (server starts without `GITHUB_TOKEN` exposed)
- Network hard block:
  - `berth config github --set berth.sandbox=basic`
  - `berth config github --set berth.sandbox-network=deny-all`
  - `berth start github` (blocked with exit code `1`)
- Audit export for review:
  - `berth audit github --since 24h --json --export audit.json`
- Undeclared network override warning (log-only):
  - `berth permissions github --grant network:example.com:443`
  - `berth start github` (prints warning and records `permission-network-warning`)

## Supported MCP Servers (seed registry)

Berth ships with a built-in registry of popular MCP servers:

| Server | Description | Category |
|--------|-------------|----------|
| `github` | Access GitHub repos, issues, PRs, and actions | Developer Tools |
| `filesystem` | Secure local filesystem access with configurable permissions | Filesystem |
| `brave-search` | Web and local search via Brave Search API | Search |
| `postgres` | Read-only PostgreSQL database access with schema inspection | Databases |
| `slack` | Access Slack workspaces, channels, messages, and users | Communication |
| `notion` | Read and update Notion pages and databases | Productivity |
| `google-drive` | Access files and folders from Google Drive | Productivity |
| `sqlite` | Query local SQLite databases | Databases |
| `fetch` | Fetch HTTP resources for tool workflows | Search |
| `memory` | Store and retrieve structured memory for assistants | Developer Tools |
| `puppeteer` | Automate browser tasks and capture screenshots | Developer Tools |
| `sequential-thinking` | Structured reasoning and planning utilities | Developer Tools |
| `google-maps` | Places, geocoding, and routing via Google Maps APIs | Search |
| `docker` | Inspect and manage local Docker containers and images | Developer Tools |
| `kubernetes` | Query and operate Kubernetes cluster resources | Developer Tools |
| `aws` | Access AWS resources across common services | Developer Tools |
| `linear` | Read and update Linear issues, projects, and teams | Productivity |
| `gitlab` | Access GitLab projects, issues, merge requests, and pipelines | Developer Tools |
| `sentry` | Inspect Sentry issues, alerts, and project error trends | Developer Tools |
| `datadog` | Query Datadog metrics, traces, dashboards, and monitors | Developer Tools |
| `redis` | Inspect keys and run safe Redis operations | Databases |
| `mongodb` | Query MongoDB collections and documents | Databases |
| `stripe` | Access Stripe customers, payments, and invoices | Developer Tools |
| `shopify` | Manage products, orders, and customers in Shopify | Productivity |
| `twilio` | Work with Twilio messaging and voice resources | Communication |
| `sendgrid` | Manage email templates, sends, and delivery stats | Communication |
| `figma` | Access Figma files, components, and comments | Productivity |
| `vercel` | Inspect projects, deployments, and logs on Vercel | Developer Tools |
| `supabase` | Access Supabase database, auth, and storage resources | Databases |
| `prisma` | Inspect Prisma schema and query connected databases | Developer Tools |

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

# License/compliance checks
cargo deny check

# Format
cargo fmt --all

# Build documentation site
make docs

# Run CI quality smoke checks locally
bash scripts/quality-checks.sh
```

## Project Structure

```
berth/
  Cargo.toml                     # Workspace root
  docs/                          # mdBook documentation source
  crates/
    berth-cli/                   # Binary crate (the `berth` command)
    berth-registry/              # MCP server registry client, types, search engine
    berth-runtime/               # MCP server runtime state management with tokio-backed supervision
```

## Related

- [Model Context Protocol (MCP)](https://modelcontextprotocol.io) — The protocol Berth manages
- [MCP Servers](https://github.com/modelcontextprotocol/servers) — Official MCP server implementations
- [Claude Desktop](https://claude.ai/download) — AI client with MCP support
- [Cursor](https://cursor.sh) — AI code editor with MCP support
- [Continue](https://www.continue.dev) — Open-source AI coding assistant

## License

Apache 2.0
