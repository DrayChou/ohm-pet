# OHM Pet

[简体中文](README.zh-CN.md) | English

OHM Pet is a lightweight, WebView-free desktop companion written in Rust. It runs Codex v2 pet packages on macOS and Windows without requiring Codex to remain open.

## Features

- Transparent, borderless native window
- Optional always-on-top mode
- Whole-pet dragging and click-to-jump
- Pointer-directed gaze within 1.5 times the pet body size
- Low-frequency, context-aware idle behavior
- Native tray menu and runtime pet switching
- Automatic discovery of local, Codex, and Claude-compatible pet directories
- One-click Claude Code, Codex, and Pi Agent lifecycle integration
- Saved pet selection, window position, and topmost preference
- No browser engine and no continuous 60 FPS render loop

The macOS renderer uses AppKit. The Windows test backend uses a native Win32 layered window with per-pixel alpha. Windows GUI builds use the Windows subsystem, so double-clicking `OHM Pet.exe` does not open a console window.

## Default pet and repository layout

The default distribution contains only OHM-1 Raven:

```text
assets/default-pets/ohm-raven/
├── pet.json
└── spritesheet.webp
```

Important directories:

```text
assets/default-pets/   Default pet source files tracked in this repository
crates/ohm-pet-core/   Pet catalog, atlas, behavior, state, and preferences
crates/ohm-pet-desktop/ Native macOS and Windows desktop runtime
packaging/             macOS and Windows icons and package metadata
scripts/               Icon generation and platform packaging scripts
docs/                  Architecture and implementation notes
dist/                  Local build output, ignored by Git
```

The generated macOS and Windows packages expose a user-facing `pets/` directory. Put additional Codex-compatible packages there, then choose `刷新宠物目录` from the tray menu. See [Pet packages](docs/pet-packages.md) for the complete format and discovery rules.

## Pet discovery

OHM Pet merges all compatible packages it finds. If two packages use the same `id`, the earlier directory wins:

1. `OHM_PET_HOME`
2. `assets/default-pets/` during development
3. `pets/` beside `OHM Pet.exe` or `OHM Pet.app`
4. `${CODEX_HOME:-~/.codex}/pets`
5. `${CLAUDE_CONFIG_DIR:-~/.claude}/pets`
6. the platform Claude application-support `pets/` directory
7. bundled fallback pets inside the macOS app

Claude Code does not currently define a built-in pet package format. Claude pet directories discovered by OHM Pet accept the Codex v2 contract described below. Shimeji packages that use `actions.xml`, `behaviors.xml`, and separate PNG frames are not yet supported.

## Agent integrations

Open `Agent 集成` in the tray menu to install or update integrations for Claude Code, Codex, and Pi Agent. OHM Pet uses local lifecycle hooks and a localhost UDP signal instead of terminal scraping or log polling.

Claude Code and Pi provide detailed working, waiting, completed, failed, and idle events. Codex currently exposes only `agent-turn-complete` through its official `notify` mechanism, so the Codex integration reports completion and preserves any previous notify command.

See [Agent integrations](docs/agent-integrations.md) for installation paths, event mapping, custom signal commands, and removal behavior.

## Pet package contract

```text
pets/<pet-id>/
├── pet.json
└── spritesheet.webp
```

Sprite version 2 uses an 8 by 11 atlas at 1536 by 2288 pixels, with 192 by 208 pixel cells.

## Development

```bash
cargo run -p ohm-pet
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Override the pet directories for a development run:

```bash
OHM_PET_HOME=/path/to/pets cargo run -p ohm-pet
```

Regenerate the OHM Raven application icons:

```bash
python3 scripts/generate-icons.py
```

## Build macOS

```bash
./scripts/build-macos-app.sh
open "dist/OHM Pet.app"
```

The script creates an ad-hoc signed application and an external `dist/pets/` directory.

## Build Windows

On Windows:

```powershell
./scripts/package-windows.ps1
```

The portable archive is written to `dist/windows/OHM-Pet-windows-x64.zip`. GitHub Actions also publishes an `OHM-Pet-windows-x64` artifact for every push to `main`.

## Resource model

- The native event loop sleeps between deadlines.
- Idle animation advances every 480 milliseconds.
- Global pointer position is sampled every 100 milliseconds.
- A redraw occurs only when the visible frame changes.
- macOS frames are lazily converted to native images and cached.
- No Node.js, WebView, or browser process remains resident.

## License

MIT
