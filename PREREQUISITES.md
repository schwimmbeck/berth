# Berth — Build Prerequisites

## Phase 0 + Early Phase 1 (current)

All Rust crate dependencies (clap, serde, colored, dirs) are pure Rust.
Only a Rust toolchain and a C linker are required.

| Dependency       | Version   | Required | Install (Ubuntu/Debian)              |
|------------------|-----------|----------|--------------------------------------|
| rustc + cargo    | >= 1.75   | YES      | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| gcc / cc         | any       | YES      | `sudo apt install build-essential`   |
| git              | any       | YES      | `sudo apt install git`               |

## Later Phases (when networking / reqwest is added)

| Dependency       | Version   | Required For          | Install (Ubuntu/Debian)              |
|------------------|-----------|-----------------------|--------------------------------------|
| pkg-config       | any       | reqwest (HTTP client) | `sudo apt install pkg-config`        |
| libssl-dev       | any       | reqwest with TLS      | `sudo apt install libssl-dev`        |
| curl             | any       | convenience / scripts | `sudo apt install curl`              |

### One-liner for all future deps

```bash
sudo apt install build-essential pkg-config libssl-dev curl git
```
