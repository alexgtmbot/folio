#!/bin/sh
set -eu
cd "$(dirname "$0")"
cargo build --release
bundle=target/Folio.app/Contents
mkdir -p "$bundle/MacOS"
cp Info.plist "$bundle/"
swiftc -O -module-name Folio -import-objc-header Folio.h main.swift target/release/libfolio.a -o "$bundle/MacOS/Folio"
strip -x "$bundle/MacOS/Folio"
xattr -cr target/Folio.app
codesign --force --sign - target/Folio.app
