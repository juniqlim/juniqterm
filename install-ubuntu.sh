#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
BIN_PATH="$BIN_DIR/growterm"
DESKTOP_PATH="$APP_DIR/growterm.desktop"

if ! command -v apt-get >/dev/null 2>&1; then
  echo "This installer requires Ubuntu or another apt-based Linux system." >&2
  exit 1
fi

sudo apt-get update
sudo apt-get install -y \
  build-essential \
  pkg-config \
  curl \
  ca-certificates \
  git \
  libx11-dev \
  libxcb1-dev \
  libxkbcommon-dev \
  libwayland-dev \
  libvulkan1 \
  mesa-vulkan-drivers

if ! command -v cargo >/dev/null 2>&1; then
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

cd "$REPO_DIR"
cargo build -p growterm-app --release

mkdir -p "$BIN_DIR" "$APP_DIR"
install -m 755 "$REPO_DIR/target/release/growterm" "$BIN_PATH"

cat > "$DESKTOP_PATH" <<EOF
[Desktop Entry]
Type=Application
Name=growTerm
Comment=GPU terminal emulator
Exec=$BIN_PATH
Terminal=false
Categories=System;TerminalEmulator;
Icon=utilities-terminal
EOF

chmod 644 "$DESKTOP_PATH"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
fi

echo "Installed growTerm to $BIN_PATH"
echo "You can run it with: growterm"
