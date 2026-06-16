#!/usr/bin/env bash
# Regenerate the trimmed OpenAPI 3.0 spec the domains-client build consumes.
#
# Pipeline:
#   1. Download the upstream GoDaddy Domains API spec (Swagger 2.0).
#   2. Trim to the GET /v1/domains (list owned domains), GET /v1/domains/available
#      + GET /v1/domains/suggest operations, the /v1/domains/{domain}/records
#      DNS-record operations, and the transitive closure of definitions they
#      reference.
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

echo "==> Trimming to domains list + available + suggest + DNS records and their definition closure"
python3 - "$v2" "$trimmed_v2" <<'PY'
import copy, json, sys

src, dst = sys.argv[1], sys.argv[2]
d = json.load(open(src))

# GET-only operations kept as-is (operationId pinned). `list` retrieves the
# Domains owned by the authenticated shopper; available/suggest are the
# availability endpoints.
AVAIL_OPS = {
    "/v1/domains": "list",
    "/v1/domains/available": "available",
    "/v1/domains/suggest": "suggest",
}

# DNS record operations: keep every method these paths expose. The upstream
# Swagger documents `recordGet` only on the {type}/{name} path even though the
# gateway also routes GET on the bare and {type} paths (see the OAuth scope
# whitelist in gdcorp-domains/api-domain-data, api/oauthscopewhitelist.json);
# we synthesize those two GETs below so the CLI can list all / by-type records.
RECORD_PATHS = [
    "/v1/domains/{domain}/records",
    "/v1/domains/{domain}/records/{type}",
    "/v1/domains/{domain}/records/{type}/{name}",
]

def only_2xx(op):
    # Keep only the 2xx response: the client needs the success type; non-2xx
    # surface via HTTP status + body. (Also avoids progenitor's single-error-type
    # constraint when the spec mixes Error + ErrorLimit across 4xx/5xx.)
    op["responses"] = {
        code: resp for code, resp in op.get("responses", {}).items()
        if str(code).startswith("2")
    }
    return op

paths = {}

# Availability + suggest (GET only).
for p, op_id in AVAIL_OPS.items():
    get = d["paths"][p]["get"]
    get["operationId"] = op_id
    get["produces"] = ["application/json"]      # drop xml/js variants
    paths[p] = {"get": only_2xx(get)}

# DNS record ops: keep all documented methods, success responses only, and
# normalize content types (drop xml/js variants; request bodies stay JSON).
for p in RECORD_PATHS:
    kept = {}
    for method, op in d["paths"][p].items():
        if method not in ("get", "put", "post", "patch", "delete"):
            continue  # skip non-operation keys (e.g. shared "parameters")
        op["produces"] = ["application/json"]
        if any(prm.get("in") == "body" for prm in op.get("parameters", [])):
            op["consumes"] = ["application/json"]
        kept[method] = only_2xx(op)
    paths[p] = kept

# Synthesize GET-all and GET-by-type from the documented recordGet operation:
# same DNSRecord-array success response, fewer path params, no offset/limit.
record_get = d["paths"]["/v1/domains/{domain}/records/{type}/{name}"]["get"]

def synth_get(keep_param_names, op_id):
    op = copy.deepcopy(record_get)
    op["operationId"] = op_id
    op["produces"] = ["application/json"]
    op["parameters"] = [
        prm for prm in op.get("parameters", [])
        if prm.get("name") in keep_param_names
    ]
    return only_2xx(op)

paths["/v1/domains/{domain}/records"]["get"] = synth_get({"domain"}, "recordGetAll")
paths["/v1/domains/{domain}/records/{type}"]["get"] = synth_get(
    {"domain", "type"}, "recordGetByType"
)

# String `format`s that typify maps to *external* crates (date-time/date ->
# chrono, uuid -> uuid). We don't depend on those (and deliberately keep this
# crate's dependency stack lean — see Cargo.toml), and the CLI only passes these
# values straight through to JSON output, so drop the format and let them
# generate as plain `String` (the RFC 3339 / UUID text is unchanged on the wire).
EXTERNAL_FORMATS = {"date", "date-time", "uuid", "partial-date-time"}

def strip_external_formats(obj):
    if isinstance(obj, dict):
        if obj.get("type") == "string" and obj.get("format") in EXTERNAL_FORMATS:
            obj.pop("format")
        for v in obj.values():
            strip_external_formats(v)
    elif isinstance(obj, list):
        for v in obj:
            strip_external_formats(v)

# Read-only response definitions the CLI only deserializes-and-reprints. The
# published spec over-promises here: it marks fields `required` that the live API
# routinely omits (e.g. DomainSummary.contactRegistrant / renewDeadline on
# cancelled/pending domains) and types `nameServers` as a non-null array while
# the API returns JSON `null`. Relax these to tolerant readers.
#
# The complement — types the CLI *constructs* (request bodies) or reads via
# non-optional fields (e.g. `if available { … }`) — must stay strict, or call
# sites break. Everything not listed here is relaxed.
# NB: these are the *spec* definition names (upper-case `DNS…`), not the
# camel-cased Rust type names progenitor emits (`DnsRecord`).
STRICT_DEFS = {
    "DomainAvailableResponse",  # `available`/`suggest` read non-optional fields
    "DomainSuggestion",
    "DNSRecord",                # built by `dns add`
    "DNSRecordCreateType",
    "DNSRecordCreateTypeName",  # built by `dns set`
    "ArrayOfDNSRecord",         # `dns add` request body
}

def relax_definition(defn):
    # Drop `required` so every property generates as `Option` (missing -> None),
    # and mark array properties `x-nullable` so an explicit `null` deserializes to
    # `None` instead of erroring (`#[serde(default)]` alone only covers a *missing*
    # key, not a present-but-null one).
    if not isinstance(defn, dict):
        return
    defn.pop("required", None)
    for pschema in (defn.get("properties") or {}).values():
        if isinstance(pschema, dict) and pschema.get("type") == "array":
            pschema["x-nullable"] = True

def dedup_enums(obj):
    # typify cannot build a Rust enum from case-only duplicates
    # (e.g. ["FAST","FULL","fast","full"]); keep the first per case-fold.
    if isinstance(obj, dict):
        e = obj.get("enum")
        if isinstance(e, list):
            seen, uniq = set(), []
            for v in e:
                key = v.lower() if isinstance(v, str) else v
                if key not in seen:
                    seen.add(key)
                    uniq.append(v)
            obj["enum"] = uniq
        for v in obj.values():
            dedup_enums(v)
    elif isinstance(obj, list):
        for v in obj:
            dedup_enums(v)

def refs(obj):
    out = set()
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k == "$ref" and isinstance(v, str):
                out.add(v.split("/")[-1])
            else:
                out |= refs(v)
    elif isinstance(obj, list):
        for v in obj:
            out |= refs(v)
    return out

dedup_enums(paths)

needed, stack = set(), list(refs(paths))
while stack:
    n = stack.pop()
    if n in needed:
        continue
    needed.add(n)
    stack += [r for r in refs(d["definitions"].get(n, {})) if r not in needed]

out = {
    "swagger": "2.0",
    "info": {"title": "GoDaddy Domains API (domains list + availability + DNS records subset)",
             "version": d.get("info", {}).get("version", "1.0.0")},
    "host": d.get("host", "api.ote-godaddy.com"),
    "basePath": d.get("basePath", "/"),
    "schemes": d.get("schemes", ["https"]),
    "paths": paths,
    "definitions": {n: d["definitions"][n] for n in sorted(needed) if n in d["definitions"]},
}
for name, defn in out["definitions"].items():
    if name not in STRICT_DEFS:
        relax_definition(defn)

strip_external_formats(out)
json.dump(out, open(dst, "w"), indent=2)
print(f"   kept {len(paths)} paths, {len(out['definitions'])} definitions")
PY

echo "==> Converting Swagger 2.0 -> OpenAPI 3.0"
# Pin the converter so regeneration is deterministic across time (an unpinned
# `npx swagger2openapi` would float to the latest release and can drift/break).
npx -y swagger2openapi@7.0.8 "$trimmed_v2" -o "$oas3"
rm -f "$trimmed_v2"
echo "==> Wrote $oas3"
