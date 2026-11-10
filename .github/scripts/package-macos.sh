#!/usr/bin/env bash
# Packages dist/Runecast.app as dist/Runecast-<version>-macos.zip and .dmg, signing the DMG when
# IDENTITY names a Developer ID. Staple runs it again on the stapled app to replace them.
# Runecast-latest-macos.dmg is the same DMG under a fixed name, so
# releases/latest/download/Runecast-latest-macos.dmg always fetches the newest release.
set -euo pipefail

version=$1
cd dist
rm -rf "Runecast-$version-macos.zip" "Runecast-$version-macos.dmg" Runecast-latest-macos.dmg
ditto -c -k --keepParent Runecast.app "Runecast-$version-macos.zip"
# The window shows Runecast beside an Applications link over a background that says to drag it there.
pipx run --spec dmgbuild==1.6.7 dmgbuild -s ../.github/scripts/dmg-settings.py Runecast "Runecast-$version-macos.dmg"
if [[ -n ${IDENTITY:-} ]]; then
  codesign --sign "$IDENTITY" --timestamp "Runecast-$version-macos.dmg"
fi
cp "Runecast-$version-macos.dmg" Runecast-latest-macos.dmg
