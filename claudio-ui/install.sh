#!/bin/bash
# Install Claudio.app to /Applications and symlink the CLI to /usr/local/bin
#
# Usage:
#   ./install.sh              # Install from build/Claudio.app
#   ./install.sh --uninstall  # Remove app and symlink

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_SRC="$SCRIPT_DIR/build/Claudio.app"
APP_DST="/Applications/Claudio.app"
CLI_LINK="/usr/local/bin/claudio"
CLI_TARGET="$APP_DST/Contents/MacOS/claudio"

if [ "${1:-}" = "--uninstall" ]; then
    echo "Uninstalling Claudio..."
    [ -L "$CLI_LINK" ] && rm -f "$CLI_LINK" && echo "  Removed $CLI_LINK"
    [ -d "$APP_DST" ] && rm -rf "$APP_DST" && echo "  Removed $APP_DST"
    echo "Done."
    exit 0
fi

if [ ! -d "$APP_SRC" ]; then
    echo "Error: $APP_SRC not found. Run ./build.sh release first."
    exit 1
fi

echo "Installing Claudio..."

# Copy .app to /Applications
echo "  $APP_SRC -> $APP_DST"
rm -rf "$APP_DST"
cp -R "$APP_SRC" "$APP_DST"

# Symlink CLI binary
mkdir -p "$(dirname "$CLI_LINK")"
echo "  $CLI_LINK -> $CLI_TARGET"
ln -sf "$CLI_TARGET" "$CLI_LINK"

echo ""
echo "Installed. You can now use:"
echo "  claudio           # TUI mode (terminal)"
echo "  claudio ui        # Liquid Glass GUI"
echo "  claudio -- cmd    # Pipe speech to a command"
echo "  open /Applications/Claudio.app  # Launch GUI directly"
