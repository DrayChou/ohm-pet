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

The Windows Win32 layered-window backend is the next platform implementation.

## Included pets

- 玄甲狻猊
- 圣骨狻猊
- OHM-1「欧姆鸦」
- 鉴鉴

## Development

```bash
cargo run -p ohm-pet
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

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
