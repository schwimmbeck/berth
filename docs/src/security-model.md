# Security Model

Berth enforces permission-aware runtime behavior with explicit local overrides.

## Permission Categories

- environment variables (`env:*`)
- network destinations (`network:host:port` and wildcards)
- filesystem scopes (`filesystem:read:/path`, `filesystem:write:/path`)
- executable allowlist (`exec:<command>`)

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

## Behavior Examples

### 1. Revoke secret exposure

```bash
berth permissions github --revoke env:GITHUB_TOKEN
berth start github
```

Expected behavior: process can launch, but `GITHUB_TOKEN` is filtered out from the runtime env map.

### 2. Block all network access

```bash
berth config github --set berth.sandbox=basic
berth config github --set berth.sandbox-network=deny-all
berth start github
```

Expected behavior: launch is blocked with exit code `1`, and a denial event is written to the audit log.

### 3. Export auditable events

```bash
berth audit github --since 24h --json --export audit.json
```

Expected behavior: matching events are exported as a JSON array for machine review.
