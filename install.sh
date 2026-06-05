#!/bin/bash
# gddy installer — downloads, verifies, and installs the GoDaddy CLI Rust-port
# ALPHA binary (`gddy`) from the rolling `alpha` GitHub prerelease.
#
# `gddy` installs alongside the legacy `godaddy` CLI so you can run both. It is
# an experimental build that tracks the tip of `rust-port`; expect compatibility
# breakage.
#
# Usage:
#   curl -fsSL https://github.com/godaddy/cli/releases/download/alpha/install.sh | bash
#
# Options (when run as a file, e.g. `bash install.sh --prefix ~/.local/bin`):
#   --prefix  DIR       Install directory (default: /usr/local/bin)
#   --help              Show this help message

set -euo pipefail

REPO="godaddy/cli"
TAG="alpha"
PREFIX="/usr/local/bin"

# ── 1. Arg parsing ──────────────────────────────────────────────────────────

usage() {
  cat <<'EOF'
gddy installer (Rust-port alpha)

Usage:
  bash install.sh [OPTIONS]

Options:
  --prefix  DIR       Binary install directory (default: /usr/local/bin)
  --help              Show this help message

Examples:
  bash install.sh                       # latest alpha to /usr/local/bin
  bash install.sh --prefix ~/.local/bin # custom prefix (no sudo for binaries)

Prerequisites:
  - curl
  - tar
  - sha256sum (coreutils) or shasum
  - install (coreutils)
EOF
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix)
      if [ $# -lt 2 ]; then echo "Error: --prefix requires a directory argument." >&2; exit 1; fi
      PREFIX="$2"; shift 2 ;;
    --help|-h)  usage ;;
    *)          echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

# ── 2. Preflight ────────────────────────────────────────────────────────────

info()  { printf "\033[1;34m==>\033[0m %s\n" "$1"; }
warn()  { printf "\033[1;33mWARN:\033[0m %s\n" "$1"; }
error() { printf "\033[1;31mERROR:\033[0m %s\n" "$1" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || error "curl is required but not found."
command -v tar  >/dev/null 2>&1 || error "tar is required but not found."

# Detect SHA-256 tool.
if command -v sha256sum >/dev/null 2>&1; then
  SHA256_CMD="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  SHA256_CMD="shasum -a 256"
else
  error "Neither sha256sum nor shasum found — cannot verify checksums."
fi

# ── 3. Detect platform ─────────────────────────────────────────────────────

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  darwin) ;;
  linux)  ;;
  *)      error "Unsupported OS: $OS. gddy supports darwin and linux." ;;
esac

case "$ARCH" in
  x86_64)  ARCH="x86_64" ;;
  aarch64) ARCH="aarch64" ;;
  arm64)   ARCH="aarch64" ;;  # macOS Apple Silicon
  *)       error "Unsupported architecture: $ARCH" ;;
esac

# Map OS+arch to the Rust target triple used in release archive names.
case "${OS}-${ARCH}" in
  darwin-x86_64)  PLATFORM="x86_64-apple-darwin" ;;
  darwin-aarch64) PLATFORM="aarch64-apple-darwin" ;;
  linux-x86_64)   PLATFORM="x86_64-unknown-linux-gnu" ;;
  linux-aarch64)  PLATFORM="aarch64-unknown-linux-gnu" ;;
  *)              error "Unsupported platform: ${OS}-${ARCH}" ;;
esac
info "Detected platform: ${PLATFORM}"
info "Installing gddy alpha (${TAG})"

# ── 4. Download + verify ───────────────────────────────────────────────────

ARCHIVE="gddy-${PLATFORM}.tar.gz"
CHECKSUMS="gddy-checksums-sha256.txt"
BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Downloading ${ARCHIVE} and checksums..."
curl -fsSL "${BASE_URL}/${ARCHIVE}"   -o "${TMPDIR}/${ARCHIVE}" \
  || error "Failed to download ${ARCHIVE}. Has the ${TAG} release been published yet?"
curl -fsSL "${BASE_URL}/${CHECKSUMS}" -o "${TMPDIR}/${CHECKSUMS}" \
  || error "Failed to download ${CHECKSUMS}."

info "Verifying checksum..."
# Exact filename match ($2 is the path field from sha256sum/shasum output) so a
# regex-special character or a substring of another asset name can't mislead us.
EXPECTED="$(awk -v f="$ARCHIVE" '$2 == f {print $1}' "${TMPDIR}/${CHECKSUMS}")"
if [ -z "$EXPECTED" ]; then
  error "Checksum for ${ARCHIVE} not found in ${CHECKSUMS}."
fi

ACTUAL="$($SHA256_CMD "${TMPDIR}/${ARCHIVE}" | awk '{print $1}')"
if [ "$EXPECTED" != "$ACTUAL" ]; then
  error "Checksum mismatch!
  Expected: ${EXPECTED}
  Got:      ${ACTUAL}"
fi
info "Checksum verified."

# ── 5. Extract + install ───────────────────────────────────────────────────

EXTRACT_DIR="${TMPDIR}/extract"
mkdir -p "$EXTRACT_DIR"
tar -xzf "${TMPDIR}/${ARCHIVE}" -C "$EXTRACT_DIR"

if [ ! -f "${EXTRACT_DIR}/gddy" ]; then
  error "Unexpected archive layout — expected a 'gddy' binary at the archive root."
fi

# Determine if we need sudo for the prefix directory.
SUDO=""
if [ ! -w "$PREFIX" ]; then
  if [ -d "$PREFIX" ]; then
    info "Write permission required for ${PREFIX} — will use sudo."
    SUDO="sudo"
  else
    # Try to create the directory; if that fails, use sudo.
    if ! mkdir -p "$PREFIX" 2>/dev/null; then
      info "Cannot create ${PREFIX} — will use sudo."
      SUDO="sudo"
    fi
  fi
fi

# If elevation is required, make sure sudo actually exists before relying on it,
# so locked-down/minimal hosts get an actionable message instead of "command not found".
if [ -n "$SUDO" ] && ! command -v sudo >/dev/null 2>&1; then
  error "Installing to ${PREFIX} requires elevated permissions, but 'sudo' was not found. Re-run with --prefix pointing to a writable directory, e.g. --prefix \"\$HOME/.local/bin\"."
fi

if [ -n "$SUDO" ]; then
  $SUDO mkdir -p "$PREFIX"
fi

$SUDO install -m 755 "${EXTRACT_DIR}/gddy" "${PREFIX}/gddy"
info "Binary installed to ${PREFIX}/gddy"

# ── 6. Success ──────────────────────────────────────────────────────────────

echo ""
info "gddy alpha (${TAG}) installed successfully!"
echo ""
echo "  Binary:    ${PREFIX}/gddy"
echo "  Verify:    gddy --version"
echo ""
echo "  Note: this is an experimental Rust-port alpha that installs alongside"
echo "  the current 'godaddy' CLI."
echo ""
if ! printf '%s' ":$PATH:" | grep -qF ":${PREFIX}:"; then
  warn "${PREFIX} is not on your PATH — add it to use 'gddy' directly."
fi
echo "Share this one-liner with your team:"
echo ""
echo "  curl -fsSL https://github.com/${REPO}/releases/download/${TAG}/install.sh | bash"
echo ""
