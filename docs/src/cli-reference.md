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
- `GET /servers` with optional `q|query`, `category`, `platform`, `trustLevel`, `offset`, `limit`, `sortBy`, `order`
- `GET /servers/suggest` with optional `q|query`, `limit`, `category`
- `GET /servers/facets` with optional `q|query`, `category`, `platform`, `trustLevel`
- `GET /servers/filters`
- `GET /servers/trending` with optional `limit`, `offset`, `category`, `platform`, `trustLevel`
- `GET /stats` with optional `top`
- `GET /servers/<name>`
- `GET /servers/<name>/related` with optional `limit`, `offset`
- `GET /servers/<name>/downloads`
- `GET /servers/<name>/community`
- `GET /servers/<name>/reports` with optional `limit`, `offset`
- `GET /reports` with optional `server`, `reason`, `offset`, and `limit`
- `GET /publish/submissions` with optional `status`, `server`, `offset`, and `limit`
- `GET /publish/submissions/filters`
- `GET /publish/submissions/<id>`
- `POST /publish/submissions/<id>/status` with JSON body `status` and optional `note`
- `POST /servers/<name>/star`
- `POST /servers/<name>/report`
- `GET /publishers/verified`
- `POST /publishers/verify`
- `POST /publishers/unverify`
- `GET /site` (HTML registry catalog with filters, sorting, and pagination query params)
- `GET /site/reports` (HTML moderation feed with `server`, `reason`, `limit`, `offset`)
- `GET /site/submissions` (HTML publish review queue with `status`, `server`, `limit`, `offset`)
- `GET /site/submissions/<id>` (HTML submission detail with manifest and quality checks)
- `GET /site/servers/<name>` (HTML server detail page with install copy button and star/report controls)
- `OPTIONS <endpoint>` for browser preflight (CORS)

`GET /servers` and `GET /servers/<name>` responses include:
- `maintainerVerified` + `badges`
- `qualityScore`
- `readmeUrl`
- `permissionsSummary`
- `installCommandCopy`

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
