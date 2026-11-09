#!/usr/bin/env bash
# Imports the Developer ID certificate in MACOS_CERT_P12 (base64 .p12) and MACOS_CERT_PASSWORD into
# a temporary keychain on a macOS runner, and prints the signing identity's name.
set -euo pipefail

keychain="$RUNNER_TEMP/release.keychain-db"
password=$(openssl rand -hex 16)
printf '%s' "$MACOS_CERT_P12" | base64 --decode > "$RUNNER_TEMP/cert.p12"
{
  security create-keychain -p "$password" "$keychain"
  security set-keychain-settings -lut 3600 "$keychain"
  security unlock-keychain -p "$password" "$keychain"
  security import "$RUNNER_TEMP/cert.p12" -k "$keychain" -P "$MACOS_CERT_PASSWORD" -T /usr/bin/codesign
  # The runner lacks Apple's Developer ID G2 intermediate, without which the certificate isn't
  # trusted and find-identity -v hides it.
  curl -sSfL https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer -o "$RUNNER_TEMP/g2.cer"
  security import "$RUNNER_TEMP/g2.cer" -k "$keychain"
  security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain"
  # Prepend ours to the search list; the existing keychains are meant to split into arguments.
  # shellcheck disable=SC2046
  security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
} >&2
rm "$RUNNER_TEMP/cert.p12"

identity=$(security find-identity -v -p codesigning "$keychain" | grep -o '"Developer ID Application[^"]*"' | head -1 | tr -d '"' || true)
if [[ -z $identity ]]; then
  security find-identity -p codesigning "$keychain" >&2
  echo "::error::No valid Developer ID Application identity in the certificate secret." >&2
  exit 1
fi
echo "$identity"
