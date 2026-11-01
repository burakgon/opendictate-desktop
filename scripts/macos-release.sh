#!/usr/bin/env bash
# Builds OpenDictate for macOS, signed - and notarized when credentials are available - as a
# universal app (Apple Silicon + Intel) plus a DMG.
#
# Signing identity: $APPLE_SIGNING_IDENTITY, else the first "Developer ID Application" identity in
# the keychain, else ad-hoc ("-"). The certificate is checked with Apple first: macOS blocks (and
# may move to the Trash) apps signed with a revoked certificate.
#
# Notarization (requires a Developer ID identity), the first of:
#   NOTARY_PROFILE=<name>                            saved with `xcrun notarytool store-credentials`
#   APPLE_API_KEY + APPLE_API_ISSUER + APPLE_API_KEY_PATH   an App Store Connect API key (.p8)
#   APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID        an app-specific password
#
# Update archives for the auto-updater are built by the GitHub release workflow, not here.
#
# Usage: scripts/macos-release.sh [--native]    (--native: only this Mac's architecture)
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)
TARGET=universal-apple-darwin
[[ "${1:-}" == "--native" ]] && TARGET=""

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$(uname)" == Darwin ]] || die "this script runs on macOS"

# --- signing identity -----------------------------------------------------------------------
identities=$(security find-identity -v -p codesigning 2>/dev/null || true)
# Lines look like `  1) <SHA-1> "<name>"`, followed by ` (CSSMERR_…)` once macOS knows the
# certificate is revoked or otherwise unusable.
matching() { grep -F "\"$1" <<<"$identities" || true; }
usable() { grep -v '" (CSSMERR' || true; }
as_hash_name() { head -1 | sed -E 's/^ *[0-9]+\) ([0-9A-F]{40}) "([^"]*)".*$/\1 \2/'; }
if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  [[ -n "$(matching "$APPLE_SIGNING_IDENTITY")" ]] || die "no signing identity named '$APPLE_SIGNING_IDENTITY' in the keychain"
  chosen=$(matching "$APPLE_SIGNING_IDENTITY" | usable | as_hash_name)
  [[ -n "$chosen" ]] || die "the certificate '$APPLE_SIGNING_IDENTITY' can't be used: $(matching "$APPLE_SIGNING_IDENTITY" | head -1 | sed -E 's/.*\((CSSMERR[^)]*)\).*/\1/'). Create a new one (Xcode → Settings → Accounts → Manage Certificates)."
else
  chosen=$(matching "Developer ID Application:" | usable | as_hash_name)
fi
IDENTITY="-"
if [[ -n "$chosen" ]]; then
  IDENTITY_HASH=${chosen%% *}
  IDENTITY=${chosen#* }
fi
DEVELOPER_ID=false
[[ "$IDENTITY" == "Developer ID Application:"* ]] && DEVELOPER_ID=true

# The PEM of the keychain certificate with the given SHA-1 hash.
pem_for_hash() {
  security find-certificate -a -Z -p 2>/dev/null | awk -v h="$1" '
    /^SHA-1 hash:/ { keep = ($3 == h) }
    /BEGIN CERTIFICATE/ { inpem = 1 }
    keep && inpem { print }
    /END CERTIFICATE/ { inpem = 0; if (keep) exit }'
}

if [[ "$IDENTITY" == "-" ]]; then
  warn "no Developer ID Application identity: ad-hoc signing (other Macs will say the developer can't be verified)"
else
  pem=$(mktemp)
  pem_for_hash "$IDENTITY_HASH" > "$pem"
  verdict=$(security verify-cert -c "$pem" -p codeSign -R ocsp -l 2>&1 || true)
  rm -f "$pem"
  if grep -q "REVOKED" <<<"$verdict"; then
    die "the certificate '$IDENTITY' has been revoked by Apple. Create a new one (Xcode → Settings → Accounts → Manage Certificates) and remove the revoked one from the keychain."
  elif ! grep -qi "successful" <<<"$verdict"; then
    warn "could not confirm the certificate with Apple (${verdict##*: }); signing anyway"
  fi
  $DEVELOPER_ID || warn "'$IDENTITY' is not a Developer ID: the app can't be notarized or shared"
  say "signing with '$IDENTITY'"
fi

# --- notarization credentials ---------------------------------------------------------------
NOTARY_ARGS=()
if [[ -n "${NOTARY_PROFILE:-}" ]]; then
  NOTARY_ARGS=(--keychain-profile "$NOTARY_PROFILE")
elif [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
  NOTARY_ARGS=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")
elif [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  NOTARY_ARGS=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
fi
NOTARIZE=false
if $DEVELOPER_ID && ((${#NOTARY_ARGS[@]})); then
  NOTARIZE=true
elif $DEVELOPER_ID; then
  warn "no notarization credentials (NOTARY_PROFILE, APPLE_API_* or APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID): the app is signed but not notarized"
fi

# --- build ----------------------------------------------------------------------------------
if [[ -n "$TARGET" ]]; then
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target list --installed | grep -qx "$t" || rustup target add "$t"
  done
fi
say "building${TARGET:+ ($TARGET)}"
# Tauri signs the bundle (hardened runtime + src-tauri/Entitlements.plist). Notarization happens
# below, so the app and the DMG are handled the same way whatever the credentials are.
(
  export APPLE_SIGNING_IDENTITY="${IDENTITY_HASH:-$IDENTITY}"
  unset APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_ID APPLE_PASSWORD APPLE_CERTIFICATE
  npx tauri build --bundles app ${TARGET:+--target "$TARGET"}
)
BUNDLE_DIR="$ROOT/src-tauri/target/${TARGET:+$TARGET/}release/bundle/macos"
APP="$BUNDLE_DIR/OpenDictate.app"
[[ -d "$APP" ]] || die "no app bundle at $APP"
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP/Contents/Info.plist")
ARCH_NAME=${TARGET:+universal}
ARCH_NAME=${ARCH_NAME:-$(uname -m)}
OUT="$ROOT/src-tauri/target/release/macos-release"
rm -rf "$OUT" && mkdir -p "$OUT"

notarize() { # <file to submit> <file to staple>
  say "notarizing $(basename "$1") (this takes a few minutes)"
  xcrun notarytool submit "$1" "${NOTARY_ARGS[@]}" --wait --timeout 30m | tee "$OUT/notarytool.log"
  grep -q "status: Accepted" "$OUT/notarytool.log" || die "notarization failed; details: xcrun notarytool log <id> ${NOTARY_ARGS[*]}"
  xcrun stapler staple "$2"
}

if [[ "$IDENTITY" != "-" ]]; then
  # Re-sign with a secure timestamp from Apple (Tauri doesn't add one; notarization requires it).
  codesign --force --options runtime --timestamp --entitlements "$ROOT/src-tauri/Entitlements.plist" -s "$IDENTITY_HASH" "$APP"
fi
codesign --verify --deep --strict "$APP"
if $NOTARIZE; then
  ditto -c -k --keepParent "$APP" "$OUT/OpenDictate.zip"
  notarize "$OUT/OpenDictate.zip" "$APP"
  rm "$OUT/OpenDictate.zip"
fi

# --- DMG ------------------------------------------------------------------------------------
DMG="$OUT/OpenDictate_${VERSION}_${ARCH_NAME}.dmg"
say "creating $(basename "$DMG")"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/OpenDictate.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "OpenDictate" -srcfolder "$STAGE" -ov -format UDZO -fs HFS+ "$DMG"
if [[ "$IDENTITY" != "-" ]]; then
  codesign --force --timestamp -s "$IDENTITY_HASH" "$DMG"
fi
$NOTARIZE && notarize "$DMG" "$DMG"
ditto "$APP" "$OUT/OpenDictate.app"

# --- report ---------------------------------------------------------------------------------
say "result"
codesign -dvv "$OUT/OpenDictate.app" 2>&1 | grep -E "^(Authority|TeamIdentifier|Timestamp|Runtime Version)=" || true
lipo -archs "$OUT/OpenDictate.app/Contents/MacOS/opendictate" | sed 's/^/Architectures: /'
if spctl --assess --type execute "$OUT/OpenDictate.app" 2>/dev/null; then
  echo "Gatekeeper: accepted ($(spctl --assess --type execute -vv "$OUT/OpenDictate.app" 2>&1 | sed -n 's/.*source=//p'))"
else
  echo "Gatekeeper: not accepted on other Macs yet (needs a notarized Developer ID signature)"
fi
echo "App: $OUT/OpenDictate.app"
echo "DMG: $DMG"
