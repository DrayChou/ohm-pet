# Pet packages

[简体中文](pet-packages.zh-CN.md) | English

## Default asset source

The repository tracks one default pet:

```text
assets/default-pets/ohm-raven/
├── pet.json
└── spritesheet.webp
```

Packaging scripts copy this directory into the user-facing `pets/` directory. Do not place private or experimental assets under `assets/default-pets/` unless they are intended for public distribution.

## User package directory

Windows portable package:

```text
OHM-Pet/
├── OHM Pet.exe
└── pets/
```

macOS package directory:

```text
Distribution folder/
├── OHM Pet.app
└── pets/
```

Add a package under `pets/<pet-id>/`, then choose `刷新宠物目录` from the tray menu.

## Codex v2 contract

```text
pets/<pet-id>/
├── pet.json
└── spritesheet.webp
```

Required manifest fields:

```json
{
  "id": "example-pet",
  "displayName": "Example Pet",
  "description": "Short description",
  "spriteVersionNumber": 2,
  "spritesheetPath": "spritesheet.webp"
}
```

The v2 atlas is 1536 by 2288 pixels, arranged as 8 columns by 11 rows with 192 by 208 pixel cells.

## Automatic discovery

OHM Pet also reads Codex-compatible packages from:

- `${CODEX_HOME:-~/.codex}/pets`
- `${CLAUDE_CONFIG_DIR:-~/.claude}/pets`
- the platform Claude application-support `pets/` directory

Duplicate IDs are resolved by directory priority. Local and explicitly configured directories win over agent-managed directories.

Claude Code does not currently define a native pet package standard. Shimeji XML and separate-frame PNG packs are not compatible with the Codex v2 loader yet.
