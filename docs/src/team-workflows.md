# Team Workflows

Use config bundles to share reproducible local setup across a team.

## Export

```bash
berth config export team-berth.toml
```

## Import

```bash
berth config import team-berth.toml
```

## Suggested Flow

1. team lead prepares baseline server installs and config values
2. team lead exports bundle and shares it through internal channels
3. teammates import bundle and run `berth start`
4. each developer sets personal secrets locally as needed
