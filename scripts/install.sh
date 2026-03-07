#!/usr/bin/env bash
# Agent Tracker — Standalone installer (non-Tauri)
# Downloads the latest release from GitHub and installs to ~/.config/agent-tracker/
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/iocrazy/ai-tracker/main/scripts/install.sh | bash

set -euo pipefail

# ============================================================================
# Colors
# ============================================================================

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

info()  { printf "${CYAN}[INFO]${NC}  %s\n" "$*"; }
ok()    { printf "${GREEN}[OK]${NC}    %s\n" "$*"; }
warn()  { printf "${YELLOW}[WARN]${NC}  %s\n" "$*"; }
err()   { printf "${RED}[ERR]${NC}   %s\n" "$*"; }
header(){ printf "\n${BOLD}=== %s ===${NC}\n\n" "$*"; }

REPO="iocrazy/ai-tracker"
INSTALL_DIR="$HOME/.config/agent-tracker"

# ============================================================================
# 1. Detect platform
# ============================================================================

header "Agent Tracker Standalone Installer"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    PLATFORM="macos"
    case "$ARCH" in
      arm64)  TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) err "Unsupported arch: $ARCH"; exit 1 ;;
    esac
    ;;
  Linux)
    PLATFORM="linux"
    case "$ARCH" in
      x86_64|amd64) TARGET="x86_64-unknown-linux-gnu" ;;
      *) err "Unsupported arch: $ARCH"; exit 1 ;;
    esac
    ;;
  *) err "Unsupported OS: $OS"; exit 1 ;;
esac

info "Platform: $PLATFORM ($TARGET)"

# ============================================================================
# 2. Check dependencies
# ============================================================================

for cmd in curl python3 jq; do
    if ! command -v "$cmd" &>/dev/null; then
        err "Required command not found: $cmd"
        exit 1
    fi
done

# ============================================================================
# 3. Get latest release
# ============================================================================

header "Downloading Latest Release"

info "Fetching latest release info from GitHub..."

RELEASE_JSON=$(curl -sf "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null || true)

if [[ -z "$RELEASE_JSON" ]]; then
    # Try draft releases (pre-release)
    RELEASE_JSON=$(curl -sf "https://api.github.com/repos/$REPO/releases" 2>/dev/null | jq '.[0]' || true)
fi

if [[ -z "$RELEASE_JSON" ]] || [[ "$RELEASE_JSON" == "null" ]]; then
    err "No releases found for $REPO."
    err "Build from source instead: cd src/rust && cargo build --release"
    exit 1
fi

VERSION=$(echo "$RELEASE_JSON" | jq -r '.tag_name')
info "Latest release: $VERSION"

# Find the right asset — look for tracker-server binary or tarball
# Asset naming convention: tracker-server-{target} or agent-tracker-{target}.tar.gz
ASSET_URL=""

# Try direct binary first
ASSET_URL=$(echo "$RELEASE_JSON" | jq -r \
    ".assets[] | select(.name | test(\"tracker-server.*${TARGET}\")) | .browser_download_url" \
    | head -1)

# Try tarball
if [[ -z "$ASSET_URL" ]] || [[ "$ASSET_URL" == "null" ]]; then
    ASSET_URL=$(echo "$RELEASE_JSON" | jq -r \
        ".assets[] | select(.name | test(\"${TARGET}.*\\\\.tar\\\\.gz\")) | .browser_download_url" \
        | head -1)
fi

if [[ -z "$ASSET_URL" ]] || [[ "$ASSET_URL" == "null" ]]; then
    err "No binary found for target: $TARGET"
    err "Available assets:"
    echo "$RELEASE_JSON" | jq -r '.assets[].name' 2>/dev/null || true
    exit 1
fi

info "Downloading: $(basename "$ASSET_URL")"

# ============================================================================
# 4. Install
# ============================================================================

header "Installing"

mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/data" "$INSTALL_DIR/logs" "$INSTALL_DIR/scripts"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

DOWNLOAD_FILE="$TMP_DIR/$(basename "$ASSET_URL")"
curl -fSL -o "$DOWNLOAD_FILE" "$ASSET_URL"

if [[ "$DOWNLOAD_FILE" == *.tar.gz ]] || [[ "$DOWNLOAD_FILE" == *.tgz ]]; then
    tar xzf "$DOWNLOAD_FILE" -C "$TMP_DIR"
    # Find tracker-server binary in extracted files
    BINARY=$(find "$TMP_DIR" -name "tracker-server" -type f | head -1)
    if [[ -z "$BINARY" ]]; then
        err "tracker-server binary not found in archive"
        exit 1
    fi
    cp "$BINARY" "$INSTALL_DIR/bin/tracker-server"

    # Copy web-dist if present
    WEB_DIST=$(find "$TMP_DIR" -type d -name "web-dist" | head -1)
    if [[ -n "$WEB_DIST" ]]; then
        rm -rf "$INSTALL_DIR/web-dist"
        cp -r "$WEB_DIST" "$INSTALL_DIR/web-dist"
        ok "Web frontend installed"
    fi

    # Copy scripts if present
    SCRIPTS=$(find "$TMP_DIR" -type d -name "scripts" | head -1)
    if [[ -n "$SCRIPTS" ]]; then
        cp -r "$SCRIPTS"/* "$INSTALL_DIR/scripts/" 2>/dev/null || true
    fi
else
    # Direct binary download
    cp "$DOWNLOAD_FILE" "$INSTALL_DIR/bin/tracker-server"
fi

chmod +x "$INSTALL_DIR/bin/tracker-server"
ok "tracker-server installed to $INSTALL_DIR/bin/"

# ============================================================================
# 5. Install service
# ============================================================================

header "Installing Service"

if [[ "$PLATFORM" == "linux" ]]; then
    UNIT_DIR="$HOME/.config/systemd/user"
    mkdir -p "$UNIT_DIR"

    cat > "$UNIT_DIR/agent-tracker.service" << UNIT
[Unit]
Description=Agent Tracker Server
After=network.target

[Service]
Type=simple
ExecStart=$INSTALL_DIR/bin/tracker-server
WorkingDirectory=$INSTALL_DIR
Restart=on-failure
RestartSec=5
Environment=TRACKER_DATA_DIR=$INSTALL_DIR

[Install]
WantedBy=default.target
UNIT

    systemctl --user daemon-reload
    systemctl --user enable agent-tracker
    systemctl --user start agent-tracker
    ok "systemd user service installed and started"

elif [[ "$PLATFORM" == "macos" ]]; then
    PLIST_DIR="$HOME/Library/LaunchAgents"
    PLIST="$PLIST_DIR/dev.heygo.tracker-server.plist"
    mkdir -p "$PLIST_DIR"

    cat > "$PLIST" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.heygo.tracker-server</string>
    <key>ProgramArguments</key>
    <array>
        <string>$INSTALL_DIR/bin/tracker-server</string>
    </array>
    <key>WorkingDirectory</key>
    <string>$INSTALL_DIR</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>TRACKER_DATA_DIR</key>
        <string>$INSTALL_DIR</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$INSTALL_DIR/logs/tracker-server.log</string>
    <key>StandardErrorPath</key>
    <string>$INSTALL_DIR/logs/tracker-server.log</string>
</dict>
</plist>
PLIST

    launchctl load "$PLIST" 2>/dev/null || true
    ok "launchd service installed and started"
fi

# ============================================================================
# 6. Run setup.sh
# ============================================================================

header "Running Setup"

SETUP_SCRIPT="$INSTALL_DIR/scripts/setup.sh"
if [[ -f "$SETUP_SCRIPT" ]]; then
    bash "$SETUP_SCRIPT"
else
    # Download setup.sh from the repo
    info "Downloading setup.sh..."
    curl -fsSL "https://raw.githubusercontent.com/$REPO/main/scripts/setup.sh" -o "$INSTALL_DIR/scripts/setup.sh"
    chmod +x "$INSTALL_DIR/scripts/setup.sh"
    bash "$INSTALL_DIR/scripts/setup.sh"
fi
