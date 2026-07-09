#!/bin/bash
# gddy installer — downloads, verifies, and installs the GoDaddy CLI (`gddy`)
# from GitHub Releases.
#
# Usage:
#   curl -fsSL https://github.com/godaddy/cli/releases/latest/download/install.sh | bash
#
# Options (when run as a file, e.g. `bash install.sh --prefix ~/.local/bin`):
#   --prefix  DIR       Install directory (default: ~/.local/bin)
#   --version VERSION   Release to install, e.g. v1.2.3 (default: latest)
#   --help              Show this help message

set -euo pipefail

REPO="godaddy/cli"
VERSION=""
PREFIX="${HOME}/.local/bin"
PREFIX_EXPLICIT=0  # set when the user passes --prefix; gates the Windows default

# ── 1. Arg parsing ──────────────────────────────────────────────────────────

usage() {
  cat <<'EOF'
gddy installer

Usage:
  bash install.sh [OPTIONS]

Options:
  --prefix  DIR       Binary install directory
                      (default: ~/.local/bin on macOS/Linux;
                       %LOCALAPPDATA%\Programs\gddy on Windows)
  --version VERSION   Release to install, e.g. v1.2.3 (default: latest)
  --help              Show this help message

Examples:
  bash install.sh                       # latest release to ~/.local/bin
  bash install.sh --prefix /usr/local/bin --version v1.2.3

Prerequisites:
  - curl
  - tar                       (macOS/Linux archives)
  - unzip or powershell.exe   (Windows .zip, e.g. under Git Bash/MSYS)
  - sha256sum (coreutils) or shasum
  - install (coreutils)

Windows:
  This script also runs under Git Bash / MSYS2 / Cygwin: it fetches the
  x86_64-pc-windows-msvc .zip and installs gddy.exe. On Windows the default
  prefix is %LOCALAPPDATA%\Programs\gddy (user-writable, no sudo) unless you
  pass --prefix. For a native PowerShell install instead, use install.ps1.
EOF
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix)
      if [ $# -lt 2 ]; then echo "Error: --prefix requires a directory argument." >&2; exit 1; fi
      PREFIX="$2"; PREFIX_EXPLICIT=1; shift 2 ;;
    --version)
      if [ $# -lt 2 ]; then echo "Error: --version requires a version argument." >&2; exit 1; fi
      VERSION="$2"; shift 2 ;;
    --help|-h)  usage ;;
    *)          echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

# ── 2. Preflight ────────────────────────────────────────────────────────────

info()  { printf "\033[1;34m==>\033[0m %s\n" "$1"; }
warn()  { printf "\033[1;33mWARN:\033[0m %s\n" "$1"; }
error() { printf "\033[1;31mERROR:\033[0m %s\n" "$1" >&2; exit 1; }

command -v curl    >/dev/null 2>&1 || error "curl is required but not found."
command -v install >/dev/null 2>&1 || error "install (coreutils) is required but not found."
# The extraction tool (tar vs unzip/powershell) is OS-specific and checked after
# platform detection below.

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
  # Git Bash, MSYS2, and Cygwin report e.g. "mingw64_nt-10.0-26200"; normalize
  # them all to "windows" and install the native gddy.exe from the .zip asset.
  mingw*|msys*|cygwin*|windows*) OS="windows" ;;
  *)      error "Unsupported OS: $OS. gddy supports darwin, linux, and windows (Git Bash/MSYS2/Cygwin)." ;;
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
  windows-x86_64) PLATFORM="x86_64-pc-windows-msvc" ;;
  *)              error "Unsupported platform: ${OS}-${ARCH}" ;;
esac

# Windows ships a .zip with gddy.exe; the unix targets ship a .tar.gz with gddy.
if [ "$OS" = "windows" ]; then
  BIN="gddy.exe"
  ARCHIVE_EXT="zip"
  # ~/.local/bin is a poor default on Windows shells: there's no `bin` PATH
  # convention there and it often isn't user-writable in the way it is on
  # macOS/Linux. Default to a user-writable dir matching install.ps1 unless
  # --prefix was given.
  if [ "$PREFIX_EXPLICIT" = "0" ]; then
    if [ -n "${LOCALAPPDATA:-}" ]; then
      lad="$(cygpath -u "$LOCALAPPDATA" 2>/dev/null || printf '%s' "$LOCALAPPDATA")"
      PREFIX="${lad}/Programs/gddy"
    elif [ -n "${HOME:-}" ]; then
      PREFIX="${HOME}/bin"
    else
      error "Cannot determine a default install directory (LOCALAPPDATA and HOME are unset). Re-run with --prefix <dir>."
    fi
  fi
else
  BIN="gddy"
  ARCHIVE_EXT="tar.gz"
fi

info "Detected platform: ${PLATFORM}"
info "Installing gddy (${VERSION:-latest})"

# ── 4. Download + verify ───────────────────────────────────────────────────

ARCHIVE="gddy-${PLATFORM}.${ARCHIVE_EXT}"
CHECKSUMS="gddy-checksums-sha256.txt"
if [ -n "$VERSION" ]; then
  BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
else
  BASE_URL="https://github.com/${REPO}/releases/latest/download"
fi
# GNU mktemp accepts a bare `-d`; BSD/macOS mktemp needs a template, so fall
# back to `-t` for portability across both.
TMPDIR="$(mktemp -d 2>/dev/null || mktemp -d -t gddy)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Downloading ${ARCHIVE} and checksums..."
curl -fsSL "${BASE_URL}/${ARCHIVE}"   -o "${TMPDIR}/${ARCHIVE}" \
  || error "Failed to download ${ARCHIVE}. Has the ${VERSION:-latest} release been published yet?"
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
if [ "$OS" = "windows" ]; then
  # Prefer unzip; fall back to PowerShell's Expand-Archive (always present on
  # Windows). cygpath converts the MSYS/Cygwin paths to the Windows form that
  # powershell.exe expects.
  if command -v unzip >/dev/null 2>&1; then
    unzip -q "${TMPDIR}/${ARCHIVE}" -d "$EXTRACT_DIR"
  elif command -v powershell.exe >/dev/null 2>&1; then
    win_src="$(cygpath -w "${TMPDIR}/${ARCHIVE}" 2>/dev/null || printf '%s' "${TMPDIR}/${ARCHIVE}")"
    win_dst="$(cygpath -w "${EXTRACT_DIR}" 2>/dev/null || printf '%s' "${EXTRACT_DIR}")"
    # Escape ' as '' so a path containing an apostrophe (legal in Windows
    # usernames/paths) can't terminate the PowerShell single-quoted literal.
    ps_src="${win_src//\'/\'\'}"
    ps_dst="${win_dst//\'/\'\'}"
    powershell.exe -NoProfile -NonInteractive -Command \
      "Expand-Archive -LiteralPath '${ps_src}' -DestinationPath '${ps_dst}' -Force" \
      || error "Failed to extract ${ARCHIVE} with PowerShell Expand-Archive."
  else
    error "Need 'unzip' or 'powershell.exe' to extract ${ARCHIVE} on Windows, but neither was found."
  fi
else
  command -v tar >/dev/null 2>&1 || error "tar is required to extract ${ARCHIVE} but was not found."
  tar -xzf "${TMPDIR}/${ARCHIVE}" -C "$EXTRACT_DIR"
fi

if [ ! -f "${EXTRACT_DIR}/${BIN}" ]; then
  error "Unexpected archive layout — expected a '${BIN}' binary at the archive root."
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

$SUDO install -m 755 "${EXTRACT_DIR}/${BIN}" "${PREFIX}/${BIN}"
info "Binary installed to ${PREFIX}/${BIN}"

# ── 6. Success ──────────────────────────────────────────────────────────────

echo ""
info "gddy (${VERSION:-latest}) installed successfully!"
echo ""
echo "  Binary:    ${PREFIX}/${BIN}"
echo "  Verify:    gddy --version"
echo ""
if ! printf '%s' ":$PATH:" | grep -qF ":${PREFIX}:"; then
  warn "${PREFIX} is not on your PATH — add it to use 'gddy' directly."
fi
echo "Share this one-liner with your team:"
echo ""
echo "  curl -fsSL https://github.com/${REPO}/releases/latest/download/install.sh | bash"
echo ""
