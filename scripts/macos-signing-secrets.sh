#!/usr/bin/env bash
# Stores the macOS signing and notarization credentials as GitHub repository secrets, so the
# release workflow signs releases with a Developer ID and has them notarized by Apple.
#
# Usage: scripts/macos-signing-secrets.sh [--check] <DeveloperID.p12>
#
#   <DeveloperID.p12>  a "Developer ID Application" certificate and its private key, exported from
#                      Keychain Access (My Certificates → right-click → Export…) with a password.
#   --check            validate everything, but don't change any secret.
#
# Notarization credentials, the first of:
#   APPLE_API_KEY + APPLE_API_ISSUER + APPLE_API_KEY_PATH   an App Store Connect API key (.p8)
#   APPLE_ID [+ APPLE_PASSWORD]   an Apple ID in the team with an app-specific password
#                                 (account.apple.com → Sign-In and Security); asked for if missing
# The p12 password is read from P12_PASSWORD, or asked for.
set -euo pipefail

cd "$(dirname "$0")/.."
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
say() { printf '\033[1m==> %s\033[0m\n' "$*"; }

CHECK=false
[[ "${1:-}" == "--check" ]] && { CHECK=true; shift; }
P12=${1:-}
[[ -f "$P12" ]] || die "usage: scripts/macos-signing-secrets.sh [--check] <DeveloperID.p12>"
[[ "$(uname)" == Darwin ]] || die "this script runs on macOS"
command -v gh >/dev/null || die "the GitHub CLI (gh) is required"
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner) || die "gh can't see this repository (gh auth login)"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# --- the certificate ------------------------------------------------------------------------
if [[ -z "${P12_PASSWORD:-}" ]]; then
  read -rsp "Password of $(basename "$P12"): " P12_PASSWORD && echo
fi
export P12_PASSWORD
# macOS's LibreSSL reads the legacy encryption Keychain Access uses; OpenSSL 3 needs -legacy.
pkcs12() {
  /usr/bin/openssl pkcs12 -in "$P12" -passin env:P12_PASSWORD "$@" 2>/dev/null ||
    openssl pkcs12 -legacy -in "$P12" -passin env:P12_PASSWORD "$@" 2>/dev/null
}
pkcs12 -nokeys -clcerts -out "$TMP/cert.pem" || die "can't read $(basename "$P12") (wrong password?)"
pkcs12 -nocerts -nodes | grep -c "PRIVATE KEY" >/dev/null ||
  die "$(basename "$P12") has no private key: export the identity from Keychain Access → My Certificates, not just the certificate"

subject=$(openssl x509 -in "$TMP/cert.pem" -noout -subject -nameopt multiline,utf8,-esc_msb)
IDENTITY=$(sed -n 's/^ *commonName *= *//p' <<<"$subject" | head -1)
TEAM_ID=$(sed -n 's/^ *organizationalUnitName *= *//p' <<<"$subject" | head -1)
[[ "$IDENTITY" == "Developer ID Application: "* ]] ||
  die "'$IDENTITY' is not a Developer ID Application certificate (other kinds can't be notarized)"
[[ -n "$TEAM_ID" ]] || die "no team ID in the certificate"
openssl x509 -in "$TMP/cert.pem" -noout -checkend 0 >/dev/null || die "the certificate has expired"
verdict=$(security verify-cert -c "$TMP/cert.pem" -p codeSign -R ocsp -l 2>&1 || true)
if grep -q "REVOKED" <<<"$verdict"; then
  die "Apple has revoked this certificate; create a new one"
elif ! grep -qi "successful" <<<"$verdict"; then
  warn "could not confirm the certificate with Apple (${verdict##*: })"
fi
say "certificate: $IDENTITY, valid until $(openssl x509 -in "$TMP/cert.pem" -noout -enddate | cut -d= -f2)"

# --- notarization ---------------------------------------------------------------------------
SECRETS=(APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_TEAM_ID)
APPLE_CERTIFICATE=$(base64 -i "$P12" | tr -d '\n')
APPLE_CERTIFICATE_PASSWORD=$P12_PASSWORD
APPLE_SIGNING_IDENTITY=$IDENTITY
if [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
  [[ -f "$APPLE_API_KEY_PATH" ]] || die "no file at APPLE_API_KEY_PATH"
  APPLE_API_KEY_P8=$(cat "$APPLE_API_KEY_PATH")
  NOTARY=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")
  SECRETS+=(APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_P8)
else
  if [[ -z "${APPLE_ID:-}" ]]; then
    read -rp "Apple ID to notarize with (empty: sign without notarizing): " APPLE_ID
  fi
  if [[ -n "$APPLE_ID" && -z "${APPLE_PASSWORD:-}" ]]; then
    read -rsp "App-specific password for $APPLE_ID: " APPLE_PASSWORD && echo
  fi
  NOTARY=()
  if [[ -n "$APPLE_ID" ]]; then
    NOTARY=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$TEAM_ID")
    SECRETS+=(APPLE_ID APPLE_PASSWORD)
  fi
fi
if ((${#NOTARY[@]})); then
  xcrun notarytool history "${NOTARY[@]}" >/dev/null 2>"$TMP/notary.err" ||
    die "Apple rejected the notarization credentials: $(tail -1 "$TMP/notary.err")"
  say "notarization credentials work"
else
  warn "no notarization credentials: releases will be signed but not notarized (Gatekeeper still warns)"
fi

# --- secrets --------------------------------------------------------------------------------
if $CHECK; then
  say "all good; would set ${SECRETS[*]} on $REPO"
  exit 0
fi
for name in "${SECRETS[@]}"; do
  # Through stdin, so no secret shows up in the process list.
  printf '%s' "${!name}" | gh secret set "$name" --repo "$REPO" >/dev/null
  echo "  set $name"
done
say "done: the next release (npm run release -- <version>) is signed${NOTARY:+ and notarized}"
