#!/usr/bin/env bash
# Download Vosk library for the current platform
#
# Usage: ./scripts/fetch-vosk.sh [--output-dir DIR]
#
# Environment variables:
#   VOSK_VERSION    - Version to download (default: 0.3.45)
#   VOSK_STRATEGY   - download|system|path (default: download)
#   VOSK_LIB_PATH   - Path to existing vosk libs (when strategy=path)
#   VOSK_OUTPUT_DIR - Where to put downloaded libs (default: ./vosk-lib)

set -euo pipefail

VOSK_VERSION="${VOSK_VERSION:-0.3.45}"
VOSK_STRATEGY="${VOSK_STRATEGY:-download}"
VOSK_OUTPUT_DIR="${VOSK_OUTPUT_DIR:-./vosk-lib}"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --output-dir)
            VOSK_OUTPUT_DIR="$2"
            shift 2
            ;;
        --version)
            VOSK_VERSION="$2"
            shift 2
            ;;
        --strategy)
            VOSK_STRATEGY="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Detect platform
detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)
            case "$arch" in
                x86_64)  echo "linux-x86_64" ;;
                aarch64) echo "linux-aarch64" ;;
                *)       echo "Unsupported Linux architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        Darwin)
            # macOS doesn't use vosk - uses native Speech framework
            echo "macos"
            ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT)
            case "$arch" in
                x86_64|AMD64) echo "win64" ;;
                *)            echo "Unsupported Windows architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        *)
            echo "Unsupported OS: $os" >&2
            exit 1
            ;;
    esac
}

# Download vosk library
download_vosk() {
    local platform="$1"
    local output_dir="$2"
    local version="$3"

    # macOS uses native Speech framework, not vosk
    if [[ "$platform" == "macos" ]]; then
        echo "macOS detected - using native Speech framework, no vosk needed"
        return 0
    fi

    local url="https://github.com/alphacep/vosk-api/releases/download/v${version}/vosk-${platform}-${version}.zip"
    local zip_file="vosk-${platform}-${version}.zip"
    local extract_dir="vosk-${platform}-${version}"

    echo "Downloading vosk ${version} for ${platform}..."
    echo "URL: ${url}"

    mkdir -p "$output_dir"
    cd "$output_dir"

    # Download if not already present
    if [[ ! -f "$zip_file" ]]; then
        curl -fSL "$url" -o "$zip_file"
    else
        echo "Using cached download: $zip_file"
    fi

    # Extract
    if [[ ! -d "$extract_dir" ]]; then
        unzip -q "$zip_file"
    fi

    # Copy library to output root for easier access
    case "$platform" in
        linux-*)
            cp -f "$extract_dir/libvosk.so" ./
            echo "Library ready: ${output_dir}/libvosk.so"
            ;;
        win64)
            cp -f "$extract_dir/libvosk.dll" ./
            cp -f "$extract_dir/vosk.lib" ./ 2>/dev/null || true
            echo "Library ready: ${output_dir}/libvosk.dll"
            ;;
    esac

    cd - > /dev/null
}

# Main
main() {
    echo "Vosk library setup"
    echo "  Strategy: $VOSK_STRATEGY"
    echo "  Version:  $VOSK_VERSION"
    echo ""

    case "$VOSK_STRATEGY" in
        download)
            local platform
            platform="$(detect_platform)"
            download_vosk "$platform" "$VOSK_OUTPUT_DIR" "$VOSK_VERSION"

            # Print setup instructions
            echo ""
            echo "To build, set these environment variables:"
            echo "  export LIBRARY_PATH=\"\$(pwd)/${VOSK_OUTPUT_DIR}:\$LIBRARY_PATH\""
            echo "  export LD_LIBRARY_PATH=\"\$(pwd)/${VOSK_OUTPUT_DIR}:\$LD_LIBRARY_PATH\""
            ;;
        system)
            echo "Using system-installed vosk library"
            echo "Ensure libvosk is in your library path"
            ;;
        path)
            if [[ -z "${VOSK_LIB_PATH:-}" ]]; then
                echo "Error: VOSK_LIB_PATH must be set when using strategy=path" >&2
                exit 1
            fi
            echo "Using vosk from: $VOSK_LIB_PATH"
            echo ""
            echo "To build, set:"
            echo "  export LIBRARY_PATH=\"${VOSK_LIB_PATH}:\$LIBRARY_PATH\""
            ;;
        *)
            echo "Unknown strategy: $VOSK_STRATEGY" >&2
            exit 1
            ;;
    esac
}

main
