#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/dist/OHM Pet.app"
CONTENTS="$APP/Contents"

cd "$ROOT"
cargo build --release -p ohm-pet
rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources/pets"
cp target/release/ohm-pet "$CONTENTS/MacOS/OHM Pet"
cp -R pets/. "$CONTENTS/Resources/pets/"
rm -rf "$ROOT/dist/pets"
mkdir -p "$ROOT/dist/pets"
cp -R pets/. "$ROOT/dist/pets/"
cp packaging/macos/Info.plist "$CONTENTS/Info.plist"
if [[ -f packaging/macos/OHMPet.icns ]]; then
  cp packaging/macos/OHMPet.icns "$CONTENTS/Resources/OHMPet.icns"
fi
codesign --force --deep --sign - "$APP"
echo "$APP"
