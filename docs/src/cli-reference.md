# CLI Reference

Core commands:

```text
berth search <query>
berth info <server>
berth list
berth install <server[@version]>
berth import-github <owner/repo>
berth uninstall <server>
berth update <server|--all>
berth publish [manifest]
berth config <server>
berth config <server> --interactive
berth config <server> --set key=value --secure
berth config export [file]
berth config import <file>
```

Install runtimes supported by metadata:
- Node (`npx`)
- Python (`uvx`)
- Binary artifacts (local path or URL)

Runtime commands:

```text
berth start [server]
berth stop [server]
berth restart <server>
berth status
berth logs <server>
berth proxy <server>
```

Security commands:

```text
berth permissions <server>
berth audit [server]
```

Registry API command:

```text
berth registry-api [--bind 127.0.0.1:8787] [--max-requests N]
```

Registry API endpoints:
- `GET /health`
- `GET /servers` with optional `q|query`, `category`, `platform`, `trustLevel`, `offset`, `limit`
- `GET /servers/filters`
- `GET /servers/<name>`
- `GET /servers/<name>/downloads`
- `GET /servers/<name>/community`
- `POST /servers/<name>/star`
- `POST /servers/<name>/report`

Client integration:

```text
berth link <client>
berth unlink <client>
```

Supported clients:

- `claude-desktop`
- `cursor`
- `windsurf`
- `continue`
- `vscode`

For complete argument details, use:

```bash
berth --help
berth <command> --help
```

Registry source overrides (advanced):

- `BERTH_REGISTRY_INDEX_URL` (remote JSON index)
- `BERTH_REGISTRY_INDEX_FILE` (local JSON index file)
- `BERTH_REGISTRY_CACHE` (cache file path)
