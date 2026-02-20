# Agent Guidelines for Berth

This document defines how AI coding agents should work with the Berth codebase.

## Project Overview

Berth is a CLI tool and runtime for managing MCP (Model Context Protocol) servers. Written in Rust, structured as a Cargo workspace with 3 crates:

- `berth-cli` — the `berth` binary (clap-based CLI)
- `berth-registry` — registry client, types, search engine
- `berth-runtime` — process management (stub, will use tokio)

## Build & Test

```bash
cargo build                                              # compile
cargo test --workspace                                   # all unit + integration tests
cargo clippy --workspace --all-targets -- -D warnings    # lint (zero warnings policy)
cargo fmt --all -- --check                               # format check
```

All four must pass before committing. CI enforces this.

## Code Conventions

- **One file per command** in `berth-cli/src/commands/`. File name matches the subcommand.
- **No async yet** — keep it synchronous until the runtime requires tokio.
- **`colored` crate** for terminal output. Use consistently: green for success, yellow for warnings, red for errors, cyan for server names, dimmed for labels, bold for headers.
- **`#[serde(rename_all = "camelCase")]`** on all registry types to match JSON keys.
- **Embedded seed data** via `include_str!` — no network calls for the seed registry.

## Testing Requirements

- Every new module must have `#[cfg(test)]` unit tests covering its core logic.
- Every new working CLI command must have integration tests in `berth-cli/tests/cli.rs`.
- Tests must be deterministic: no network, no timing, no home directory pollution.
- Stub commands only need a single integration test asserting "not yet implemented" output.
- When a stub becomes a real implementation, replace its stub test with proper assertions.

## Commit Guidelines

- Keep commit messages concise: one summary line, optional bullet list of changes.
- Always run `cargo fmt --all` before committing.
- Do not commit with warnings — fix them or justify the suppression.
- Do not commit `BERTH.md` (it's in `.gitignore`, used as internal spec only).

## Adding a New Command

1. Create `berth-cli/src/commands/<name>.rs`
2. Add `pub mod <name>;` to `commands/mod.rs`
3. Add the variant to the `Commands` enum with clap attributes
4. Add the dispatch arm in `execute()`
5. Add integration test(s) in `tests/cli.rs`
6. Update `README.md` to reflect the new command's status

## Architecture Decisions

- **No over-engineering**: only build what's needed now. Three similar lines > a premature abstraction.
- **Flat command modules**: no nested subcommand trees. Matches cargo/rustup conventions.
- **Registry overlay pattern**: seed data is the base; future cache/network data overlays it.
- **Exit codes**: 0 for success, 1 for errors (e.g. server not found). Stubs always exit 0.
