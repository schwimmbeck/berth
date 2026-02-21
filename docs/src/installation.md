# Installation

## Prerequisites

- Rust 1.75+
- C toolchain (`gcc` or `clang`)

## Binary Installer

```bash
curl -fsSL https://raw.githubusercontent.com/berth-dev/berth/main/install.sh | sh
```

Installer environment variables:

- `BERTH_VERSION` (example: `v0.1.0`)
- `BERTH_INSTALL_DIR` (example: `$HOME/.local/bin`)
- `BERTH_REPO` (defaults to `berth-dev/berth`)

## Homebrew (source build)

```bash
brew install --HEAD ./Formula/berth.rb
```

## Build From Source

```bash
git clone https://github.com/berth-dev/berth.git
cd berth
cargo build --release
```

Binary path:

```text
target/release/berth
```

## Verify

```bash
./target/release/berth --help
```
