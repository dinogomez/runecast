#!/usr/bin/env bash
# Packages dist/Runecast.app as dist/Runecast-<version>-macos.zip and .dmg, signing the DMG when
# IDENTITY names a Developer ID. Staple runs it again on the stapled app to replace both.
set -euo pipefail

version=$1
cd dist
rm -rf dmg "Runecast-$version-macos.zip" "Runecast-$version-macos.dmg"
ditto -c -k --keepParent Runecast.app "Runecast-$version-macos.zip"
mkdir dmg
cp -R Runecast.app dmg/
ln -s /Applications dmg/Applications
hdiutil create -volname Runecast -srcfolder dmg -ov -format UDZO "Runecast-$version-macos.dmg"
rm -rf dmg
if [[ -n ${IDENTITY:-} ]]; then
  codesign --sign "$IDENTITY" --timestamp "Runecast-$version-macos.dmg"
fi
