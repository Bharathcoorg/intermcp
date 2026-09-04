#!/usr/bin/env bash
set -e

# ==============================================================================
# InterMCP 1-Click Universal Installer for macOS, Linux, and WSL
# https://github.com/Bharathcoorg/intermcp
# ==============================================================================

BOLD="\033[1m"
GREEN="\033[32m"
CYAN="\033[36m"
YELLOW="\033[33m"
RESET="\033[0m"

echo -e "${CYAN}${BOLD}⚡ InterMCP 1-Click Installer${RESET}"
echo -e "============================================================"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64|amd64)
    ARCH="x86_64"
    ;;
  arm64|aarch64)
    ARCH="aarch64"
    ;;
  *)
    echo -e "${YELLOW}Warning: Unsupported architecture $ARCH. Attempting fallback...${RESET}"
    ;;
esac

INSTALL_DIR="${INTERMCP_INSTALL_DIR:-$HOME/.intermcp/bin}"
mkdir -p "$INSTALL_DIR"

BINARY_NAME="intermcp"
REPO="Bharathcoorg/intermcp"

echo -e "🔍 Detected platform: ${BOLD}${OS}-${ARCH}${RESET}"
echo -e "📁 Target installation path: ${BOLD}${INSTALL_DIR}/${BINARY_NAME}${RESET}"

# Check if local cargo exists
if command -v cargo >/dev/null 2>&1 && [ -f "Cargo.toml" ]; then
  echo -e "\n🔨 Building native optimized binary using local Rust toolchain..."
  cargo build --release
  cp target/release/"$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
else
  TAR_NAME="intermcp-${OS}-${ARCH}.tar.gz"
  DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${TAR_NAME}"

  echo -e "\n📥 Downloading release binary from: ${DOWNLOAD_URL}"
  if curl -sSfL "$DOWNLOAD_URL" -o "/tmp/$TAR_NAME" 2>/dev/null; then
    tar -xzf "/tmp/$TAR_NAME" -C "$INSTALL_DIR"
    rm -f "/tmp/$TAR_NAME"
  elif command -v cargo >/dev/null 2>&1; then
    echo -e "${YELLOW}Prebuilt binary asset not reached. Compiling via cargo install...${RESET}"
    cargo install intermcp --root "$HOME/.intermcp"
  else
    echo -e "❌ Could not download binary and Rust 'cargo' is not installed."
    echo -e "Please visit https://github.com/${REPO}/releases to download the binary manually."
    exit 1
  fi
fi

chmod +x "$INSTALL_DIR/$BINARY_NAME"

echo -e "\n${GREEN}✅ InterMCP binary installed successfully!${RESET}"

# Automatically configure all desktop AI IDEs
echo -e "\n⚡ Running 1-Click IDE Auto-Configuration..."
"$INSTALL_DIR/$BINARY_NAME" setup

echo -e "============================================================"
echo -e "${GREEN}${BOLD}🎉 Installation and IDE Setup Complete!${RESET}"
echo -e "Ensure ${BOLD}${INSTALL_DIR}${RESET} is in your PATH:"
echo -e "   export PATH=\"\$HOME/.intermcp/bin:\$PATH\""
echo -e "============================================================"
