#!/usr/bin/env bash
# Writes latest.json - what running copies of OpenDictate check for updates - for a GitHub release,
# from the signed update files (*.sig) its builds uploaded, and attaches it to the release.
#
# Usage: scripts/latest-json.sh <tag>        e.g. scripts/latest-json.sh v0.2.1
#
# The release workflow runs it once every platform is uploaded. Needs gh (GH_TOKEN) and jq.
set -euo pipefail

TAG=${1:?usage: scripts/latest-json.sh <tag>}
REPO=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

if ! gh release download "$TAG" --repo "$REPO" --pattern '*.sig' --dir "$WORK" 2>"$WORK/error"; then
  grep -q "no assets match" "$WORK/error" || { cat "$WORK/error" >&2; exit 1; }
fi
shopt -s nullglob
sigs=("$WORK"/*.sig)
if ((${#sigs[@]} == 0)); then
  echo "::warning::$TAG has no signed update files (no TAURI_SIGNING_PRIVATE_KEY secret?), so no latest.json" >&2
  exit 0
fi

# The keys the updater looks for: `<os>-<arch>-<installer>`, then `<os>-<arch>`.
platforms='{}'
add() { # <platform> <asset>
  platforms=$(jq -c --arg p "$1" --arg url "https://github.com/$REPO/releases/download/$TAG/$2" \
    --rawfile sig "$WORK/$2.sig" '.[$p] = {signature: $sig, url: $url}' <<<"$platforms")
}
for sig in "${sigs[@]}"; do
  asset=$(basename "$sig" .sig)
  case "$asset" in
    *_aarch64.app.tar.gz) add darwin-aarch64 "$asset"; add darwin-aarch64-app "$asset" ;;
    *_x64.app.tar.gz) add darwin-x86_64 "$asset"; add darwin-x86_64-app "$asset" ;;
    *_amd64.AppImage) add linux-x86_64 "$asset"; add linux-x86_64-appimage "$asset" ;;
    *_amd64.deb) add linux-x86_64-deb "$asset" ;;
    *.x86_64.rpm) add linux-x86_64-rpm "$asset" ;;
    *_x64_*.msi) add windows-x86_64 "$asset"; add windows-x86_64-msi "$asset" ;;
    *_x64-setup.exe) add windows-x86_64-nsis "$asset" ;;
    *) echo "::warning::don't know which platform $asset.sig is for" >&2 ;;
  esac
done
missing=$(jq -r '["darwin-aarch64", "darwin-x86_64", "linux-x86_64", "windows-x86_64"] - keys | join(", ")' <<<"$platforms")
if [[ -n "$missing" ]]; then
  echo "::error::$TAG has no update files for $missing" >&2
  exit 1
fi

gh release view "$TAG" --repo "$REPO" --json body -q .body >"$WORK/notes.md"
jq -n --arg version "${TAG#v}" --rawfile notes "$WORK/notes.md" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson platforms "$platforms" '{version: $version, notes: $notes, pub_date: $date, platforms: $platforms}' \
  >"$WORK/latest.json"
gh release upload "$TAG" "$WORK/latest.json" --repo "$REPO" --clobber
echo "latest.json for $TAG: $(jq -r '.platforms | keys | join(", ")' "$WORK/latest.json")"
