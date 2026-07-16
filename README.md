# OHM Pet

OHM Pet is a WebView-free Rust desktop companion for macOS and Windows. It runs existing v2 pet atlases independently from Codex and sleeps between low-frequency animation deadlines.

## Current status

The macOS runtime is functional with:

- transparent, borderless, always-on-top native window
- AppKit-native image composition
- idle and state animations
- pointer-directed gaze
- click-to-jump
- whole-pet click-and-drag repositioning
- click-vs-drag detection so a stationary click still jumps
- native status menu with persisted always-on-top control
- runtime pet switching
- saved pet selection and window position
- context-aware low-frequency behavior based on idle time, pointer proximity and recent interaction

The Windows runtime uses a native Win32 layered window with per-pixel alpha. GitHub Actions publishes a portable Windows x64 test bundle for every push to `main`.

## Included pet

The default distribution includes only OHM-1「欧姆鸦」. Add any Codex v2-compatible pet package under the external `pets/` directory and choose “刷新宠物目录” from the tray menu.

## Development

```bash
cargo run -p ohm-pet
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Pet discovery order:

1. `OHM_PET_HOME` when set
2. `pets/` beside `OHM Pet.exe` on Windows
3. `pets/` beside `OHM Pet.app` on macOS
4. bundled fallback pets inside the macOS app

Override the pet package directory with:

```bash
OHM_PET_HOME=/path/to/pets cargo run -p ohm-pet
```

## Build the macOS app

```bash
./scripts/build-macos-app.sh
open "dist/OHM Pet.app"
```

The script creates an ad-hoc signed application at `dist/OHM Pet.app`.

## Build the Windows test bundle

On Windows:

```powershell
./scripts/package-windows.ps1
```

The portable archive is written to `dist/windows/OHM-Pet-windows-x64.zip`. The `Build` GitHub Actions workflow also publishes it as the `OHM-Pet-windows-x64` artifact.

## Pet package contract

```text
pets/<pet-id>/
├── pet.json
└── spritesheet.webp
```

Sprite version 2 uses an 8×11 atlas at 1536×2288 pixels with 192×208 cells.

## Resource model

- No browser or WebView
- No continuous 60 FPS render loop
- Native event loop sleeps with `WaitUntil`
- Idle animation changes frame every 480 ms
- Static frames trigger no redraw
- Frames are lazily converted to native images and cached
