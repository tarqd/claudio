#!/bin/bash
# Build Claudio.app — a unified macOS app bundle containing:
#   - ClaudioUI  (SwiftUI Liquid Glass GUI)
#   - claudio    (Rust TUI CLI)
#
# Usage:
#   ./build.sh          # Debug build
#   ./build.sh release  # Release build
#
# Output: build/Claudio.app
#
# After building:
#   open build/Claudio.app          # Launch GUI
#   build/Claudio.app/Contents/MacOS/claudio    # Run TUI
#   ./install.sh                    # Install to /Applications + symlink CLI

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

CONFIG="${1:-debug}"
SWIFT_FLAGS=""
CARGO_FLAGS="--features ui"

if [ "$CONFIG" = "release" ]; then
    SWIFT_FLAGS="-c release"
    CARGO_FLAGS="--release --features ui"
fi

echo "==> Building ClaudioUI (SwiftUI)..."
cd "$SCRIPT_DIR"
swift build $SWIFT_FLAGS

SWIFT_BUILD_DIR="$SCRIPT_DIR/.build/${CONFIG}"
SWIFT_BINARY="$SWIFT_BUILD_DIR/ClaudioUI"

if [ ! -f "$SWIFT_BINARY" ]; then
    echo "Error: Swift build failed, binary not found at $SWIFT_BINARY"
    exit 1
fi

echo "==> Building claudio (Rust TUI)..."
cd "$ROOT_DIR"
cargo build $CARGO_FLAGS

if [ "$CONFIG" = "release" ]; then
    RUST_BINARY="$ROOT_DIR/target/release/claudio"
else
    RUST_BINARY="$ROOT_DIR/target/debug/claudio"
fi

if [ ! -f "$RUST_BINARY" ]; then
    echo "Error: Cargo build failed, binary not found at $RUST_BINARY"
    exit 1
fi

# Create unified .app bundle
APP_DIR="$SCRIPT_DIR/build/Claudio.app"
CONTENTS="$APP_DIR/Contents"
MACOS="$CONTENTS/MacOS"

echo "==> Assembling Claudio.app..."
rm -rf "$APP_DIR"
mkdir -p "$MACOS"

# Both binaries live in Contents/MacOS/
cp "$SWIFT_BINARY" "$MACOS/ClaudioUI"
cp "$RUST_BINARY"  "$MACOS/claudio"

# Info.plist — ClaudioUI is the main executable (for GUI launch via open/Finder)
cp "$SCRIPT_DIR/Info.plist" "$CONTENTS/Info.plist"

# Sign with entitlements (ad-hoc)
codesign --force --sign - --entitlements "$SCRIPT_DIR/ClaudioUI.entitlements" "$APP_DIR"

echo ""
echo "Built: $APP_DIR"
echo ""
echo "  open build/Claudio.app          # Launch Liquid Glass GUI"
echo "  build/Claudio.app/Contents/MacOS/claudio          # Run TUI"
echo "  build/Claudio.app/Contents/MacOS/claudio ui       # Launch GUI from CLI"
echo ""
echo "To install: ./install.sh"
