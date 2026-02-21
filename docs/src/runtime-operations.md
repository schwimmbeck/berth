# Runtime Operations

Berth manages server processes through a local runtime state model.

## Lifecycle

```bash
berth start github
berth stop github
berth restart github
```

Stop behavior is graceful-first: Berth sends a normal termination signal, waits briefly for exit,
and escalates to force termination only when needed.

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

When sandbox mode is enabled:

- Linux uses `landlock-restrict` for filesystem scope enforcement when available and `setpriv --no-new-privs` for additional hardening
- macOS uses `sandbox-exec` with a generated profile and declared write-path allowances
- Other platforms fall back to standard process launch while preserving policy config

Example:

```bash
berth config github --set berth.auto-restart=true
berth config github --set berth.max-restarts=3
berth config github --set berth.sandbox=basic
berth config github --set berth.sandbox-network=inherit
```
