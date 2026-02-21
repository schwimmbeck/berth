# CLI Reference

Core commands:

```text
berth search <query>
berth info <server>
berth list
berth install <server[@version]>
berth uninstall <server>
berth update <server|--all>
berth config <server>
berth config <server> --interactive
berth config export [file]
berth config import <file>
```

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
