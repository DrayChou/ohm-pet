#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/dist/OHM Pet.app"
CONTENTS="$APP/Contents"

cd "$ROOT"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
for target in "${TARGETS[@]}"; do
  if ! rustup target list --installed | grep -qx "$target"; then
    echo "missing Rust target: $target (run: rustup target add $target)" >&2
    exit 1
  fi
  MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release -p ohm-pet --target "$target"
done
rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources/pets"
lipo -create \
  "$ROOT/target/aarch64-apple-darwin/release/ohm-pet" \
  "$ROOT/target/x86_64-apple-darwin/release/ohm-pet" \
  -output "$CONTENTS/MacOS/OHM Pet"
cp -R assets/default-pets/. "$CONTENTS/Resources/pets/"
rm -rf "$ROOT/dist/pets"
mkdir -p "$ROOT/dist/pets"
cp -R assets/default-pets/. "$ROOT/dist/pets/"
cp packaging/macos/Info.plist "$CONTENTS/Info.plist"
if [[ -f packaging/macos/OHMPet.icns ]]; then
  cp packaging/macos/OHMPet.icns "$CONTENTS/Resources/OHMPet.icns"
fi
codesign --force --deep --sign - "$APP"
ARCHS="$(lipo -archs "$CONTENTS/MacOS/OHM Pet")"
for architecture in arm64 x86_64; do
  if [[ " $ARCHS " != *" $architecture "* ]]; then
    echo "missing macOS architecture: $architecture (found: $ARCHS)" >&2
    exit 1
  fi
done
if [[ "$(plutil -extract LSMinimumSystemVersion raw "$CONTENTS/Info.plist")" != "11.0" ]]; then
  echo "unexpected LSMinimumSystemVersion" >&2
  exit 1
fi
codesign --verify --deep --strict --verbose=2 "$APP"
echo "$APP"
