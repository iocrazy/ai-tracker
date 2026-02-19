#!/usr/bin/env bash
set -euo pipefail

# Build tracker-server and copy it as a Tauri sidecar binary.
# Tauri sidecar naming convention: <name>-<target-triple>

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TAURI_DIR="$REPO_ROOT/tauri-menubar/src-tauri"
RUST_DIR="$REPO_ROOT/src/rust"

# Detect target triple
ARCH="$(uname -m)"
case "$ARCH" in
  arm64) TARGET="aarch64-apple-darwin" ;;
  x86_64) TARGET="x86_64-apple-darwin" ;;
  *) echo "Unsupported arch: $ARCH"; exit 1 ;;
esac

echo "Building tracker-server (release)..."
cd "$RUST_DIR"
cargo build --release

echo "Copying binary to sidecar location..."
mkdir -p "$TAURI_DIR/bin"
cp "$RUST_DIR/target/release/tracker-server" "$TAURI_DIR/bin/tracker-server-${TARGET}"

echo "Done. Sidecar binary: bin/tracker-server-${TARGET}"
