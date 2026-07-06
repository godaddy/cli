#!/usr/bin/env bash
# Regenerate the merged OpenAPI 3.0 spec the domains-client build consumes.
#
# The CLI's domain/dns commands span two API generations:
#   * v3 — the Domain Lifecycle Management API (availability, suggest, get,
#     registration quote→register, single DNS-record create, nameservers).
#   * v1 — the operations v3 does not yet serve (list, agreements, DNS record
#     list/set/delete).
#
# Pipeline:
#   1. Download the upstream GoDaddy Domains API spec (Swagger 2.0).
#   2. Trim (trim-spec.py) to the retained v1 operations + their definition closure.
#   3. Convert that v1 subset Swagger 2.0 -> OpenAPI 3.0 with `swagger2openapi`.
#   4. Merge (merge-spec.py) the v1 OAS3 into the vendored v3 spec, producing the
#      single domains.oas3.json progenitor consumes (build.rs never hits the network).
#
# The v3 spec (`openapi/swagger_domains.v3.yaml`) is a vendored, self-contained
# bundle. Its source of truth is
# `gdcorp-platform/domains.domain-lifecycle-specification` (v3/schemas), surfaced
# as the pre-bundled `api-spec.yaml` in `gdcorp-domains/api-domain-v3-prototype`.
# That repo is private, so refresh the vendored copy manually (authenticated):
#
#   gh api repos/gdcorp-domains/api-domain-v3-prototype/contents/api-spec.yaml \
#     --jq '.content' | base64 -d > openapi/swagger_domains.v3.yaml
#
# Run this script ONLY when either upstream spec changes. Requires: curl,
# python3 (with PyYAML), and Node/npx (for swagger2openapi).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"   # domains-client/
openapi_dir="$here/openapi"
v2="$openapi_dir/swagger_domains.v2.json"            # upstream Swagger 2.0 (v1+v2 source)
v3="$openapi_dir/swagger_domains.v3.yaml"            # vendored, self-contained v3 OAS3 bundle
trimmed_v1="$openapi_dir/.swagger_domains.trimmed.v1.json"
v1_oas3="$openapi_dir/.domains.v1.oas3.json"
oas3="$openapi_dir/domains.oas3.json"

# The document host. base_url is overridden at runtime by the CLI per environment,
# so this is cosmetic; keep it on the public OTE host.
host="https://api.ote-godaddy.com"

echo "==> Downloading upstream Swagger 2.0 spec"
curl -fsSL "https://developer.godaddy.com/swagger/swagger_domains.json" -o "$v2"

echo "==> Trimming to retained v1 ops (list + agreements)"
python3 "$here/scripts/trim-spec.py" "$v2" "$trimmed_v1"

echo "==> Converting v1 subset Swagger 2.0 -> OpenAPI 3.0"
# Pin the converter so regeneration is deterministic across time.
npx -y swagger2openapi@7.0.8 "$trimmed_v1" -o "$v1_oas3"

echo "==> Merging v1 OAS3 into the vendored v3 spec"
python3 "$here/scripts/merge-spec.py" "$v1_oas3" "$v3" "$oas3" "$host"

rm -f "$trimmed_v1" "$v1_oas3"
echo "==> Wrote $oas3"
