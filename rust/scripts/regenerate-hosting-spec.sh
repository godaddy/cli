#!/usr/bin/env bash
# Regenerate the vendored Node.js Hosting OpenAPI spec and embedded API catalog.
#
# The hosting API publishes its spec on prod only (OTE has no spec endpoint):
#   https://api.godaddy.com/v1/hosting/nodejs/openapi.yaml
#
# Overrides:
#   HOSTING_SPEC_URL   — download from this URL instead (local Katana/dev only; not OTE)
#   HOSTING_SPEC_PATH  — copy from a local file (e.g. hosting-web-apps checkout)
#
# Requires: curl (unless HOSTING_SPEC_PATH is set), cargo
set -euo pipefail

rust_root="$(cd "$(dirname "$0")/.." && pwd)"
out="$rust_root/schemas/openapi/hosting-nodejs-public-v1.yaml"
default_url="https://api.godaddy.com/v1/hosting/nodejs/openapi.yaml"

if [ -n "${HOSTING_SPEC_PATH:-}" ]; then
  echo "==> Copying spec from HOSTING_SPEC_PATH=$HOSTING_SPEC_PATH"
  cp "$HOSTING_SPEC_PATH" "$out"
elif [ -n "${HOSTING_SPEC_URL:-}" ]; then
  echo "==> Downloading spec from HOSTING_SPEC_URL=$HOSTING_SPEC_URL"
  curl -fsSL "$HOSTING_SPEC_URL" -o "$out"
else
  echo "==> Downloading prod hosting OpenAPI spec"
  curl -fsSL "$default_url" -o "$out"
fi

echo "==> Wrote $out"
echo "==> Regenerating API catalog"
(cd "$rust_root" && cargo run -p generate-api-catalog)
echo "==> Done"
