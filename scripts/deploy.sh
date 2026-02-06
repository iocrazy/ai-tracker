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

# 运行时目录（XDG 标准）
CONFIG_DIR="$HOME/.config/agent-tracker"
DATA_DIR="$HOME/.local/share/agent-tracker"
BIN_DIR="$HOME/.local/bin"
LOG_DIR="$HOME/Library/Logs/agent-tracker"

# 服务名称
LAUNCHD_LABEL="com.heygo.tracker-server"

echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     Agent Tracker Deploy Script        ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"
echo ""

# 解析参数
QUICK_MODE=false
RESTART_ONLY=false

for arg in "$@"; do
    case $arg in
        --quick)
            QUICK_MODE=true
            ;;
        --restart)
            RESTART_ONLY=true
            ;;
    esac
done

# 创建目录
create_directories() {
    echo -e "${YELLOW}[1/5] 创建目录...${NC}"
    mkdir -p "$CONFIG_DIR"
    mkdir -p "$DATA_DIR"
    mkdir -p "$BIN_DIR"
    mkdir -p "$LOG_DIR"
    mkdir -p "$DATA_DIR/web"
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
        rm -rf "$DATA_DIR/web"
        cp -r "$WEB_DIR/dist" "$DATA_DIR/web"
        echo "  ✓ 前端文件已安装到 $DATA_DIR/web"
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

# 主流程
if [ "$RESTART_ONLY" = true ]; then
    restart_service
else
    create_directories
    build_rust
    build_web
    install_files
    restart_service
fi

echo ""
echo -e "${GREEN}╔════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║           部署完成！                    ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════╝${NC}"
echo ""
echo -e "  服务地址: ${BLUE}http://localhost:3099${NC}"
echo -e "  日志文件: ${BLUE}$LOG_DIR/tracker-server.log${NC}"
echo -e "  数据目录: ${BLUE}$DATA_DIR${NC}"
echo ""
