# Security Model

Berth enforces permission-aware runtime behavior with explicit local overrides.

## Permission Categories

- environment variables (`env:*`)
- network destinations (`network:host:port` and wildcards)

## Commands

Inspect and manage permissions:

```bash
berth permissions github
berth permissions github --grant env:GITHUB_TOKEN
berth permissions github --revoke network:*
berth permissions github --reset
berth permissions github --export
```

Audit runtime actions:

```bash
berth audit
berth audit github --since 24h
berth audit github --action start
berth audit github --json
berth audit github --export audit.jsonl
```

## Enforcement Notes

- launch and link flows apply effective env permissions
- full network revocation blocks launch/proxy and is recorded in audit
- audit data is stored as JSONL for deterministic parsing
