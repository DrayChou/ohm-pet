#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

./scripts/build-macos-app.sh
OUT="$ROOT/dist/macos"
STAGE="$OUT/OHM-Pet-macos"
rm -rf "$STAGE"
mkdir -p "$STAGE/pets"
cp -R "$ROOT/dist/OHM Pet.app" "$STAGE/"
cp -R "$ROOT/assets/default-pets/." "$STAGE/pets/"
cp "$ROOT/README.md" "$ROOT/README.zh-CN.md" "$STAGE/"
find "$STAGE" -name .DS_Store -delete
mkdir -p "$OUT"
rm -f "$OUT/OHM-Pet-macos.zip"
ditto -c -k --sequesterRsrc --keepParent "$STAGE" "$OUT/OHM-Pet-macos.zip"
echo "$OUT/OHM-Pet-macos.zip"
