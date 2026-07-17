# OHM Pet

[简体中文](README.zh-CN.md) | English

OHM Pet is a lightweight, WebView-free desktop companion written in Rust. It runs Codex v2 pet packages on macOS and Windows without requiring Codex to remain open.

## Features

- Transparent, borderless native window
- Optional always-on-top mode
- Whole-pet dragging with an immediate lifted jump pose
- Pointer-directed gaze within 1.5 times the pet body size
- Low-frequency, context-aware idle behavior
- Native tray menu and runtime pet switching
- Automatic discovery of local, Codex, and Claude-compatible pet directories
- One-click Claude Code, Codex, and Pi Agent lifecycle integration
- Foreground-window edge detection and occasional left/right roaming
- Direct Shimeji, wl_shimeji, Ukagaka/伪春菜, and loose visual asset loading
- Ukagaka bind-overlay costumes from the tray or pet right-click menu
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

Claude Code does not currently define a built-in pet package format. Claude pet directories discovered by OHM Pet accept every format supported by the catalog, including Codex v2, Shimeji XML plus PNG frames, `.wlshm`, Ukagaka shells, and selected loose visual layouts. See [External pet formats](docs/external-pet-formats.md).

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
rustup target add aarch64-apple-darwin x86_64-apple-darwin
./scripts/package-macos.sh
open "dist/OHM Pet.app"
```

The script creates a Universal 2 application for Apple Silicon and Intel Macs with a minimum system version of macOS 11. It is ad-hoc signed; public downloads are not notarized until Apple Developer ID credentials are configured. If Gatekeeper blocks a trusted download, right-click the app and choose **Open**, or use **System Settings → Privacy & Security → Open Anyway**.

Two local archives are created when `test-fixtures/external/` is available:

- `dist/macos/OHM-Pet-macos-lite.zip`, containing only OHM Raven
- `dist/macos/OHM-Pet-macos-collection.zip`, containing OHM Raven and sanitized local test pets

## Build Windows

On Windows:

```powershell
./scripts/package-windows.ps1
```

The portable default archive is written to `dist/windows/OHM-Pet-windows-x64-lite.zip`. If local external fixtures exist, the script also writes `OHM-Pet-windows-x64-collection.zip`. GitHub Actions publishes license-safe lite Windows and macOS artifacts for every push to `main` and every pull request. Pushing a `v*` tag creates a GitHub Release with both lite platform ZIP files.

## Resource model

- The native event loop sleeps between deadlines.
- Idle animation advances every 480 milliseconds.
- Global pointer position is sampled every 100 milliseconds.
- A redraw occurs only when the visible frame changes.
- macOS frames are lazily converted to native images and cached.
- No Node.js, WebView, or browser process remains resident.

## License

MIT
