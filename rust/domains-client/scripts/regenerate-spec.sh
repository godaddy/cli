#!/usr/bin/env bash
# Regenerate the trimmed OpenAPI 3.0 spec the domains-client build consumes.
#
# Pipeline:
#   1. Download the upstream GoDaddy Domains API spec (Swagger 2.0).
#   2. Trim (via trim-spec.py) to the GET /v1/domains (list owned domains) and
#      GET /v1/domains/{domain} (get) operations, GET /v1/domains/available +
#      GET /v1/domains/suggest, GET /v1/domains/agreements, POST
#      /v1/domains/purchase + its GET /v1/domains/purchase/schema/{tld}, the v2
#      POST /v2/customers/{customerId}/domains/register operation, the
#      /v1/domains/{domain}/records DNS-record operations, and the transitive
#      closure of definitions they reference.
#   3. Convert Swagger 2.0 -> OpenAPI 3.0 with `swagger2openapi` (Node, via npx).
#
# Run this ONLY when the upstream spec changes; the committed domains.oas3.json
# is what the crate's build.rs feeds to progenitor (the build never hits the
# network). Requires: curl, python3, and Node/npx (for swagger2openapi).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"   # domains-client/
openapi_dir="$here/openapi"
v2="$openapi_dir/swagger_domains.v2.json"
trimmed_v2="$openapi_dir/.swagger_domains.trimmed.v2.json"
oas3="$openapi_dir/domains.oas3.json"

echo "==> Downloading upstream Swagger 2.0 spec"
curl -fsSL "https://developer.godaddy.com/swagger/swagger_domains.json" -o "$v2"

echo "==> Trimming to domains list + get + available + suggest + agreements + purchase (+ schema) + v2 register + DNS records and their definition closure"
python3 "$here/scripts/trim-spec.py" "$v2" "$trimmed_v2"

echo "==> Converting Swagger 2.0 -> OpenAPI 3.0"
# Pin the converter so regeneration is deterministic across time (an unpinned
# `npx swagger2openapi` would float to the latest release and can drift/break).
npx -y swagger2openapi@7.0.8 "$trimmed_v2" -o "$oas3"
rm -f "$trimmed_v2"
echo "==> Wrote $oas3"
