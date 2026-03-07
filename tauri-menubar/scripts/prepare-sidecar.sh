#!/usr/bin/env bash
set -euo pipefail

# Build tracker-server and copy it as a Tauri sidecar binary.
# Tauri sidecar naming convention: <name>-<target-triple>
#
# Usage:
#   ./prepare-sidecar.sh                    # auto-detect local arch
#   ./prepare-sidecar.sh --target <triple>  # CI: skip build, just copy

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TAURI_DIR="$REPO_ROOT/tauri-menubar/src-tauri"
RUST_DIR="$REPO_ROOT/src/rust"

# Parse args
CI_TARGET=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target) CI_TARGET="$2"; shift 2 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

if [[ -n "$CI_TARGET" ]]; then
    # CI mode: binary already built by cargo, just copy
    TARGET="$CI_TARGET"
    SRC="$RUST_DIR/target/${TARGET}/release/tracker-server"
    if [[ ! -f "$SRC" ]]; then
        echo "Error: $SRC not found. Build with: cargo build --release --target $TARGET"
        exit 1
    fi
else
    # Local mode: detect arch, build, copy
    ARCH="$(uname -m)"
    case "$ARCH" in
      arm64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) echo "Unsupported arch: $ARCH"; exit 1 ;;
    esac

    echo "Building tracker-server (release)..."
    cd "$RUST_DIR"
    cargo build --release
    SRC="$RUST_DIR/target/release/tracker-server"
fi

echo "Copying binary to sidecar location..."
mkdir -p "$TAURI_DIR/bin"
cp "$SRC" "$TAURI_DIR/bin/tracker-server-${TARGET}"

echo "Done. Sidecar binary: bin/tracker-server-${TARGET}"
