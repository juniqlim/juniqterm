#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_NAME="growterm"
VERSION="0.1.0"
ARCH="$(dpkg --print-architecture)"
STAGING="$REPO_DIR/target/deb/$PACKAGE_NAME"
OUT_DIR="$REPO_DIR/target/packages"
DEB_PATH="$OUT_DIR/${PACKAGE_NAME}_${VERSION}_${ARCH}.deb"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "dpkg-deb is required to build a Debian package." >&2
  exit 1
fi

cargo build -p growterm-app --release

rm -rf "$STAGING"
mkdir -p \
  "$STAGING/DEBIAN" \
  "$STAGING/usr/bin" \
  "$STAGING/usr/share/applications" \
  "$STAGING/usr/share/doc/$PACKAGE_NAME" \
  "$OUT_DIR"

install -m 755 "$REPO_DIR/target/release/growterm" "$STAGING/usr/bin/growterm"
install -m 644 "$REPO_DIR/LICENSE" "$STAGING/usr/share/doc/$PACKAGE_NAME/copyright"

cat > "$STAGING/usr/share/applications/growterm.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=growTerm
Comment=GPU terminal emulator
Exec=growterm
Terminal=false
Categories=System;TerminalEmulator;
Icon=utilities-terminal
EOF

cat > "$STAGING/DEBIAN/control" <<EOF
Package: $PACKAGE_NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: juniqlim
Depends: libc6, libgcc-s1, libx11-6, libxcb1, libxkbcommon0, libwayland-client0, libvulkan1, mesa-vulkan-drivers
Description: GPU terminal emulator
 growTerm is a GPU-accelerated terminal emulator written in Rust.
EOF

dpkg-deb --build "$STAGING" "$DEB_PATH"
echo "Built $DEB_PATH"
echo "Install on Ubuntu with: sudo apt install $DEB_PATH"
