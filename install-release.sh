#!/bin/bash
# Apex Gateway 预编译包安装脚本
# 用法：./install-release.sh [选项] [目标路径]
#
# 功能:
#   - 根据当前系统和架构，从 GitHub Releases 下载对应预编译包
#   - 安装 apex 二进制到目标目录
#   - 可选地将包内示例配置写入指定路径
#
# 选项:
#   --version <tag>      安装指定版本，默认 latest
#   --repo <owner/repo>  指定 GitHub 仓库，默认 cregis-dev/apex
#   --config-path <file> 将包内 config.example.json 写入指定路径
#   --force-config       当 --config-path 已存在时强制覆盖
#   --skip-checksum      跳过 SHA256 校验
#   --help, -h           显示帮助

set -euo pipefail

REPO="${APEX_REPO:-cregis-dev/apex}"
VERSION="latest"
CONFIG_PATH=""
FORCE_CONFIG=false
SKIP_CHECKSUM=false
TARGET_DIR="/opt/apex"
TMP_DIR=""

print_usage() {
    cat <<'EOF'
用法：./install-release.sh [选项] [目标路径]

选项:
  --version <tag>      安装指定版本，默认 latest
  --repo <owner/repo>  指定 GitHub 仓库，默认 cregis-dev/apex
  --config-path <file> 将包内 config.example.json 写入指定路径
  --force-config       当 --config-path 已存在时强制覆盖
  --skip-checksum      跳过 SHA256 校验
  --help, -h           显示此帮助信息

示例:
  ./install-release.sh
  ./install-release.sh /opt/apex
  ./install-release.sh --config-path /etc/apex/config.json /opt/apex
  ./install-release.sh --version v0.1.1 /opt/apex
  ./install-release.sh --repo your-org/apex --skip-checksum /opt/apex
EOF
}

cleanup() {
    if [ -n "${TMP_DIR:-}" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}

trap cleanup EXIT

have_command() {
    command -v "$1" >/dev/null 2>&1
}

require_any_command() {
    local found=false
    for candidate in "$@"; do
        if have_command "$candidate"; then
            found=true
            break
        fi
    done
    if [ "$found" = false ]; then
        echo "错误：缺少依赖命令，需满足以下之一: $*"
        exit 1
    fi
}

download_file() {
    local url="$1"
    local dest="$2"

    if have_command curl; then
        curl -fL --retry 3 --retry-delay 1 -o "$dest" "$url"
    elif have_command wget; then
        wget -O "$dest" "$url"
    else
        echo "错误：未找到下载工具 curl 或 wget"
        exit 1
    fi
}

try_download_file() {
    local url="$1"
    local dest="$2"

    if have_command curl; then
        curl -fsSL -o "$dest" "$url"
    elif have_command wget; then
        wget -q -O "$dest" "$url"
    else
        return 1
    fi
}

sha256_file() {
    local file_path="$1"
    if have_command sha256sum; then
        sha256sum "$file_path" | awk '{print $1}'
    elif have_command shasum; then
        shasum -a 256 "$file_path" | awk '{print $1}'
    else
        echo "错误：未找到 sha256sum 或 shasum，无法进行校验"
        exit 1
    fi
}

resolve_artifact_name() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)
            case "$arch" in
                x86_64|amd64)
                    echo "apex-x86_64-linux"
                    ;;
                aarch64|arm64)
                    echo "apex-aarch64-linux"
                    ;;
                *)
                    echo "错误：暂不支持的 Linux 架构: $arch" >&2
                    exit 1
                    ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64|amd64)
                    echo "apex-x86_64-macos"
                    ;;
                arm64|aarch64)
                    echo "apex-aarch64-macos"
                    ;;
                *)
                    echo "错误：暂不支持的 macOS 架构: $arch" >&2
                    exit 1
                    ;;
            esac
            ;;
        *)
            echo "错误：暂不支持的操作系统: $os" >&2
            exit 1
            ;;
    esac
}

release_base_url() {
    if [ "$VERSION" = "latest" ]; then
        echo "https://github.com/$REPO/releases/latest/download"
    else
        echo "https://github.com/$REPO/releases/download/$VERSION"
    fi
}

expand_input_path() {
    local raw_path="$1"
    if [[ "$raw_path" == "~" ]]; then
        printf '%s\n' "${HOME:-$raw_path}"
        return 0
    fi
    if [[ "$raw_path" == ~/* ]]; then
        printf '%s/%s\n' "${HOME:-}" "${raw_path#~/}"
        return 0
    fi
    printf '%s\n' "$raw_path"
}

verify_checksum() {
    local archive_path="$1"
    local checksum_path="$2"
    local archive_name expected actual

    archive_name="$(basename "$archive_path")"
    expected="$(awk -v name="$archive_name" '$2 == name {print $1}' "$checksum_path")"

    if [ -z "$expected" ]; then
        echo "警告：checksums.txt 中未找到 $archive_name，跳过校验"
        return 0
    fi

    actual="$(sha256_file "$archive_path")"
    if [ "$expected" != "$actual" ]; then
        echo "错误：SHA256 校验失败"
        echo "  期望: $expected"
        echo "  实际: $actual"
        exit 1
    fi

    echo "SHA256 校验通过"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            if [ "$#" -lt 2 ]; then
                echo "错误：--version 需要一个值"
                exit 1
            fi
            VERSION="$2"
            shift 2
            ;;
        --repo)
            if [ "$#" -lt 2 ]; then
                echo "错误：--repo 需要一个值"
                exit 1
            fi
            REPO="$2"
            shift 2
            ;;
        --config-path)
            if [ "$#" -lt 2 ]; then
                echo "错误：--config-path 需要一个值"
                exit 1
            fi
            CONFIG_PATH="$2"
            shift 2
            ;;
        --force-config)
            FORCE_CONFIG=true
            shift
            ;;
        --skip-checksum)
            SKIP_CHECKSUM=true
            shift
            ;;
        --help|-h)
            print_usage
            exit 0
            ;;
        -*)
            echo "错误：未知选项 $1"
            print_usage
            exit 1
            ;;
        *)
            TARGET_DIR="$1"
            shift
            ;;
    esac
done

require_any_command curl wget
require_any_command tar
require_any_command mktemp

ARTIFACT_NAME="$(resolve_artifact_name)"
ARCHIVE_NAME="${ARTIFACT_NAME}.tar.gz"
BASE_URL="$(release_base_url)"

TMP_DIR="$(mktemp -d)"
ARCHIVE_PATH="$TMP_DIR/$ARCHIVE_NAME"
CHECKSUM_PATH="$TMP_DIR/checksums.txt"
EXTRACT_DIR="$TMP_DIR/extracted"

echo "=== Apex Gateway 预编译包安装 ==="
echo "仓库：$REPO"
echo "版本：$VERSION"
echo "平台包：$ARCHIVE_NAME"
echo "目标路径：$TARGET_DIR"
if [ -n "$CONFIG_PATH" ]; then
    echo "配置文件：$CONFIG_PATH"
fi

echo ""
echo "=== 1. 下载发布包 ==="
download_file "$BASE_URL/$ARCHIVE_NAME" "$ARCHIVE_PATH"

if [ "$SKIP_CHECKSUM" = false ]; then
    echo ""
    echo "=== 2. 校验发布包 ==="
    if try_download_file "$BASE_URL/checksums.txt" "$CHECKSUM_PATH"; then
        verify_checksum "$ARCHIVE_PATH" "$CHECKSUM_PATH"
    else
        echo "警告：未找到 checksums.txt，跳过校验"
    fi
else
    echo ""
    echo "=== 2. 校验发布包 ==="
    echo "已按参数要求跳过校验"
fi

echo ""
echo "=== 3. 解压并安装 ==="
mkdir -p "$EXTRACT_DIR"
tar -xzf "$ARCHIVE_PATH" -C "$EXTRACT_DIR"

EXTRACTED_BINARY="$(find "$EXTRACT_DIR" -type f -name apex | head -n 1)"
EXTRACTED_CONFIG_EXAMPLE="$(find "$EXTRACT_DIR" -type f -name config.example.json | head -n 1)"

if [ -z "$EXTRACTED_BINARY" ]; then
    echo "错误：发布包中未找到 apex 可执行文件"
    exit 1
fi

mkdir -p "$TARGET_DIR"
TARGET_DIR="$(cd "$TARGET_DIR" && pwd)"

cp "$EXTRACTED_BINARY" "$TARGET_DIR/apex"
chmod +x "$TARGET_DIR/apex"

if [ "$(uname -s)" = "Darwin" ] && have_command codesign; then
    echo "在 macOS 上重新签名安装产物..."
    codesign --force --sign - "$TARGET_DIR/apex"
fi

echo ""
echo "=== 4. 处理配置文件 ==="
if [ -n "$CONFIG_PATH" ]; then
    if [ -z "$EXTRACTED_CONFIG_EXAMPLE" ]; then
        echo "错误：发布包中未找到 config.example.json，无法写入配置文件"
        exit 1
    fi

    CONFIG_PATH="$(expand_input_path "$CONFIG_PATH")"

    if [ -f "$CONFIG_PATH" ] && [ "$FORCE_CONFIG" = false ]; then
        echo "错误：配置文件已存在：$CONFIG_PATH"
        echo "如需覆盖，请追加 --force-config"
        exit 1
    fi

    mkdir -p "$(dirname "$CONFIG_PATH")"
    cp "$EXTRACTED_CONFIG_EXAMPLE" "$CONFIG_PATH"
    echo "已写入配置文件：$CONFIG_PATH"
else
    echo "未请求写入配置文件，跳过"
fi

echo ""
echo "=== 安装完成 ==="
echo "目录结构:"
echo "  $TARGET_DIR/apex - 主程序"
if [ -n "$CONFIG_PATH" ]; then
    echo "  $CONFIG_PATH - 用户指定配置文件"
fi
echo ""
echo "运行示例：$TARGET_DIR/apex gateway start --config /path/to/config.json"
