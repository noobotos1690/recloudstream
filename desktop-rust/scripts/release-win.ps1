#! /usr/bin/env bash
# -*- coding: utf-8 -*-
set -euo pipefail
set -o pipefail 2>/dev/null || true

# Release the CloudStream Desktop Windows installer to GitHub in one shot.
# Prerequisites (all optional, picked up from the environment if set):
#   GH_TOKEN          GitHub PAT with repo write on this repo
#   RUNNER_TEMP       a writable temp dir (defaults to mktemp -d when absent)
#
# What it does:
#   1. Finds the latest Windows installer under installer/Output/ (or RELEASE_ASSET
#      if set). Fails early if nothing is there.
#   2. Uses gh release create --generate-notes so GitHub writes the release notes
#      from the commit history. The release NOTES live in RELEASE_NOTES.md in this
#      repo and are shown to the user BEFORE gh runs so they can read / edit the
#      draft.
#   3. Publishes the release (not a draft) to the current branch's HEAD.

REPO="${GITHUB_REPOSITORY:-$(git remote get-url origin 2>/dev/null | sed -n 's|.*github.com[:/]\([^.]*\/\([^.]*\)\..*|\1/\2|p')}"
[[ -n "$REPO" ]] || { echo "No GITHUB_REPOSITORY and no origin remote; aborting."; exit 2; }

cd "$(dirname "$0")/.."

ASSET="${RELEASE_ASSET:-}"
if [[ -z "$ASSET" ]]; then
  ASSET=$(find installer/Output -type f \
           -name 'CloudStreamDesktop-*.exe' \
           -printf '%T@ %p\n' 2>/dev/null \
           | sort -rn \
           | head -n1 \
           | sed 's/^[^ ]* //')
fi
[[ -n "$ASSET" && -f "$ASSET" ]] || { echo "No installer found (looked in installer/Output/ and RELEASE_ASSET=$ASSET)."; exit 1; }

TAG="v1.3.0"
NOTES="RELEASE_NOTES.md"
[[ -f "$NOTES" ]] || { echo "RELEASE_NOTES.md missing."; exit 2; }

echo "---- RELEASE NOTES (editable before publish) ----"
cat "$NOTES"
echo "--------------------------------------------------"
printf "\nPushing release %s with asset %s\n" "$TAG" "$(basename "$ASSET")"

if [[ -n "${GH_TOKEN:-}" ]]; then
  export GH_TOKEN
fi

gh release create "$TAG" --target HEAD --title "v1.3.0" --notes-file "$NOTES" --generate-notes --verify-tag -- json "$ASSET"

echo "Released $(basename "$ASSET") as https://github.com/$REPO/releases/tag/$TAG"
