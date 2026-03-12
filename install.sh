#!/bin/bash
# Apex Gateway 安装脚本
# 用法：./install.sh [选项] <目标路径>
#
# 功能:
#   - 构建并安装内嵌 Dashboard 静态资源的 apex 二进制
#   - 保护现有 config.json 不被覆盖
#
# 选项:
#   --force-config    强制重新生成配置文件

set -euo pipefail

require_command() {
    local command_name="$1"
    local install_hint="$2"
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "错误：未找到依赖命令 '$command_name'"
        echo "请先安装：$install_hint"
        exit 1
    fi
}

# 解析命令行参数
FORCE_CONFIG=false
TARGET_DIR=""

for arg in "$@"; do
    case $arg in
        --force-config)
            FORCE_CONFIG=true
            ;;
        --help|-h)
            echo "用法：$0 [选项] <目标路径>"
            echo ""
            echo "选项:"
            echo "  --force-config    强制重新生成配置文件"
            echo "  --help, -h        显示此帮助信息"
            echo ""
            echo "示例:"
            echo "  $0 /opt/apex"
            echo "  $0 --force-config /opt/apex"
            exit 0
            ;;
        *)
            if [ -z "$TARGET_DIR" ]; then
                TARGET_DIR="$arg"
            fi
            ;;
    esac
done

# 检查参数
if [ -z "$TARGET_DIR" ]; then
    echo "用法：$0 [选项] <目标路径>"
    echo "示例：$0 /opt/apex"
    echo "       $0 --force-config /opt/apex"
    echo ""
    echo "使用 --help 查看选项说明"
    exit 1
fi

# 获取脚本所在目录（支持符号链接）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Apex Gateway 安装脚本 ==="
echo "目标路径：$TARGET_DIR"
if [ "$FORCE_CONFIG" = true ]; then
    echo "模式：强制重新生成配置文件"
fi

# 创建目标目录
mkdir -p "$TARGET_DIR"
TARGET_DIR="$(cd "$TARGET_DIR" && pwd)"

require_command cargo "Rust toolchain (https://rustup.rs/)"
require_command node "Node.js 18+"
require_command npm "npm (通常随 Node.js 一起安装)"

echo ""
echo "=== 1. 构建 Web 前端 ==="
cd "$SCRIPT_DIR/web"
npm ci
npm run build

echo ""
echo "=== 2. 构建 Rust 后端（embedded-web） ==="
cd "$SCRIPT_DIR"
cargo build --release --features embedded-web

echo ""
echo "=== 3. 安装文件 ==="

if [ ! -f "$SCRIPT_DIR/target/release/apex" ]; then
    echo "错误：未找到构建产物 $SCRIPT_DIR/target/release/apex"
    exit 1
fi

if [ ! -f "$SCRIPT_DIR/target/web/dashboard/index.html" ]; then
    echo "错误：未找到前端构建产物 $SCRIPT_DIR/target/web/dashboard/index.html"
    echo "请确认 web 构建脚本已成功导出静态文件"
    exit 1
fi

# 安装 apex 二进制
echo "安装 apex 二进制到：$TARGET_DIR/apex"
cp "$SCRIPT_DIR/target/release/apex" "$TARGET_DIR/apex"
chmod +x "$TARGET_DIR/apex"

if [ "$(uname -s)" = "Darwin" ]; then
    echo "在 macOS 上重新签名安装产物..."
    codesign --force --sign - "$TARGET_DIR/apex"
fi

mkdir -p "$TARGET_DIR/data" "$TARGET_DIR/logs"

# 生成示例配置文件（如果不存在或用户强制重新生成）
if [ -f "$TARGET_DIR/config.json" ] && [ "$FORCE_CONFIG" = false ]; then
    # 验证 JSON 有效性
    if command -v python3 &> /dev/null; then
        if ! python3 -c "import json; json.load(open('$TARGET_DIR/config.json'))" 2>/dev/null; then
            echo "  警告：现有配置文件无效 JSON，请检查"
        fi
    fi
    echo "  配置文件已存在，跳过生成（保留原有配置）"
else
    echo "生成示例配置文件：$TARGET_DIR/config.json"
    cat > "$TARGET_DIR/config.json" << EOF
{
  "version": "1.0",
  "logging": {
    "level": "info",
    "dir": "$TARGET_DIR/logs"
  },
  "data_dir": "$TARGET_DIR/data",
  "global": {
    "listen": "0.0.0.0:12356",
    "auth_keys": ["replace-with-dashboard-admin-key"],
    "timeouts": {
      "connect_ms": 10000,
      "request_ms": 120000,
      "response_ms": 300000
    },
    "retries": {
      "max_attempts": 3,
      "backoff_ms": 100,
      "retry_on_status": [429, 500, 502, 503, 504]
    },
    "enable_mcp": true,
    "cors_allowed_origins": []
  },
  "metrics": {
    "enabled": true,
    "path": "/metrics"
  },
  "hot_reload": {
    "config_path": "$TARGET_DIR/config.json",
    "watch": false
  },
  "teams": [
    {
      "id": "demo-team",
      "api_key": "replace-with-team-api-key",
      "policy": {
        "allowed_routers": []
      }
    }
  ],
  "channels": [],
  "routers": [],
  "prompts": []
}
EOF
    echo "  已生成与当前代码一致的配置模板"
    echo "  请至少修改以下内容后再启动："
    echo "    - global.auth_keys"
    echo "    - teams[0].api_key"
    echo "    - channels"
    echo "    - routers"
fi

echo ""
echo "=== 安装完成 ==="
echo ""
echo "目录结构:"
echo "  $TARGET_DIR/apex           - 主程序"
echo "  $TARGET_DIR/config.json    - 配置文件"
echo "  $TARGET_DIR/data           - SQLite/运行数据"
echo "  $TARGET_DIR/logs           - 日志目录"
echo ""
echo "运行：$TARGET_DIR/apex --config $TARGET_DIR/config.json gateway start"
