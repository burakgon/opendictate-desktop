#!/usr/bin/env bash
# Renews the Developer ID certificate that releases are signed with (Apple issues them for about
# a year) and stores it where the release workflow and this Mac's keychain use it. You click
# through one page on developer.apple.com; this script does the rest.
#
# Usage: scripts/macos-renew-certificate.sh
#
# Needs: macOS, the GitHub CLI signed in to this repository. The notarization secrets stay as they are.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
say() { printf '\033[1m==> %s\033[0m\n' "$*"; }

[[ "$(uname)" == Darwin ]] || die "this script runs on macOS"
command -v gh >/dev/null || die "the GitHub CLI (gh) is required"
gh repo view --json nameWithOwner >/dev/null 2>&1 || die "gh can't see this repository (gh auth login)"

umask 077
WORK=$(mktemp -d)
CSR="$HOME/Desktop/OpenDictate.certSigningRequest"
trap 'rm -rf "$WORK"; rm -f "$CSR"' EXIT

# A new key pair; the certificate Apple issues for its signing request completes the identity.
/usr/bin/openssl genrsa -out "$WORK/key.pem" 2048 2>/dev/null
/usr/bin/openssl req -new -key "$WORK/key.pem" -subj "/CN=OpenDictate Developer ID" -out "$CSR"
chmod 644 "$CSR"
modulus=$(/usr/bin/openssl rsa -in "$WORK/key.pem" -noout -modulus | shasum -a 256)
touch "$WORK/started"

say "create the certificate on developer.apple.com (opening the page)"
cat <<'EOF'
   1. Select "Developer ID Application", then Continue.
   2. Select "G2 Sub-CA (Xcode 11.4.1 or later)".
   3. Choose File → Desktop → OpenDictate.certSigningRequest, then Continue.
   4. Download.
   Waiting for the downloaded certificate in Downloads or on the Desktop…
EOF
open "https://developer.apple.com/account/resources/certificates/add"

cer=""
for _ in $(seq 900); do # 30 minutes
  while IFS= read -r f; do
    if [[ "$(/usr/bin/openssl x509 -inform DER -in "$f" -noout -modulus 2>/dev/null | shasum -a 256)" == "$modulus" ]]; then
      cer=$f
      break
    fi
  done < <(find "$HOME/Downloads" "$HOME/Desktop" -maxdepth 1 -iname '*.cer' -newer "$WORK/started" 2>/dev/null)
  [[ -n "$cer" ]] && break
  sleep 2
done
[[ -n "$cer" ]] || die "no certificate for this request arrived in Downloads or on the Desktop"
sleep 1 # let the browser finish writing it

/usr/bin/openssl x509 -inform DER -in "$cer" -out "$WORK/cert.pem"
NAME=$(/usr/bin/openssl x509 -in "$WORK/cert.pem" -noout -subject -nameopt multiline,utf8,-esc_msb | sed -n 's/^ *commonName *= *//p' | head -1)
[[ "$NAME" == "Developer ID Application: "* ]] || die "'$NAME' is not a Developer ID Application certificate"
say "received '$NAME', valid until $(/usr/bin/openssl x509 -in "$WORK/cert.pem" -noout -enddate | cut -d= -f2)"

# The intermediate that issued it, so the chain is complete wherever the identity is imported.
if /usr/bin/openssl x509 -in "$WORK/cert.pem" -noout -issuer | grep -q "OU *= *G2"; then
  ca=https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer
else
  ca=https://www.apple.com/certificateauthority/DeveloperIDCA.cer
fi
curl -fsSL "$ca" | /usr/bin/openssl x509 -inform DER -out "$WORK/ca.pem"

P12_PASSWORD=$(/usr/bin/openssl rand -base64 24 | tr -d '\n/+=')
export P12_PASSWORD
# macOS' LibreSSL writes the legacy encryption `security import` reads.
/usr/bin/openssl pkcs12 -export -inkey "$WORK/key.pem" -in "$WORK/cert.pem" -certfile "$WORK/ca.pem" \
  -name "$NAME" -passout env:P12_PASSWORD -out "$WORK/DeveloperID.p12"
security import "$WORK/DeveloperID.p12" -k "$HOME/Library/Keychains/login.keychain-db" -P "$P12_PASSWORD" \
  -T /usr/bin/codesign -T /usr/bin/security >/dev/null
say "added it to your keychain, for local builds (npm run release:mac)"

"$ROOT/scripts/macos-signing-secrets.sh" --certificate-only "$WORK/DeveloperID.p12"
rm -f "$cer"
