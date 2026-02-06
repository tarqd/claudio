#!/bin/bash
# Build ClaudioUI as a macOS .app bundle
#
# Usage:
#   ./build.sh          # Debug build
#   ./build.sh release  # Release build
#
# Output: .build/{debug|release}/ClaudioUI.app

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

CONFIG="${1:-debug}"

if [ "$CONFIG" = "release" ]; then
    swift build -c release
    BUILD_DIR=".build/release"
else
    swift build
    BUILD_DIR=".build/debug"
fi

BINARY="$BUILD_DIR/ClaudioUI"

if [ ! -f "$BINARY" ]; then
    echo "Error: Build failed, binary not found at $BINARY"
    exit 1
fi

# Create .app bundle
APP_DIR="$BUILD_DIR/ClaudioUI.app"
CONTENTS="$APP_DIR/Contents"
MACOS="$CONTENTS/MacOS"

rm -rf "$APP_DIR"
mkdir -p "$MACOS"

# Copy binary
cp "$BINARY" "$MACOS/ClaudioUI"

# Copy Info.plist
cp Info.plist "$CONTENTS/Info.plist"

# Sign with entitlements (ad-hoc)
codesign --force --sign - --entitlements ClaudioUI.entitlements "$APP_DIR"

echo "Built: $APP_DIR"
echo ""
echo "Run with:"
echo "  open $APP_DIR"
echo "  # or: claudio ui  (if claudio was built with --features ui)"
