# Runtime Operations

Berth manages server processes through a local runtime state model.

## Lifecycle

```bash
berth start github
berth stop github
berth restart github
```

## Status and Logs

```bash
berth status
berth logs github --tail 100
```

Status includes process state and, when available, PID and memory metadata.

## Auto-Restart Policy

Config keys:

- `berth.auto-restart` (`true` / `false`)
- `berth.max-restarts` (positive integer)
- `berth.sandbox` (`basic` / `off`)
- `berth.sandbox-network` (`inherit` / `deny-all`)

Example:

```bash
berth config github --set berth.auto-restart=true
berth config github --set berth.max-restarts=3
berth config github --set berth.sandbox=basic
berth config github --set berth.sandbox-network=inherit
```
