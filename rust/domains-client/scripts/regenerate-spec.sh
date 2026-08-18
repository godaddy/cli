#!/usr/bin/env bash
# Regenerate the merged OpenAPI 3.0 spec the domains-client build consumes.
#
# The CLI's domain/dns commands span two API generations:
#   * v3 — the Domain Lifecycle Management API (availability, suggest, get,
#     list, registration quote→register, the full DNS record lifecycle,
#     nameservers).
#   * v1 — the one operation v3 does not yet serve (agreements).
#
# Pipeline:
#   1. Download the upstream GoDaddy Domains API spec (Swagger 2.0).
#   2. Trim (trim-spec.py) to the retained v1 operation + its definition closure.
#   3. Convert that v1 subset Swagger 2.0 -> OpenAPI 3.0 with `swagger2openapi`.
#   4. Merge (merge-spec.py) the v1 OAS3 into the vendored v3 spec, producing the
#      single domains.oas3.json progenitor consumes (build.rs never hits the network).
#   5. Patch (jq, below) the merged output to re-apply the couple of deliberate
#      deviations from the literal upstream contract that this codebase needs.
#      This step is what lets step 6 below be a dumb wholesale copy — nobody
#      has to remember to hand-reapply anything.
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
# That's a wholesale overwrite of the *input* to this script, which is fine —
# step 5's jq patch re-applies the deviations to the *output* (domains.oas3.json)
# every time this script runs, regardless of what the upstream bundle says. If a
# deviation is ever intentionally dropped (the upstream bug gets fixed, etc.),
# remove its rule from the jq filter below — do not just stop running this script.
#
# Run this script ONLY when either upstream spec changes. Requires: curl, jq,
# python3 (with PyYAML), and Node/npx (for swagger2openapi).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"   # domains-client/
openapi_dir="$here/openapi"
v2="$openapi_dir/swagger_domains.v2.json"            # upstream Swagger 2.0 (v1+v2 source)
v3="$openapi_dir/swagger_domains.v3.yaml"            # vendored, self-contained v3 OAS3 bundle
trimmed_v1="$openapi_dir/.swagger_domains.trimmed.v1.json"
v1_oas3="$openapi_dir/.domains.v1.oas3.json"
oas3="$openapi_dir/domains.oas3.json"

# The document host. `gddy domain`/`dns` commands never read this — their base
# URL is resolved at runtime via `environments::resolve_domains` — but
# `generate-api-catalog` does: `resolve_catalog_base_url` treats this value as
# the prod URL and derives every other environment from it by host
# substitution (see its `prod_base_url` parameter and
# `resolve_catalog_base_url_returns_prod_unchanged`/`_applies_convention_for_non_prod`
# in `environments/catalog.rs`). Keep it prod-canonical, consistent with every
# other embedded catalog domain's baseUrl, so that derivation stays correct.
host="https://api.godaddy.com"

echo "==> Downloading upstream Swagger 2.0 spec"
curl -fsSL "https://developer.godaddy.com/swagger/swagger_domains.json" -o "$v2"

echo "==> Trimming to retained v1 ops (agreements)"
python3 "$here/scripts/trim-spec.py" "$v2" "$trimmed_v1"

echo "==> Converting v1 subset Swagger 2.0 -> OpenAPI 3.0"
# Pin the converter so regeneration is deterministic across time.
npx -y swagger2openapi@7.0.8 "$trimmed_v1" -o "$v1_oas3"

echo "==> Merging v1 OAS3 into the vendored v3 spec"
python3 "$here/scripts/merge-spec.py" "$v1_oas3" "$v3" "$oas3" "$host"

echo "==> Patching deliberate deviations from the literal upstream contract"
# * statuses: force `items` to a plain string. progenitor always
#   seq-serializes an array-of-enum setter as repeated `statuses=` pairs
#   regardless of the spec's `explode: false`, and DomainStatus (a strict
#   enum) can't hold a pre-joined "A,B" value the way a bare-string schema
#   can. Losing this silently breaks `--status` with a live
#   `400 MISMATCH_FORMAT` — caught once already by live-testing, now pinned
#   by `domains_client::tests::statuses_param_items_stay_a_plain_string`.
# * replaceDNSRecord: drop the PUT entirely. Despite returning 200, it
#   silently replaces the entire dns-records collection instead of the one
#   record — see https://github.com/godaddy/cli/issues/136. Do not remove
#   this rule without a confirmed server-side fix and a fresh live check
#   against a real zone with unrelated records; pinned by
#   `domains_client::tests::replace_dns_record_stays_removed`.
patched=$(jq -a '
  (.paths["/v3/domains/domain-names"].get.parameters[] | select(.name == "statuses") | .schema.items) = {"type": "string"}
  | del(.paths["/v3/domains/zones/{zone}/dns-records/{recordId}"].put)
' "$oas3")
printf '%s' "$patched" >"$oas3"

rm -f "$trimmed_v1" "$v1_oas3"
echo "==> Wrote $oas3"
