#!/bin/sh
set -eu
cd "$(dirname "$0")"
[ "$(uname -m)" = arm64 ] || { echo "Build this package on Apple Silicon." >&2; exit 1; }
./build.sh
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/image"
app="$stage/image/Folio.app"
ditto target/Folio.app "$app"
if [ -n "${SIGN_IDENTITY:-}" ]; then
    codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$app"
fi
codesign --verify --strict "$app"
"$app/Contents/MacOS/Folio" --self-test
ln -s /Applications "$stage/image/Applications"
version=$(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist")
file="Folio-$version-macOS-arm64.dmg"
hdiutil create -quiet -format UDZO -volname Folio -srcfolder "$stage/image" "$stage/$file"
hdiutil verify "$stage/$file"
mv "$stage/$file" "target/$file"
echo "target/$file"
