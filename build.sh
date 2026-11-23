#!/bin/sh
set -eu
cd "$(dirname "$0")"
cargo build --release --locked
bundle=target/Folio.app/Contents
mkdir -p "$bundle/MacOS" "$bundle/Resources"
cp assets/Folio.icns assets/fonts/UFL.txt LICENSE "$bundle/Resources/"
cp Info.plist "$bundle/"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)
plutil -insert CFBundleShortVersionString -string "$version" "$bundle/Info.plist"
cp target/release/folio "$bundle/MacOS/Folio"
strip -x "$bundle/MacOS/Folio"
xattr -cr target/Folio.app
codesign --force --sign - target/Folio.app
