#!/bin/sh
set -eu

VERSION="${CLAW_VERSION:-latest}"
INSTALL_DIR="${CLAW_INSTALL_DIR:-/usr/local/bin}"

detect_platform() {
  OS=$(uname -s | tr '[:upper:]' '[:lower:]')
  ARCH=$(uname -m)

  case "$OS" in
    linux)
      case "$ARCH" in
        x86_64) TRIPLET="x86_64-unknown-linux-gnu" ;;
        aarch64|arm64) TRIPLET="aarch64-unknown-linux-gnu" ;;
        *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
      esac ;;
    darwin)
      case "$ARCH" in
        x86_64) TRIPLET="x86_64-apple-darwin" ;;
        arm64) TRIPLET="aarch64-apple-darwin" ;;
        *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
      esac ;;
    *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
  esac
}

resolve_version() {
  if [ "$VERSION" = "latest" ]; then
    VERSION=$(curl -sI "https://github.com/nicholasoxford/Claw/releases/latest" \
      | grep -i '^location:' | sed 's|.*/v||' | tr -d '\r\n')
    if [ -z "$VERSION" ]; then
      echo "Failed to resolve latest version" >&2
      exit 1
    fi
  fi
}

download_and_install() {
  MIRROR="${CLAW_DOWNLOAD_MIRROR:-https://github.com/nicholasoxford/Claw/releases/download/v${VERSION}}"
  FILENAME="claw-${TRIPLET}.tar.gz"
  URL="${MIRROR}/${FILENAME}"
  TMPDIR=$(mktemp -d)

  echo "Downloading Claw v${VERSION} for ${TRIPLET}..."
  curl -fSL "$URL" -o "${TMPDIR}/${FILENAME}"
  tar -xzf "${TMPDIR}/${FILENAME}" -C "${TMPDIR}"

  # macOS Gatekeeper fix
  if [ "$(uname -s)" = "Darwin" ]; then
    xattr -d com.apple.quarantine "${TMPDIR}/claw" 2>/dev/null || true
  fi

  install -m 755 "${TMPDIR}/claw" "${INSTALL_DIR}/claw"
  rm -rf "${TMPDIR}"

  echo "Claw v${VERSION} installed to ${INSTALL_DIR}/claw"
}

detect_platform
resolve_version
download_and_install
