#!/bin/bash
# deploy.sh - 部署 Agent Tracker 到本地运行环境
#
# 用法:
#   ./scripts/deploy.sh          # 完整部署（构建 + 安装）
#   ./scripts/deploy.sh --quick  # 快速部署（仅复制，不重新构建）
#   ./scripts/deploy.sh --restart # 仅重启服务

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 目录配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RUST_DIR="$PROJECT_DIR/src/rust"
WEB_DIR="$PROJECT_DIR/web"

# Shared path resolution
source "$SCRIPT_DIR/env.sh"

# Standalone deploy targets (legacy layout)
CONFIG_DIR="$TRACKER_DATA"
BIN_DIR="$HOME/.config/agent-tracker/bin"
LOG_DIR="$TRACKER_LOG_DIR"

# 服务名称
LAUNCHD_LABEL="com.heygo.tracker-server"

echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     Agent Tracker Deploy Script        ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"
echo ""

# 解析参数
QUICK_MODE=false
RESTART_ONLY=false
TAURI_MODE=false
CHECK_ONLY=false

for arg in "$@"; do
    case $arg in
        --quick)
            QUICK_MODE=true
            ;;
        --restart)
            RESTART_ONLY=true
            ;;
        --tauri)
            TAURI_MODE=true
            ;;
        --check)
            CHECK_ONLY=true
            ;;
    esac
done

# Tauri app paths
TAURI_APP="/Applications/Agent Tracker.app"
TAURI_BIN="$TAURI_APP/Contents/MacOS/tracker-server"
TAURI_WEB="$TAURI_APP/Contents/Resources/web-dist"

# 创建目录
create_directories() {
    echo -e "${YELLOW}[1/5] 创建目录...${NC}"
    mkdir -p "$CONFIG_DIR"
    mkdir -p "$CONFIG_DIR/data"
    mkdir -p "$BIN_DIR"
    mkdir -p "$LOG_DIR"
    mkdir -p "$CONFIG_DIR/web/dist"
    echo -e "${GREEN}  ✓ 目录已创建${NC}"
}

# 构建 Rust
build_rust() {
    if [ "$QUICK_MODE" = true ]; then
        echo -e "${YELLOW}[2/5] 跳过构建 (--quick 模式)${NC}"
        return
    fi

    echo -e "${YELLOW}[2/5] 构建 Rust...${NC}"
    cd "$RUST_DIR"
    cargo build --release
    echo -e "${GREEN}  ✓ Rust 构建完成${NC}"
}

# 构建前端
build_web() {
    if [ "$QUICK_MODE" = true ]; then
        echo -e "${YELLOW}[3/5] 跳过前端构建 (--quick 模式)${NC}"
        return
    fi

    echo -e "${YELLOW}[3/5] 构建前端...${NC}"
    cd "$WEB_DIR"

    # 检查 node_modules
    if [ ! -d "node_modules" ]; then
        echo "  安装依赖..."
        npm install
    fi

    npm run build
    echo -e "${GREEN}  ✓ 前端构建完成${NC}"
}

# 安装文件
install_files() {
    echo -e "${YELLOW}[4/5] 安装文件...${NC}"

    # 复制二进制文件
    cp "$RUST_DIR/target/release/tracker-server" "$BIN_DIR/"
    cp "$RUST_DIR/target/release/tracker-tui" "$BIN_DIR/" 2>/dev/null || true
    chmod +x "$BIN_DIR/tracker-server"
    chmod +x "$BIN_DIR/tracker-tui" 2>/dev/null || true
    echo "  ✓ 二进制文件已安装到 $BIN_DIR"

    # 复制前端文件
    if [ -d "$WEB_DIR/dist" ]; then
        rm -rf "$CONFIG_DIR/web/dist"
        cp -r "$WEB_DIR/dist" "$CONFIG_DIR/web/dist"
        echo "  ✓ 前端文件已安装到 $CONFIG_DIR/web/dist"
    fi

    # 复制配置文件模板（如果不存在）
    if [ ! -f "$CONFIG_DIR/config.json" ]; then
        cp "$PROJECT_DIR/agent-config.example.json" "$CONFIG_DIR/config.json"
        echo "  ✓ 配置文件已创建"
    fi

    echo -e "${GREEN}  ✓ 文件安装完成${NC}"
}

# 重启服务
restart_service() {
    echo -e "${YELLOW}[5/5] 重启服务...${NC}"

    # 尝试停止现有服务
    launchctl stop "$LAUNCHD_LABEL" 2>/dev/null || true
    sleep 1

    # 启动服务
    launchctl start "$LAUNCHD_LABEL" 2>/dev/null || {
        echo -e "${YELLOW}  ! launchd 服务未配置，手动启动...${NC}"
        # 杀死旧进程
        pkill -f "tracker-server" 2>/dev/null || true
        sleep 1
        # 启动新进程
        nohup "$BIN_DIR/tracker-server" > "$LOG_DIR/tracker-server.log" 2>&1 &
        echo "  PID: $!"
    }

    echo -e "${GREEN}  ✓ 服务已重启${NC}"
}

# Tauri 模式 — 部署到 Agent Tracker.app
install_tauri() {
    echo -e "${YELLOW}[4/5] 安装到 Tauri app...${NC}"

    if [ ! -d "$TAURI_APP" ]; then
        echo -e "${RED}  ✗ Agent Tracker.app not found${NC}"
        exit 1
    fi

    # 复制前端 (清理旧 assets 防止 SW 缓存旧版)
    if [ -d "$WEB_DIR/dist" ]; then
        rm -rf "$TAURI_WEB/assets"
        cp -r "$WEB_DIR/dist/"* "$TAURI_WEB/"
        echo "  ✓ 前端 → $TAURI_WEB (旧 assets 已清理)"
    fi

    # 复制后端 + codesign
    cp "$RUST_DIR/target/release/tracker-server" "$TAURI_BIN"
    codesign -fs - "$TAURI_BIN" 2>/dev/null
    echo "  ✓ 后端 → $TAURI_BIN (codesigned)"

    echo -e "${GREEN}  ✓ Tauri 安装完成${NC}"
}

# 重启 Tauri app
restart_tauri() {
    echo -e "${YELLOW}[5/5] 重启 Tauri app...${NC}"

    # Kill existing
    pkill -f "agent-tracker-menubar" 2>/dev/null || true
    sleep 1

    # Reopen
    open -a "Agent Tracker"
    echo "  等待 sidecar 启动..."

    # Wait for server to come up (max 15s)
    for i in $(seq 1 15); do
        if curl -sf http://localhost:3099/api/health >/dev/null 2>&1; then
            echo -e "${GREEN}  ✓ Sidecar 启动成功 (${i}s)${NC}"
            return 0
        fi
        sleep 1
    done

    echo -e "${RED}  ✗ Sidecar 启动超时 (15s)${NC}"
    return 1
}

# 自检 — 验证所有关键端点
health_check() {
    echo ""
    echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║           自检 (Health Check)           ║${NC}"
    echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"
    echo ""

    local ALL_OK=true
    local URL="${TRACKER_URL:-http://localhost:3099}"

    # 1. Health
    local health
    health=$(curl -sf "$URL/api/health" 2>/dev/null)
    if [ $? -eq 0 ]; then
        local status=$(echo "$health" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null)
        local uptime=$(echo "$health" | python3 -c "import sys,json; print(json.load(sys.stdin)['checks']['uptime'])" 2>/dev/null || echo "?")
        echo -e "  ✅ Health:      ${GREEN}$status${NC} (uptime: $uptime)"
    else
        echo -e "  ❌ Health:      ${RED}FAILED — server not responding${NC}"
        ALL_OK=false
    fi

    # 2. Passkey status
    local passkey
    passkey=$(curl -sf "$URL/api/auth/passkey/status" 2>/dev/null)
    if [ $? -eq 0 ]; then
        local has_pk=$(echo "$passkey" | python3 -c "import sys,json; print(json.load(sys.stdin)['has_passkey'])" 2>/dev/null)
        echo -e "  ✅ Passkey:     has_passkey=${GREEN}$has_pk${NC}"
    else
        echo -e "  ❌ Passkey:     ${RED}endpoint failed${NC}"
        ALL_OK=false
    fi

    # 3. TOTP status
    local totp
    totp=$(curl -sf "$URL/api/auth/totp/status" 2>/dev/null)
    if [ $? -eq 0 ]; then
        local enabled=$(echo "$totp" | python3 -c "import sys,json; print(json.load(sys.stdin)['enabled'])" 2>/dev/null)
        echo -e "  ✅ TOTP:        enabled=${GREEN}$enabled${NC}"
    else
        echo -e "  ❌ TOTP:        ${RED}endpoint failed${NC}"
        ALL_OK=false
    fi

    # 4. Frontend
    local fe_code
    fe_code=$(curl -sf -o /dev/null -w "%{http_code}" "$URL/" 2>/dev/null)
    if [ "$fe_code" = "200" ]; then
        echo -e "  ✅ Frontend:    ${GREEN}200 OK${NC}"
    else
        echo -e "  ❌ Frontend:    ${RED}HTTP $fe_code${NC}"
        ALL_OK=false
    fi

    # 5. WebSocket (expect 401 without token, 101 with token)
    if [ -n "$TRACKER_TOKEN" ]; then
        local ws_code
        ws_code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 3 "$URL/ws?token=$TRACKER_TOKEN" 2>/dev/null || true)
        if [ "$ws_code" = "101" ] || [ "$ws_code" = "000" ]; then
            echo -e "  ✅ WebSocket:   ${GREEN}auth OK${NC}"
        else
            echo -e "  ⚠️  WebSocket:   HTTP $ws_code (token may be expired)"
        fi
    else
        local ws_code
        ws_code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 3 "$URL/ws" 2>/dev/null || true)
        echo -e "  ⚠️  WebSocket:   ${YELLOW}no token, got $ws_code (expected 401)${NC}"
    fi

    echo ""
    if [ "$ALL_OK" = true ]; then
        echo -e "  ${GREEN}🎉 All checks passed!${NC}"
    else
        echo -e "  ${RED}⚠️  Some checks failed${NC}"
    fi
    echo ""
}

# 主流程
if [ "$CHECK_ONLY" = true ]; then
    health_check
    exit 0
fi

if [ "$RESTART_ONLY" = true ]; then
    if [ "$TAURI_MODE" = true ]; then
        restart_tauri && health_check
    else
        restart_service
    fi
elif [ "$TAURI_MODE" = true ]; then
    build_rust
    build_web
    install_tauri
    restart_tauri && health_check
else
    create_directories
    build_rust
    build_web
    install_files
    restart_service
fi

if [ "$CHECK_ONLY" != true ] && [ "$TAURI_MODE" != true ]; then
    echo ""
    echo -e "${GREEN}╔════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║           部署完成！                    ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "  服务地址: ${BLUE}http://localhost:3099${NC}"
    echo -e "  日志文件: ${BLUE}$LOG_DIR/tracker-server.log${NC}"
    echo -e "  数据目录: ${BLUE}$CONFIG_DIR${NC}"
    echo ""
fi
