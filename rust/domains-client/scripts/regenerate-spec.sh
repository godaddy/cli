#!/usr/bin/env bash
# Regenerate the trimmed OpenAPI 3.0 spec the domains-client build consumes.
#
# Pipeline:
#   1. Download the upstream GoDaddy Domains API spec (Swagger 2.0).
#   2. Trim to the GET /v1/domains/available + GET /v1/domains/suggest operations
#      and the transitive closure of definitions they reference.
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

echo "==> Trimming to available + suggest (GET) and their definition closure"
python3 - "$v2" "$trimmed_v2" <<'PY'
import json, sys

src, dst = sys.argv[1], sys.argv[2]
d = json.load(open(src))

KEEP = ["/v1/domains/available", "/v1/domains/suggest"]
OP_IDS = {"/v1/domains/available": "available", "/v1/domains/suggest": "suggest"}

paths = {}
for p in KEEP:
    get = d["paths"][p]["get"]
    get["operationId"] = OP_IDS[p]
    get["produces"] = ["application/json"]      # drop xml/js variants
    # Keep only the 2xx response: the client needs the success type; non-2xx
    # surface via HTTP status + body. (Also avoids progenitor's single-error-type
    # constraint when the spec mixes Error + ErrorLimit across 4xx/5xx.)
    get["responses"] = {
        code: resp for code, resp in get.get("responses", {}).items()
        if str(code).startswith("2")
    }
    paths[p] = {"get": get}

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
    "info": {"title": "GoDaddy Domains API (availability subset)",
             "version": d.get("info", {}).get("version", "1.0.0")},
    "host": d.get("host", "api.ote-godaddy.com"),
    "basePath": d.get("basePath", "/"),
    "schemes": d.get("schemes", ["https"]),
    "paths": paths,
    "definitions": {n: d["definitions"][n] for n in sorted(needed) if n in d["definitions"]},
}
json.dump(out, open(dst, "w"), indent=2)
print(f"   kept {len(paths)} paths, {len(out['definitions'])} definitions")
PY

echo "==> Converting Swagger 2.0 -> OpenAPI 3.0"
npx -y swagger2openapi "$trimmed_v2" -o "$oas3"
rm -f "$trimmed_v2"
echo "==> Wrote $oas3"
