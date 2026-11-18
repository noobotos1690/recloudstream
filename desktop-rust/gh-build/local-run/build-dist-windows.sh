#!/usr/bin/env bash
set -euo pipefail

TAG="v1.3.0"
NOTES_FILE="RELEASE_NOTES.md"
ASSET_DIR="installer/Output"

if [[ ! -d "$ASSET_DIR" ]]; then
    echo "[ERROR] $ASSET_DIR does not exist — did the Inno Setup build run?"
    exit 1
fi

ASSET=$(find "$ASSET_DIR" -maxdepth 1 -type f -name 'CloudStreamDesktop-*.exe' -printf '%T@ %p\n' | sort -rn | head -n1 | sed 's/^[^ ]* //')
if [[ -z "$ASSET" ]]; then
    echo "[ERROR] No CloudStreamDesktop-*.exe found in $ASSET_DIR"
    exit 1
fi
echo "Found asset: $ASSET"

if [[ -z "${GH_TOKEN:-}" ]]; then
    echo "[ERROR] GH_TOKEN not set. Provide it as a secret or env var."
    exit 1
fi

gh release create "$TAG" --target HEAD --title "$TAG" --notes-file "$NOTES_FILE" "$ASSET"
echo "Published $TAG with $ASSET"
