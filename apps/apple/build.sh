#!/bin/bash
# Build MailHerd for macOS: one app for Intel and Apple Silicon, packed in a .dmg.
#
# usage: apps/apple/build.sh [build-number]     (on a Mac with Xcode, Rust and XcodeGen)
# Output: dist/MailHerd.app and dist/MailHerd-<version>-<build>.dmg
set -euo pipefail

VERSION=0.1.0
BUILD=${1:-1}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
GEN="$HERE/Generated"
DIST="$ROOT/dist"
export MACOSX_DEPLOYMENT_TARGET=13.0

echo "== Rust core for Intel and Apple Silicon"
cd "$ROOT/core"
for target in x86_64-apple-darwin aarch64-apple-darwin; do
    cargo build --release --locked -p mh-ffi --target "$target"
done

rm -rf "$GEN"
mkdir -p "$GEN/include"
lipo -create \
    target/x86_64-apple-darwin/release/libmailherd_core.a \
    target/aarch64-apple-darwin/release/libmailherd_core.a \
    -output "$GEN/libmailherd_core.a"

echo "== Swift bindings"
cargo run --release --locked -p mh-ffi --features bindgen --bin uniffi-bindgen -- \
    generate --library target/aarch64-apple-darwin/release/libmailherd_core.dylib \
    --language swift --out-dir "$GEN/bindings"
mv "$GEN/bindings/mailherd_core.swift" "$GEN/"
mv "$GEN/bindings/mailherd_coreFFI.h" "$GEN/include/"
mv "$GEN/bindings/mailherd_coreFFI.modulemap" "$GEN/include/module.modulemap"
rm -rf "$GEN/bindings"

echo "== App"
cd "$HERE"
xcodegen generate --quiet
mkdir -p build
if ! xcodebuild -project MailHerd.xcodeproj -scheme MailHerd -configuration Release \
    -derivedDataPath build MARKETING_VERSION="$VERSION" CURRENT_PROJECT_VERSION="$BUILD" \
    build > build/xcodebuild.log 2>&1; then
    grep -E "error:|Undefined symbol|ld: " build/xcodebuild.log | sort -u | head -n 60
    tail -n 30 build/xcodebuild.log
    exit 1
fi
grep -E "warning:" build/xcodebuild.log | grep -v "^ld: warning: object file" | sort -u | head -n 30 || true

APP="$HERE/build/Build/Products/Release/MailHerd.app"
lipo -info "$APP/Contents/MacOS/MailHerd"

echo "== Disk image"
rm -rf "$DIST/MailHerd.app" "$DIST/dmg"
mkdir -p "$DIST/dmg"
cp -R "$APP" "$DIST/MailHerd.app"
cp -R "$APP" "$DIST/dmg/"
ln -s /Applications "$DIST/dmg/Applications"
hdiutil create -volname MailHerd -srcfolder "$DIST/dmg" -ov -format UDZO \
    "$DIST/MailHerd-$VERSION-$BUILD.dmg"
rm -rf "$DIST/dmg"
echo "Built $DIST/MailHerd-$VERSION-$BUILD.dmg"
