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
LITE_ZIP="$OUT/OHM-Pet-macos-lite.zip"
COLLECTION_ZIP="$OUT/OHM-Pet-macos-collection.zip"
rm -f "$LITE_ZIP" "$COLLECTION_ZIP"
ditto -c -k --sequesterRsrc --keepParent "$STAGE" "$LITE_ZIP"
echo "$LITE_ZIP"
if [[ -d "$ROOT/test-fixtures/external" ]]; then
  python3 "$ROOT/scripts/make-collection-archive.py" \
    "$LITE_ZIP" "$ROOT/test-fixtures/external" "$COLLECTION_ZIP"
  python3 "$ROOT/scripts/collect-external-pets.py" \
    "$ROOT/test-fixtures/external" "$ROOT/dist/pets"
fi
