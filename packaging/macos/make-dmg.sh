#!/bin/sh
# Builds MVB.app around a (universal) binary and packs it in a disk image.
# Usage: make-dmg.sh <binary> <version> <output directory>
set -eu

binary=$1
version=$2
output=$3
here=$(cd "$(dirname "$0")" && pwd)

work=$(mktemp -d)
app="$work/MVB.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/mvb"
chmod 755 "$app/Contents/MacOS/mvb"
cp "$here/../icons/mvb.icns" "$app/Contents/Resources/mvb.icns"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"

# Ad-hoc signature, required to run on Apple Silicon. Without an Apple Developer account the app
# is not notarized: the first launch must be allowed in System Settings > Privacy & Security.
codesign --force --deep --sign - "$app"

# Drag-and-drop installation: the app next to a link to /Applications.
ln -s /Applications "$work/Applications"
mkdir -p "$output"
hdiutil create -volname "MVB" -srcfolder "$work" -ov -format UDZO \
    "$output/mvb-$version-macos-universal.dmg"
rm -rf "$work"
